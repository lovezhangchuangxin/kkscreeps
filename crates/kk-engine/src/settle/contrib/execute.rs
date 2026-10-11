//! 执行内建动作与 tick 链；Hosted 资源动作提供局部拒绝恢复。
use super::*;

impl ActionSlot {
    pub(super) fn run(&self, ctx: &RoomCtx, id: ObjId, d: &IntentData) -> Result<(), String> {
        match self {
            ActionSlot::Native(f) => native_action(*f)(ctx, id, d),
            ActionSlot::Hosted { slot, native } => {
                // 移动槽额外保存执行者的请求登记与意图疲劳；拒绝时按快照
                // 恢复，不触碰其他执行者的请求或拉拽关系。三个资源动作只改
                // 源/目标记录并追加事件；实验室绑定容量也在目标记录内。
                // JS 额外资源写入由运行时的触达日志恢复；此边界继续恢复
                // 原 native 两端和整链新增事件。不分配 ID、不改位置/玩家
                // 或其他工作队列。在整链开始前保存，才能撤销 before 与
                // native 之后的拒绝，同时保留此前动作对共享接收方的写入。
                // 无 Mod 的 Native 不走这里。
                //
                // 战斗槽边界另含三块：延迟伤害/治疗队列条目、攻击经 rampart
                // 重定向后的真实受击对象、以及结构摧毁产生的废墟插入与
                // 对象移除（增删经 chain_mutations 日志逆序撤销并回退 ID
                // 游标）。能力改向触达的链外对象由运行时首触日志恢复。
                //
                // 建造族槽与战斗槽共用该日志化边界：build 的完工变换
                // （移除工地+插入结构）、dismantle 的击毁废墟与超容掉落
                // 插入同样经增删日志撤销；dismantle 的受影响集另含目标格
                // rampart 与执行者格上的掉落面（掉落堆与容器）。
                let move_slot =
                    kk_mod_api::builtin::MOVE_ACTION_SLOTS.contains(&slot.plan.key.as_str());
                let combat_slot =
                    kk_mod_api::builtin::COMBAT_ACTION_SLOTS.contains(&slot.plan.key.as_str());
                let build_slot =
                    kk_mod_api::builtin::BUILD_ACTION_SLOTS.contains(&slot.plan.key.as_str());
                enum Boundary {
                    Plain {
                        objects: Vec<(ObjId, RoomObject)>,
                        event_len: usize,
                        moves: Option<crate::settle::movement::MoveRequestSnapshot>,
                    },
                    Journaled(crate::settle::ChainSnapshot),
                }
                let boundary = {
                    if combat_slot {
                        let mut w = ctx.borrow_mut();
                        let mass = slot.plan.key.as_str() == "builtin:creep.rangedMassAttack";
                        Boundary::Journaled(w.snapshot_combat(id, d.id, mass))
                    } else if build_slot {
                        let mut w = ctx.borrow_mut();
                        let action = crate::settle::creep::BuildAction::from_slot_key(
                            slot.plan.key.as_str(),
                        )
                        .expect("建造族槽键已由 BUILD_ACTION_SLOTS 判定");
                        Boundary::Journaled(w.snapshot_build(id, d.id, action))
                    } else {
                        let w = ctx.borrow();
                        let moves = move_slot.then(|| w.snapshot_move_requests(id));
                        let objects: Vec<_> = std::iter::once(id)
                            .chain(d.id.filter(|target| *target != id))
                            .filter_map(|id| w.room.objects.get(&id).cloned().map(|o| (id, o)))
                            .collect();
                        let event_len = w.events.len();
                        drop(w);
                        Boundary::Plain {
                            objects,
                            event_len,
                            moves,
                        }
                    }
                };
                let executor = super::hosted_executor(slot.executor)?;

                let outcome = executor.run(
                    &slot.plan,
                    ctx,
                    SlotCall::Action {
                        id,
                        data: d,
                        native: *native,
                    },
                )?;
                match (boundary, matches!(outcome, SlotOutcome::Declined(_))) {
                    (Boundary::Journaled(snapshot), true) => {
                        ctx.borrow_mut().restore_chain_snapshot(snapshot);
                    }
                    (Boundary::Journaled(_), false) => {
                        ctx.borrow_mut().finish_chain_snapshot();
                    }
                    (
                        Boundary::Plain {
                            objects,
                            event_len,
                            moves,
                        },
                        true,
                    ) => {
                        let mut w = ctx.borrow_mut();
                        for (id, object) in objects {
                            w.room.objects.insert(id, object);
                        }
                        w.events.truncate(event_len);
                        if let Some(snapshot) = moves {
                            w.restore_move_requests(id, snapshot);
                        }
                    }
                    (Boundary::Plain { .. }, false) => {}
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
            TickSlot::Hosted(slot) => {
                let executor = super::hosted_executor(slot.executor)?;
                executor
                    .run(
                        &slot.plan,
                        ctx,
                        SlotCall::Tick {
                            id,
                            native: crate::settle::economy::link_cooldown_tick,
                        },
                    )
                    .and_then(require_completed)
            }
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
    wraps: &[Arc<ActionWrap>],
    center: &Arc<ActionCenter>,
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
    center: &Arc<ActionCenter>,
    ctx: &RoomCtx,
    id: ObjId,
    d: &IntentData,
) -> Result<(), String> {
    match &**center {
        ActionCenter::Native(f) => native_action(*f)(ctx, id, d),
        ActionCenter::Custom(f) => f(ctx, id, d),
    }
}

fn run_tick_center(center: &Arc<TickCenter>, ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
    match &**center {
        TickCenter::Native(f) => native_tick(*f)(ctx, id),
        TickCenter::Custom(f) => f(ctx, id),
    }
}

/// 递归装箱内层链；每层 wrap 通过 [`Next`] 恰好调用一次内层。
fn box_inner<'a>(wraps: &'a [Arc<ActionWrap>], center: &'a Arc<ActionCenter>) -> ActionChain<'a> {
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
    wraps: &[Arc<TickWrap>],
    center: &Arc<TickCenter>,
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

fn box_tick_inner<'a>(wraps: &'a [Arc<TickWrap>], center: &'a Arc<TickCenter>) -> TickChain<'a> {
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
