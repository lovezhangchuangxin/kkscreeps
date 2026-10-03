//! 贡献槽执行器：把冻结计划的分派装配为可执行的处理链。
//!
//! 无附加处理器时直接调用内建 native；有贡献时按预编排链执行：
//! before 按依赖顺序，wrap 从外到内，中心为唯一 replace 或 native，
//! 成功后的 after 已在冻结时逆序。wrap 不调用 next 即截止后续；
//! next 至多一次，重复调用使本轮候选失败（SettleError）。
//! 处理器键与实现由装配调用方提供；native 键固定指向内建实现。
//! 本执行器不捕获 panic，也不提供跨线程共享；候选仍由引擎统一回滚。

use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::{ContributionPlan, Dispatch, Key};
use kk_world::ObjId;
use serde_json::Value;

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
/// 扩展意图槽的处理器签名：args 为按计划 schema 校验后的原始 JSON，
/// 目标对象读写由实现方经 Working 完成。
pub type ExtSimple = Box<dyn Fn(&mut Working, ObjId, &Value) -> Result<(), String>>;

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

/// Mod 自有槽贡献处理器的安装形态；变体须与描述声明的 kind 一致。
pub enum ExtHandler {
    Before(ExtSimple),
    Wrap(ExtSimple),
    Replace(ExtSimple),
    After(ExtSimple),
}

impl ExtHandler {
    fn label(&self) -> &'static str {
        match self {
            ExtHandler::Before(_) => "before",
            ExtHandler::Wrap(_) => "wrap",
            ExtHandler::Replace(_) => "replace",
            ExtHandler::After(_) => "after",
        }
    }
}

/// 扩展意图槽的已装配执行计划。中心恒为可执行实现：动作处理桥
/// （描述符的 native 形态）或 replace 处理器。
enum ExtSlot {
    Process(ExtSimple),
    Chained {
        before: Vec<ExtSimple>,
        center: ExtSimple,
        /// 冻结时已按依赖逆序；执行器顺序调用，不再次反转。
        after: Vec<ExtSimple>,
    },
}

impl ExtSlot {
    fn run(&self, w: &mut Working, id: ObjId, args: &Value) -> Result<(), String> {
        match self {
            ExtSlot::Process(f) => f(w, id, args),
            ExtSlot::Chained {
                before,
                center,
                after,
            } => {
                for f in before {
                    f(w, id, args)?;
                }
                center(w, id, args)?;
                for f in after {
                    f(w, id, args)?;
                }
                Ok(())
            }
        }
    }
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
    /// Mod 自有槽（按槽键查）；扩展意图结算在此派发。
    ext: BTreeMap<Key, ExtSlot>,
}

impl Default for ContributionDispatch {
    fn default() -> Self {
        Self {
            transfer: ActionSlot::Native(super::structures::link_transfer),
            tick: TickSlot::Native(super::economy::link_cooldown_tick),
            ext: BTreeMap::new(),
        }
    }
}

/// link 两槽的既有键；装配扩展槽时跳过（它们由 Rust native 执行器消费）。
/// 宿主桥装配（kk-runtime）复用同一常量，两侧槽清单不得漂移。
pub const LINK_SLOT_KEYS: [&str; 2] = ["builtin:link.transfer", "builtin:link.tick"];

