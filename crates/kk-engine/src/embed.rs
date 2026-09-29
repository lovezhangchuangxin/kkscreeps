//! 嵌入式 API：以库形式驱动引擎（ADR-008 第 8 条）。
//!
//! 方法集与 screeps-ok 适配器契约同构，与将来 `kkscreeps sim` 驱动引擎的方式
//! 是同一个接口——适配器（compat/，经 `kkscreeps embed` sidecar 的 JSON-RPC，
//! ADR-010）与模拟器共用本模块，一石二鸟。
//!
//! 世界为 kk_world::World（typed）；tick/runPlayer 的推进走真实结算 settle_room
//! （intents 恒空——玩家 intent 收集通道是 M2-2A，当前玩家代码在真实 deno_core
//! 沙箱执行但不产出 intent）；世界投影 world_json 保持 M0 契约形状（prelude
//! 消费面零改动）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::Terrain;
use kk_runner::{EvalReport, PlayerThreads, SandboxConfig};

use crate::intent::IntentBatch;
use kk_world::place::{PlaceCtx, normalize};
use serde_json::{Value, json};

use crate::settle::Engine;

/// runPlayer 的结局（TS 适配器据此映射 RunPlayerError 三分类或普通 Error）。
#[derive(Debug)]
pub struct PlayerRun {
    /// 代码是否真的执行过（false = 跳帧惩罚等基础设施原因，适配器须抛普通 Error）。
    pub executed: bool,
    /// runExpr 结果：`{ok, value?} | {ok:false, kind: syntax|runtime|serialization, error}`。
    pub expr: Value,
    /// 该玩家本次执行期间的 console 输出。
    pub console: Vec<String>,
}

pub struct EmbedEngine {
    players: Option<PlayerThreads>,
    engine: Engine,
    /// 玩家 handle → 线程 id（BTreeMap：固定遍历序，确定性的前提）。
    handles: BTreeMap<String, u64>,
    next_thread_id: u64,
    last_console: BTreeMap<String, Vec<String>>,
    /// 性能观测点（A6）：每 tick 三阶段 + 墙钟采样
    perf: PerfRecorder,
}

/// 性能观测点（A6，[performance-acceptance](docs/architecture/performance-acceptance.md)
/// 「阶段耗时」口径）：run_player/run_players 每 tick 的四路采样——投影
/// （world_json_for_viewer 每玩家合计）、玩家运行（run_player 合计）、结算
/// （tick_with_intents）、tick 墙钟。Instant 读表 ~20ns，采样开销可忽略
/// （文档要求采样开销单独测量——以同负载重复运行的 p50 稳定性自证）。
/// 采样有 cap（B0 收口，A6 评审 P3 登记销账）：满 cap 后丢弃新样本（窗口
/// 保留最早段），`perf_reset` 清零重开——长寿命部署不再无界增长。
/// 正式场景基线（场景 A/C × 3 实现）随 M2B/M5；本观测点是 M2A 的架构验收
/// 通道（玩家 API → intent → 结算闭环及快照读取路径的阶段耗时）。
#[derive(Default)]
struct PerfRecorder {
    /// 每 tick 的 [project_ms, run_ms, settle_ms, wall_ms]
    ticks: Vec<[f64; 4]>,
}

