//! 以库接口创建分片、执行玩家代码并结算世界。
//!
//! 每轮玩家读取同一已提交世界，运行结果先作为候选保存。Memory、segments
//! 与 CPU 桶、世界一起发布；结算失败则缓存
//! 整轮候选，下次调用只重试结算，复用首次尝试的墙钟输入。
//!
//! 这些边界不涵盖 V8 全局变量：玩家代码对全局变量的修改不会随结算失败回滚。
//! 执行阶段基础设施故障会保留已确认结果并阻止自动重派。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::Terrain;
use serde_json::{Value, json};

use std::cell::RefCell;
use std::rc::Rc;

use crate::mod_trace::ModTraceEntry;
use crate::mod_vm::ModVm;
use crate::{PlayerExecutor, RunEpoch, RunOutcome, RunResult, ThreadExecutor, WorldSource};
use kk_engine::intent::IntentBatch;
use kk_engine::settle::RoomCtx;
use kk_engine::settle::contrib::{SlotCall, SlotExecutor};
use kk_engine::{PlayerWriteback, RunState, SegmentWriteback, TickInput, TickStage};
use kk_mod_api::{Key, SlotOutcome, SlotPlan};
use kk_mod_host::{
    ChainContext, ChainHost, ChainTarget, ChainWrite, ModPackage, ModRealm, RegisterOptions,
};
use kk_world::ObjId;
use kk_world::place::{PlaceCtx, normalize};

use kk_engine::settle::Engine;

/// 玩家求值结果；适配器根据 `executed` 和 `expr.kind` 区分错误类别。
#[derive(Debug)]
pub struct PlayerRun {
    /// 是否进入表达式求值流程。跳帧惩罚和世界注入失败为 false；
    /// 语法错误也为 true，不能据此判断是否执行过 JavaScript 语句。
    pub executed: bool,
    /// 表达式结果：成功包含 `ok` 和可选的 `value`；失败包含 `ok`、`kind`、`error`。
    pub expr: Value,
    /// 该玩家本次执行期间的 console 输出。
    pub console: Vec<String>,
}

/// 世界结算失败后保留的整轮候选。重试复用结果和墙钟，不再运行玩家代码。
struct PendingRound {
    /// 玩家意图、Memory、segments、CPU 用量和 console，均已脱离线程本地计费状态。
    results: BTreeMap<String, RunResult>,
    /// 首次尝试的结算输入，保证重试不改变签名时间等墙钟相关字段。
    input: TickInput,
}

/// 执行结果无法全部确认的轮次。保留已确认结果供诊断，禁止自动重派。
struct ExecutionFailure {
    confirmed: BTreeMap<String, RunResult>,
    reason: String,
}

