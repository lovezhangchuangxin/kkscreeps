//! 房间规则结算与世界 tick 的协调。
//!
//! 房间先处理指令，再仲裁移动，最后按对象 ID 执行 tick 规则。
//! Engine 在副本中结算所有房间和市场，校验成功后统一写回内存并推进
//! 时间；直接调用 settle_room 则由调用方负责副本和失败处理。

pub mod buildings;
pub mod checkpoint;
pub mod contrib;
pub mod controller;
pub mod creep;
pub mod damage;
pub mod economy;
pub mod factories;
pub mod incoming;
pub mod inter_room;
pub mod labs;
pub mod market;
mod mod_intents;
pub mod movement;
// 兼容旧调用路径，领域实现只维护在 movement 中。
pub use movement::{admission, contest, move_cost, obstacle, precheck, pull_registration};
pub mod objects;
pub mod resources;
pub mod room;
pub mod spawn;
pub mod structures;

mod pipeline;
pub use pipeline::{
    PlayerWriteback, RunState, SegmentWriteback, TickStage, TickState, WritebackStep,
};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, RoomIndex, RoomState};
use serde_json::{Value, json};

use crate::intent::{Action, IntentBatch, IntentData};

/// 检查控制器是否存在、等级是否至少为 1，以及是否属于结构的主人。
/// 无主结构是否启用由调用方决定；本函数不检查结构数量配额。
/// 来源：上游 engine/src/utils.js 的 checkStructureAgainstController。
pub(crate) fn structure_rcl_prologue<'a>(
    w: &'a Working,
    owner: &str,
) -> Option<&'a kk_schema::room::ControllerObj> {
    let ctl = w.controller()?;
    if ctl.level < 1 || ctl.owner.as_deref() != Some(owner) {
        return None;
    }
    Some(ctl)
}

/// 查询给定 RCL 允许的结构数量；未知类型或等级返回 0。
pub fn controller_allowed(structure_type: &str, rcl: i64) -> i64 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<serde_json::Value> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str(kk_core::constants::CONTROLLER_STRUCTURES)
            .expect("CONTROLLER_STRUCTURES 常量必为合法 JSON（kk-core 生成物测试锁定）")
    });
    table[structure_type][rcl.to_string()].as_i64().unwrap_or(0)
}

/// 规则结算错误。Engine 丢弃候选；直接调用 settle_room 的调用方须自行丢弃副本。
#[derive(Debug, Clone)]
pub struct SettleError(pub String);

impl std::fmt::Display for SettleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "结算失败: {}", self.0)
    }
}

/// 跨 tick 保留的玩家内存状态。
///
/// rooms 在全部房间结算后按控制器所有权重建；结算期间仍是上一 tick 的集合。
/// 因此同 tick 的后一次 claim 看不到前一次新占的房间。
/// 来源：上游 engine/src/processor/intents/creeps/claimController.js
/// 与 driver/lib/index.js 的 addRoomToUser（排队写入，未修改运行中的用户对象）。
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerState {
    /// 已提交 CPU 桶；执行预算与计费候选从此值派生，随世界发布。
    pub cpu: kk_core::cpu::Bucket,
    /// 累计 GCL 进度，不是玩家 API 显示的 GCL 等级。
    pub gcl: i64,
    pub rooms: BTreeSet<String>,
    /// 玩家 RawMemory。运行结果在候选中合并，随世界发布；下一次执行时注入。
    pub raw_memory: String,
    /// Game.notify 的内存记录，跨 tick 保留最近 100 条；这里不发送邮件。
    pub notifications: Vec<NotifyRecord>,
    /// RawMemory 段内容。执行器限制 ID 为 0..=99，每段最多 100×1024 个
    /// UTF-16 码元（JavaScript 字符串 length）。Map 不自行校验；
    /// PlayerWriteback 在候选中合并段内容及请求。
    pub segments: BTreeMap<u8, String>,
    /// 最近一次 setActiveSegments 请求，跨 tick 保留；空集表示空活动窗口。
    pub active_request: Vec<u8>,
    /// 公开段 ID。正常执行通道不提交空列表，已有公开列表因而保留。
    /// 直接由宿主修改此字段不会经过该请求规则。
    pub public_ids: Vec<u8>,
    /// 默认公开段 ID；None 表示未设置或已清除。
    pub default_public: Option<u8>,
    /// 外部段请求：发布者与可选 ID；省略 ID 时使用其默认公开段。
    /// kk-runtime 每轮按当前公开列表重新授权，撤回公开后停止提供数据。
    pub foreign_request: Option<(String, Option<u8>)>,
    /// 余额，单位千分之一 credit；玩家 API 读取时除以 1000。
    /// Default 的余额为 0，初始资金由调用方设置。
    pub money: i64,
    /// token、pixel 等跨 shard 资源余额，供市场成交划转；缺省为空。
    pub resources: BTreeMap<String, i64>,
}

