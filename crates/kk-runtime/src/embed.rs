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

use crate::{PlayerExecutor, RunEpoch, ThreadExecutor, WorldSource};
use kk_engine::intent::IntentBatch;
use kk_engine::{
    PlayerWriteback, RunOutcome, RunResult, RunState, SegmentWriteback, TickInput, TickStage,
};
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
    /// 下一次实际调用引擎结算时使用的房间故障钩子，用后清除。
    fail_next_settle_room: Option<String>,
    /// 下一次实际调用引擎结算时使用的阶段故障钩子，用后清除。
    fail_next_settle_stage: Option<TickStage>,
    fail_next_writeback: Option<kk_engine::WritebackStep>,
    /// 测试用执行顺序，必须是全部注册玩家的排列；默认按玩家名排序。
    execution_order_for_test: Option<Vec<String>>,
    /// 最近一次启用顺序覆盖时的完整排列，包含本轮未请求执行的玩家。
    last_execution_order: Option<Vec<String>>,
    /// 下次通过输入校验的 `create_shard` 使用的沙箱配置；开始注册前整表取走。
    sandbox_configs_for_test: BTreeMap<String, kk_runner::SandboxConfig>,
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

/// `controller: false` 与显式控制器字段互斥（screeps-ok 契约，adapter.ts:45-48）。
const CONTROLLER_ONLY_FIELDS: [&str; 6] = [
    "rcl",
    "owner",
    "safeMode",
    "safeModeAvailable",
    "ticksToDowngrade",
    "powerEnabled",
];

struct RoomSetup<'a> {
    name: &'a str,
    terrain: Terrain,
    spec: &'a Value,
}

fn room_setups(spec: &Value) -> anyhow::Result<Vec<RoomSetup<'_>>> {
    let mut setups = Vec::new();
    let mut seen = BTreeSet::new();
    let rooms = spec
        .get("rooms")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("ShardSpec.rooms 必须是数组"))?;
    for room in rooms {
        let name = room
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("RoomSpec.name 必须是非空字符串"))?;
        if !seen.insert(name) {
            anyhow::bail!("重复房间名: {name}");
        }
        // RoomPosition 用偏移 128 后的 8 位坐标编码房间；超出 -128..=127
        // 会溢出或产生别名，须在世界注入沙箱前拒绝。
        let parsed =
            kk_core::RoomName::parse(name).map_err(|e| anyhow::anyhow!("RoomSpec {name}: {e}"))?;
        if !(-128..=127).contains(&parsed.xx) || !(-128..=127).contains(&parsed.yy) {
            anyhow::bail!(
                "RoomSpec {name}: 世界界外房间坐标 ({}, {})，须在 ±127 内",
                parsed.xx,
                parsed.yy
            );
        }
        if room.get("controller").and_then(Value::as_bool) == Some(false) {
            let set: Vec<&str> = CONTROLLER_ONLY_FIELDS
                .iter()
                .copied()
                .filter(|k| room.get(*k).is_some())
                .collect();
            if !set.is_empty() {
                anyhow::bail!(
                    "RoomSpec {name}: controller: false takes no {}",
                    set.join(", ")
                );
            }
        }
        // 通过浮点数读取再检查整数性，使 JSON 3.0 与 JavaScript 的整数 3 等价。
        // 在转成控制器等级前检查范围，避免截断或夹取掩盖非法输入。
        if let Some(value) = room.get("rcl")
            && !value
                .as_f64()
                .is_some_and(|rcl| rcl.fract() == 0.0 && (0.0..=8.0).contains(&rcl))
        {
            anyhow::bail!("RoomSpec {name}: rcl 非法（需 0..=8 整数）: {value}");
        }
        let mut terrain = match room.get("terrain") {
            Some(value) => {
                Terrain::from_codes(&terrain_codes_from_json(value)?).map_err(anyhow::Error::msg)?
            }
            None => Terrain::plains(),
        };
        terrain.normalize();
        setups.push(RoomSetup {
            name,
            terrain,
            spec: room,
        });
    }
    Ok(setups)
}