/// 嵌入式分片及其玩家沙箱，协调玩家执行、世界结算和失败重试。
pub struct EmbedEngine {
    players: Option<ThreadExecutor>,
    engine: Engine,
    /// 等待重试或丢弃的失败轮。
    pending: Option<PendingRound>,
    execution_failure: Option<ExecutionFailure>,
    player_concurrency: usize,
    fail_after_player_for_test: Option<String>,
    last_console: BTreeMap<String, Vec<String>>,
    /// 成功的 `run_players` 调用的耗时样本。
    perf: PerfRecorder,
    /// 下一次进入结算入口时返回测试错误，不调用引擎。
    fail_next_settle: bool,
    /// 下一次日志段写入模拟 I/O 失败（不落盘），用后清除。
    fail_next_log_write: bool,
    /// 下一次 `switch_mod_packages` 在候选验证全部通过后、切换前失败，
    /// 验证旧分片在切换失败的每个阶段都保持可运行。
    fail_next_switch_for_test: bool,
    /// 下一次实际调用引擎结算时使用的房间故障钩子，用后清除。
    fail_next_settle_room: Option<String>,
    /// 下一次实际调用引擎结算时使用的阶段故障钩子，用后清除。
    fail_next_settle_stage: Option<TickStage>,
    fail_next_writeback: Option<kk_engine::WritebackStep>,
    /// 测试用执行顺序，必须是全部注册玩家的排列；默认按玩家名排序。
    execution_order_for_test: Option<Vec<String>>,
    /// 持久输入日志（G25.a）：每轮成功提交后追加一段；None 即未记录。
    /// 生命周期=当前分片运行期，teardown/重建分片/切换/恢复时终止
    /// （文件保留）。每段携带身份快照，切换后的段以新身份自描述。
    input_log: Option<input_log::InputLog>,
    /// 最近一次启用顺序覆盖时的完整排列，包含本轮未请求执行的玩家。
    last_execution_order: Option<Vec<String>>,
    /// 下次通过输入校验的 `create_shard` 使用的沙箱配置；开始注册前整表取走。
    sandbox_configs_for_test: BTreeMap<String, kk_runner::SandboxConfig>,
    /// 是否经 KKPV 二进制通道向沙箱注入世界数据；默认走 JSON 参考通道。
    /// 两条通道装配出的玩家可见面等价，测试按此锁定。
    kkpv_channel: bool,
    /// 公开地形束的发布库：按代际发布/获取共享束，承载保留报告
    /// （固定内存测量面）。仅 KKPV 通道使用。
    pathfinder_terrain: Option<(
        std::sync::Arc<()>,
        u64,
        std::sync::Arc<kk_runner::TerrainSet>,
    )>,
    kkpv_public_store: crate::kkpv_store::KkpvBlockStore,
    /// 可信 Mod 注册与结算 realm；仅 `create_shard_with_mods` 装配。持有它使
    /// EmbedEngine 不再可跨线程移动（结算与 realm 须同线程）。
    mod_realm: Option<Rc<RefCell<ModVm>>>,
    /// 本引擎的运行 trace 开启意愿；setup 与替代分片恢复均应用。
    mod_trace_enabled: bool,
    /// 分片计划是否含 Mod 意图；决定玩家沙箱是否装配扩展意图原语。
    ext_intents: bool,
    /// 冻结计划导出的 Mod 玩家面目录；投影通道按它裁切 Mod 对象。
    player_surface: Option<std::sync::Arc<kk_mod_api::player_surface::PlayerSurface>>,
    /// 注入玩家沙箱的 Mod 玩家面安装清单（模块集 + 查询常量 + emit 目录）。
    mod_player_api: Option<std::sync::Arc<kk_runner::ModPlayerApi>>,
}

/// 记录成功调用的投影、执行、结算和整次调用耗时。失败尝试不记录；
/// 重试成功时投影与执行耗时为零。达到容量上限后忽略新样本。
#[derive(Default)]
struct PerfRecorder {
    /// 每次成功调用的 [project_ms, run_ms, settle_ms, wall_ms]，单位为毫秒。
    ticks: Vec<[f64; 4]>,
}

/// 采样窗口上限（32B/tick × 50k ≈ 1.6MB）。
const PERF_MAX_SAMPLES: usize = 50_000;

impl PerfRecorder {
    fn record(&mut self, phases: [f64; 4]) {
        if self.ticks.len() < PERF_MAX_SAMPLES {
            self.ticks.push(phases);
        }
    }

    /// 按排序后 `round((N - 1) * q)` 的下标取分位数；耗时单位为毫秒。
    fn phase_summary(ticks: &[[f64; 4]], idx: usize) -> Value {
        let mut v: Vec<f64> = ticks.iter().map(|t| t[idx]).collect();
        if v.is_empty() {
            return json!({ "count": 0 });
        }
        v.sort_by(|a, b| a.total_cmp(b));
        let pick = |q: f64| v[(((v.len() - 1) as f64) * q).round() as usize];
        json!({
            "count": v.len(),
            "p50_ms": pick(0.5),
            "p95_ms": pick(0.95),
            "p99_ms": pick(0.99),
            "max_ms": v[v.len() - 1],
            "total_ms": v.iter().sum::<f64>(),
        })
    }
}

