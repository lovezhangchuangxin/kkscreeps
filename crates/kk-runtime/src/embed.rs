//! 嵌入式 API：以库形式驱动引擎（ADR-008 第 8 条；B0-5 迁入 kk-runtime 并
//! 落实统一提交点）。
//!
//! 方法集与 screeps-ok 适配器契约同构，与将来 `kkscreeps sim` 驱动引擎的方式
//! 是同一个接口——适配器（compat/，经 `kkscreeps embed` sidecar 的 JSON-RPC，
//! ADR-010）与模拟器共用本模块，一石两鸟。
//!
//! ## 统一提交点（B0-5，ADR-013 复现场景①的修复）
//!
//! `run_player`/`run_players` 按 tick 契约「状态与可见性」编排：
//! ①逐玩家注入（PlayerState.raw_memory）+ 执行 → 候选 RunResult（无持久
//! 副作用）；②世界结算；③成功 → 提交（世界版本 + 各玩家 RawMemory/计费
//! 一次切换）；失败 → 整轮 Err，候选缓存于 `pending`——**下一次调用不重跑
//! 玩家、只重试结算**（重试只消费已缓存 RunResult，tick 契约「失败、重试
//! 与持久状态」）。玩家级失败（注入失败/硬中断/软异常）玩家级恢复，不冒泡
//! 整轮；基础设施故障（线程失联）整轮停顿（Err，不缓存）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::Terrain;
use serde_json::{Value, json};

use crate::{PlayerExecutor, ThreadExecutor, WorldSource};
use kk_engine::intent::IntentBatch;
use kk_engine::{RunOutcome, RunResult, RunSpec, TickInput};
use kk_world::place::{PlaceCtx, normalize};

use kk_engine::settle::Engine;

/// runPlayer 的结局（TS 适配器据此映射 RunPlayerError 三分类或普通 Error）。
#[derive(Debug)]
pub struct PlayerRun {
    /// 代码是否真的执行过（false = 跳帧惩罚/注入失败等，适配器须抛普通 Error）。
    pub executed: bool,
    /// runExpr 结果：`{ok, value?} | {ok:false, kind: syntax|runtime|serialization, error}`。
    pub expr: Value,
    /// 该玩家本次执行期间的 console 输出。
    pub console: Vec<String>,
}

/// 上轮世界结算失败的候选缓存（重试只重结算，玩家代码不再执行——提交成功
/// 即清空）。输入钉子（B0-6，ADR-013 决策 5）：缓存轮固定**首次尝试**的
/// TickInput，重试复用记录值（「重放用记录值」），不再重采样墙钟——
/// signController datetime 等落盘值在重试后与首次尝试一致。
struct PendingRound {
    /// 各玩家候选运行结果（intents/RawMemory/计费候选/console）。
    results: BTreeMap<String, RunResult>,
    /// 首次尝试使用的结算输入（墙钟）。
    input: TickInput,
}

pub struct EmbedEngine {
    players: Option<ThreadExecutor>,
    engine: Engine,
    /// 上轮世界结算失败的候选缓存（重试只重结算，玩家代码不再执行——
    /// 提交成功即清空）。
    pending: Option<PendingRound>,
    last_console: BTreeMap<String, Vec<String>>,
    /// 性能观测点（A6）：每 tick 三阶段 + 墙钟采样
    perf: PerfRecorder,
    /// 测试钩子：下一次世界结算注入 Err（部分提交三连回归的场景②）。
    /// #[doc(hidden)]：仅集成测试消费，生产面不可达（误置最多导致一轮 Err）。
    fail_next_settle: bool,
}

/// 性能观测点（A6，[performance-acceptance](docs/architecture/performance-acceptance.md)
/// 「阶段耗时」口径）：run_player/run_players 每 tick 的四路采样——投影
/// （world_json_for_viewer 每玩家合计）、玩家运行（run_player 合计）、结算
/// （tick_with_intents）、tick 墙钟。采样有 cap（B0 收口）：满 cap 后丢弃
/// 新样本，`perf_reset` 清零重开。
#[derive(Default)]
struct PerfRecorder {
    /// 每 tick 的 [project_ms, run_ms, settle_ms, wall_ms]
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