/// 提取玩家名并拒绝空名或重名；对象形式的 `gcl`、`power` 当前不在这里使用。
fn player_handles(spec: &Value) -> anyhow::Result<Vec<String>> {
    let mut handles = Vec::new();
    let mut seen = BTreeSet::new();
    let players = spec
        .get("players")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("ShardSpec.players 必须是数组"))?;
    for entry in players {
        let name = match entry {
            Value::String(name) => name.as_str(),
            Value::Object(fields) => fields.get("name").and_then(Value::as_str).unwrap_or(""),
            _ => "",
        };
        if name.is_empty() {
            anyhow::bail!("PlayerSpec.name 必须是非空字符串");
        }
        if !seen.insert(name) {
            anyhow::bail!("重复玩家 handle: {name}");
        }
        handles.push(name.to_string());
    }
    Ok(handles)
}

/// 解析 JSON 地形字节数组，元素须为整数类型且在 0..=255 内。
/// 数组长度和地形位是否合法，另由 [`Terrain::from_codes`] 校验。
pub fn terrain_codes_from_json(v: &Value) -> anyhow::Result<Vec<u8>> {
    let array = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("terrain 必须是数值数组"))?;
    array
        .iter()
        .map(|x| {
            let n = x
                .as_i64()
                .filter(|n| *n >= 0 && *n <= u8::MAX as i64)
                .ok_or_else(|| anyhow::anyhow!("地形元素非法: {x}"))?;
            Ok(n as u8)
        })
        .collect()
}

impl EmbedEngine {
    pub fn new() -> Self {
        Self {
            players: None,
            engine: Engine::default(),
            pending: None,
            execution_failure: None,
            player_concurrency: std::thread::available_parallelism().map_or(1, |n| n.get().min(2)),
            fail_after_player_for_test: None,
            last_console: BTreeMap::new(),
            perf: PerfRecorder::default(),
            fail_next_settle: false,
            fail_next_settle_room: None,
            fail_next_settle_stage: None,
            fail_next_writeback: None,
            execution_order_for_test: None,
            last_execution_order: None,
            sandbox_configs_for_test: BTreeMap::new(),
        }
    }

    /// 校验房间和玩家输入后，回收旧沙箱并建立新分片。
    /// 输入校验失败时保留旧分片；后续沙箱注册或对象初始化失败不恢复旧分片。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        // 在回收旧沙箱前完成输入校验，失败时保留现有分片。
        let rooms = room_setups(spec)?;
        let handles = player_handles(spec)?;

        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        self.last_console.clear();
        self.perf = PerfRecorder::default();
        self.execution_order_for_test = None;
        self.last_execution_order = None;
        self.fail_next_settle = false;
        self.fail_next_settle_room = None;
        self.fail_next_settle_stage = None;
        self.fail_next_writeback = None;