impl Default for EmbedEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn ns_ms(ns: u128) -> f64 {
    ns as f64 / 1e6
}

impl EmbedEngine {
    pub fn new() -> Self {
        // 种子 0 保持既有默认；宿主选种经 with_world_seed 注入（G22.b）。
        Self::with_world_seed(0)
    }

    /// 以指定世界种子建分片：种子随世界持久化，是玩法随机流与确定性
    /// 时间的派生根；同种子同输入同抽值，异种子抽值序列不同。
    pub fn with_world_seed(seed: u64) -> Self {
        Self {
            players: None,
            engine: Engine::new(kk_world::World::new(seed)),
            mod_realm: None,
            mod_trace_enabled: false,
            pending: None,
            execution_failure: None,
            player_concurrency: std::thread::available_parallelism().map_or(1, |n| n.get().min(2)),
            fail_after_player_for_test: None,
            last_console: BTreeMap::new(),
            perf: PerfRecorder::default(),
            fail_next_settle: false,
            fail_next_log_write: false,
            fail_next_switch_for_test: false,
            fail_next_settle_room: None,
            fail_next_settle_stage: None,
            fail_next_writeback: None,
            execution_order_for_test: None,
            input_log: None,
            last_execution_order: None,
            sandbox_configs_for_test: BTreeMap::new(),
            ext_intents: false,
            player_surface: None,
            mod_player_api: None,
            kkpv_channel: false,
            pathfinder_terrain: None,
            kkpv_public_store: crate::kkpv_store::KkpvBlockStore::new(),
        }
    }

    /// 设置同时执行的玩家上限；1 为串行参考，失败轮存在时不能修改。
    pub fn set_player_concurrency(&mut self, limit: usize) -> anyhow::Result<()> {
        self.no_open_round("set_player_concurrency")?;
        if !(1..=32).contains(&limit) {
            anyhow::bail!("玩家并发上限须为 1..=32");
        }
        self.player_concurrency = limit;
        Ok(())
    }
    pub fn player_concurrency(&self) -> usize {
        self.player_concurrency
    }

    /// 在轮次之间替换玩家模块，不推进时间；失败候选未处置时拒绝修改。
    /// 执行器同时保存模块表，工作者恢复时复用这份源码和二进制资产。
    pub fn set_player_modules(
        &mut self,
        player: &str,
        modules: kk_runner::ModuleTable,
    ) -> anyhow::Result<()> {
        self.no_open_round("set_player_modules")?;
        self.players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .set_modules(player, modules)
    }