/// 一条已提交的内存通知；tick 是处理指令时的世界时间。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotifyRecord {
    pub message: String,
    pub group_interval: Option<i64>,
    pub tick: u64,
}

/// 内存通知最多保留最近 100 条。
const NOTIFY_RECORD_CAP: usize = 100;

/// 本轮结算的外部输入；控制器签名与市场订单时间使用此墙钟。
/// 重试或重放须复用首次尝试的值，由调用方保存并注入。
/// 来源：ADR-013 的非确定性输入约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickInput {
    /// Unix 毫秒墙钟。
    pub now_ms: u64,
}

impl TickInput {
    /// 读取当前 Unix 时间；系统时间早于 Unix 纪元时返回 0。
    pub fn now() -> Self {
        Self {
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

/// 按固定顺序结算世界、玩家和市场的内存状态。
///
/// 房间、玩家和市场先在副本中处理，所有返回 Result 的检查通过后才发布。
/// 这是同步内存提交，不保证 panic、进程崩溃或磁盘写入时的原子性。
/// 沙箱执行和内部状态不在结算副本内；失败重试的输入缓存由调用方负责。
/// 玩家执行入口位于 kk-runtime；引擎不持有或调用玩家沙箱。
///
/// ```compile_fail
/// let mut engine = kk_engine::Engine::default();
/// engine.tick();
/// ```
///
/// ```compile_fail
/// let mut engine = kk_engine::Engine::default();
/// engine.sandboxes.clear();
/// ```
pub struct Engine {
    pub(crate) world: kk_world::World,
    pub(crate) players: BTreeMap<String, PlayerState>,
    /// 跨房间订单簿和交易记录，不在 kk-world 的二进制房间快照中。
    pub(crate) market: market::MarketState,
    next_id: u64,
    /// 最近成功 tick 的事件，按房间结算顺序拼接；失败不覆盖。
    pub(crate) last_events: Vec<serde_json::Value>,
    /// 持续生效的房间故障注入，由调用方显式解除。
    fail_room: Option<String>,
    /// 失败后记录恢复状态；世界版本检查不能检测玩家或市场的直接修改。
    run_state: RunState,
    /// 持续生效的阶段入口故障注入。
    fail_stage: Option<TickStage>,
    fail_writeback: Option<WritebackStep>,
    /// 本引擎生效的 link 有效规则；放置、结算与常量同步读取。
    /// 装配后不应直接改写——规则与已装配的处理链须一起换，经 with_plan。
    pub(crate) links: kk_mod_api::builtin::LinkRules,
    /// Generic 仓库默认容量与槽切换规则，与计划及玩家常量一同装配。
    pub(crate) stores: kk_mod_api::builtin::StoreRules,
    /// 单目标战斗动作射程规则，结算与玩家常量注入同源消费；与计划一同装配。
    pub(crate) combat: kk_mod_api::builtin::CombatRanges,
    /// 移动玩家门规则（疲劳/MOVE 预检），仲裁原生准入预填与玩家常量
    /// 注入同源消费；与计划一同装配。
    pub(crate) move_gate: kk_mod_api::builtin::MoveGate,
    /// 地形静态通行规则：仲裁静态障碍判定的墙项与玩家面原生寻路地形
    /// 同源消费；与计划一同装配。
    pub(crate) terrain_passability: kk_mod_api::builtin::TerrainPassability,
    /// 内建结构 RCL 配额快照（G11.a）：结算配额判定与玩家面
    /// CONTROLLER_STRUCTURES 常量注入同源消费；与计划一同装配。
    pub(crate) quotas: Rc<kk_mod_api::builtin::StructureQuotaSnapshot>,
    /// 本引擎生效的资源目录：内建 RESOURCES_ALL 加计划声明的命名空间
    /// 资源。搬运/掉落/领域能力按它校验；市场与跨房发送不读它（明确
    /// 只认内建资源）。与计划一同装配，不允许单独换。
    pub(crate) resources: Rc<kk_mod_api::resources::ResourceCatalog>,
    /// 本引擎生效的建筑目录：计划声明的可建造结构与配方。与计划一同装配。
    pub(crate) buildings: Rc<kk_mod_api::buildings::BuildingCatalog>,
    /// 内建与自有槽执行表；默认直连 native，装配计划后可携带贡献链。
    pub(crate) contrib: Rc<contrib::ContributionDispatch>,
    /// 扩展字段目录；默认为空（不接受扩展写入），with_plan 装配。
    pub(crate) extensions: Rc<kk_mod_api::extension::ExtensionCatalog>,
    /// 已提交的各 Mod 命名空间状态；每 tick 克隆为候选，发布时整体换回。
    /// 服务端私有，不进世界快照与玩家投影。
    pub(crate) mod_states: BTreeMap<String, serde_json::Value>,
    /// 当前生效的冻结计划（默认含 link 与仓库容量规则）。扩展意图描述符与
    /// Mod 类型目录从此解析；规则/处理链/扩展目录是它的派生物。
    plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
}

/// 内置 link 与仓库容量的冻结计划；无 Mod 引擎的默认贡献平面。
/// 测试夹具构造 Plane 时也使用它。
pub fn builtin_plan() -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    static PLAN: std::sync::OnceLock<std::sync::Arc<kk_mod_api::ContributionPlan>> =
        std::sync::OnceLock::new();
    PLAN.get_or_init(|| {
        std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![],
                kk_mod_api::FreezeOptions::default(),
            )
            .expect("内置贡献冻结必成"),
        )
    })
    .clone()
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(kk_world::World::new(0))
    }
}

