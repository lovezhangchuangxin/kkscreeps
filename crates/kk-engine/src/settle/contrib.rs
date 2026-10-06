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
use kk_schema::room::RoomObject;
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
/// 自有槽的处理器签名：动作 args 已按 schema 校验，生命周期为 null；
/// 目标对象读写由实现方经 Working 完成。
pub type ExtSimple = Box<dyn Fn(&RoomCtx, ObjId, &Value) -> Result<(), String>>;
/// 自有槽的 wrap 处理器：`next` 续行到剩余 wrap 与中心。
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
/// 载荷经 `Rc` 共享，与内建槽同一形态。
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
    Hosted {
        slot: HostedSlot,
        native: fn(&mut Working, ObjId, &IntentData),
    },
}

enum TickSlot {
    Native(fn(&mut Working, ObjId)),
    Chained(Rc<TickChained>),
    Hosted(HostedSlot),
}

/// 一次槽调用的权威参数。native 步骤只能修改当前候选，不得进入 JS。
/// 宿主在一次调用期间借用此描述；JS 的 next 不携带 Rust 续行闭包。
pub enum SlotCall<'a> {
    Incoming {
        id: ObjId,
        candidate: Rc<crate::settle::incoming::IncomingDamage>,
    },
    /// 房间级移动竞争：整房一次，无单一目标对象；native 中心为无操作
    /// （保留 before 设置的优先级）。
    Contest {
        candidate: Rc<crate::settle::contest::MoveContest>,
    },
    /// 房间级移动准入：整房一次，无单一目标对象；native 中心为无操作
    /// （保留候选中的原生准入终值）。
    Admission {
        candidate: Rc<crate::settle::admission::MoveAdmission>,
    },
    /// 房间级移动障碍：整房一次，无单一目标对象；native 中心为无操作
    /// （无追加阻挡）。
    Obstacle {
        candidate: Rc<crate::settle::obstacle::MoveObstacle>,
    },
    /// 房间级移动请求预检：整房一次，无单一目标对象；native 中心为
    /// 无操作（无否决）。
    Precheck {
        candidate: Rc<crate::settle::precheck::MovePrecheck>,
    },
    /// 房间级拉拽登记：整房一次，无单一目标对象；native 中心为无操作
    /// （无否决）。
    PullRegistration {
        candidate: Rc<crate::settle::pull_registration::PullRegistration>,
    },
    /// 房间级移动代价：整房一次，无单一目标对象；native 中心为无操作
    /// （无覆盖，执行原生代价）。
    MoveCost {
        candidate: Rc<crate::settle::move_cost::MoveCost>,
    },
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
    /// Declined 适用于扩展动作与内建资源动作。扩展动作由宿主恢复全部写入；
    /// 内建资源动作的源/目标对象和新增事件由引擎恢复，宿主须丢弃暂存
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

/// 内建与自有槽的执行表。默认实例直连 native；
/// [`ContributionDispatch::assemble`] 从冻结计划装配贡献链。
pub struct ContributionDispatch {
    transfer: ActionSlot,
    tick: TickSlot,
    creep_transfer: ActionSlot,
    creep_withdraw: ActionSlot,
    creep_move: ActionSlot,
    creep_attack: ActionSlot,
    creep_ranged_attack: ActionSlot,
    creep_ranged_mass_attack: ActionSlot,
    creep_heal: ActionSlot,
    creep_ranged_heal: ActionSlot,
    creep_build: ActionSlot,
    creep_repair: ActionSlot,
    creep_dismantle: ActionSlot,
    incoming: IncomingSlot,
    /// 房间级移动竞争优先级槽。
    move_contest: ContestSlot,
    /// 房间级移动准入终值槽。
    move_admission: AdmissionSlot,
    /// 房间级移动障碍追加槽。
    move_obstacle: ObstacleSlot,
    /// 房间级移动请求预检槽。
    move_precheck: PrecheckSlot,
    move_pull_registration: PullRegistrationSlot,
    move_cost: MoveCostSlot,
    /// Mod 自有槽（按槽键查）；扩展意图结算在此派发。
    ext: BTreeMap<Key, ExtSlot>,
}

impl Default for ContributionDispatch {
    fn default() -> Self {
        Self {
            transfer: ActionSlot::Native(super::structures::link_transfer),
            tick: TickSlot::Native(super::economy::link_cooldown_tick),
            creep_transfer: ActionSlot::Native(super::creep::intent_transfer),
            creep_withdraw: ActionSlot::Native(super::creep::intent_withdraw),
            creep_move: ActionSlot::Native(super::creep::intent_move),
            creep_attack: ActionSlot::Native(super::creep::intent_attack),
            creep_ranged_attack: ActionSlot::Native(super::creep::intent_ranged_attack),
            creep_ranged_mass_attack: ActionSlot::Native(super::creep::ranged_mass_native),
            creep_heal: ActionSlot::Native(super::creep::heal_melee_native),
            creep_ranged_heal: ActionSlot::Native(super::creep::heal_ranged_native),
            creep_build: ActionSlot::Native(super::creep::intent_build),
            creep_repair: ActionSlot::Native(super::creep::intent_repair),
            creep_dismantle: ActionSlot::Native(super::creep::intent_dismantle),
            incoming: IncomingSlot::Native,
            move_contest: ContestSlot::Native,
            move_admission: AdmissionSlot::Native,
            move_obstacle: ObstacleSlot::Native,
            move_precheck: PrecheckSlot::Native,
            move_pull_registration: PullRegistrationSlot::Native,
            move_cost: MoveCostSlot::Native,
            ext: BTreeMap::new(),
        }
    }
}

