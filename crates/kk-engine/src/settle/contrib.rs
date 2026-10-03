//! 贡献槽执行器：把冻结计划的分派装配为可执行的处理链。
//!
//! 无附加处理器时直接调用内建 native；有贡献时按预编排链执行：
//! before 按依赖顺序，wrap 从外到内，中心为唯一 replace 或 native，
//! 成功后的 after 已在冻结时逆序。wrap 不调用 next 即截止后续；
//! next 至多一次，重复调用使本轮候选失败（SettleError）。
//! 处理器键与实现由装配调用方提供；native 键固定指向内建实现。
//! 本执行器不捕获 panic，也不提供跨线程共享；候选仍由引擎统一回滚。

use std::collections::BTreeMap;

use kk_mod_api::{ContributionPlan, Dispatch, Key};
use kk_world::ObjId;

use super::Working;
use crate::intent::IntentData;

/// wrap 的续行句柄。`call` 至多一次；第二次调用返回错误，
/// 该错误沿处理链返回并使本轮结算候选失败。
pub struct Next<'a> {
    calls: u32,
    inner: &'a mut (dyn FnMut(&mut Working, ObjId, &IntentData) -> Result<(), String> + 'a),
}

impl Next<'_> {
    /// 继续执行内层（剩余 wrap 或中心）；重复调用报错。
    pub fn call(&mut self, w: &mut Working, id: ObjId, d: &IntentData) -> Result<(), String> {
        self.calls += 1;
        if self.calls > 1 {
            return Err("wrap 处理器重复调用 next，本轮候选失败".into());
        }
        (self.inner)(w, id, d)
    }

    /// 本层内层是否已被调用过。注意这只反映直接内层：若内层 wrap
    /// 自身吞掉 next，本方法为真而中心并未执行。
    pub fn called(&self) -> bool {
        self.calls > 0
    }
}

/// 对象 tick 槽的 wrap 续行句柄，语义与 [`Next`] 一致。
pub struct TickNext<'a> {
    calls: u32,
    inner: &'a mut (dyn FnMut(&mut Working, ObjId) -> Result<(), String> + 'a),
}

impl TickNext<'_> {
    pub fn call(&mut self, w: &mut Working, id: ObjId) -> Result<(), String> {
        self.calls += 1;
        if self.calls > 1 {
            return Err("wrap 处理器重复调用 next，本轮候选失败".into());
        }
        (self.inner)(w, id)
    }

    pub fn called(&self) -> bool {
        self.calls > 0
    }
}