    /// 单阶段汇总：count / p50 / p95 / max / total（ms）。
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

/// 纳秒 → 毫秒（观测点采样换算）。
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
        // 房名域校验（A5 片 2 评审 P2 清偿）：官方 RoomPosition 构造器要求房名
        // 可解析且 +128 偏移落 0..256（rooms.js:1280-1287）；偏移出界会使
        // beginTick 宿主装配（wrapObject → new RoomPosition）每 tick 抛错硬中断，
        // 且 256 恰好 int32 回绕成 sim 别名——embed 边界显式拒绝把静默契约变
        // 早失败。±127 上界严于构造器的 ±128：拒绝回绕别名域（官方世界不产生）
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
        // rcl 校验（typed level 是 u8，截断会得错值；字符串/非整数/越界拒绝）。
        // 数值形态须走 as_f64：JSON `3.0` 在 JS 语义里就是整数 3（vanilla 接受），
        // as_i64 会把浮点形态的整数一并拒掉
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

/// PlayerSpec 的 gcl/power 影响玩家可见的 Game.gcl/gpl 状态——M1 无规则消费
/// 面，延后到 M2 规则实装时接上（这里只取 name 注册 handle）。
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

/// JSON 数组 → 地形字节（元素必须是 0..=255 内的整数，不静默丢弃；
/// 位域合法性由 [`Terrain::from_codes`] 校验）。
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
            last_console: BTreeMap::new(),
            perf: PerfRecorder::default(),
            fail_next_settle: false,
        }
    }

    /// 建立新分片。输入校验完成后，重置世界和玩家沙箱。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        // 在回收旧沙箱前完成输入校验，失败时保留现有分片。
        let rooms = room_setups(spec)?;
        let handles = player_handles(spec)?;

        // 逆序回收旧沙箱（稳妥起见的 teardown 默认——LIFO 是每线程约束，
        // shutdown_all 的创建逆序是防御性保守，ADR-013），再重置世界
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.pending = None;
        self.last_console.clear();
        self.perf = PerfRecorder::default();

        let players = self.players.get_or_insert_with(ThreadExecutor::new);
        for handle in handles {
            players.spawn(&handle, kk_runner::SandboxConfig::default())?;
            // 玩家持久状态条目随分片建立（B0-5 统一提交点：RawMemory 候选的
            // 提交位与注入源——缺省 gcl 0/raw_memory 空，与旧的 get().unwrap_or
            // 缺省口径一致）
            self.engine.players.entry(handle.clone()).or_default();
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
                // vanilla 双分支种子（index.ts:904/968）：未占房 rcl 缺省 0、
                // 占房缺省 1；占房另种 downgrade/safeMode 族——相对值换绝对
                // tick（+time+1，warm-up tick 消耗一拍，index.ts:966-993）
                let owner = room.get("owner").and_then(Value::as_str);
                // rcl 消费端与校验同口径（as_f64：浮点形态的整数合法）
                let rcl = room
                    .get("rcl")
                    .and_then(Value::as_f64)
                    .map(|f| f as i64)
                    .unwrap_or(if owner.is_some() { 1 } else { 0 })
                    .clamp(0, 8);
                let time = self.engine.world.time;
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

    /// 摆放对象（全部 placeXxx 的统一后端；spec 是 screeps-ok 的 PlaceSpec JSON）。
    /// `kind` 为适配器通道类别（creep/structure/constructionSite/flag/resource/…），
    /// 归一化由 [`kk_world::place::normalize`] 完成（初态表 + spec 私有字段折算）。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        self.no_pending_round("placeObject")?;
        // 无名稳定名序号用 id 分配器（单调不减）——对象会随结算消亡，
        // 可回退的计数（如对象总数）会让 mid-run 摆放重名
        let ctx = PlaceCtx {
            time: self.engine.world.time,
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

    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> anyhow::Result<()> {
        self.no_pending_round("setTerrain")?;
        let mut t = Terrain::from_codes(&terrain).map_err(anyhow::Error::msg)?;
        t.normalize();
        self.engine
            .world
            .set_terrain(room, t)
            .map_err(anyhow::Error::msg)
    }

    /// 先运行玩家代码，再结算一个 tick（统一提交点，模块头述）。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let mut codes = BTreeMap::new();
        codes.insert(handle.to_string(), code.to_string());
        let mut runs = self.run_players(&codes)?;
        Ok(runs.remove(handle).expect("请求的 handle 必在结果中"))
    }

    /// 多玩家对同一状态求值后合计推进 1 tick（runPlayers 契约；统一提交点）。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        let t0 = std::time::Instant::now();
        // 重试语义：上轮结算失败的候选缓存直接重试结算（玩家代码不再执行）。
        // 键集守卫（B0-5 评审 P1 清偿）：请求了缓存轮之外的玩家说明调用方意图
        // 是新一轮（或缓存的玩家集已不匹配）——显式拒绝，防 runs.remove 的
        // panic 与「panic 前缓存轮已提交却无回执」的事故形态（子集请求合法：
        // 重试以缓存轮为原子单位——结算消费并提交全部缓存玩家，仅返回所请求
        // 子集的回执）。
        // 校验先于取用（B0-6，外部评审清偿）：被拒的请求不得销毁缓存——
        // 原实现 take 后 bail，缓存随之丢失，原请求重试会重跑玩家代码
        //（tick 契约「不得为重试再次运行玩家代码」的违例）
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
        // ④世界结算（墙钟输入显式经 TickInput——首跑取实际值、失败轮随候选
        // 缓存、重试复用记录值，ADR-013 决策 5 + B0-6）
        let t_settle = std::time::Instant::now();
        let settled = self.settle_world(Self::batch_from_results(&results), input);
        match settled {
            Ok(()) => {
                let settle_ns = t_settle.elapsed().as_nanos();
                // ⑤⑥统一提交：世界版本（tick_with_intents 内部已切换）+ 玩家
                // 持久状态一次切换——RawMemory 候选入 PlayerState、计费候选
                // 落桶（失败不达此处：候选回缓存、桶不扣、Memory 不写）
                self.commit_players(&results);
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
                // 整轮回退：候选缓存（重试只重结算）、玩家持久状态与世界版本
                // 全部保持上一 tick（部分提交泄漏消灭——ADR-013 复现①）
                self.pending = Some(PendingRound { results, input });
                Err(e)
            }
        }
    }

    /// 世界结算入口（含测试故障注入钩子）。`input` 由调用方携带：首跑取
    /// 实际值、重试复用失败轮缓存的记录值（B0-6，ADR-013 决策 5）。
    fn settle_world(&mut self, batch: IntentBatch, input: TickInput) -> anyhow::Result<()> {
        if self.fail_next_settle {
            self.fail_next_settle = false;
            anyhow::bail!("故障注入：世界结算失败（测试钩子）");
        }
        self.engine
            .tick_with_intents_with_input(batch, input)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// ①②逐玩家：注入 PlayerState.raw_memory → per-viewer 投影 → 执行 →
    /// 候选 RunResult。基础设施故障（线程失联）整轮停顿（不缓存——停顿轮
    /// 等待修复后完整重跑）；玩家级失败（注入失败/硬中断）玩家级恢复。
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
        let mut run_ns = 0u128;
        let handles = registered.clone();
        // 版本绑定（M2B 前置批 P2，snapshot 契约「同一 tick 内所有 getter
        // 绑定同一版本」纪律）：本轮全部玩家读同一已提交版本——循环期间
        // 世界不得变异（结算在循环后），debug 断言守护
        let bound_generation = self.engine.world.generation();
        // 世界读取统一经 seam（WorldSource，ADR-013 决策 2）——M2B 新
        // getter 的登记面
        let source: &dyn WorldSource = &self.engine.world;
        // 代际绊线（评审 3 P2-2 清偿）：正常尾之外的两条轮中止路径
        //（注入失败 `?` / InfraFailure bail）同样守护——世界在玩家执行期间
        // 结构上不可变，断言是把该不变量钉死的绊线而非空转
        let assert_bound = || {
            debug_assert_eq!(
                source.generation(),
                bound_generation,
                "玩家执行期间世界代际不得变动（同一 tick 绑定同一版本）"
            );
        };
        // 固定顺序（handle 排序）求值——确定性；各玩家看同一已提交世界的
        // 自家视野（官方 runtimeData.get(userId) 同构）
        for handle in handles {
            let Some(code) = codes.get(&handle) else {
                continue;
            };
            let players = self
                .players
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
            // ①注入：每 tick 从已提交的 PlayerState.raw_memory 恢复（官方
            // make.js 每 tick 从存档恢复同构——世界回退轮注入的仍是旧值）
            let inject = players.set_raw_memory(
                &handle,
                &self
                    .engine
                    .players
                    .get(&handle)
                    .map(|p| p.raw_memory.clone())
                    .unwrap_or_default(),
            );
            assert_bound();
            inject?;
            // per-viewer 投影（gcl 点数 = PlayerState 真值，官方 runtimeData.user.gcl）
            let gcl = self.engine.players.get(&handle).map(|p| p.gcl).unwrap_or(0);
            let t_project = std::time::Instant::now();
            let world = source.world_json_for_viewer(&handle, gcl);
            project_ns += t_project.elapsed().as_nanos();
            let t_run = std::time::Instant::now();
            let result = players.execute(&world, &RunSpec::new(&handle, code));
            run_ns += t_run.elapsed().as_nanos();
            // 基础设施故障 → 整轮停顿（Err；不缓存候选——停顿轮的局部结果作废）
            if let RunOutcome::InfraFailure(err) = &result.outcome {
                assert_bound();
                anyhow::bail!("基础设施故障（玩家 {handle}）: {err}");
            }
            results.insert(handle, result);
        }
        assert_bound();
        Ok((results, run_ns, project_ns))
    }

    /// 候选意图批（全部玩家的已清洗 intents 合并——身份边界在 ThreadExecutor
    /// 的 from_json 已保证）。
    fn batch_from_results(results: &BTreeMap<String, RunResult>) -> IntentBatch {
        let mut batch = IntentBatch::default();
        for result in results.values() {
            batch.extend_from(result.intents.clone());
        }
        batch
    }

    /// ⑤提交：玩家持久状态一次切换（RawMemory 候选 + 计费候选落桶）。
    fn commit_players(&mut self, results: &BTreeMap<String, RunResult>) {
        for (handle, result) in results {
            if let Some(state) = self.engine.players.get_mut(handle)
                && let Some(raw) = &result.raw_memory
            {
                state.raw_memory = raw.clone();
            }
        }
        if let Some(players) = &mut self.players {
            for handle in results.keys() {
                // 提交失败（线程失联）不再回滚本轮——世界已切换，计费丢失
                // 记为桶账误差（低危，evict 重建即重置）。同族口径：基础设施
                // 故障停顿轮（execute_players 的注入失败 `?` 与 InfraFailure
                // bail）已执行玩家
                // 的候选滞留沙箱、由下一次 begin_tick 防御丢弃——漏计而非双计，
                // 与「停顿轮局部结果作废」语义一致（B0-5 评审登记）
                let _ = players.settle_bill(handle);
            }
        }
    }

    /// 直接以外部构造的 intent 批结算一轮（测试/工具面；无玩家执行与提交点
    /// 语义——生产路径经 run_players 的统一提交点）。
    pub fn settle_external(&mut self, batch: IntentBatch) -> anyhow::Result<()> {
        self.no_pending_round("settle_external")?;
        self.engine
            .tick_with_intents_with_input(batch, TickInput::now())
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// 失败轮缓存存在时拒绝推进/变异世界（B0-5 评审 P1 清偿）：缓存候选的
    /// intents 对应执行时的世界投影——先行推进/变异世界会使重试把旧 intents
    /// 结算到已变世界（tick 契约「失败后不得启动下一轮」）。先重试
    /// （run_players）或显式丢弃（discard_pending_round）。
    fn no_pending_round(&self, op: &str) -> anyhow::Result<()> {
        if self.pending.is_some() {
            anyhow::bail!(
                "{op} 被拒：存在未处置的失败轮候选（先 run_players 重试或 discard_pending_round）"
            );
        }
        Ok(())
    }

    /// RunResult → PlayerRun（executed 判定 + console 捕获）。executed 口径：
    /// Terminated（停跑，代码未执行）或 inject_failed（注入失败，同未执行）
    /// = false（适配器普通 Error 路径——此时 expr.kind 不会被读取，形态
    /// 兼容）；软异常 runtime = 已执行（错误经 expr 携带走 RunPlayerError）。
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

    /// 推进 n tick（无玩家执行；单次结算失败时保持该 tick 开始前的世界状态）。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        self.no_pending_round("tick")?;
        for _ in 0..count {
            self.engine
                .tick_with_intents_with_input(IntentBatch::default(), TickInput::now())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(())
    }

    /// 对象快照：screeps-ok 契约形状（kind 族 + 玩家 getter 值，snapshot.rs；
    /// flag 不在查询面契约内 → None）。
    pub fn get_object(&self, id: &str) -> Option<Value> {
        self.source().object_snapshot(id)
    }

    /// 房间 FIND 查询：契约快照形状（过滤复用 matches_find 单源语义）。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.source().find_in_room(room, find_const)
    }

    pub fn get_game_time(&self) -> u64 {
        self.source().game_time()
    }

    pub fn capture_console_logs(&self, handle: &str) -> Vec<String> {
        self.last_console.get(handle).cloned().unwrap_or_default()
    }

    pub fn get_controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.source().controller_pos(room)
    }

    /// 世界读取面（seam——WorldSource 的动态分发位；M2B 新 getter 一律经此，
    /// 不直连 kk_world::World）。
    fn source(&self) -> &dyn WorldSource {
        &self.engine.world
    }

    /// 性能观测点汇总（阶段耗时 p50/p95/max，ms）。
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

    /// 观测点清零（预热段不计入汇总——perf-probe 的 warmup 用）。
    pub fn perf_reset(&mut self) {
        self.perf.ticks.clear();
    }

    /// 测试钩子：下一次世界结算注入失败（部分提交三连回归场景②）。
    #[doc(hidden)]
    pub fn fail_next_settle_for_test(&mut self) {
        self.fail_next_settle = true;
    }

    /// 是否存在未提交的失败轮候选（部分提交三连回归的断言面）。
    pub fn has_pending_round(&self) -> bool {
        self.pending.is_some()
    }

    /// 显式丢弃失败轮缓存及其计费候选（回退语义的处置面；重试路径见
    /// run_players 的 pending 分支）。
    pub fn discard_pending_round(&mut self) {
        if let Some(pending) = self.pending.take()
            && let Some(players) = &mut self.players
        {
            for handle in pending.results.keys() {
                let _ = players.discard_bill(handle);
            }
        }
    }

    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.pending = None;
        self.last_console.clear();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}