impl Engine {
    pub fn new(world: kk_world::World) -> Self {
        let next_id = world.next_obj_id();
        Self {
            world,
            players: BTreeMap::new(),
            market: market::MarketState::default(),
            next_id,
            last_events: Vec::new(),
            fail_room: None,
            run_state: RunState::Ready,
            fail_stage: None,
            fail_writeback: None,
            links: kk_world::place::builtin_link_rules(),
            stores: kk_world::place::builtin_store_rules(),
            combat: kk_world::place::builtin_combat_ranges(),
            move_gate: kk_world::place::builtin_move_gate(),
            terrain_passability: kk_mod_api::builtin::TerrainPassability::default(),
            quotas: Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            resources: Rc::new(kk_mod_api::resources::ResourceCatalog::default()),
            buildings: Rc::new(kk_mod_api::buildings::BuildingCatalog::default()),
            contrib: Rc::new(contrib::ContributionDispatch::default()),
            extensions: Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            mod_states: BTreeMap::new(),
            plan: builtin_plan(),
        }
    }

    /// 当前已装配冻结计划的指纹：KKPV 块头的 plan_hash 来源——计划变化
    /// 使旧块失效，块库按指纹淘汰代际。
    pub fn plan_hash(&self) -> kk_mod_api::PlanHash {
        self.plan.hash()
    }

