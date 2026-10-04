//! 贡献槽执行器：把冻结计划的分派装配为可执行的处理链。
//!
//! 无附加处理器时直接调用内建 native；有贡献时按预编排链执行：
//! before 按依赖顺序，wrap 从外到内，中心为唯一 replace 或 native，
//! 成功后的 after 已在冻结时逆序。wrap 不调用 next 截止内层，
//! 但整槽成功返回仍执行 after；异常或 next 违规则不执行 after。
//! next 至多一次，重复调用使本轮候选失败（SettleError）。
//! 处理器键与实现由装配调用方提供；native 键固定指向内建实现。
//! 本执行器不捕获 panic，也不提供跨线程共享；候选仍由引擎统一回滚。

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use kk_mod_api::{ContributionPlan, Dispatch, Key, SlotOutcome, SlotPlan};
use kk_world::ObjId;
use serde_json::Value;

use super::{RoomCtx, Working};
use crate::intent::IntentData;

/// wrap 的续行句柄。`call` 至多一次；第二次调用返回错误，
/// 该错误沿处理链返回并使本轮结算候选失败。
///
/// 只用于 Rust 处理链；JS 的组合与续行由整槽宿主执行，不借此回入 realm。
pub struct Next<'a> {
    calls: u32,
    fault: Option<String>,
    inner: &'a mut (dyn FnMut(&RoomCtx, ObjId, &IntentData) -> Result<(), String> + 'a),
    ctx: &'a RoomCtx,
}

impl<'a> Next<'a> {
    /// 继续执行内层（剩余 wrap 或中心）；重复调用报错。
    ///
    /// 调用方在进入本方法前必须已释放自己对 `ctx` 的可变借用：内层链
    /// 会重新借用同一 `RefCell`，重入可变借用会 panic。
    pub fn call(&mut self, id: ObjId, d: &IntentData) -> Result<(), String> {
        self.calls = self.calls.saturating_add(1);
        if self.calls > 1 {
            let error = "wrap 处理器重复调用 next，本轮候选失败".to_string();
            self.fault = Some(error.clone());
            return Err(error);
        }
        let result = (self.inner)(self.ctx, id, d);
        if let Err(error) = &result {
            self.fault = Some(error.clone());
        }
        result
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
    fault: Option<String>,
    inner: &'a mut (dyn FnMut(&RoomCtx, ObjId) -> Result<(), String> + 'a),
    ctx: &'a RoomCtx,
}

impl<'a> TickNext<'a> {
    pub fn call(&mut self, id: ObjId) -> Result<(), String> {
        self.calls = self.calls.saturating_add(1);
        if self.calls > 1 {
            let error = "wrap 处理器重复调用 next，本轮候选失败".to_string();
            self.fault = Some(error.clone());
            return Err(error);
        }
        let result = (self.inner)(self.ctx, id);
        if let Err(error) = &result {
            self.fault = Some(error.clone());
        }
        result
    }

    pub fn called(&self) -> bool {
        self.calls > 0
    }
}