/// 保留 link 的既有公开键表；完整内建派发表取共享的 builtin 目录。
pub const LINK_SLOT_KEYS: [&str; 2] = [
    kk_mod_api::builtin::SLOT_KEYS[0],
    kk_mod_api::builtin::SLOT_KEYS[1],
];
pub const BUILTIN_SLOT_KEYS: [&str; 20] = kk_mod_api::builtin::SLOT_KEYS;

impl ContributionDispatch {
    pub(crate) fn run_incoming(&self, ctx: &RoomCtx, id: ObjId) -> Result<Option<i64>, String> {
        self.incoming.run(ctx, id)
    }
    /// 运行移动竞争优先级槽；见 [`ContestSlot::run`] 的返回语义。
    pub(crate) fn run_contest(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<BTreeMap<ObjId, i64>>, String> {
        self.move_contest.run(ctx)
    }

    /// 运行移动准入槽；见 [`AdmissionSlot::run`] 的返回语义。
    pub(crate) fn run_admission(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<BTreeMap<ObjId, bool>>, String> {
        self.move_admission.run(ctx)
    }

    /// 运行移动障碍槽；见 [`ObstacleSlot::run`] 的返回语义。
    pub(crate) fn run_obstacle(&self, ctx: &RoomCtx) -> Result<Option<BTreeSet<ObjId>>, String> {
        self.move_obstacle.run(ctx)
    }

    /// 运行移动请求预检槽；成功返回否决集（调用方在竞争输入构造前
    /// 应用），Native 或拒绝返回 None。
    pub(crate) fn run_precheck(&self, ctx: &RoomCtx) -> Result<Option<BTreeSet<ObjId>>, String> {
        self.move_precheck.run(ctx)
    }

    /// 运行拉拽登记槽；见 [`PullRegistrationSlot::run`] 的返回语义。
    pub(crate) fn run_pull_registration(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<BTreeSet<ObjId>>, String> {
        self.move_pull_registration.run(ctx)
    }

    /// 运行移动代价槽；见 [`MoveCostSlot::run`] 的返回语义。
    pub(crate) fn run_move_cost(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<BTreeMap<ObjId, u32>>, String> {
        self.move_cost.run(ctx)
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

    pub(crate) fn run_creep_transfer(
        &self,
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        self.creep_transfer.run(ctx, id, d)
    }

    pub(crate) fn run_creep_withdraw(
        &self,
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        self.creep_withdraw.run(ctx, id, d)
    }

    pub(crate) fn run_creep_move(
        &self,
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        self.creep_move.run(ctx, id, d)
    }

    /// 五个战斗槽共用一个入口：按槽键查表后执行，避免逐槽复制运行方法。
    pub(crate) fn run_combat(
        &self,
        slot_key: &str,
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        let slot = match slot_key {
            "builtin:creep.attack" => &self.creep_attack,
            "builtin:creep.rangedAttack" => &self.creep_ranged_attack,
            "builtin:creep.rangedMassAttack" => &self.creep_ranged_mass_attack,
            "builtin:creep.heal" => &self.creep_heal,
            "builtin:creep.rangedHeal" => &self.creep_ranged_heal,
            other => return Err(format!("未知战斗槽 {other}")),
        };
        slot.run(ctx, id, d)
    }

    /// 三个建造族槽共用一个入口，与战斗族同型。
    pub(crate) fn run_build(
        &self,
        slot_key: &str,
        ctx: &RoomCtx,
        id: ObjId,
        d: &IntentData,
    ) -> Result<(), String> {
        let slot = match slot_key {
            "builtin:creep.build" => &self.creep_build,
            "builtin:creep.repair" => &self.creep_repair,
            "builtin:creep.dismantle" => &self.creep_dismantle,
            other => return Err(format!("未知建造族槽 {other}")),
        };
        slot.run(ctx, id, d)
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
mod admission;
mod assemble;
mod contest;
mod execute;
mod incoming;
mod move_cost;
mod obstacle;
mod precheck;
mod pull_registration;
use admission::AdmissionSlot;
use contest::ContestSlot;
use incoming::IncomingSlot;
use move_cost::MoveCostSlot;
use obstacle::ObstacleSlot;
use precheck::PrecheckSlot;
use pull_registration::PullRegistrationSlot;