    /// 计划中的 Mod 类型描述符（管理面创建对象与诊断用）；builtin 类型
    /// 不在此列。类型键须为 `namespace:name`。
    pub fn mod_object_type(&self, type_key: &str) -> Option<&kk_mod_api::ObjectTypeDescriptor> {
        let key = kk_mod_api::Key::new(type_key).ok()?;
        // builtin 类型有 Rust 权威表示，以 Mod 信封再造一份会形成双表示，
        // 挂 builtin 键的 Mod 对象也会把玩家面引向无执行器的 builtin 槽。
        if key.namespace() == "builtin" {
            return None;
        }
        let handle = self.plan.projection().resolve(&key)?;
        Some(&self.plan.projection().get(handle)?.descriptor)
    }

    /// 结算指定房间前返回错误，传 None 解除。
    /// 公开方法只隐藏文档，发布版本仍可调用；并非一次性故障。
    #[doc(hidden)]
    pub fn set_fail_room_for_test(&mut self, room: Option<String>) {
        self.fail_room = room;
    }

    /// 读取当前结算状态。
    pub fn run_state(&self) -> &RunState {
        &self.run_state
    }

    /// 当前生效的 link 有效规则（只读快照，拷贝语义）。
    pub fn links(&self) -> kk_mod_api::builtin::LinkRules {
        self.links
    }

    /// 当前生效的 Generic 仓库容量规则（只读快照）。
    pub fn stores(&self) -> kk_mod_api::builtin::StoreRules {
        self.stores
    }

    /// 当前生效的单目标战斗射程规则（只读快照）；runtime 将它同步到玩家常量。
    pub fn combat_ranges(&self) -> kk_mod_api::builtin::CombatRanges {
        self.combat
    }

    /// 当前生效的移动玩家门规则（只读快照）；runtime 将它同步到玩家常量。
    pub fn move_gate(&self) -> kk_mod_api::builtin::MoveGate {
        self.move_gate
    }

    /// 当前生效的地形静态通行规则（只读快照）；runtime 将它同步到玩家
    /// 沙箱的原生寻路地形。
    pub fn terrain_passability(&self) -> kk_mod_api::builtin::TerrainPassability {
        self.terrain_passability
    }

    /// 当前生效的结构配额快照：结算配额判定与 runtime 的
    /// CONTROLLER_STRUCTURES 玩家常量注入同源读取。
    pub fn structure_quotas(&self) -> &kk_mod_api::builtin::StructureQuotaSnapshot {
        &self.quotas
    }

    /// 当前生效的资源目录：内建 RESOURCES_ALL 加计划声明的命名空间
    /// 资源（只读引用）。市场/跨房发送不读它——它们显式只认内建资源。
    pub fn buildings(&self) -> &kk_mod_api::buildings::BuildingCatalog {
        &self.buildings
    }

    pub fn resources(&self) -> &kk_mod_api::resources::ResourceCatalog {
        &self.resources
    }

    /// 当前生效的扩展字段目录（只读引用）。
    pub fn extensions(&self) -> &kk_mod_api::extension::ExtensionCatalog {
        &self.extensions
    }

    /// 进入指定阶段前返回错误，传 None 解除。
    /// 公开方法只隐藏文档，发布版本仍可调用。
    #[doc(hidden)]
    pub fn set_fail_stage_for_test(&mut self, stage: Option<TickStage>) {
        self.fail_stage = stage;
    }

    /// 在指定候选子步完成后注入错误；不触碰已发布状态。传 None 解除。
    #[doc(hidden)]
    pub fn set_fail_writeback_for_test(&mut self, step: Option<WritebackStep>) {
        self.fail_writeback = step;
    }

    fn writeback_checkpoint(&self, step: WritebackStep) -> Result<(), SettleError> {
        if self.fail_writeback == Some(step) {
            return Err(SettleError(format!(
                "故障注入：{} 候选子步完成后失败",
                step.label()
            )));
        }
        Ok(())
    }