type ActionChain<'a> = Box<dyn FnMut(&mut Working, ObjId, &IntentData) -> Result<(), String> + 'a>;
type TickChain<'a> = Box<dyn FnMut(&mut Working, ObjId) -> Result<(), String> + 'a>;
/// before/after/replace 处理器同样可失败：JS 侧处理器的异常必须能使
/// 本轮候选失败，不能因执行器形状退化为 panic 或静默。
type ActionSimple = Box<dyn Fn(&mut Working, ObjId, &IntentData) -> Result<(), String>>;
type ActionWrap =
    Box<dyn Fn(&mut Working, ObjId, &IntentData, &mut Next<'_>) -> Result<(), String>>;
type TickSimple = Box<dyn Fn(&mut Working, ObjId) -> Result<(), String>>;
type TickWrap = Box<dyn Fn(&mut Working, ObjId, &mut TickNext<'_>) -> Result<(), String>>;

/// 调用方安装的自定义处理器；变体须与描述声明的 kind 一致，错配在装配时拒绝。
pub enum ActionHandler {
    Before(ActionSimple),
    Wrap(ActionWrap),
    Replace(ActionSimple),
    After(ActionSimple),
}

pub enum TickHandler {
    Before(TickSimple),
    Wrap(TickWrap),
    Replace(TickSimple),
    After(TickSimple),
}

enum ActionCenter {
    Native(fn(&mut Working, ObjId, &IntentData)),
    Custom(ActionSimple),
}

enum TickCenter {
    Native(fn(&mut Working, ObjId)),
    Custom(TickSimple),
}

/// 一个槽的已装配执行计划。
enum ActionSlot {
    Native(fn(&mut Working, ObjId, &IntentData)),
    Chained {
        before: Vec<ActionSimple>,
        wraps: Vec<ActionWrap>,
        center: ActionCenter,
        /// 冻结时已按依赖逆序；执行器必须顺序调用，不能再次反转。
        after: Vec<ActionSimple>,
    },
}

enum TickSlot {
    Native(fn(&mut Working, ObjId)),
    Chained {
        before: Vec<TickSimple>,
        wraps: Vec<TickWrap>,
        center: TickCenter,
        after: Vec<TickSimple>,
    },
}

/// link 两个槽的执行表。默认实例直连 native；
/// [`ContributionDispatch::assemble`] 从冻结计划装配贡献链。
pub struct ContributionDispatch {
    transfer: ActionSlot,
    tick: TickSlot,
}

impl Default for ContributionDispatch {
    fn default() -> Self {
        Self {
            transfer: ActionSlot::Native(super::structures::link_transfer),
            tick: TickSlot::Native(super::economy::link_cooldown_tick),
        }
    }
}

impl ContributionDispatch {
    /// 按冻结计划装配执行链。native 键固定指向内建实现；
    /// 处理器键必须在 `action_handlers` / `tick_handlers` 中提供，
    /// kind 与描述不符、或键缺失时装配失败。
    pub fn assemble(
        plan: &ContributionPlan,
        action_handlers: BTreeMap<Key, ActionHandler>,
        tick_handlers: BTreeMap<Key, TickHandler>,
    ) -> Result<Self, String> {
        let transfer = assemble_action(
            plan,
            "builtin:link.transfer",
            "builtin:link.transfer.native",
            action_handlers,
        )?;
        let tick = assemble_tick(
            plan,
            "builtin:link.tick",
            "builtin:link.tick.native",
            tick_handlers,
        )?;
        Ok(Self { transfer, tick })
    }

    pub(crate) fn run_transfer(
        &self,
        w: &mut Working,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        self.transfer.run(w, id, d)
    }

    pub(crate) fn run_tick(&self, w: &mut Working, id: ObjId) -> Result<(), String> {
        self.tick.run(w, id)
    }
}

fn key(value: &str) -> Key {
    Key::new(value).expect("内建槽键合法")
}

fn dispatch_of(plan: &ContributionPlan, slot: &str) -> Result<Dispatch, String> {
    let handle = plan
        .resolve_slot(&key(slot))
        .ok_or_else(|| format!("计划缺少槽 {slot}"))?;
    Ok(plan
        .get_slot(handle)
        .expect("句柄来自计划")
        .dispatch
        .clone())
}

fn missing(slot: &str, binding: &Key, kind: &str) -> String {
    format!(
        "槽 {slot} 的处理器 {binding} 声明为 {kind}，但装配未提供该实现（或同一 binding 被多个处理器共用导致二次消耗）"
    )
}

/// 计划未引用的处理器实现是装配调用方的键名错误，静默丢弃会掩盖它。
fn reject_leftover<T>(slot: &str, handlers: &BTreeMap<Key, T>) -> Result<(), String> {
    if let Some(binding) = handlers.keys().next() {
        return Err(format!(
            "装配提供了计划未引用的处理器 {binding}（槽 {slot}）"
        ));
    }
    Ok(())
}

fn kind_mismatch(binding: &Key, want: &str, got: &str) -> String {
    format!("处理器 {binding} 声明 kind 为 {want}，装配提供的实现是 {got}")
}

fn assemble_action(
    plan: &ContributionPlan,
    slot: &str,
    native_binding: &str,
    mut handlers: BTreeMap<Key, ActionHandler>,
) -> Result<ActionSlot, String> {
    let native = key(native_binding);
    match dispatch_of(plan, slot)? {
        Dispatch::Native(binding) => {
            if binding != native {
                return Err(format!(
                    "槽 {slot} 的 native 绑定须为 {native_binding}，实得 {binding}"
                ));
            }
            Ok(ActionSlot::Native(super::structures::link_transfer))
        }
        Dispatch::Chained {
            before,
            wraps,
            center,
            after,
        } => {
            let mut before_fns = Vec::new();
            for binding in &before {
                match handlers.remove(binding) {
                    Some(ActionHandler::Before(f)) => before_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "before", other.label())),
                    None => return Err(missing(slot, binding, "before")),
                }
            }
            let mut wrap_fns = Vec::new();
            for binding in &wraps {
                match handlers.remove(binding) {
                    Some(ActionHandler::Wrap(f)) => wrap_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "wrap", other.label())),
                    None => return Err(missing(slot, binding, "wrap")),
                }
            }
            let center_fn = if center == native {
                ActionCenter::Native(super::structures::link_transfer)
            } else {
                match handlers.remove(&center) {
                    Some(ActionHandler::Replace(f)) => ActionCenter::Custom(f),
                    Some(other) => {
                        return Err(kind_mismatch(&center, "replace", other.label()));
                    }
                    None => return Err(missing(slot, &center, "replace")),
                }
            };
            let mut after_fns = Vec::new();
            for binding in &after {
                match handlers.remove(binding) {
                    Some(ActionHandler::After(f)) => after_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
                    None => return Err(missing(slot, binding, "after")),
                }
            }
            let slot_result = ActionSlot::Chained {
                before: before_fns,
                wraps: wrap_fns,
                center: center_fn,
                after: after_fns,
            };
            reject_leftover(slot, &handlers)?;
            Ok(slot_result)
        }
    }
}

