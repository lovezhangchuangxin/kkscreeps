//! 执行内建动作与 tick 链；Hosted 资源动作提供局部拒绝恢复。
use super::*;

impl ActionSlot {
    pub(super) fn run(&self, ctx: &RoomCtx, id: ObjId, d: &IntentData) -> Result<(), String> {
        match self {
            ActionSlot::Native(f) => native_action(*f)(ctx, id, d),
            ActionSlot::Hosted { slot, native } => {
                // 三个资源动作只改源/目标记录并追加事件；实验室绑定容量
                // 也在目标记录内。JS 只允许 link cooldown 写入，creep 只读。
                // 不分配 ID、不改位置/玩家或其他工作队列。
                // 在整链开始前保存，才能撤销 before 与 native 之后的拒绝，
                // 同时保留此前动作对共享接收方的写入。无 Mod 的 Native 不走这里。
                let (objects, event_len) = {
                    let w = ctx.borrow();
                    let objects: Vec<_> = std::iter::once(id)
                        .chain(d.id.filter(|target| *target != id))
                        .filter_map(|id| w.room.objects.get(&id).cloned().map(|o| (id, o)))
                        .collect();
                    (objects, w.events.len())
                };
                let outcome = slot.executor.run(
                    &slot.plan,
                    ctx,
                    SlotCall::Action {
                        id,
                        data: d,
                        native: *native,
                    },
                )?;
                if matches!(outcome, SlotOutcome::Declined(_)) {
                    let mut w = ctx.borrow_mut();
                    for (id, object) in objects {
                        w.room.objects.insert(id, object);
                    }
                    w.events.truncate(event_len);
                }
                Ok(())
            }
            ActionSlot::Chained(chain) => {
                for f in &chain.before {
                    f(ctx, id, d)?;
                }
                let result = run_wrapped(&chain.wraps, &chain.center, ctx, id, d);
                if result.is_ok() {
                    for f in &chain.after {
                        f(ctx, id, d)?;
                    }
                }
                result
            }
        }
    }
}

impl TickSlot {
    pub(super) fn run(&self, ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
        match self {
            TickSlot::Native(f) => native_tick(*f)(ctx, id),
            TickSlot::Hosted(slot) => slot
                .executor
                .run(
                    &slot.plan,
                    ctx,
                    SlotCall::Tick {
                        id,
                        native: crate::settle::economy::link_cooldown_tick,
                    },
                )
                .and_then(require_completed),
            TickSlot::Chained(chain) => {
                for f in &chain.before {
                    f(ctx, id)?;
                }
                let result = run_tick_wrapped(&chain.wraps, &chain.center, ctx, id);
                if result.is_ok() {
                    for f in &chain.after {
                        f(ctx, id)?;
                    }
                }
                result
            }
        }
    }
}

/// wraps[0] 最外层，中心最内；从中心向外逐层装箱。
fn run_wrapped(
    wraps: &[Rc<ActionWrap>],
    center: &Rc<ActionCenter>,
    ctx: &RoomCtx,
    id: ObjId,
    d: &IntentData,
) -> Result<(), String> {
    let Some((outer, rest)) = wraps.split_first() else {
        return run_center(center, ctx, id, d);
    };
    let mut inner = box_inner(rest, center);
    let mut next = Next {
        calls: 0,
        fault: None,
        inner: &mut inner,
        ctx,
    };
    {
        let result = outer(ctx, id, d, &mut next);
        next.fault.map_or(result, Err)
    }
}

fn run_center(
    center: &Rc<ActionCenter>,
    ctx: &RoomCtx,
    id: ObjId,
    d: &IntentData,
) -> Result<(), String> {
    match &**center {
        ActionCenter::Native(f) => native_action(*f)(ctx, id, d),
        ActionCenter::Custom(f) => f(ctx, id, d),
    }
}

fn run_tick_center(center: &Rc<TickCenter>, ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
    match &**center {
        TickCenter::Native(f) => native_tick(*f)(ctx, id),
        TickCenter::Custom(f) => f(ctx, id),
    }
}

/// 递归装箱内层链；每层 wrap 通过 [`Next`] 恰好调用一次内层。
fn box_inner<'a>(wraps: &'a [Rc<ActionWrap>], center: &'a Rc<ActionCenter>) -> ActionChain<'a> {
    let Some((outer, rest)) = wraps.split_first() else {
        return Box::new(move |ctx: &RoomCtx, id: ObjId, d: &IntentData| {
            run_center(center, ctx, id, d)
        });
    };
    let mut inner = box_inner(rest, center);
    Box::new(
        move |ctx: &RoomCtx, id: ObjId, d: &IntentData| -> Result<(), String> {
            let mut next = Next {
                calls: 0,
                fault: None,
                inner: &mut inner,
                ctx,
            };
            {
                let result = outer(ctx, id, d, &mut next);
                next.fault.map_or(result, Err)
            }
        },
    )
}

fn run_tick_wrapped(
    wraps: &[Rc<TickWrap>],
    center: &Rc<TickCenter>,
    ctx: &RoomCtx,
    id: ObjId,
) -> Result<(), String> {
    let Some((outer, rest)) = wraps.split_first() else {
        return run_tick_center(center, ctx, id);
    };
    let mut inner = box_tick_inner(rest, center);
    let mut next = TickNext {
        calls: 0,
        fault: None,
        inner: &mut inner,
        ctx,
    };
    {
        let result = outer(ctx, id, &mut next);
        next.fault.map_or(result, Err)
    }
}

fn box_tick_inner<'a>(wraps: &'a [Rc<TickWrap>], center: &'a Rc<TickCenter>) -> TickChain<'a> {
    let Some((outer, rest)) = wraps.split_first() else {
        return Box::new(move |ctx: &RoomCtx, id: ObjId| run_tick_center(center, ctx, id));
    };
    let mut inner = box_tick_inner(rest, center);
    Box::new(move |ctx: &RoomCtx, id: ObjId| -> Result<(), String> {
        let mut next = TickNext {
            calls: 0,
            fault: None,
            inner: &mut inner,
            ctx,
        };
        {
            let result = outer(ctx, id, &mut next);
            next.fault.map_or(result, Err)
        }
    })
}

impl ActionHandler {
    pub(super) fn label(&self) -> &'static str {
        match self {
            ActionHandler::Before(_) => "before",
            ActionHandler::Wrap(_) => "wrap",
            ActionHandler::Replace(_) => "replace",
            ActionHandler::After(_) => "after",
        }
    }
}

impl TickHandler {
    pub(super) fn label(&self) -> &'static str {
        match self {
            TickHandler::Before(_) => "before",
            TickHandler::Wrap(_) => "wrap",
            TickHandler::Replace(_) => "replace",
            TickHandler::After(_) => "after",
        }
    }
}

// 未接局部回滚的 native 槽不能把宿主 Declined 当成功，以免发布部分写入。
pub(super) fn require_completed(outcome: SlotOutcome) -> Result<(), String> {
    match outcome {
        SlotOutcome::Completed => Ok(()),
        SlotOutcome::Declined(_) => Err("内建槽尚不支持正常拒绝".into()),
    }
}