    /// 将 NeedsRecovery 改回 Ready，放弃恢复。
    /// 仅允许世界时间和版本未变；不删除调用方保存的运行结果。
    pub fn abandon_recovery(&mut self) -> Result<(), String> {
        match &self.run_state {
            RunState::NeedsRecovery {
                tick, generation, ..
            } if self.world.time() == *tick && self.world.generation() == *generation => {
                self.run_state = RunState::Ready;
                Ok(())
            }
            RunState::NeedsRecovery {
                tick, generation, ..
            } => Err(format!(
                "放弃恢复被拒：世界已被外部变异（失败轮 tick={tick}, gen={generation}；当前 tick={}, gen={}）",
                self.world.time(),
                self.world.generation()
            )),
            _ => Err("无需恢复（不在 NeedsRecovery 态）".to_string()),
        }
    }

    /// 结算调用方提供的指令，并读取当前墙钟。
    /// 调用方负责清洗与身份绑定；此处不执行沙箱或提交玩家 Memory。
    /// 重试需固定墙钟时使用 tick_with_intents_with_input。
    pub fn tick_with_intents(&mut self, batch: IntentBatch) -> Result<(), SettleError> {
        self.tick_with_intents_with_input(batch, TickInput::now())
    }

    /// 使用指定指令和外部时间推进一个 tick。
    ///
    /// 返回 Err 时世界、玩家、市场、ID 游标和 last_events 保持提交前状态，
    /// 并记录 NeedsRecovery。重试须重新提供相同 batch 和 TickInput，且不可
    /// 直接修改玩家或市场；引擎只检查世界时间和版本是否改变。
    /// 不捕获 panic，发布中的 panic 不享有上述回滚保证。
    pub fn tick_with_intents_with_input(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
    ) -> Result<(), SettleError> {
        self.tick_with_writebacks(batch, input, BTreeMap::new())
    }

    /// 将玩家内存候选与房间、市场一起发布；失败时所有已提交数据保持旧值。
    /// 调用方负责绑定玩家身份，并在重试时复用相同的指令、时间及写回候选。
    /// 不捕获 panic；桶值须由调用方根据固定预算和执行回执计算。
    pub fn tick_with_writebacks(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> Result<(), SettleError> {
        // 失败后仅在世界时间和版本未变时放行；输入是否相同由调用方保证。
        match &self.run_state {
            RunState::Ready | RunState::Committed { .. } => {}
            RunState::NeedsRecovery {
                tick, generation, ..
            } => {
                if self.world.time() != *tick || self.world.generation() != *generation {
                    return Err(SettleError(format!(
                        "恢复前世界已被外部变异（失败轮 tick={tick}, gen={generation}；当前 tick={}, gen={}）",
                        self.world.time(),
                        self.world.generation()
                    )));
                }
            }
            // 同步调用结束后若残留这些状态，可能由 panic 中断导致；拒绝继续。
            RunState::Running { .. } | RunState::Prepared { .. } => {
                return Err(SettleError(
                    "状态机接线错误：Running/Prepared 不得跨调用存在".into(),
                ));
            }
        }
        self.run_state = RunState::Running {
            tick: self.world.time(),
        };
        let mut cursor = pipeline::StageCursor::new(self.fail_stage);
        let result = self.staged_body(&mut cursor, batch, &input, writebacks);
        match result {
            Ok(()) => {
                self.run_state = RunState::Committed {
                    tick: self.world.time(),
                };
                Ok(())
            }
            Err(e) => {
                // 丢弃候选并记录失败阶段，不缓存指令或墙钟。
                self.run_state = RunState::NeedsRecovery {
                    tick: self.world.time(),
                    generation: self.world.generation(),
                    stage: cursor.stage(),
                    reason: e.to_string(),
                };
                Err(e)
            }
        }
    }
}

mod install;
mod room_loop;
mod state_api;
mod working;
pub use room_loop::{energy_resource_name, settle_room};
pub use working::{ChainSnapshot, ObjectJournal, Plane, RoomCtx, Working};
#[cfg(test)]
mod tests;