fn assemble_tick(
    plan: &ContributionPlan,
    slot: &str,
    native_binding: &str,
    mut handlers: BTreeMap<Key, TickHandler>,
) -> Result<TickSlot, String> {
    let native = key(native_binding);
    match dispatch_of(plan, slot)? {
        Dispatch::Native(binding) => {
            if binding != native {
                return Err(format!(
                    "槽 {slot} 的 native 绑定须为 {native_binding}，实得 {binding}"
                ));
            }
            Ok(TickSlot::Native(super::economy::link_cooldown_tick))
        }
        Dispatch::Chained {
            before,
            wraps,
            center,
            after,
        } => {
            let mut before_fns = Vec::new();
            for binding in &before {
                match handlers.remove(binding) {
                    Some(TickHandler::Before(f)) => before_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "before", other.label())),
                    None => return Err(missing(slot, binding, "before")),
                }
            }
            let mut wrap_fns = Vec::new();
            for binding in &wraps {
                match handlers.remove(binding) {
                    Some(TickHandler::Wrap(f)) => wrap_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "wrap", other.label())),
                    None => return Err(missing(slot, binding, "wrap")),
                }
            }
            let center_fn = if center == native {
                TickCenter::Native(super::economy::link_cooldown_tick)
            } else {
                match handlers.remove(&center) {
                    Some(TickHandler::Replace(f)) => TickCenter::Custom(f),
                    Some(other) => {
                        return Err(kind_mismatch(&center, "replace", other.label()));
                    }
                    None => return Err(missing(slot, &center, "replace")),
                }
            };
            let mut after_fns = Vec::new();
            for binding in &after {
                match handlers.remove(binding) {
                    Some(TickHandler::After(f)) => after_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
                    None => return Err(missing(slot, binding, "after")),
                }
            }
            let slot_result = TickSlot::Chained {
                before: before_fns,
                wraps: wrap_fns,
                center: center_fn,
                after: after_fns,
            };
            reject_leftover(slot, &handlers)?;
            Ok(slot_result)
        }
    }
}

impl ActionSlot {
    fn run(&self, w: &mut Working, id: ObjId, d: &IntentData) -> Result<(), String> {
        match self {
            ActionSlot::Native(f) => {
                f(w, id, d);
                Ok(())
            }
            ActionSlot::Chained {
                before,
                wraps,
                center,
                after,
            } => {
                for f in before {
                    f(w, id, d)?;
                }
                let result = run_wrapped(wraps, center, w, id, d);
                if result.is_ok() {
                    for f in after {
                        f(w, id, d)?;
                    }
                }
                result
            }
        }
    }
}