/// 采样窗口上限（32B/tick × 50k ≈ 1.6MB——probe/bot-check（≤2000t）远用不满，
/// 长寿命部署的内存上界）。
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
            handles: BTreeMap::new(),
            next_thread_id: 0,
            last_console: BTreeMap::new(),
            perf: PerfRecorder::default(),
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
        self.handles.clear();
        self.last_console.clear();
        self.perf = PerfRecorder::default();

        let players = self.players.get_or_insert_with(PlayerThreads::new);
        for handle in handles {
            self.next_thread_id += 1;
            let thread_id = self.next_thread_id;
            players.spawn_player(thread_id, SandboxConfig::default())?;
            self.handles.insert(handle, thread_id);
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
        let mut t = Terrain::from_codes(&terrain).map_err(anyhow::Error::msg)?;
        t.normalize();
        self.engine
            .world
            .set_terrain(room, t)
            .map_err(anyhow::Error::msg)
    }

    /// 先运行玩家代码，再结算一个 tick。玩家看到的是结算前的世界。
    pub fn run_player(&mut self, handle: &str, code: &str) -> anyhow::Result<PlayerRun> {
        let thread_id = *self
            .handles
            .get(handle)
            .ok_or_else(|| anyhow::anyhow!("未知玩家 handle: {handle}"))?;
        let t0 = std::time::Instant::now();
        let players = self
            .players
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
        // per-viewer 视野裁切（外部复审 P1-3 修复）：注入世界只含该玩家有
        // 对象的房间 + user/gcl 字段（prelude 装配 per-viewer 集合与 Game.gcl）
        let gcl = self.engine.players.get(handle).map(|p| p.gcl).unwrap_or(0);
        let t_project = std::time::Instant::now();
        let world = self.engine.world.world_json_for_viewer(handle, gcl);
        let project_ns = t_project.elapsed().as_nanos();
        let t_run = std::time::Instant::now();
        let report: EvalReport = players.run_player(thread_id, &world, code)?;
        let run_ns = t_run.elapsed().as_nanos();
        let t_settle = std::time::Instant::now();
        self.settle_tick_with_reports(std::iter::once((handle, &report)))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let settle_ns = t_settle.elapsed().as_nanos();
        self.perf.record([
            ns_ms(project_ns),
            ns_ms(run_ns),
            ns_ms(settle_ns),
            t0.elapsed().as_secs_f64() * 1000.0,
        ]);
        let out = self.finish_player_run(handle, report);
        Ok(out)
    }

    /// 一轮结算：把各玩家沙箱带出的 intentsList 清洗成 IntentBatch 进引擎
    /// （M2A-A2 通道闭合：真实玩家代码 → intents → sanitize → settle_room →
    /// 下一 tick 世界可见）。身份边界：batch 按注册 handle 逐玩家构造，冒名面
    /// 结构上不存在。硬中断/停跑玩家的 intents 为 Null（from_json 收空批）。
    /// 墙钟输入显式经 [`TickInput::now`]（首跑记录实际值；重放路径由调用方
    /// 注入记录值——ADR-013 决策 5）。
    fn settle_tick_with_reports<'a, I>(&mut self, reports: I) -> Result<(), crate::SettleError>
    where
        I: IntoIterator<Item = (&'a str, &'a EvalReport)>,
    {
        let mut batch = IntentBatch::default();
        for (handle, report) in reports {
            if let Some(list) = report.intents.as_object() {
                batch.extend_from(IntentBatch::from_json(handle, &Value::Object(list.clone())));
            }
        }
        self.engine
            .tick_with_intents_with_input(batch, crate::settle::TickInput::now())
    }

    /// 多玩家对同一状态求值后合计推进 1 tick（runPlayers 契约）。
    pub fn run_players(
        &mut self,
        codes: &BTreeMap<String, String>,
    ) -> anyhow::Result<BTreeMap<String, PlayerRun>> {
        // 未知 handle 显式报错：静默跳过会让结果映射缺项，调用方无从得知玩家没跑
        for handle in codes.keys() {
            if !self.handles.contains_key(handle) {
                anyhow::bail!("未知玩家 handle: {handle}");
            }
        }
        let mut reports = BTreeMap::new();
        let mut project_ns = 0u128;
        let mut run_ns = 0u128;
        let t0 = std::time::Instant::now();
        {
            let players = self
                .players
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("分片未创建"))?;
            // 固定顺序（handle 排序）求值——确定性；各玩家看同一已提交世界的
            // 自家视野（官方 runtimeData.get(userId) 同构）
            for (handle, thread_id) in &self.handles {
                let Some(code) = codes.get(handle) else {
                    continue;
                };
                // gcl 点数 = PlayerState 真值（官方 runtimeData.user.gcl）
                let gcl = self.engine.players.get(handle).map(|p| p.gcl).unwrap_or(0);
                let t_project = std::time::Instant::now();
                let world = self.engine.world.world_json_for_viewer(handle, gcl);
                project_ns += t_project.elapsed().as_nanos();
                let t_run = std::time::Instant::now();
                let report = players.run_player(*thread_id, &world, code)?;
                run_ns += t_run.elapsed().as_nanos();
                reports.insert(handle.clone(), report);
            }
        }
        let t_settle = std::time::Instant::now();
        self.settle_tick_with_reports(reports.iter().map(|(h, r)| (h.as_str(), r)))
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let settle_ns = t_settle.elapsed().as_nanos();
        self.perf.record([
            ns_ms(project_ns),
            ns_ms(run_ns),
            ns_ms(settle_ns),
            t0.elapsed().as_secs_f64() * 1000.0,
        ]);
        let mut out = BTreeMap::new();
        for (handle, report) in reports {
            let run = self.finish_player_run(&handle, report);
            out.insert(handle, run);
        }
        Ok(out)
    }

    /// 直接以外部构造的 intent 批结算一轮（测试/工具面；生产路径经 run_player
    /// 的 intentsList 清洗）。身份边界同 from_json 约定：批内玩家键由调用方负责。
    pub fn settle_external(&mut self, batch: IntentBatch) -> anyhow::Result<()> {
        self.engine
            .tick_with_intents(batch)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// 求值报告 → PlayerRun（executed 判定 + console 捕获）。
    fn finish_player_run(&mut self, handle: &str, report: EvalReport) -> PlayerRun {
        let executed = report
            .expr
            .get("kind")
            .and_then(Value::as_str)
            .map(|k| k != "terminated")
            .unwrap_or(true);
        let console = report
            .console
            .iter()
            .map(|l| l.text.clone())
            .collect::<Vec<_>>();
        self.last_console
            .insert(handle.to_string(), console.clone());
        PlayerRun {
            executed,
            expr: report.expr,
            console,
        }
    }

    /// 推进 n tick；单次结算失败时保持该 tick 开始前的世界状态。
    pub fn tick(&mut self, count: u64) -> anyhow::Result<()> {
        for _ in 0..count {
            self.engine.tick().map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Ok(())
    }

    /// 对象快照：screeps-ok 契约形状（kind 族 + 玩家 getter 值，snapshot.rs；
    /// flag 不在查询面契约内 → None）。
    pub fn get_object(&self, id: &str) -> Option<Value> {
        kk_world::ObjId::parse(id).and_then(|id| self.engine.world.object_snapshot_by_id(id))
    }

    /// 房间 FIND 查询：契约快照形状（过滤复用 matches_find 单源语义）。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        self.engine.world.find_snapshots_in_room(room, find_const)
    }

    pub fn get_game_time(&self) -> u64 {
        self.engine.world.time
    }

    pub fn capture_console_logs(&self, handle: &str) -> Vec<String> {
        self.last_console.get(handle).cloned().unwrap_or_default()
    }

    pub fn get_controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.engine.world.controller_pos(room)
    }

    /// 性能观测点汇总（阶段耗时 p50/p95/max，ms）——perf-probe 与后续基准
    /// 的采集出口；ticks 数即采样数。
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

    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.handles.clear();
        self.last_console.clear();
    }
}