    /// 设置宿主提供的初始 JSON Memory，不执行玩家代码或推进世界。
    /// 长度按玩家 RawMemory.set 的 UTF-16 码元上限检查，不能按 UTF-8 字节替代。
    pub fn set_player_memory(&mut self, player: &str, memory: &Value) -> anyhow::Result<()> {
        self.no_open_round("set_player_memory")?;
        anyhow::ensure!(memory.is_object(), "初始 Memory 必须是 JSON 对象");
        let raw = serde_json::to_string(memory)?;
        const RAW_MEMORY_LIMIT_UTF16: usize = 2 * 1024 * 1024;
        anyhow::ensure!(
            raw.encode_utf16().count() <= RAW_MEMORY_LIMIT_UTF16,
            "初始 Memory 超过 2 Mi UTF-16 码元"
        );
        self.engine
            .edit_players(|players| {
                players
                    .get_mut(player)
                    .ok_or_else(|| format!("未知玩家 {player}"))?
                    .raw_memory = raw;
                Ok(())
            })
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// 宿主读取已提交的玩家状态；不包含失败轮候选，不用于玩家授权投影。
    pub fn player_state(&self, player: &str) -> Option<&kk_engine::PlayerState> {
        self.engine.players().get(player)
    }

    /// 宿主诊断世界投影，不执行玩家视野过滤；不是可恢复的完整检查点。
    pub fn world_snapshot(&self) -> Value {
        self.engine.world().world_json()
    }

    /// 世界种子：玩法随机流域与确定性时间的派生根（G22.b）。随检查点
    /// 持久化；sim 报告与管理面读取，玩家不可见。
    pub fn world_seed(&self) -> u64 {
        self.engine.world().seed()
    }

    /// 管理诊断用的完整 Mod 对象列表，包含自主创建对象和 server-only 字段。
    /// world_snapshot 沿用原生 JSON 投影，Mod 对象在此独立读取；不是检查点。
    pub fn mod_object_snapshots(&self) -> Vec<Value> {
        self.engine
            .world()
            .rooms()
            .values()
            .flat_map(|room| &room.objects)
            .filter(|(_, object)| matches!(object, kk_schema::room::RoomObject::Mod(_)))
            .map(|(id, _)| self.get_object(&id.as_str()).expect("已确认存活 Mod 对象"))
            .collect()
    }

    /// 读取引擎状态，供测试断言失败恢复行为。
    #[doc(hidden)]
    pub fn engine_run_state_for_test(&self) -> &RunState {
        self.engine.run_state()
    }

    /// 读取不按玩家视野过滤的对象快照；未知对象或不支持的类别返回 None。
    pub fn get_object(&self, id: &str) -> Option<Value> {
        self.source().object_snapshot(id)
    }

    /// 按 FIND 常量查询房间，不按玩家视野过滤。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.source().find_in_room(room, find_const)
    }

    /// 已提交的某 Mod 命名空间状态（诊断与测试面；未初始化为 null）。
    pub fn engine_mod_state(&self, namespace: &str) -> Value {
        self.engine
            .mod_states()
            .get(namespace)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// 已冻结的组合及当前停用掩码；无 JS Mod VM 的原生默认路径返回 None。
    pub fn mod_composition(&self) -> Option<kk_mod_host::CompositionReport> {
        self.mod_realm.as_ref().map(|vm| vm.borrow().composition())
    }

    /// 当前执行策略身份（组合报告摘要）；无 Mod 分片为 None。
    /// 摘要输入全为派生纯数据，失败理论不可达——报告路径按降级 None
    /// 处理（debug 下仍断言），检查点保存/恢复路径独立取值且硬失败。
    pub fn mod_execution_strategy(&self) -> Option<String> {
        let digest = self
            .mod_realm
            .as_ref()?
            .borrow()
            .composition()
            .execution_strategy_digest();
        debug_assert!(digest.is_ok(), "执行策略摘要不可失败：{digest:?}");
        digest.ok()
    }

    /// 当前已安装贡献计划的身份；诊断报告须记录实际计划而非输入推算值。
    pub fn contribution_plan_hash(&self) -> kk_mod_api::PlanHash {
        self.engine.plan_hash()
    }

    /// 当前世界代际（失败轮不前进；诊断与测试面）。
    pub fn engine_generation(&self) -> u64 {
        self.engine.world().generation()
    }

    pub fn get_game_time(&self) -> u64 {
        self.source().game_time()
    }

    /// 读取该玩家最近一次完成结算的 console；失败缓存尚未提交时仍返回旧日志。
    pub fn capture_console_logs(&self, handle: &str) -> Vec<String> {
        self.last_console.get(handle).cloned().unwrap_or_default()
    }

    pub fn get_controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.source().controller_pos(room)
    }

    fn source(&self) -> &dyn WorldSource {
        self.engine.world()
    }