impl TickSlot {
    fn run(&self, w: &mut Working, id: ObjId) -> Result<(), String> {
        match self {
            TickSlot::Native(f) => {
                f(w, id);
                Ok(())
            }
            TickSlot::Chained {
                before,
                wraps,
                center,
                after,
            } => {
                for f in before {
                    f(w, id)?;
                }
                let result = run_tick_wrapped(wraps, center, w, id);
                if result.is_ok() {
                    for f in after {
                        f(w, id)?;
                    }
                }
                result
            }
        }
    }
}

/// wraps[0] 最外层，中心最内；从中心向外逐层装箱。
fn run_wrapped(
    wraps: &[ActionWrap],
    center: &ActionCenter,
    w: &mut Working,
    id: ObjId,
    d: &IntentData,
) -> Result<(), String> {
    let Some((outer, rest)) = wraps.split_first() else {
        return match center {
            ActionCenter::Native(f) => {
                f(w, id, d);
                Ok(())
            }
            ActionCenter::Custom(f) => f(w, id, d),
        };
    };
    let mut inner = box_inner(rest, center);
    let mut next = Next {
        calls: 0,
        inner: &mut inner,
    };
    outer(w, id, d, &mut next)
}

/// 递归装箱内层链；每层 wrap 通过 [`Next`] 恰好调用一次内层。
fn box_inner<'a>(wraps: &'a [ActionWrap], center: &'a ActionCenter) -> ActionChain<'a> {
    let Some((outer, rest)) = wraps.split_first() else {
        return Box::new(
            move |w: &mut Working, id: ObjId, d: &IntentData| match center {
                ActionCenter::Native(f) => {
                    f(w, id, d);
                    Ok(())
                }
                ActionCenter::Custom(f) => f(w, id, d),
            },
        );
    };
    let mut inner = box_inner(rest, center);
    Box::new(
        move |w: &mut Working, id: ObjId, d: &IntentData| -> Result<(), String> {
            let mut next = Next {
                calls: 0,
                inner: &mut inner,
            };
            outer(w, id, d, &mut next)
        },
    )
}

fn run_tick_wrapped(
    wraps: &[TickWrap],
    center: &TickCenter,
    w: &mut Working,
    id: ObjId,
) -> Result<(), String> {
    let Some((outer, rest)) = wraps.split_first() else {
        return match center {
            TickCenter::Native(f) => {
                f(w, id);
                Ok(())
            }
            TickCenter::Custom(f) => f(w, id),
        };
    };
    let mut inner = box_tick_inner(rest, center);
    let mut next = TickNext {
        calls: 0,
        inner: &mut inner,
    };
    outer(w, id, &mut next)
}

fn box_tick_inner<'a>(wraps: &'a [TickWrap], center: &'a TickCenter) -> TickChain<'a> {
    let Some((outer, rest)) = wraps.split_first() else {
        return Box::new(move |w: &mut Working, id: ObjId| match center {
            TickCenter::Native(f) => {
                f(w, id);
                Ok(())
            }
            TickCenter::Custom(f) => f(w, id),
        });
    };
    let mut inner = box_tick_inner(rest, center);
    Box::new(move |w: &mut Working, id: ObjId| -> Result<(), String> {
        let mut next = TickNext {
            calls: 0,
            inner: &mut inner,
        };
        outer(w, id, &mut next)
    })
}

impl ActionHandler {
    fn label(&self) -> &'static str {
        match self {
            ActionHandler::Before(_) => "before",
            ActionHandler::Wrap(_) => "wrap",
            ActionHandler::Replace(_) => "replace",
            ActionHandler::After(_) => "after",
        }
    }
}

impl TickHandler {
    fn label(&self) -> &'static str {
        match self {
            TickHandler::Before(_) => "before",
            TickHandler::Wrap(_) => "wrap",
            TickHandler::Replace(_) => "replace",
            TickHandler::After(_) => "after",
        }
    }
}