        let players = self.players.get_or_insert_with(ThreadExecutor::new);
        // 先取走全部配置，避免注册中途失败后残留配置被下一次创建意外使用。
        let configs = std::mem::take(&mut self.sandbox_configs_for_test);
        for handle in handles {
            let mut config = configs.get(&handle).cloned().unwrap_or_default();
            // link 有效规则同步到玩家全局常量。计划是常量的
            // 权威来源，测试配置里的同名覆盖以引擎规则为准。
            config.constant_overrides.insert(
                "LINK_CAPACITY".to_string(),
                serde_json::json!(self.engine.links().capacity),
            );
            let cpu = kk_core::cpu::Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            );
            players.spawn(&handle, config)?;
            self.engine.players.entry(handle.clone()).or_default().cpu = cpu;
            // 初始余额为 10M credits，内部按千分之一存储。
            // 来源：screeps-ok adapters/vanilla/index.ts 的 createShard 玩家初始化。
            self.engine.players.entry(handle.clone()).or_default().money = 10_000_000_000;
        }
        for setup in rooms {
            let name = setup.name;
            let room = setup.spec;
            self.engine.world.register_room(name, setup.terrain);
            let has_controller = room
                .get("controller")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if has_controller {
                // 有主控制器默认等级 1，无主默认 0。倒计时转成绝对 tick 时多加 1，
                // 来源：screeps-ok adapters/vanilla/index.ts 的 createShard 控制器初始化。
                let owner = room.get("owner").and_then(Value::as_str);
                let rcl = room
                    .get("rcl")
                    .and_then(Value::as_f64)
                    .map(|f| f as i64)
                    .unwrap_or(if owner.is_some() { 1 } else { 0 })
                    .clamp(0, 8);
                let time = self.engine.world.time();
                let mut spec = serde_json::json!({
                    "pos": [1, 1],
                    "structureType": "controller",
                    "level": rcl,
                    "isPowerEnabled": room
                        .get("powerEnabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
                if let Some(owner) = owner {
                    spec["owner"] = serde_json::json!(owner);
                    let downgrade_table = kk_core::constants::CONTROLLER_DOWNGRADE;
                    let default_clock = downgrade_table
                        .iter()
                        .find(|(k, _)| *k == rcl.to_string())
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    let downgrade_time = match room.get("ticksToDowngrade").and_then(Value::as_i64)
                    {
                        Some(t) => Some(time.saturating_add(t.max(0) as u64).saturating_add(1)),
                        None if rcl > 0 => {
                            Some(time.saturating_add(default_clock as u64).saturating_add(1))
                        }
                        None => None,
                    };
                    if let Some(dt) = downgrade_time {
                        spec["downgradeTime"] = serde_json::json!(dt);
                    }
                    if let Some(sm) = room
                        .get("safeMode")
                        .and_then(Value::as_i64)
                        .filter(|v| *v > 0)
                    {
                        spec["safeMode"] =
                            serde_json::json!(time.saturating_add(sm as u64).saturating_add(1));
                    }
                    if let Some(avail) = room.get("safeModeAvailable").and_then(Value::as_i64) {
                        spec["safeModeAvailable"] = serde_json::json!(avail.max(0));
                    }
                }
                let ctx = PlaceCtx {
                    time,
                    links: self.engine.links(),
                    controller: None,
                    name_seq: self.engine.world.next_obj_id() as usize,
                };
                let controller = normalize("structure", &spec, &ctx).map_err(anyhow::Error::msg)?;
                self.engine
                    .world
                    .insert(name, controller)
                    .map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
    }

    /// 按适配器的对象类别和初始字段摆放对象，返回对象 ID。
    /// 字段归一化由 [`kk_world::place::normalize`] 完成；未处置的失败轮会阻止摆放。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        self.no_open_round("placeObject")?;
        // 使用单调递增的对象 ID 生成默认名字；对象数量会随消亡减少，不能用作序号。
        let ctx = PlaceCtx {
            time: self.engine.world.time(),
            links: self.engine.links(),
            controller: self.engine.world.room(room).and_then(|r| r.controller()),
            name_seq: self.engine.world.next_obj_id() as usize,
        };
        let object = normalize(kind, spec, &ctx).map_err(anyhow::Error::msg)?;
        let id = self
            .engine
            .world
            .insert(room, object)
            .map_err(anyhow::Error::msg)?;
        Ok(id.as_str())
    }

    /// 直接注入测试用市场订单，不经过玩家 API 的资格检查和创建费用结算。
    /// `price` 以 credits 输入，乘 1000 四舍五入后存储；`amount` 初始为 0，
    /// `remainingAmount` 等于 `totalAmount`。`active` 默认 true，
    /// `created` 默认当前 tick，`createdTimestamp` 默认墙钟毫秒。
    pub fn place_market_order(&mut self, spec: &Value) -> anyhow::Result<String> {
        self.no_open_round("placeMarketOrder")?;
        let owner = spec
            .get("owner")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.owner 必须是字符串"))?
            .to_string();
        let order_type = spec
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.type 必须是 'buy'/'sell'"))?
            .to_string();
        if order_type != "buy" && order_type != "sell" {
            anyhow::bail!("MarketOrderSpec.type 非法: {order_type}");
        }
        let resource_type = spec
            .get("resourceType")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.resourceType 必须是字符串"))?
            .to_string();
        let price = spec
            .get("price")
            .and_then(Value::as_f64)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.price 必须是数字"))?;
        let total_amount = spec
            .get("totalAmount")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.totalAmount 必须是整数"))?;
        let id = self.engine.market.next_order_id();
        self.engine.market.insert_order(kk_engine::MarketOrder {
            id: id.clone(),
            user: owner,
            active: spec.get("active").and_then(Value::as_bool).unwrap_or(true),
            order_type,
            resource_type,
            price: (price * 1000.0).round() as i64,
            amount: 0,
            remaining_amount: total_amount,
            total_amount,
            room_name: spec
                .get("roomName")
                .and_then(Value::as_str)
                .map(str::to_string),
            created: spec
                .get("created")
                .and_then(Value::as_u64)
                .unwrap_or(self.engine.world.time()),
            created_timestamp: spec
                .get("createdTimestamp")
                .and_then(Value::as_u64)
                .unwrap_or(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                ),
        });
        Ok(id)
    }

    /// 设置房间地形；存在失败轮或世界已执行过玩家 tick 时拒绝修改。
    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> anyhow::Result<()> {
        self.no_open_round("setTerrain")?;
        let mut t = Terrain::from_codes(&terrain).map_err(anyhow::Error::msg)?;
        t.normalize();
        self.engine
            .world
            .set_terrain(room, t)
            .map_err(anyhow::Error::msg)
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

    /// 请求一个玩家并推进一轮；存在失败缓存时，仍会提交整轮缓存的结果。
    /// 重试时忽略 `code`，且 `handle` 必须属于缓存轮。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let mut codes = BTreeMap::new();
        codes.insert(handle.to_string(), code.to_string());
        let mut runs = self.run_players(&codes)?;
        Ok(runs.remove(handle).expect("请求的 handle 必在结果中"))
    }

    /// 让请求的玩家读取同一已提交世界，求值后合并意图并结算一个 tick。
    /// 空请求也会推进世界。玩家级错误保留在结果中，基础设施故障返回整轮错误。
    ///
    /// 世界结算失败后缓存整轮候选；再次调用忽略传入代码，只重试结算。
    /// 重试可请求缓存玩家的子集或空集，但始终提交并返回全部缓存玩家的结果；
    /// 请求缓存轮之外的玩家会返回错误并保留缓存。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        self.no_execution_failure("run_players")?;
        let t0 = std::time::Instant::now();
        // 先校验再取走缓存，防止错误请求丢掉失败轮。子集请求仍重试整个缓存轮。
        if let Some(pending) = &self.pending {
            for handle in codes.keys() {
                if !pending.results.contains_key(handle) {
                    anyhow::bail!(
                        "上轮结算失败的重试轮不含玩家 {handle}（先经 run_players 重试缓存轮，或 discard_pending_round 丢弃后开新一轮）"
                    );
                }
            }
        }
        let (results, run_ns, project_ns, input) = match self.pending.take() {
            Some(pending) => (pending.results, 0u128, 0u128, pending.input),
            None => {
                let (results, run_ns, project_ns) = self.execute_players(codes)?;
                (results, run_ns, project_ns, TickInput::now())
            }
        };
        // 墙钟输入随候选一起缓存，重试不会重新采样签名时间等值。
        let t_settle = std::time::Instant::now();
        let writebacks = results
            .iter()
            .map(|(handle, result)| {
                Ok((
                    handle.clone(),
                    PlayerWriteback {
                        cpu: Some(
                            kk_core::cpu::RunBudget::new(&self.engine.players[handle].cpu)
                                .finish(result.cpu_billed_ms)
                                .map_err(anyhow::Error::msg)?,
                        ),
                        raw_memory: result.raw_memory.clone(),
                        segments: result
                            .segments_out
                            .as_ref()
                            .map(SegmentWriteback::from_runner),
                    },
                ))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>();
        let writebacks = match writebacks {
            Ok(writebacks) => writebacks,
            Err(error) => {
                self.execution_failure = Some(ExecutionFailure {
                    confirmed: results,
                    reason: error.to_string(),
                });
                return Err(error);
            }
        };
        let settled = self.settle_world_with_writebacks(
            Self::batch_from_results(&results),
            input,
            writebacks,
        );
        match settled {
            Ok(()) => {
                let settle_ns = t_settle.elapsed().as_nanos();
                // 世界、玩家内存和 CPU 桶已经统一发布，无需线程计费回执。
                self.perf.record([
                    ns_ms(project_ns),
                    ns_ms(run_ns),
                    ns_ms(settle_ns),
                    t0.elapsed().as_secs_f64() * 1000.0,
                ]);
                let mut out = BTreeMap::new();
                for (handle, result) in results {
                    let run = self.finish_player_run(&handle, result);
                    out.insert(handle, run);
                }
                Ok(out)
            }
            Err(e) => {
                // 保留候选以重试结算；已提交世界、Memory 和 CPU 桶保持旧值。
                // V8 全局变量不在回滚范围内。
                self.pending = Some(PendingRound { results, input });
                Err(e)
            }
        }
    }

    /// 执行世界结算并消费测试故障钩子。重试的 `input` 来自失败轮缓存。
    fn settle_world(&mut self, batch: IntentBatch, input: TickInput) -> anyhow::Result<()> {
        self.settle_world_with_writebacks(batch, input, BTreeMap::new())
    }

    fn settle_world_with_writebacks(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> anyhow::Result<()> {
        if self.fail_next_settle {
            self.fail_next_settle = false;
            anyhow::bail!("故障注入：世界结算失败（测试钩子）");
        }
        // 引擎钩子持续生效，这里只借用一次，并在结算返回后解除。
        let fail_room = self.fail_next_settle_room.take();
        if fail_room.is_some() {
            self.engine.set_fail_room_for_test(fail_room.clone());
        }
        let fail_stage = self.fail_next_settle_stage.take();
        if fail_stage.is_some() {
            self.engine.set_fail_stage_for_test(fail_stage);
        }
        let fail_writeback = self.fail_next_writeback.take();
        self.engine.set_fail_writeback_for_test(fail_writeback);
        let settled = self
            .engine
            .tick_with_writebacks(batch, input, writebacks)
            .map_err(|e| anyhow::anyhow!("{e}"));
        if fail_room.is_some() {
            self.engine.set_fail_room_for_test(None);
        }
        if fail_stage.is_some() {
            self.engine.set_fail_stage_for_test(None);
        }
        self.engine.set_fail_writeback_for_test(None);
        settled
    }

    /// 从同一已提交状态注入 Memory 和世界，有界派发并回收请求玩家的候选。
    /// 注入通信失败或 `InfraFailure` 中止整轮，保留已确认结果并阻止自动重跑；
    /// 玩家的语法错误、运行错误和跳帧惩罚则由后续结算统一处理。
    #[allow(clippy::type_complexity)]
    fn execute_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<(BTreeMap<String, RunResult>, u128, u128)> {
        // 未知 handle 显式报错：静默跳过会让结果映射缺项，调用方无从得知玩家没跑
        let registered = self
            .players
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?
            .players();
        for handle in codes.keys() {
            if !registered.iter().any(|p| p == handle) {
                anyhow::bail!("未知玩家 handle: {handle}");
            }
        }
        let mut results = BTreeMap::new();
        let mut project_ns = 0u128;
        let handles = registered.clone();
        // 本轮所有玩家读取同一代世界，玩家执行结束后才开始结算。
        let bound_generation = self.engine.world.generation();
        // 回执必须匹配派发时的 tick 和 generation，防止接受另一轮的结果。
        let epoch = RunEpoch {
            tick: self.engine.world.time(),
            generation: bound_generation,
        };
        let source: &dyn WorldSource = &self.engine.world;
        // 包括提前返回路径在内，检查玩家执行期间世界未改变。
        let assert_bound = || {
            debug_assert_eq!(
                source.generation(),
                bound_generation,
                "玩家执行期间世界代际不得变动（同一 tick 绑定同一版本）"
            );
        };
        // 覆盖顺序必须包含全部注册玩家且无重复，否则可能漏跑或重复运行。
        let handles = match &self.execution_order_for_test {
            Some(order) => {
                let mut sorted = order.clone();
                sorted.sort();
                assert_eq!(sorted, registered, "执行序覆盖必须是注册玩家集的排列");
                // 只在启用测试覆盖时保存排列，默认执行不额外复制观测记录。
                self.last_execution_order = Some(order.clone());
                order.clone()
            }
            None => handles,
        };
        let mut requested = handles
            .into_iter()
            .filter(|handle| codes.contains_key(handle));
        let mut pending = BTreeMap::<String, crate::PendingExecution>::new();
        let (notify, completion) = std::sync::mpsc::channel();
        let mut failure: Option<String> = None;
        let started = std::time::Instant::now();
        loop {
            while failure.is_none() && pending.len() < self.player_concurrency {
                let Some(handle) = requested.next() else {
                    break;
                };
                let code = &codes[&handle];
                let state = self.engine.players.get(&handle);
                let raw_mem = state.map(|p| p.raw_memory.clone()).unwrap_or_default();
                let segments_json = match state {
                    Some(p) if !p.active_request.is_empty() => serde_json::to_string(
                        &p.active_request
                            .iter()
                            .map(|id| (*id, p.segments.get(id).cloned().unwrap_or_default()))
                            .collect::<Vec<(u8, String)>>(),
                    )
                    .unwrap_or_else(|_| "[]".into()),
                    _ => "[]".into(),
                };
                let foreign_json = self
                    .resolve_foreign_segment(&handle)
                    .unwrap_or(serde_json::Value::Null);
                let gcl = self.engine.players.get(&handle).map(|p| p.gcl).unwrap_or(0);
                let t_project = std::time::Instant::now();
                let mut world = source.world_json_for_viewer(&handle, gcl);
                // 市场读取已提交的引擎状态，随世界 JSON 注入，不存放在 V8 持久闭包中。
                if let Some(obj) = world.as_object_mut() {
                    obj.insert("market".into(), market_view(&self.engine, &handle));
                }
                project_ns += t_project.elapsed().as_nanos();
                let budget = kk_core::cpu::RunBudget::new(&self.engine.players[&handle].cpu);
                match self.players.as_mut().expect("已检查分片创建").dispatch_job(
                    &handle,
                    kk_runner::RunJob {
                        world,
                        code: code.clone(),
                        epoch,
                        attempt: 0,
                        deadline: None,
                        raw_memory: raw_mem,
                        segments_json,
                        foreign_json: foreign_json.to_string(),
                        budget,
                    },
                    Some(notify.clone()),
                ) {
                    Ok(job) => {
                        pending.insert(handle, job);
                    }
                    Err(e) => {
                        failure = Some(format!("基础设施故障（玩家 {handle}）: {e}"));
                    }
                }
            }
            if pending.is_empty() {
                break;
            }
            let completed: Vec<_> = pending
                .iter()
                .filter_map(|(handle, job)| job.try_take().map(|result| (handle.clone(), result)))
                .collect();
            if completed.is_empty() {
                let deadline = pending.values().map(|job| job.deadline()).min().unwrap();
                let _ = completion
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()));
                continue;
            }
            for (handle, result) in completed {
                pending.remove(&handle);
                if let RunOutcome::InfraFailure(err) = &result.outcome {
                    failure.get_or_insert_with(|| format!("基础设施故障（玩家 {handle}）: {err}"));
                } else {
                    results.insert(handle.clone(), result);
                    if self.fail_after_player_for_test.as_deref() == Some(handle.as_str()) {
                        self.fail_after_player_for_test = None;
                        failure.get_or_insert_with(|| {
                            format!("故障注入：玩家 {handle} 回执后执行轮中止")
                        });
                    }
                }
            }
        }
        let run_ns = started.elapsed().as_nanos().saturating_sub(project_ns);
        assert_bound();
        if let Some(reason) = failure {
            self.execution_failure = Some(ExecutionFailure {
                confirmed: results,
                reason: reason.clone(),
            });
            anyhow::bail!(reason);
        }
        Ok((results, run_ns, project_ns))
    }

    /// 合并已清洗并绑定玩家身份的意图，不使用 JavaScript 自报的身份。
    fn batch_from_results(results: &BTreeMap<String, RunResult>) -> IntentBatch {
        let mut batch = IntentBatch::default();
        for result in results.values() {
            batch.extend_from(result.intents.clone());
        }
        batch
    }

    /// 按发布者当前公开列表解析外部段；未指定 ID 时使用其默认公开段。
    /// 默认段也必须仍在公开列表中。无请求、无发布者或无授权时返回 None；
    /// 已授权但未写入的段返回 `data: null`，区别于活动段的空字符串。
    fn resolve_foreign_segment(&self, reader: &str) -> Option<serde_json::Value> {
        let req = self.engine.players.get(reader)?.foreign_request.clone()?;
        let (username, id_opt) = req;
        let publisher = self.engine.players.get(&username)?;
        let id = match id_opt {
            Some(id) => {
                if !publisher.public_ids.contains(&id) {
                    return None;
                }
                id
            }
            // 默认公开段仍须通过当前公开列表检查，残留的默认段 ID 不授予访问权限。
            None => {
                let id = publisher.default_public?;
                if !publisher.public_ids.contains(&id) {
                    return None;
                }
                id
            }
        };
        Some(serde_json::json!({
            "username": username,
            "id": id,
            "data": publisher
                .segments
                .get(&id)
                .map(|s| serde_json::Value::String(s.clone()))
                .unwrap_or(serde_json::Value::Null),
        }))
    }

    /// 直接结算调用方构造的意图，不执行玩家代码或提交玩家 Memory、CPU 桶。
    /// 有失败轮缓存时拒绝调用；本入口失败不会缓存意图，重试须重新提供。
    pub fn settle_external(&mut self, batch: IntentBatch) -> anyhow::Result<()> {
        self.no_pending_round("settle_external")?;
        self.settle_world(batch, TickInput::now())
    }

    /// 执行结果不完整时禁止开始新轮，等待调用方显式处置。
    fn no_execution_failure(&self, op: &str) -> anyhow::Result<()> {
        if let Some(failure) = &self.execution_failure {
            anyhow::bail!(
                "{op} 被拒：执行轮存在基础设施故障，已确认 {} 个玩家结果；先 discard_pending_round 显式放弃或重建分片。原因：{}",
                failure.confirmed.len(),
                failure.reason
            );
        }
        Ok(())
    }

    /// 防止缓存玩家结果与新世界结算混用。
    fn no_pending_round(&self, op: &str) -> anyhow::Result<()> {
        self.no_execution_failure(op)?;
        if self.pending.is_some() {
            anyhow::bail!(
                "{op} 被拒：存在未处置的失败轮候选（先 run_players 重试或 discard_pending_round）"
            );
        }
        Ok(())
    }

    /// 修改世界或市场前，同时检查玩家失败缓存与引擎恢复状态。
    /// `tick`、`settle_external` 失败可能只有恢复状态而没有缓存；市场修改
    /// 又不会改变世界 generation，因此不能仅依赖引擎的世界版本检查。
    fn no_open_round(&self, op: &str) -> anyhow::Result<()> {
        self.no_pending_round(op)?;
        if matches!(self.engine.run_state(), RunState::NeedsRecovery { .. }) {
            anyhow::bail!(
                "{op} 被拒：引擎处于恢复态（上次结算段失败、世界保持上一 tick）——先重试结算或 discard_pending_round 放弃恢复"
            );
        }
        Ok(())
    }

    /// 保存最近一次完成结算的 console，并转换适配器结果。
    /// `executed` 表示进入求值流程，语法错误也属于该流程。
    fn finish_player_run(&mut self, handle: &str, result: RunResult) -> PlayerRun {
        let executed = !matches!(result.outcome, RunOutcome::Terminated)
            && result.expr.get("kind").and_then(Value::as_str) != Some("inject_failed");
        let console = result.console.clone();
        self.last_console
            .insert(handle.to_string(), console.clone());
        PlayerRun {
            executed,
            expr: result.expr,
            console,
        }
    }

    /// 不运行玩家代码，逐轮结算 `count` 个 tick。
    /// 途中失败仅保留失败 tick 之前的状态，已经成功的 tick 不会撤销。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        self.no_pending_round("tick")?;
        for _ in 0..count {
            self.settle_world(IntentBatch::default(), TickInput::now())?;
        }
        Ok(())
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
        &self.engine.world
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
    pub fn fail_next_settle_for_test(&mut self) {
        self.fail_next_settle = true;
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
            .players
            .get(player)
            .map(|p| p.notifications.as_slice())
            .unwrap_or(&[])
    }

    /// 读取已提交玩家状态，供故障注入测试检查候选是否提前写入。
    #[doc(hidden)]
    pub fn player_state_for_test(&self, player: &str) -> Option<&kk_engine::PlayerState> {
        self.engine.players.get(player)
    }

    /// 读取宿主已提交 CPU 桶，不包含本轮待发布费用。
    #[doc(hidden)]
    pub fn player_bucket_for_test(&mut self, player: &str) -> anyhow::Result<f64> {
        self.engine
            .players
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
    pub fn discard_pending_round(&mut self) {
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        let _ = self.engine.abandon_recovery();
    }

    /// 停止所有玩家线程，清空世界、失败缓存和 console。
    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        self.last_console.clear();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// 构造玩家市场视图：收支交易各取最近 100 条，金额和价格保留千分之一 credit 单位。
/// 交易中的玩家名保持字符串，由 prelude 转成 `{username}` 并换算价格。
fn market_view(engine: &kk_engine::settle::Engine, viewer: &str) -> Value {
    fn tx_list(engine: &kk_engine::settle::Engine, viewer: &str, outgoing: bool) -> Vec<Value> {
        engine
            .market
            .transactions_for(viewer, outgoing, 100)
            .iter()
            .map(|t| {
                let mut v = json!({
                    "id": t.id.to_string(),
                    "time": t.time,
                    "resourceType": t.resource_type,
                    "amount": t.amount,
                    "from": t.from,
                    "to": t.to,
                });
                if let Some(d) = &t.description {
                    v["description"] = json!(d);
                }
                if let Some(o) = &t.order {
                    v["order"] = o.clone();
                }
                if let Some(s) = &t.sender {
                    v["sender"] = json!(s);
                }
                if let Some(r) = &t.recipient {
                    v["recipient"] = json!(r);
                }
                v
            })
            .collect()
    }
    // 活动订单按资源分组，本人订单还包括非活动订单；价格保持内部单位。
    // 历史行情当前未实现，返回只有 all 空数组的占位对象。
    let mut orders_all = serde_json::Map::new();
    let mut by_rt: BTreeMap<String, serde_json::Map<String, serde_json::Value>> = BTreeMap::new();
    for order in engine.market.orders.values().filter(|o| o.active) {
        let v = order_projection(order);
        by_rt
            .entry(order.resource_type.clone())
            .or_default()
            .insert(order.id.clone(), v.clone());
        orders_all.insert(order.id.clone(), v);
    }
    let mut orders = serde_json::Map::new();
    orders.insert("all".into(), Value::Object(orders_all));
    for (rt, m) in by_rt {
        orders.insert(rt, Value::Object(m));
    }
    let my_orders: Vec<Value> = engine
        .market
        .orders
        .values()
        .filter(|o| o.user == viewer)
        .map(order_projection)
        .collect();
    // 跨分片资源余额用于市场 deal 的资源检查。
    let mut resources = serde_json::Map::new();
    if let Some(p) = engine.players.get(viewer) {
        for (k, v) in &p.resources {
            resources.insert(k.clone(), json!(v));
        }
    }
    json!({
        "money_milli": engine.players.get(viewer).map(|p| p.money).unwrap_or(0),
        "resources": Value::Object(resources),
        "transactions": {
            "outgoing": tx_list(engine, viewer, true),
            "incoming": tx_list(engine, viewer, false),
        },
        "orders": Value::Object(orders),
        "my_orders": my_orders,
        "history": { "all": [] },
    })
}

/// 序列化订单内部字段，价格保留千分之一 credit 单位，时间保留绝对值。
/// 面向玩家的价格换算和 `user` 字段移除由 prelude 完成。
fn order_projection(o: &kk_engine::MarketOrder) -> Value {
    json!({
        "id": o.id,
        "user": o.user,
        "active": o.active,
        "type": o.order_type,
        "resourceType": o.resource_type,
        "price": o.price,
        "amount": o.amount,
        "remainingAmount": o.remaining_amount,
        "totalAmount": o.total_amount,
        "roomName": o.room_name,
        "created": o.created,
        "createdTimestamp": o.created_timestamp,
    })
}