    /// 汇总成功调用的耗时，单位为毫秒。run 为派发回收窗口减去宿主投影时间，
    /// settle 包含玩家候选与统一发布；各阶段不是工作者 CPU 用量之和。
    pub fn perf_report(&self) -> Value {
        json!({
            "ticks": self.perf.ticks.len(),
            "phases": {
                "project": PerfRecorder::phase_summary(&self.perf.ticks, 0),
                "run": PerfRecorder::phase_summary(&self.perf.ticks, 1),
                "settle": PerfRecorder::phase_summary(&self.perf.ticks, 2),
                "wall": PerfRecorder::phase_summary(&self.perf.ticks, 3),
            },
        })
    }

    /// 清空耗时样本，常用于排除预热调用。
    pub fn perf_reset(&mut self) {
        self.perf.ticks.clear();
    }

    /// 让下一次进入结算入口的调用返回错误，不改变引擎运行状态。
    #[doc(hidden)]
    #[doc(hidden)]
    pub fn bump_next_obj_id_for_test(&mut self, next: u64) -> anyhow::Result<()> {
        self.no_open_round("bump_next_obj_id_for_test")?;
        self.engine
            .bump_next_obj_id_for_test(next)
            .map_err(anyhow::Error::msg)
    }

    pub fn fail_next_settle_for_test(&mut self) {
        self.fail_next_settle = true;
    }

    /// 下一次日志段写入模拟 I/O 失败：走真实的停用与「已提交不可重试」
    /// 错误路径，用后清除。
    pub fn fail_next_log_write_for_test(&mut self) {
        self.fail_next_log_write = true;
    }

    /// 让下一次实际调用引擎时在指定房间结算失败，用后清除。
    /// 若玩家执行提前失败，或整轮故障钩子先返回错误，该钩子留待下一次结算。
    #[doc(hidden)]
    pub fn fail_next_settle_room_for_test(&mut self, room: &str) {
        self.fail_next_settle_room = Some(room.to_string());
    }

    /// 让下一次实际调用引擎时在指定阶段入口失败，用后清除。
    /// 未调用引擎时保留钩子，行为与房间故障钩子相同。
    #[doc(hidden)]
    pub fn fail_next_settle_stage_for_test(&mut self, stage: TickStage) {
        self.fail_next_settle_stage = Some(stage);
    }

    /// 下一次实际结算在对应候选子步完成后失败，用后清除。
    #[doc(hidden)]
    pub fn fail_next_writeback_for_test(&mut self, step: kk_engine::WritebackStep) {
        self.fail_next_writeback = Some(step);
    }

    /// 覆盖玩家执行顺序；执行前检查是否为全部注册玩家的排列，release 也检查。
    /// 仅影响投影和派发顺序，不保证完成顺序；结果映射和意图合并顺序不变。
    #[doc(hidden)]
    pub fn set_execution_order_for_test(&mut self, order: Option<Vec<String>>) {
        self.execution_order_for_test = order;
    }

    /// 读取最近一次启用顺序覆盖时的完整排列，包含未请求执行的玩家。
    /// 重试和禁用覆盖不更新记录；`create_shard` 清空记录。
    #[doc(hidden)]
    pub fn last_execution_order_for_test(&self) -> Option<&[String]> {
        self.last_execution_order.as_deref()
    }

    /// 设置下一次创建分片时使用的玩家沙箱配置，未设置的玩家使用默认配置。
    /// 通过输入校验后整表消费，即使注册中途失败也不会留给下一次创建。
    #[doc(hidden)]
    pub fn set_sandbox_config_for_test(&mut self, player: &str, config: kk_runner::SandboxConfig) {
        self.sandbox_configs_for_test
            .insert(player.to_string(), config);
    }

    /// 读取该玩家已提交的通知记录。
    #[doc(hidden)]
    pub fn player_notifications_for_test(
        &self,
        player: &str,
    ) -> &[kk_engine::settle::NotifyRecord] {
        self.engine
            .players()
            .get(player)
            .map(|p| p.notifications.as_slice())
            .unwrap_or(&[])
    }