impl Drop for EmbedEngine {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbedEngine, terrain_codes_from_json};
    use serde_json::json;

    /// 元素级严格性：非整数（小数/字符串/null）、负数、超 255 一律拒绝。
    #[test]
    fn terrain_codes_from_json_is_strict() {
        assert_eq!(
            terrain_codes_from_json(&json!([0, 1, 2, 3])).unwrap(),
            vec![0, 1, 2, 3]
        );
        for bad in [json!(1.5), json!("1"), json!(null), json!(-1), json!(256)] {
            assert!(
                terrain_codes_from_json(&json!([bad])).is_err(),
                "元素 {bad} 必须被拒绝"
            );
        }
        assert!(terrain_codes_from_json(&json!("not-array")).is_err());
    }

    #[test]
    fn float_form_integer_rcl_is_accepted() {
        // JSON `3.0` 在 JS 语义里就是整数 3（vanilla 接受）——数值校验走 as_f64，
        // 只有字符串/非整数/越界才拒
        let mut engine = EmbedEngine::new();
        engine
            .create_shard(&json!({
                "players": [],
                "rooms": [{ "name": "W1N1", "rcl": 3.0, "owner": "p1" }],
            }))
            .unwrap();
        let level = engine
            .engine
            .world
            .room("W1N1")
            .and_then(|r| r.controller())
            .map(|ctl| ctl.level);
        assert_eq!(level, Some(3));
    }

    #[test]
    fn invalid_shard_spec_preserves_previous_world() {
        let mut engine = EmbedEngine::new();
        engine
            .create_shard(
                &json!({ "players": [], "rooms": [{ "name": "W1N1", "controller": false }] }),
            )
            .unwrap();
        engine.tick(1).unwrap();

        for invalid in [
            json!({ "players": [], "rooms": [{ "name": "W2N2", "rcl": "3" }] }),
            json!({ "players": [], "rooms": [{ "name": "W2N2", "rcl": 3.5 }] }),
            json!({ "players": [], "rooms": [{ "name": "W2N2" }, { "name": "W2N2" }] }),
            json!({ "players": ["alice", { "name": "alice" }], "rooms": [] }),
            json!({}),
            json!({ "players": [], "rooms": "W2N2" }),
            json!({ "players": "alice", "rooms": [] }),
            json!({ "players": [], "rooms": [{}] }),
            json!({ "players": [{}], "rooms": [] }),
            // 房名域校验（A5 片 2 评审 P2）：世界界外（E128 起 +128 偏移回绕/
            // E130 越界构造抛错）与不可解析名在 embed 边界显式拒绝
            json!({ "players": [], "rooms": [{ "name": "E130N0" }] }),
            json!({ "players": [], "rooms": [{ "name": "E128N0" }] }),
            json!({ "players": [], "rooms": [{ "name": "middle0" }] }),
        ] {
            assert!(engine.create_shard(&invalid).is_err());
            assert_eq!(engine.get_game_time(), 1);
            assert!(engine.engine.world.room("W1N1").is_some());
            assert!(engine.engine.world.room("W2N2").is_none());
        }
    }
}