impl ContributionDispatch {
    /// 按冻结计划装配执行链。native 键固定指向内建实现；
    /// 处理器键必须在 `action_handlers` / `tick_handlers` 中提供，
    /// kind 与描述不符、或键缺失时装配失败。
    pub fn assemble(
        plan: &ContributionPlan,
        action_handlers: BTreeMap<Key, ActionHandler>,
        tick_handlers: BTreeMap<Key, TickHandler>,
    ) -> Result<Self, String> {
        Self::assemble_with_ext(
            plan,
            action_handlers,
            tick_handlers,
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    /// [`Self::assemble`] 的完整形态：`ext_handlers` 是 Mod 自有槽上的
    /// before/wrap/replace/after 贡献处理器，`ext_processes` 是各槽中心
    /// 的动作处理实现（键为槽的 native 绑定，通常是 `{intent}.process`）。
    /// wrap 在扩展槽同样不支持执行装配，非空即拒。
    pub fn assemble_with_ext(
        plan: &ContributionPlan,
        action_handlers: BTreeMap<Key, ActionHandler>,
        tick_handlers: BTreeMap<Key, TickHandler>,
        ext_handlers: BTreeMap<Key, ExtHandler>,
        ext_processes: BTreeMap<Key, ExtSimple>,
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
        let ext = assemble_ext_slots(plan, ext_handlers, ext_processes)?;
        Ok(Self {
            transfer,
            tick,
            ext,
        })
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

    /// 执行一个 Mod 自有槽的处理链。槽未装配说明计划与执行表不一致，
    /// 属装配期错误，按失败处理而不是跳过。
    pub(crate) fn run_ext(
        &self,
        slot: &Key,
        w: &mut Working,
        id: ObjId,
        args: &Value,
    ) -> Result<(), String> {
        let chain = self
            .ext
            .get(slot)
            .ok_or_else(|| format!("扩展意图槽 {slot} 没有已装配的执行器（计划与执行表不一致）"))?;
        chain.run(w, id, args)
    }
}

/// 装配计划中全部非 link 槽的扩展执行链。每个槽的中心取其
/// native 绑定的动作处理实现；贡献处理器按冻结链位接入。
/// 冻结后的 SlotPlan 不保留 native 绑定：Native 分派的绑定即中心，
/// Chained 的中心在未被 replace 占位时等于 native 绑定。
fn assemble_ext_slots(
    plan: &ContributionPlan,
    mut handlers: BTreeMap<Key, ExtHandler>,
    mut processes: BTreeMap<Key, ExtSimple>,
) -> Result<BTreeMap<Key, ExtSlot>, String> {
    let mut result = BTreeMap::new();
    for slot in plan.slots() {
        if LINK_SLOT_KEYS.contains(&slot.key.as_str()) {
            continue;
        }
        match &slot.dispatch {
            Dispatch::Native(binding) => {
                let center = processes
                    .remove(binding)
                    .ok_or_else(|| missing(&slot.key.to_string(), binding, "动作处理"))?;
                result.insert(slot.key.clone(), ExtSlot::Process(center));
            }
            Dispatch::Chained {
                before,
                wraps,
                center,
                after,
            } => {
                if !wraps.is_empty() {
                    return Err(format!(
                        "槽 {} 的 wrap 处理器不支持执行装配（拒绝而不是静默截断链）",
                        slot.key
                    ));
                }
                let slot_label = slot.key.to_string();
                let mut before_fns = Vec::new();
                for binding in before {
                    match handlers.remove(binding) {
                        Some(ExtHandler::Before(f)) => before_fns.push(f),
                        Some(other) => {
                            return Err(kind_mismatch(binding, "before", other.label()));
                        }
                        None => return Err(missing(&slot_label, binding, "before")),
                    }
                }
                let after_fns = take_after(&slot_label, after, &mut handlers)?;
                let center_fn = match handlers.remove(center) {
                    Some(ExtHandler::Replace(f)) => f,
                    Some(other) => {
                        return Err(kind_mismatch(center, "replace", other.label()));
                    }
                    None => processes
                        .remove(center)
                        .ok_or_else(|| missing(&slot_label, center, "replace/动作处理"))?,
                };
                result.insert(
                    slot.key.clone(),
                    ExtSlot::Chained {
                        before: before_fns,
                        center: center_fn,
                        after: after_fns,
                    },
                );
            }
        }
    }
    // 计划未引用的键是装配调用方的键名错误；静默丢弃会掩盖它。
    if let Some(binding) = handlers.keys().next() {
        return Err(format!("装配提供了计划未引用的扩展处理器 {binding}"));
    }
    // 动作处理实现的残余只有一种合法形态：意图自身的 process 中心被
    // Replace 处理器占位（声明绑定仍要求安装、执行不再经它）。按此推导
    // 允许集，其余残余（含挂在字段/声明性绑定键上的杂项实现）一律拒绝。
    let mut replaced_processes = BTreeSet::new();
    for intent in plan.intents() {
        if intent.key.namespace() == "builtin" {
            continue; // builtin 意图的派生键不构成合法残余
        }
        let Ok(own) = Key::new(format!("{}.process", intent.key)) else {
            continue;
        };
        let Some(handle) = plan.resolve_slot(&intent.slot) else {
            continue;
        };
        let Some(slot) = plan.get_slot(handle) else {
            continue;
        };
        let center = match &slot.dispatch {
            Dispatch::Native(binding) => binding,
            Dispatch::Chained { center, .. } => center,
        };
        if center != &own {
            replaced_processes.insert(own);
        }
    }
    for binding in processes.keys() {
        if !replaced_processes.contains(binding) {
            return Err(format!(
                "装配提供了未被任何入口消费的动作处理实现 {binding}（键名错误或挂错通道）"
            ));
        }
    }
    Ok(result)
}

fn take_after(
    slot: &str,
    after: &[Key],
    handlers: &mut BTreeMap<Key, ExtHandler>,
) -> Result<Vec<ExtSimple>, String> {
    let mut after_fns = Vec::new();
    for binding in after {
        match handlers.remove(binding) {
            Some(ExtHandler::After(f)) => after_fns.push(f),
            Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
            None => return Err(missing(slot, binding, "after")),
        }
    }
    Ok(after_fns)
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