    /// 读取已提交玩家状态，供故障注入测试检查候选是否提前写入。
    #[doc(hidden)]
    pub fn player_state_for_test(&self, player: &str) -> Option<&kk_engine::PlayerState> {
        self.player_state(player)
    }

    /// 读取宿主已提交 CPU 桶，不包含本轮待发布费用。
    #[doc(hidden)]
    pub fn player_bucket_for_test(&mut self, player: &str) -> anyhow::Result<f64> {
        self.engine
            .players()
            .get(player)
            .map(|p| p.cpu.bucket())
            .ok_or_else(|| anyhow::anyhow!("未知玩家 {player}"))
    }

    /// 在指定玩家回执确认后中止执行轮，用于验证部分结果不会被盲目重跑。
    #[doc(hidden)]
    pub fn fail_after_player_for_test(&mut self, player: &str) {
        self.fail_after_player_for_test = Some(player.into());
    }

    /// 是否有等待重试或丢弃的玩家失败轮缓存。
    pub fn has_pending_round(&self) -> bool {
        self.pending.is_some() || self.execution_failure.is_some()
    }

    /// Mod VM 已因结算错误销毁；独立于玩家缓存和引擎恢复状态。
    pub fn needs_mod_recovery(&self) -> bool {
        self.mod_realm
            .as_ref()
            .is_some_and(|vm| vm.borrow().needs_recovery())
    }

    /// 最近一次实际结算的 Mod 槽调用故障；恢复不清除，下一次结算开始时清除。
    /// 非 Mod 的引擎错误不产生此记录，正常拒绝也不记为故障。
    pub fn last_mod_settlement_fault(&self) -> Option<crate::ModSettlementFault> {
        self.mod_realm
            .as_ref()
            .and_then(|vm| vm.borrow().last_fault.clone())
    }

    /// 连续实际结算失败的 Mod 来源计数；不随 VM 重建丢失。
    /// 成功、不可归责失败或另一 Mod 的失败会中断该序列。
    pub fn mod_fault_streak(&self) -> Option<crate::ModFaultStreak> {
        self.mod_realm
            .as_ref()
            .and_then(|vm| vm.borrow().fault_streak.clone())
    }

    /// 最近自动停用决定；成功结算不会擦除，宿主可据此发出诊断通知。
    pub fn last_mod_auto_disable(&self) -> Option<crate::ModAutoDisable> {
        self.mod_realm
            .as_ref()
            .and_then(|vm| vm.borrow().last_auto_disable.clone())
    }

    /// 开启 Mod 运行 trace（幂等）。Mod 未装配时挂起，装配后生效；
    /// 只影响开启之后的事件，无 Mod 分片保持无 trace。
    pub fn enable_mod_trace(&mut self) {
        self.mod_trace_enabled = true;
        if let Some(vm) = &self.mod_realm {
            vm.borrow_mut().enable_trace();
        }
    }

    /// 当前 trace 快照（条目按发生序）与累计丢弃数；未开启或无 Mod
    /// 分片返回 None。
    pub fn mod_trace_snapshot(&self) -> Option<(Vec<ModTraceEntry>, usize)> {
        self.mod_realm
            .as_ref()
            .and_then(|vm| vm.borrow().trace_snapshot())
    }