type ActionChain<'a> = Box<dyn FnMut(&RoomCtx, ObjId, &IntentData) -> Result<(), String> + 'a>;
type TickChain<'a> = Box<dyn FnMut(&RoomCtx, ObjId) -> Result<(), String> + 'a>;
/// before/after/replace 处理器同样可失败：JS 侧处理器的异常必须能使
/// 本轮候选失败，不能因执行器形状退化为 panic 或静默。
///
/// 处理器收 `&RoomCtx` 而非 `&mut Working`：处理器自行管理短借用——
/// 处理器自己取短借用，需要进入 JS 或调用 `next()` 时先释放，
/// 内层链才能重新借用同一候选。
type ActionSimple = Box<dyn Fn(&RoomCtx, ObjId, &IntentData) -> Result<(), String>>;
type ActionWrap = Box<dyn Fn(&RoomCtx, ObjId, &IntentData, &mut Next<'_>) -> Result<(), String>>;
type TickSimple = Box<dyn Fn(&RoomCtx, ObjId) -> Result<(), String>>;
type TickWrap = Box<dyn Fn(&RoomCtx, ObjId, &mut TickNext<'_>) -> Result<(), String>>;
/// 扩展意图槽的处理器签名：args 为按计划 schema 校验后的原始 JSON，
/// 目标对象读写由实现方经 Working 完成。
pub type ExtSimple = Box<dyn Fn(&RoomCtx, ObjId, &Value) -> Result<(), String>>;
/// 扩展意图槽的 wrap 处理器：`next` 续行到剩余 wrap 与中心。
pub type ExtWrap = Box<dyn Fn(&RoomCtx, ObjId, &Value, &mut ExtNext<'_>) -> Result<(), String>>;

/// 把内建 native 中心适配成处理器形态：取一次短借用后调用。
/// native 中心不回入 JS，单次借用安全。
fn native_action(
    f: fn(&mut Working, ObjId, &IntentData),
) -> impl Fn(&RoomCtx, ObjId, &IntentData) -> Result<(), String> {
    move |ctx, id, d| {
        f(&mut ctx.borrow_mut(), id, d);
        Ok(())
    }
}

fn native_tick(f: fn(&mut Working, ObjId)) -> impl Fn(&RoomCtx, ObjId) -> Result<(), String> {
    move |ctx, id| {
        f(&mut ctx.borrow_mut(), id);
        Ok(())
    }
}

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

/// 扩展意图槽的 wrap 续行句柄，语义与 [`Next`] 一致：至多调用一次。
pub struct ExtNext<'a> {
    calls: u32,
    fault: Option<String>,
    inner: &'a mut (dyn FnMut(&RoomCtx, ObjId, &Value) -> Result<(), String> + 'a),
    ctx: &'a RoomCtx,
    args: &'a Value,
}

impl<'a> ExtNext<'a> {
    /// 继续执行内层（剩余 wrap 与中心）；重复调用报错。
    /// 调用方在进入本方法前必须已释放对 `ctx` 的可变借用。
    pub fn call(&mut self, id: ObjId) -> Result<(), String> {
        self.calls = self.calls.saturating_add(1);
        if self.calls > 1 {
            let error = "wrap 处理器重复调用 next，本轮候选失败".to_string();
            self.fault = Some(error.clone());
            return Err(error);
        }
        let result = (self.inner)(self.ctx, id, self.args);
        if let Err(error) = &result {
            self.fault = Some(error.clone());
        }
        result
    }

    pub fn called(&self) -> bool {
        self.calls > 0
    }
}

type ExtInnerChain<'a> = Box<dyn FnMut(&RoomCtx, ObjId, &Value) -> Result<(), String> + 'a>;

/// 扩展槽 wrap 链：wraps[0] 最外层，中心最内；after 仅在内层成功后执行。
fn run_ext_wrapped(
    wraps: &[Rc<ExtWrap>],
    center: &Rc<ExtSimple>,
    after: &[Rc<ExtSimple>],
    ctx: &RoomCtx,
    id: ObjId,
    args: &Value,
) -> Result<(), String> {
    let result = box_ext_inner(wraps, center)(ctx, id, args);
    if result.is_ok() {
        for f in after {
            f(ctx, id, args)?;
        }
    }
    result
}

fn box_ext_inner<'a>(wraps: &'a [Rc<ExtWrap>], center: &'a Rc<ExtSimple>) -> ExtInnerChain<'a> {
    let Some((outer, rest)) = wraps.split_first() else {
        return Box::new(move |ctx, id, args| (center)(ctx, id, args));
    };
    let mut inner = box_ext_inner(rest, center);
    Box::new(move |ctx, id, args| {
        let mut next = ExtNext {
            calls: 0,
            fault: None,
            inner: &mut inner,
            ctx,
            args,
        };
        {
            let result = outer(ctx, id, args, &mut next);
            next.fault.map_or(result, Err)
        }
    })
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
    Wrap(ExtWrap),
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
///
/// 载荷经 `Rc` 共享，与 link 两槽同一形态。
enum ExtSlot {
    Process(Rc<ExtSimple>),
    Chained(Rc<ExtChained>),
    Hosted(HostedSlot),
}

struct ExtChained {
    before: Vec<Rc<ExtSimple>>,
    wraps: Vec<Rc<ExtWrap>>,
    center: Rc<ExtSimple>,
    /// 冻结时已按依赖逆序；执行器顺序调用，不再次反转。
    after: Vec<Rc<ExtSimple>>,
}

impl ExtSlot {
    fn run(&self, ctx: &RoomCtx, id: ObjId, args: &Value) -> Result<SlotOutcome, String> {
        match self {
            ExtSlot::Process(f) => f(ctx, id, args).map(|()| SlotOutcome::Completed),
            ExtSlot::Hosted(slot) => {
                slot.executor
                    .run(&slot.plan, ctx, SlotCall::Extension { id, args })
            }
            ExtSlot::Chained(chain) => {
                for f in &chain.before {
                    f(ctx, id, args)?;
                }
                run_ext_wrapped(&chain.wraps, &chain.center, &chain.after, ctx, id, args)
                    .map(|()| SlotOutcome::Completed)
            }
        }
    }
}

/// 一个槽的已装配 Rust 执行计划；载荷经 `Rc` 共享。
struct ActionChained {
    before: Vec<ActionSimple>,
    wraps: Vec<Rc<ActionWrap>>,
    center: Rc<ActionCenter>,
    /// 冻结时已按依赖逆序；执行器必须顺序调用，不能再次反转。
    after: Vec<ActionSimple>,
}

struct TickChained {
    before: Vec<TickSimple>,
    wraps: Vec<Rc<TickWrap>>,
    center: Rc<TickCenter>,
    after: Vec<TickSimple>,
}

enum ActionSlot {
    Native(fn(&mut Working, ObjId, &IntentData)),
    Chained(Rc<ActionChained>),
    Hosted(HostedSlot),
}

enum TickSlot {
    Native(fn(&mut Working, ObjId)),
    Chained(Rc<TickChained>),
    Hosted(HostedSlot),
}

/// 一次槽调用的权威参数。native 步骤只能修改当前候选，不得进入 JS。
/// 宿主在一次调用期间借用此描述；JS 的 next 不携带 Rust 续行闭包。
pub enum SlotCall<'a> {
    Action {
        id: ObjId,
        data: &'a IntentData,
        native: fn(&mut Working, ObjId, &IntentData),
    },
    Tick {
        id: ObjId,
        native: fn(&mut Working, ObjId),
    },
    Extension {
        id: ObjId,
        args: &'a Value,
    },
}

/// 整槽宿主执行接口：引擎只提供冻结计划、候选与权威参数，保持对 V8 无依赖。
/// 无扩展的 builtin 槽不会调用本接口；JS 宿主在自身 VM 内完成组合。
pub trait SlotExecutor {
    /// Declined 适用于扩展动作与 link.transfer。扩展动作由宿主恢复全部写入；
    /// link.transfer 的源/目标对象和新增事件由引擎恢复，宿主须丢弃暂存
    /// namespace state，且不得写入这个局部边界以外的候选。
    fn run(
        &self,
        slot: &SlotPlan,
        ctx: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<SlotOutcome, String>;
}

struct HostedSlot {
    plan: Rc<SlotPlan>,
    executor: Rc<dyn SlotExecutor>,
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
    /// 装配整槽宿主执行器。冻结计划中的可执行绑定必须由宿主逐项确认；
    /// 未扩展的 link 保留 native 直调，Mod 自有槽交给宿主执行 JS 中心。
    pub fn assemble_hosted(
        plan: &ContributionPlan,
        executor: Rc<dyn SlotExecutor>,
        bindings: &BTreeSet<Key>,
    ) -> Result<Self, String> {
        for slot in plan.slots() {
            let (before, wraps, center, after) = match &slot.dispatch {
                Dispatch::Native(center) => (&[][..], &[][..], center, &[][..]),
                Dispatch::Chained {
                    before,
                    wraps,
                    center,
                    after,
                } => (
                    before.as_slice(),
                    wraps.as_slice(),
                    center,
                    after.as_slice(),
                ),
            };
            for binding in before
                .iter()
                .chain(wraps)
                .chain(std::iter::once(center))
                .chain(after)
            {
                if binding.namespace() != "builtin" && !bindings.contains(binding) {
                    return Err(missing(slot.key.as_str(), binding, "宿主"));
                }
                if binding.namespace() == "builtin"
                    && !matches!(
                        (slot.key.as_str(), binding.as_str()),
                        ("builtin:link.transfer", "builtin:link.transfer.native")
                            | ("builtin:link.tick", "builtin:link.tick.native")
                    )
                {
                    return Err(format!("槽 {} 的内建绑定 {} 未接线", slot.key, binding));
                }
            }
        }
        let host = |slot: &str| -> Result<HostedSlot, String> {
            let handle = plan
                .resolve_slot(&key(slot))
                .ok_or_else(|| format!("计划缺少槽 {slot}"))?;
            Ok(HostedSlot {
                plan: Rc::new(plan.get_slot(handle).expect("句柄来自计划").clone()),
                executor: executor.clone(),
            })
        };
        let transfer = match dispatch_of(plan, LINK_SLOT_KEYS[0])? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:link.transfer.native" => {
                ActionSlot::Native(super::structures::link_transfer)
            }
            _ => ActionSlot::Hosted(host(LINK_SLOT_KEYS[0])?),
        };
        let tick = match dispatch_of(plan, LINK_SLOT_KEYS[1])? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:link.tick.native" => {
                TickSlot::Native(super::economy::link_cooldown_tick)
            }
            _ => TickSlot::Hosted(host(LINK_SLOT_KEYS[1])?),
        };
        let mut ext = BTreeMap::new();
        for slot in plan.slots() {
            if !LINK_SLOT_KEYS.contains(&slot.key.as_str()) {
                ext.insert(slot.key.clone(), ExtSlot::Hosted(host(slot.key.as_str())?));
            }
        }
        Ok(Self {
            transfer,
            tick,
            ext,
        })
    }

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
    /// 扩展槽的 wrap 处理器同样装配成链，语义与 link 槽一致（`next()`
    /// 至多一次，续行到剩余 wrap 与中心，成功后才跑 after）。装配门不在
    /// 此处：JS 宿主经 assemble_hosted 安装整槽执行器。
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
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        self.transfer.run(ctx, id, d)
    }

    pub(crate) fn run_tick(&self, ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
        self.tick.run(ctx, id)
    }

    /// 执行一个 Mod 自有槽的处理链。槽未装配说明计划与执行表不一致，
    /// 属装配期错误，按失败处理而不是跳过。
    pub(crate) fn run_ext(
        &self,
        slot: &Key,
        ctx: &RoomCtx,
        id: ObjId,
        args: &Value,
    ) -> Result<SlotOutcome, String> {
        let chain = self
            .ext
            .get(slot)
            .ok_or_else(|| format!("扩展意图槽 {slot} 没有已装配的执行器（计划与执行表不一致）"))?;
        chain.run(ctx, id, args)
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
                result.insert(slot.key.clone(), ExtSlot::Process(Rc::new(center)));
            }
            Dispatch::Chained {
                before,
                wraps,
                center,
                after,
            } => {
                let slot_label = slot.key.to_string();
                let mut before_fns = Vec::new();
                for binding in before {
                    match handlers.remove(binding) {
                        Some(ExtHandler::Before(f)) => before_fns.push(Rc::new(f)),
                        Some(other) => {
                            return Err(kind_mismatch(binding, "before", other.label()));
                        }
                        None => return Err(missing(&slot_label, binding, "before")),
                    }
                }
                // wrap 处理器在扩展槽同样装配：它们由宿主桥经
                // Rust 的局部续行执行；JS 组合经整槽宿主执行器。
                let mut wrap_fns = Vec::new();
                for binding in wraps.iter() {
                    match handlers.remove(binding) {
                        Some(ExtHandler::Wrap(f)) => wrap_fns.push(Rc::new(f)),
                        Some(other) => {
                            return Err(kind_mismatch(binding, "wrap", other.label()));
                        }
                        None => return Err(missing(&slot_label, binding, "wrap")),
                    }
                }
                let after_fns = take_after(&slot_label, after, &mut handlers)?;
                let center_fn = match handlers.remove(center) {
                    Some(ExtHandler::Replace(f)) => Rc::new(f),
                    Some(other) => {
                        return Err(kind_mismatch(center, "replace", other.label()));
                    }
                    None => Rc::new(
                        processes
                            .remove(center)
                            .ok_or_else(|| missing(&slot_label, center, "replace/动作处理"))?,
                    ),
                };
                result.insert(
                    slot.key.clone(),
                    ExtSlot::Chained(Rc::new(ExtChained {
                        before: before_fns,
                        wraps: wrap_fns,
                        center: center_fn,
                        after: after_fns,
                    })),
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
) -> Result<Vec<Rc<ExtSimple>>, String> {
    let mut after_fns = Vec::new();
    for binding in after {
        match handlers.remove(binding) {
            Some(ExtHandler::After(f)) => after_fns.push(Rc::new(f)),
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
            let slot_result = ActionSlot::Chained(Rc::new(ActionChained {
                before: before_fns,
                wraps: wrap_fns.into_iter().map(Rc::new).collect(),
                center: Rc::new(center_fn),
                after: after_fns,
            }));
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
            let slot_result = TickSlot::Chained(Rc::new(TickChained {
                before: before_fns,
                wraps: wrap_fns.into_iter().map(Rc::new).collect(),
                center: Rc::new(center_fn),
                after: after_fns,
            }));
            reject_leftover(slot, &handlers)?;
            Ok(slot_result)
        }
    }
}

impl ActionSlot {
    fn run(&self, ctx: &RoomCtx, id: ObjId, d: &IntentData) -> Result<(), String> {
        match self {
            ActionSlot::Native(f) => native_action(*f)(ctx, id, d),
            ActionSlot::Hosted(slot) => {
                // 本槽只有 link.transfer：native 改源/目标记录并追加事件，
                // JS 字段门禁只允许源 cooldown；不分配 ID、不改位置/玩家。
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
                        native: super::structures::link_transfer,
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
    fn run(&self, ctx: &RoomCtx, id: ObjId) -> Result<(), String> {
        match self {
            TickSlot::Native(f) => native_tick(*f)(ctx, id),
            TickSlot::Hosted(slot) => slot
                .executor
                .run(
                    &slot.plan,
                    ctx,
                    SlotCall::Tick {
                        id,
                        native: super::economy::link_cooldown_tick,
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

// 未接局部回滚的 native 槽不能把宿主 Declined 当成功，以免发布部分写入。
fn require_completed(outcome: SlotOutcome) -> Result<(), String> {
    match outcome {
        SlotOutcome::Completed => Ok(()),
        SlotOutcome::Declined(_) => Err("内建槽尚不支持正常拒绝".into()),
    }
}