    /// 停用指定 Mod 及其传递依赖者的结算执行，返回本次新增停用的 ID。
    /// 保留冻结 schema、规则、玩家 API 和缓存结果；下一次结算前重建 VM。
    /// 分片内单调停用，重新启用须重建分片。未知 ID 不改变任何状态。
    pub fn disable_mod(&mut self, id: &str) -> anyhow::Result<BTreeSet<String>> {
        self.mod_realm
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("分片未装配 Mod VM"))?
            .borrow_mut()
            .disable(id)
            .map_err(anyhow::Error::msg)
    }

    /// 启用持久输入日志（G25.a）：每轮成功提交后追加一段可回放输入。
    /// 无开放轮次才可启用；文件存在时先整读校验再续接（撕裂/断链拒绝）。
    /// 生命周期到 teardown/重建分片/包切换/检查点恢复为止。
    pub fn enable_input_log(
        &mut self,
        path: &std::path::Path,
        metadata: Value,
    ) -> anyhow::Result<()> {
        self.no_open_round("enable_input_log")?;
        anyhow::ensure!(
            self.input_log.is_none(),
            "已有输入日志在记录；一个分片运行期只挂一份"
        );
        let mut header = serde_json::Map::new();
        header.insert("format".into(), json!(input_log::LOG_FORMAT));
        header.insert("version".into(), json!(input_log::LOG_VERSION));
        header.insert("seq".into(), json!(0));
        header.insert("kind".into(), json!("header"));
        header.insert("prev_hash".into(), Value::Null);
        header.insert("identity".into(), self.input_log_identity()?);
        header.insert("metadata".into(), metadata);
        let log = input_log::InputLog::open(path, header)?;
        self.input_log = Some(log);
        Ok(())
    }

    /// 日志文件中的段数（含续接时既有的段）；未启用为 None。
    pub fn input_log_segments(&self) -> Option<u64> {
        self.input_log
            .as_ref()
            .map(input_log::InputLog::segments_written)
    }

    /// 当前已持久化末段的历史凭据；未挂载日志或只有头段时没有游标。
    pub fn input_log_cursor(&self) -> Option<LogCursor> {
        self.input_log
            .as_ref()
            .and_then(input_log::InputLog::cursor)
    }

    /// 头段绑定首次记录时的完整持久状态；续接保留旧头段，不重新定义历史根。
    fn input_log_identity(&self) -> anyhow::Result<Value> {
        let (plan_hash, strategy, disabled) = self.log_identity_fields();
        let initial_state_sha256 =
            input_log::sha256_hex(&serde_json::to_vec(&self.checkpoint(Value::Null)?)?);
        Ok(json!({
            "engine_version": env!("CARGO_PKG_VERSION"),
            "host_protocol": kk_mod_host::GAMEPLAY_PROTOCOL,
            "seed": self.world_seed(),
            "plan_hash": plan_hash,
            "execution_strategy": strategy,
            "disabled_mods": disabled,
            "initial_state_sha256": initial_state_sha256,
        }))
    }

    /// 段级身份三元组（plan_hash/执行策略/停用集），头段与逐段共用同源。
    fn log_identity_fields(&self) -> (String, Option<String>, BTreeMap<String, Value>) {
        let disabled = self
            .disabled_mods()
            .into_iter()
            .map(|id| (id, Value::Bool(true)))
            .collect();
        (
            self.contribution_plan_hash().hex(),
            self.mod_execution_strategy(),
            disabled,
        )
    }

    /// 当前已停用的 Mod ID；包含随依赖关系被停用的 Mod。
    pub fn disabled_mods(&self) -> BTreeSet<String> {
        self.mod_realm
            .as_ref()
            .map(|vm| vm.borrow().disabled().clone())
            .unwrap_or_default()
    }

    /// 从分片创建时的包字节与配置重建 Mod VM，核验原冻结计划后才恢复可用。
    /// 不改变世界、玩家候选或引擎恢复状态；正常结算重试会自动调用。健康 VM 不重建。
    pub fn rebuild_mod_realm(&mut self) -> anyhow::Result<()> {
        if let Some(vm) = &self.mod_realm {
            let mut vm = vm.borrow_mut();
            let needed = vm.needs_recovery();
            let tracing = vm.trace_enabled();
            vm.recover().map_err(anyhow::Error::msg)?;
            if needed && tracing {
                vm.record_trace(crate::mod_trace::ModTraceEntry {
                    tick: self.engine.world().time(),
                    room: String::new(),
                    target: String::new(),
                    slot: String::new(),
                    stage: "",
                    event: crate::mod_trace::ModTraceEvent::RealmRebuilt,
                });
            }
        }
        Ok(())
    }

    /// 真实重新注册完成后使下一次恢复失败，验证新 VM 不会提前发布。
    #[doc(hidden)]
    pub fn fail_next_mod_rebuild_for_test(&mut self) -> anyhow::Result<()> {
        self.mod_realm
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("分片未装配 Mod VM"))?
            .borrow_mut()
            .fail_next_rebuild_for_test();
        Ok(())
    }

    /// 使下一次 `switch_mod_packages` 在候选全部验证通过后、切换前失败；
    /// 旧分片须保持可运行。用后清除。
    #[doc(hidden)]
    pub fn fail_next_switch_for_test(&mut self) {
        self.fail_next_switch_for_test = true;
    }

    /// 显式放弃执行故障轮并重建全部玩家工作者，保留已提交世界和玩家状态。
    /// 新线程丢弃全部 V8 全局变量；下一轮从已提交 Memory、segments 和 CPU 桶注入。
    /// 重建失败时保留故障记录，继续禁止运行；不自动重派故障轮。
    pub fn rebuild_player_workers(&mut self) -> anyhow::Result<()> {
        if self.execution_failure.is_none() {
            anyhow::bail!("没有待处置的执行故障轮");
        }
        self.players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .rebuild_all()?;
        if matches!(self.engine.run_state(), RunState::NeedsRecovery { .. }) {
            self.engine
                .abandon_recovery()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        Ok(())
    }

    /// 首个替代工作者就绪后使下一次重建失败，用于验证故障门保持关闭。
    #[doc(hidden)]
    pub fn fail_next_worker_rebuild_for_test(&mut self) -> anyhow::Result<()> {
        self.players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .fail_next_rebuild_for_test();
        Ok(())
    }

    #[doc(hidden)]
    pub fn gate_next_job_for_test(&mut self, player: &str) -> anyhow::Result<kk_runner::JobGate> {
        self.players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .gate_next_job_for_test(player)
    }

    /// 在下一条 RunJob 前退出指定玩家线程，使真实回执通道断开。
    #[doc(hidden)]
    pub fn exit_worker_before_next_job_for_test(&mut self, player: &str) -> anyhow::Result<()> {
        self.players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .exit_before_next_job_for_test(player)
    }

    /// 丢弃缓存结果或执行故障记录，并放弃引擎恢复状态。
    /// 不撤销玩家已经修改的 V8 全局变量。正常接口会阻止恢复期间修改世界，
    /// 因此这里放弃恢复不会遇到世界版本不匹配。
    /// 不解除 Mod VM 恢复门；再次结算前仍须成功重建。
    pub fn discard_pending_round(&mut self) {
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        let _ = self.engine.abandon_recovery();
    }

    /// 停止所有玩家线程，清空世界、失败缓存和 console。世界种子保留
    /// （与 `World::reset` 同口径）：种子是分片身份的一部分，重建世界
    /// 不换随机协议根。
    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        let seed = self.engine.world().seed();
        self.input_log = None;
        self.engine = Engine::new(kk_world::World::new(seed));
        self.mod_realm = None;
        self.mod_trace_enabled = false;
        self.ext_intents = false;
        self.player_surface = None;
        self.mod_player_api = None;
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        self.last_console.clear();
        // 世界已清空；公开束随代际一并作废（沙箱先于此回收）。
        self.kkpv_public_store = crate::kkpv_store::KkpvBlockStore::new();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

mod checkpoint;
mod execution;
mod input_log;
mod mod_chain;
mod placement;
mod projection;
mod replay;
mod setup;
mod switch;

pub use input_log::{CommittedLogError, InputLogData, InputLogError, LogCursor, read_input_log};
use mod_chain::ModSlotExecutor;
use projection::{kkpv_audience_hash, market_view};
pub use replay::{ReplayRound, assert_header_paired, rebuild_round, segments_from_checkpoint};

pub use setup::terrain_codes_from_json;
