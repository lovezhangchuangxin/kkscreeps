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

use crate::mod_vm::ModVm;
use crate::{PlayerExecutor, RunEpoch, ThreadExecutor, WorldSource};
use kk_engine::intent::IntentBatch;
use kk_engine::settle::RoomCtx;
use kk_engine::settle::contrib::{SlotCall, SlotExecutor};
use kk_engine::{
    PlayerWriteback, RunOutcome, RunResult, RunState, SegmentWriteback, TickInput, TickStage,
};
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
    /// 是否经 KKPV 二进制通道向沙箱注入世界数据；默认走 JSON 参考通道。
    /// 两条通道装配出的玩家可见面等价，测试按此锁定。
    kkpv_channel: bool,
    /// 公开地形束的发布库：按代际发布/获取共享束，承载保留报告
    /// （固定内存测量面）。仅 KKPV 通道使用。
    kkpv_public_store: crate::kkpv_store::KkpvBlockStore,
    /// 可信 Mod 注册与结算 realm；仅 `create_shard_with_mods` 装配。持有它使
    /// EmbedEngine 不再可跨线程移动（结算与 realm 须同线程）。
    mod_realm: Option<Rc<RefCell<ModVm>>>,
    /// 分片计划是否含 Mod 意图；决定玩家沙箱是否装配扩展意图原语。
    ext_intents: bool,
    /// 冻结计划导出的 Mod 玩家面目录；投影通道按它裁切 Mod 对象。
    player_surface: Option<std::sync::Arc<kk_mod_api::player_surface::PlayerSurface>>,
    /// 注入玩家沙箱的 Mod 玩家面安装清单（模块集 + 查询常量 + emit 目录）。
    mod_player_api: Option<std::sync::Arc<kk_runner::ModPlayerApi>>,
}

/// JS 组合的整槽执行器。进入 VM 一次，native 中心只经普通 op 执行。
struct ModSlotExecutor {
    realm: Rc<RefCell<ModVm>>,
}

enum NativeCall {
    Action {
        data: Box<kk_engine::intent::IntentData>,
        step: fn(&mut kk_engine::settle::Working, ObjId, &kk_engine::intent::IntentData),
    },
    Tick(fn(&mut kk_engine::settle::Working, ObjId)),
    None,
}

struct ModChainContext {
    ctx: RoomCtx,
    id: ObjId,
    args: Value,
    native: NativeCall,
    /// 仅在本次能力内缓存已解析字段；改写与 native 前失效，不跨槽/对象/tick。
    cached_fields: Option<Value>,
    /// 自有动作的 namespace 写入暂存到调用成功；拒绝时无需改动共享候选。
    staged_states: Option<Rc<RefCell<BTreeMap<String, Value>>>>,
}

impl SlotExecutor for ModSlotExecutor {
    fn run(
        &self,
        slot: &SlotPlan,
        ctx: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<SlotOutcome, String> {
        // 仅自有对象动作可拒绝：它的写面只有本对象字段与 namespace state，
        // 没有 native/ID/玩家/事件写面，因而可使用这个局部恢复边界。
        let extension = matches!(&call, SlotCall::Extension { .. });
        let (id, args, native) = match call {
            SlotCall::Action { id, data, native } => (
                id,
                json!({
                    "id": data.id.map(|id| id.as_str()), "amount": data.amount, "resourceType": data.resource_type,
                }),
                NativeCall::Action {
                    data: Box::new(data.clone()),
                    step: native,
                },
            ),
            SlotCall::Tick { id, native } => (id, Value::Null, NativeCall::Tick(native)),
            SlotCall::Extension { id, args } => (id, args.clone(), NativeCall::None),
        };
        let room = ctx.borrow().room.name.clone();
        let original_fields = if extension {
            Some(
                ctx.borrow()
                    .mod_fields(id)
                    .ok_or("扩展动作目标不是有效 Mod 对象")?,
            )
        } else {
            None
        };
        let staged_states = extension.then(|| Rc::new(RefCell::new(BTreeMap::new())));
        let mut vm = self
            .realm
            .try_borrow_mut()
            .map_err(|_| format!("槽 {} @ {room}: realm 已在执行", slot.key))?;
        let outcome = vm
            .realm_mut()?
            .invoke_chain(
                slot.key.as_str(),
                Box::new(ModChainContext {
                    ctx: ctx.clone(),
                    id,
                    args,
                    native,
                    cached_fields: None,
                    staged_states: staged_states.clone(),
                }),
            )
            .map_err(|error| {
                // 引擎仍接收原有字符串错误，另在 VM 生命周期容器中保留类型化证据。
                vm.last_fault
                    .get_or_insert_with(|| crate::ModSettlementFault {
                        tick: ctx.borrow().time,
                        room: room.clone(),
                        target: id.as_str(),
                        error: error.clone(),
                    });
                format!("槽 {} @ {room}: {error}", slot.key)
            })?;
        drop(vm);
        match &outcome {
            SlotOutcome::Completed => {
                if let Some(states) = staged_states {
                    let states = std::mem::take(&mut *states.borrow_mut());
                    for (namespace, value) in states {
                        ctx.borrow().state.set_mod_state(&namespace, value);
                    }
                }
            }
            SlotOutcome::Declined(_) => {
                let fields = original_fields.ok_or("该槽没有正常拒绝的局部恢复边界")?;
                ctx.borrow_mut()
                    .update_mod_fields(id, &fields)
                    .map_err(|error| format!("动作拒绝恢复失败：{error}"))?;
            }
        }
        Ok(outcome)
    }
}

impl ModChainContext {
    fn fields(&mut self) -> Result<&Value, String> {
        if self.cached_fields.is_none() {
            self.cached_fields = Some(self.load_fields()?);
        }
        Ok(self.cached_fields.as_ref().expect("已加载字段"))
    }

    fn load_fields(&self) -> Result<Value, String> {
        let w = self.ctx.borrow();
        match w.room.objects.get(&self.id) {
            Some(kk_schema::room::RoomObject::Mod(_)) => w
                .mod_fields(self.id)
                .ok_or_else(|| "Mod payload 非法".into()),
            Some(kk_schema::room::RoomObject::Generic(object)) => {
                let fields: Value =
                    serde_json::from_str(&object.data).map_err(|error| error.to_string())?;
                if fields["structureType"] != "link" {
                    return Err("内建目标不是 link".into());
                }
                Ok(fields)
            }
            _ => Err(format!("槽目标 {} 已不存在或未接线", self.id.as_str())),
        }
    }

    fn write_fields(&mut self, fields: &Value) -> Result<(), String> {
        if fields.is_null() {
            return Ok(());
        }
        if !fields.is_object() {
            return Err("target.fields 终值须为对象或 null".into());
        }
        if self.fields()? == fields {
            return Ok(());
        }
        let is_mod = matches!(
            self.ctx.borrow().room.objects.get(&self.id),
            Some(kk_schema::room::RoomObject::Mod(_))
        );
        if is_mod {
            self.cached_fields = None;
            return self.ctx.borrow_mut().update_mod_fields(self.id, fields);
        }
        // 当前内建写面仅接 cooldown；身份/坐标和其余字段只读。后续规则
        // 字段按领域校验逐项开放，不能把 Generic JSON 整写当通用事务 API。
        let mut current = self.fields()?.clone();
        let mut proposed = fields.clone();
        let cooldown = proposed
            .as_object_mut()
            .expect("上面已确认对象")
            .remove("cooldown");
        current
            .as_object_mut()
            .ok_or("link 记录不是对象")?
            .remove("cooldown");
        if proposed != current {
            return Err("link 当前仅允许改写 cooldown，其余字段只读".into());
        }
        if let Some(value) = &cooldown {
            if value.as_u64().is_none_or(|n| n > u64::from(u32::MAX)) {
                return Err("link.cooldown 须为 u32 整数".into());
            }
            current["cooldown"] = value.clone();
        }
        self.cached_fields = None;
        if let Some(kk_schema::room::RoomObject::Generic(object)) =
            self.ctx.borrow_mut().room.objects.get_mut(&self.id)
        {
            object.data = current.to_string();
            Ok(())
        } else {
            Err("link 目标已失效".into())
        }
    }
}

impl ChainHost for ModChainContext {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String> {
        let key = Key::new(binding)?;
        let fields = self.fields()?.clone();
        let w = self.ctx.borrow();
        let state = self
            .staged_states
            .as_ref()
            .and_then(|states| states.borrow().get(key.namespace()).cloned())
            .unwrap_or_else(|| w.state.mod_state(key.namespace()));
        let tick = w.time;
        let room = w.room.name.clone();
        drop(w);
        // 固定封套不再建动态 Map；直接交给 serde_v8 的 struct 路径。
        // 缓存字段与固定 args 仍各自克隆，保持候选隔离。
        Ok(ChainContext {
            tick,
            room,
            state,
            target: ChainTarget {
                id: self.id.as_str(),
                fields,
            },
            args: self.args.clone(),
        })
    }

    fn write(&mut self, binding: &str, context: ChainWrite) -> Result<(), String> {
        let key = Key::new(binding)?;
        if let Some(fields) = context.target_fields.as_ref() {
            self.write_fields(fields)?;
        }
        // 字段校验完成后移交 state；固定封套不再构造或拆除 JSON Map。
        if let Some(states) = &self.staged_states {
            states
                .borrow_mut()
                .insert(key.namespace().to_string(), context.state);
        } else {
            self.ctx
                .borrow()
                .state
                .set_mod_state(key.namespace(), context.state);
        }
        Ok(())
    }

    fn native(&mut self) -> Result<(), String> {
        // native 可改资源、字段或移除目标，返回后的 read 必须重新取候选。
        self.cached_fields = None;
        match &self.native {
            NativeCall::Action { data, step } => step(&mut self.ctx.borrow_mut(), self.id, data),
            NativeCall::Tick(step) => step(&mut self.ctx.borrow_mut(), self.id),
            NativeCall::None => return Err("Mod 自有槽没有 Rust native 中心".into()),
        }
        Ok(())
    }
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

/// 私有块的查看者摘要：SHA-256(玩家名)。audience_hash 只是宿主定义的
/// 一致性摘要，不是授权凭证（设计见 domain-projection.md §6）。
fn kkpv_audience_hash(viewer: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(viewer.as_bytes());
    hasher.finalize().into()
}

/// 公开地形束的受众组摘要：SHA-256("kkpv-public-terrain-v1")。公开通道
/// 对全部查看者使用同一值——束内容与查看者无关，摘要只承载"哪个公开
/// 受众组"的一致性校验与缓存分组，同样不是授权凭证。
const KKPV_PUBLIC_AUDIENCE_HASH: [u8; 32] = [
    0x52, 0x96, 0x13, 0x9a, 0x7f, 0x68, 0xc1, 0x8a, 0x0a, 0xe7, 0x05, 0x50, 0xa1, 0xb4, 0x17, 0x76,
    0x5c, 0x9c, 0x88, 0xa3, 0x29, 0xcc, 0x59, 0xe8, 0xc7, 0xfe, 0x27, 0x9e, 0xc8, 0x4a, 0xdb, 0x7b,
];

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

/// 校验玩家名及 GCL，统一转成累计点数；power 尚未接入账户状态。
/// 默认等级给已有房间留一次扩张空间，来源：screeps-ok PlayerSpec/gclPoints。
fn player_setups(spec: &Value) -> anyhow::Result<Vec<(String, i64)>> {
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
        let owned_rooms = spec["rooms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|room| room["owner"].as_str() == Some(name))
            .count();
        let default_level = (owned_rooms + 1).max(2) as f64;
        let (level, progress) = match entry.get("gcl") {
            None => (default_level, 0.0),
            Some(gcl) => {
                let level = gcl
                    .get("level")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| anyhow::anyhow!("PlayerSpec {name}: gcl.level 必须为正整数"))?;
                let progress = match gcl.get("progress") {
                    None => 0.0,
                    Some(value) => value.as_f64().ok_or_else(|| {
                        anyhow::anyhow!("PlayerSpec {name}: gcl.progress 必须为非负整数")
                    })?,
                };
                (level, progress)
            }
        };
        let threshold = |level: f64| {
            (kk_core::constants::GCL_MULTIPLY as f64
                * (level - 1.0).powf(kk_core::constants::GCL_POW))
            .ceil()
        };
        let points = threshold(level) + progress;
        let reads_back = (points / kk_core::constants::GCL_MULTIPLY as f64)
            .powf(1.0 / kk_core::constants::GCL_POW)
            .floor()
            + 1.0;
        // 玩家 JS 使用 Number，拒绝不能精确表示的点数，避免饱和转换或等级失真。
        if level < 1.0
            || level.fract() != 0.0
            || progress < 0.0
            || progress.fract() != 0.0
            || !points.is_finite()
            || points > 9_007_199_254_740_991.0
            || points >= threshold(level + 1.0)
            || reads_back != level
        {
            anyhow::bail!("PlayerSpec {name}: gcl 等级、进度或累计点数非法");
        }
        handles.push((name.to_string(), points as i64));
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
            mod_realm: None,
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
            ext_intents: false,
            player_surface: None,
            mod_player_api: None,
            kkpv_channel: false,
            kkpv_public_store: crate::kkpv_store::KkpvBlockStore::new(),
        }
    }

    /// 校验房间和玩家输入后，回收旧沙箱并建立新分片。
    /// 输入校验失败时保留旧分片；后续沙箱注册或对象初始化失败不恢复旧分片。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        self.create_shard_inner(spec, None)
    }

    /// `mod_install` 在引擎重置后、玩家沙箱 spawn 前生效——玩家常量与
    /// 结算读同一份已装配规则，不允许两个平面看到不同的 link 规则。
    fn create_shard_inner(
        &mut self,
        spec: &Value,
        mod_install: Option<(&BTreeMap<String, ModPackage>, &RegisterOptions)>,
    ) -> anyhow::Result<()> {
        // 输入校验先于回收旧沙箱，失败时保留现有分片；其后的 mod 注册、
        // 玩家面导出与 canary 安装失败发生在回收之后——旧分片已销毁、
        // 引擎已重置为空（零房间零玩家），宿主须重新调用创建接口。
        let rooms = room_setups(spec)?;
        let handles = player_setups(spec)?;

        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.mod_realm = None;
        self.ext_intents = false;
        self.player_surface = None;
        self.mod_player_api = None;
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
        // 新分片从 tick 0 重新计代；旧公开束句柄全部作废（沙箱先于此
        // 回收，不存在跨分片的合法持有者）。
        self.kkpv_public_store = crate::kkpv_store::KkpvBlockStore::new();

        if let Some((packages, options)) = mod_install {
            let mut realm = ModRealm::with_options(options)
                .map_err(|error| anyhow::anyhow!("mod realm：{error}"))?;
            let registered = realm
                .register(packages, options.clone())
                .map_err(|error| anyhow::anyhow!("mod 注册：{error}"))?;
            // 玩家面目录先于引擎装配导出：保留键冲突/类名非法/条件可见性
            // 在此失败，不产出半装好的分片。
            let surface = std::sync::Arc::new(
                kk_mod_api::player_surface::PlayerSurface::from_plan(&registered.plan)
                    .map_err(|error| anyhow::anyhow!("mod 玩家面：{error}"))?,
            );
            let mod_player_api = {
                let modules = registered
                    .player_modules
                    .iter()
                    .map(|module| kk_runner::ModPlayerModule {
                        namespace: module.namespace.clone(),
                        entry: module.entry.clone(),
                        files: module.files.clone(),
                    })
                    .collect::<Vec<_>>();
                let types = surface
                    .types
                    .iter()
                    .map(|ty| {
                        (
                            ty.key.as_str().to_string(),
                            ty.find_const,
                            ty.look_key.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let intents = surface
                    .intents
                    .iter()
                    .map(|key| key.as_str().to_string())
                    .collect::<Vec<_>>();
                (!modules.is_empty() || !types.is_empty() || !intents.is_empty()).then(|| {
                    std::sync::Arc::new(kk_runner::ModPlayerApi {
                        modules: std::sync::Arc::new(modules),
                        types,
                        intents,
                    })
                })
            };
            // 玩家面安装先在一次性 canary 沙箱上完整验证（清单注入 → 模块
            // 装载 → install 调用 → finalize）：坏 player.js（导入缺陷、
            // install 抛错、缺绑定）在引擎装配与玩家 spawn 之前失败，
            // 不留下「计划已装 + 部分玩家已 spawn」的中间态。
            let has_mod_intents = registered
                .plan
                .intents()
                .iter()
                .any(|intent| intent.key.namespace() != "builtin");
            if let Some(api) = &mod_player_api {
                kk_runner::Sandbox::new(kk_runner::SandboxConfig {
                    extension_intents: has_mod_intents,
                    mod_api: Some(api.clone()),
                    ..kk_runner::SandboxConfig::default()
                })
                .map_err(|error| anyhow::anyhow!("Mod 玩家面预检失败：{error}"))?;
            }
            let realm = Rc::new(RefCell::new(ModVm::new(
                realm,
                packages.clone(),
                options.clone(),
                &registered,
            )));
            let executor = Rc::new(ModSlotExecutor {
                realm: realm.clone(),
            });
            let replacements = registered
                .contributions
                .iter()
                .flat_map(|contribution| &contribution.handlers)
                .filter(|handler| handler.kind == kk_mod_api::HandlerKind::Replace)
                .map(|handler| handler.binding.clone())
                .collect();
            let plan = std::sync::Arc::new(registered.plan.clone());
            self.engine
                .install_hosted_plan(plan, executor, registered.executables.clone(), replacements)
                .map_err(anyhow::Error::msg)?;
            // 装配成功后才置位：失败路径保持 vanilla 语义与标志一致。
            self.ext_intents = has_mod_intents;
            self.player_surface = Some(surface);
            self.mod_player_api = mod_player_api;
            self.mod_realm = Some(realm);
        }

        let players = self.players.get_or_insert_with(ThreadExecutor::new);
        // 先取走全部配置，避免注册中途失败后残留配置被下一次创建意外使用。
        let configs = std::mem::take(&mut self.sandbox_configs_for_test);
        for (handle, gcl) in handles {
            let mut config = configs.get(&handle).cloned().unwrap_or_default();
            // link 有效规则同步到玩家全局常量。计划是常量的
            // 权威来源，测试配置里的同名覆盖以引擎规则为准。
            config.constant_overrides.insert(
                "LINK_CAPACITY".to_string(),
                serde_json::json!(self.engine.links().capacity),
            );
            // 扩展意图原语与玩家面安装清单的装配由引擎计划决定，测试
            // 配置不得改写。
            config.extension_intents = self.ext_intents;
            config.mod_api = self.mod_player_api.clone();
            let cpu = kk_core::cpu::Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            );
            players.spawn(&handle, config)?;
            self.engine.players.entry(handle.clone()).or_default().cpu = cpu;
            self.engine.players.entry(handle.clone()).or_default().gcl = gcl;
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

    /// 以 Mod 包装建分片：注册、玩家面导出/预检与计划装配发生在引擎
    /// 重置后、玩家沙箱 spawn 前（常量与结算同源）。可装配面是 link 两槽
    /// 的 before/wrap/after/replace 处理器、规则，以及 Mod 自有动作入口的对象
    /// 域执行链。JS 贡献由整槽宿主组合，native 只经普通 op 修改候选。
    /// 对象域外的执行形态（房间/玩家/
    /// 世界域、队列合并、非严格转换、显式入口）同样显式拒绝。玩家面目录导出与
    /// 安装预检先于引擎装配；后续失败不恢复旧分片，已装配计划也可能留存。
    /// 这与 create_shard 的失败语义一致——调用方
    /// 须以 teardown/重建收尾，不得复用半装状态）。
    pub fn create_shard_with_mods(
        &mut self,
        spec: &Value,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
    ) -> anyhow::Result<()> {
        self.create_shard_inner(spec, Some((packages, &options)))
    }

    /// 按适配器的对象类别和初始字段摆放对象，返回对象 ID。
    /// 字段归一化由 [kk_world::place::normalize] 完成，显式字段优先。
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

    /// 管理命令：按已装配计划里的 Mod 类型创建对象并放入房间，返回对象 ID。
    /// `fields` 须提供除带默认值外的全部声明字段（挂载字段同样要求）；
    /// 缺失的有默认值字段落入默认，整体通过 schema 校验后写入世界存储。
    /// 声明位置字段的类型以 payload 位置为准（信封坐标同步）。
    pub fn place_mod_object(
        &mut self,
        room: &str,
        type_key: &str,
        fields: &Value,
    ) -> anyhow::Result<String> {
        self.no_open_round("placeModObject")?;
        let descriptor = self
            .engine
            .mod_object_type(type_key)
            .ok_or_else(|| {
                anyhow::anyhow!("类型 {type_key} 不在已装配计划中（Mod 对象须经 Mod 分片创建）")
            })?
            .clone();
        let Some(provided) = fields.as_object() else {
            anyhow::bail!("Mod 对象字段须为 JSON 对象");
        };
        let mut payload = serde_json::Map::new();
        for field in &descriptor.fields {
            let name = kk_mod_api::validate::field_name(&field.key);
            match provided.get(name) {
                Some(value) => {
                    payload.insert(name.to_string(), value.clone());
                }
                None => match &field.default {
                    kk_mod_api::FieldDefault::Value(value) => {
                        payload.insert(name.to_string(), value.clone());
                    }
                    kk_mod_api::FieldDefault::Missing => {
                        anyhow::bail!("类型 {type_key} 的字段 {name} 未提供且无默认值")
                    }
                },
            }
        }
        for key in provided.keys() {
            if !descriptor
                .fields
                .iter()
                .any(|f| kk_mod_api::validate::field_name(&f.key) == key)
            {
                anyhow::bail!("字段 {key} 未在类型 {type_key} 中声明");
            }
        }
        let mut payload = Value::Object(payload);
        kk_mod_api::validate::check_object_payload(&payload, &descriptor.fields, type_key)
            .map_err(anyhow::Error::msg)?;
        // 数值 backing 规范化（与结算写入口同口径——Float 域统一浮点
        // backing，防 KKPV 列型混派）。
        kk_mod_api::validate::normalize_object_payload(&mut payload, &descriptor.fields);
        let (x, y) = match &descriptor.position_fields {
            Some([fx, fy]) => {
                let read = |key: &kk_mod_api::Key| {
                    u8::try_from(
                        payload
                            .get(kk_mod_api::validate::field_name(key))
                            .and_then(Value::as_u64)
                            .expect("payload 已通过校验，位置字段必为 0..49 整数"),
                    )
                    .expect("0..49 在 u8 内")
                };
                (read(fx), read(fy))
            }
            None => (0, 0),
        };
        let object = kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
            id: 0,
            x,
            y,
            type_key: type_key.to_string(),
            data: payload.to_string(),
        });
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
        // 恢复失败不能取走候选或重新执行玩家；重建只读取原启动快照。
        self.rebuild_mod_realm()?;
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
        self.rebuild_mod_realm()?;
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
        if let Some(vm) = &self.mod_realm {
            let mut vm = vm.borrow_mut();
            // 新的实际结算清除上一轮诊断；恢复和结算前的拒绝不清除。
            vm.last_fault = None;
            vm.realm_mut()
                .map_err(anyhow::Error::msg)?
                .begin_settlement()
                .map_err(anyhow::Error::msg)?;
        }
        let mut settled = self
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
        if let Some(vm) = &self.mod_realm {
            vm.borrow_mut()
                .realm_mut()
                .map_err(anyhow::Error::msg)?
                .end_settlement();
        }
        if let Some(vm) = &self.mod_realm {
            let mut vm = vm.borrow_mut();
            if let Some(decision) = vm.finish_settlement(settled.is_ok()) {
                let error = settled.expect_err("自动停用只能发生在结算失败后");
                settled = Err(anyhow::anyhow!(
                    "{error}；连续 {} 次可归责故障，已自动停用 Mod [{}]；本轮未发布，下次重试使用新链",
                    decision.consecutive_faults,
                    decision
                        .disabled
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if settled.is_err() {
                vm.invalidate();
            }
        }
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
        // KKPV 通道的公开地形束按代际发布一次、整轮共享：acquire 命中
        // （同代际重入或重试轮复投影）不重编码；miss 才投影+编码+发布。
        // 耗时归入投影相。失败即整轮失败——没有公开束任何玩家都无法装配。
        let mut kkpv_public: Option<kk_runner::SharedKkpvBundle> = None;
        if self.kkpv_channel {
            let t_public = std::time::Instant::now();
            match self.public_bundle_for_epoch(source, epoch) {
                Ok(bundle) => kkpv_public = Some(bundle),
                Err(e) => failure = Some(e.to_string()),
            }
            project_ns += t_public.elapsed().as_nanos();
        }
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
                // KKPV 通道：对象记录走二进制块束，world 是不含 objects 的信封；
                // JSON 参考通道：整世界一个 Value。市场都注入 world 顶层。
                let (world, kkpv_bundle) = if self.kkpv_channel {
                    match source.kkpv_projection_for_viewer(
                        &handle,
                        gcl,
                        self.engine.extensions(),
                        self.player_surface.as_deref(),
                        self.engine.plan_hash().bytes(),
                        kkpv_audience_hash(&handle),
                    ) {
                        Some(projection) => match projection
                            .map_err(|e| anyhow::anyhow!("KKPV 投影失败（玩家 {handle}）: {e}"))
                            .and_then(|(mut envelope, blocks)| {
                                if let Some(obj) = envelope.as_object_mut() {
                                    obj.insert("market".into(), market_view(&self.engine, &handle));
                                }
                                kk_schema::kkpv::encode_bundle(
                                    &blocks,
                                    &kk_schema::kkpv::KkpvLimits::default(),
                                )
                                .map_err(|e| {
                                    anyhow::anyhow!("KKPV 块束编码失败（玩家 {handle}）: {e}")
                                })
                                .map(|bundle| (envelope, Some(bundle)))
                            }) {
                            Ok(pair) => pair,
                            Err(e) => {
                                // 投影耗时归入投影相，不混入执行相。
                                project_ns += t_project.elapsed().as_nanos();
                                failure = Some(e.to_string());
                                continue;
                            }
                        },
                        None => {
                            project_ns += t_project.elapsed().as_nanos();
                            failure = Some(format!("世界源未提供 KKPV 投影（玩家 {handle}）"));
                            continue;
                        }
                    }
                } else {
                    let mut world = source.world_json_for_viewer_with_extensions(
                        &handle,
                        gcl,
                        self.engine.extensions(),
                        self.player_surface.as_deref(),
                    );
                    // 市场读取已提交的引擎状态，随世界 JSON 注入，不存放在 V8 持久闭包中。
                    if let Some(obj) = world.as_object_mut() {
                        obj.insert("market".into(), market_view(&self.engine, &handle));
                    }
                    (world, None)
                };
                project_ns += t_project.elapsed().as_nanos();
                let budget = kk_core::cpu::RunBudget::new(&self.engine.players[&handle].cpu);
                match self.players.as_mut().expect("已检查分片创建").dispatch_job(
                    &handle,
                    kk_runner::RunJob {
                        world,
                        kkpv_bundle,
                        kkpv_public: kkpv_public.clone(),
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
        if self.needs_mod_recovery() {
            anyhow::bail!("{op} 被拒：Mod VM 待恢复；先 rebuild_mod_realm 或重试结算");
        }
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

    /// 已提交的某 Mod 命名空间状态（诊断与测试面；未初始化为 null）。
    pub fn engine_mod_state(&self, namespace: &str) -> Value {
        self.engine
            .mod_states
            .get(namespace)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// 当前世界代际（失败轮不前进；诊断与测试面）。
    pub fn engine_generation(&self) -> u64 {
        self.engine.world.generation()
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

    /// 获取当前代际的公开地形束共享句柄；未发布则投影、编码并发布。
    /// 同代际的重入（一轮内多次 run_players、失败轮重投影）命中缓存
    /// 不重编码；结算推进 tick 或世界变异推进 generation 后的首次获取
    /// 重编码（块头 tick 与信封 time 交叉校验要求束随代际更新）。
    /// 零注册房世界直接下发空束（库发布要求可校验的块版本，空束没有；
    /// 沙箱侧空公开束解出空地形表，与 JSON 通道的零房行为一致）。
    ///
    /// # Errors
    /// 世界源不提供公开块、编码失败或发布校验失败时返回错误。
    fn public_bundle_for_epoch(
        &self,
        source: &dyn WorldSource,
        epoch: RunEpoch,
    ) -> anyhow::Result<kk_runner::SharedKkpvBundle> {
        if let Some(handle) = self.kkpv_public_store.acquire(epoch) {
            if let Some(bundle) = handle.shared_bundle() {
                return Ok(bundle.clone());
            }
            anyhow::bail!("公开束载荷不是共享形态（发布库被误用）");
        }
        let blocks = source
            .kkpv_public_terrain_blocks(self.engine.plan_hash().bytes(), KKPV_PUBLIC_AUDIENCE_HASH)
            .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 公开地形块"))?;
        let bytes =
            kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                .map_err(|e| anyhow::anyhow!("KKPV 公开束编码失败：{e}"))?;
        if blocks.is_empty() {
            return Ok(kk_runner::SharedKkpvBundle::from_vec(bytes));
        }
        let handle = self.kkpv_public_store.publish_shared(
            epoch,
            bytes,
            &kk_schema::kkpv::KkpvLimits::default(),
        )?;
        Ok(handle
            .shared_bundle()
            .expect("publish_shared 载荷恒为共享形态")
            .clone())
    }

    /// 读取公开束保留报告（固定内存测量面）：仍被句柄持有的代际、
    /// 字节数与库外句柄数。V8 侧 ArrayBuffer 引用只延长真实存活，
    /// 不进入读数——报告是存活下界。
    pub fn kkpv_public_retention_report(&self) -> Vec<crate::kkpv_store::RetainedBlock> {
        self.kkpv_public_store.retention_report()
    }

    /// 切换世界注入通道：true 走 KKPV 二进制块束，false（默认）走 JSON
    /// 参考通道。两条通道装配出的玩家可见面等价，集成测试按此锁定。
    pub fn set_kkpv_channel(&mut self, enabled: bool) {
        self.kkpv_channel = enabled;
    }

    /// 采样指定玩家当前世界的注入体积（诊断/基准用）：JSON 通道是整
    /// 世界串字节，KKPV 通道是信封串 + 私有束字节 + 公开束字节（共享，
    /// 全玩家一份）+ 公开束保留报告。只读世界、复用与派发相同的投影
    /// 入口（含 market 注入），不在任何计时窗口内调用。
    pub fn injection_size_sample(&self, handle: &str) -> anyhow::Result<Value> {
        let source: &dyn WorldSource = &self.engine.world;
        let gcl = self.engine.players.get(handle).map(|p| p.gcl).unwrap_or(0);
        if self.kkpv_channel {
            let audience = kkpv_audience_hash(handle);
            let (mut envelope, blocks) = source
                .kkpv_projection_for_viewer(
                    handle,
                    gcl,
                    self.engine.extensions(),
                    self.player_surface.as_deref(),
                    self.engine.plan_hash().bytes(),
                    audience,
                )
                .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 投影"))?
                .map_err(|e| anyhow::anyhow!("KKPV 投影失败：{e}"))?;
            if let Some(obj) = envelope.as_object_mut() {
                obj.insert("market".into(), market_view(&self.engine, handle));
            }
            let bundle =
                kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                    .map_err(|e| anyhow::anyhow!("KKPV 块束编码失败：{e}"))?;
            // 公开束按当前世界重算字节数（不发布、不动库）；块数与私
            // 有束的房间集一致（全注册房间）。
            let public_blocks = source
                .kkpv_public_terrain_blocks(
                    self.engine.plan_hash().bytes(),
                    KKPV_PUBLIC_AUDIENCE_HASH,
                )
                .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 公开地形块"))?;
            let public_bundle = kk_schema::kkpv::encode_bundle(
                &public_blocks,
                &kk_schema::kkpv::KkpvLimits::default(),
            )
            .map_err(|e| anyhow::anyhow!("KKPV 公开束编码失败：{e}"))?;
            let retention: Vec<Value> = self
                .kkpv_public_retention_report()
                .into_iter()
                .map(|r| {
                    json!({
                        "tick": r.epoch.tick,
                        "generation": r.epoch.generation,
                        "bytes": r.bytes,
                        "handles": r.handles,
                    })
                })
                .collect();
            Ok(json!({
                "channel": "kkpv",
                "envelope_bytes": serde_json::to_string(&envelope)?.len(),
                "bundle_bytes": bundle.len(),
                "public_bundle_bytes": public_bundle.len(),
                "public_shared": true,
                "blocks": blocks.len(),
                "public_retained": retention,
            }))
        } else {
            let mut world = source.world_json_for_viewer_with_extensions(
                handle,
                gcl,
                self.engine.extensions(),
                self.player_surface.as_deref(),
            );
            if let Some(obj) = world.as_object_mut() {
                obj.insert("market".into(), market_view(&self.engine, handle));
            }
            Ok(json!({
                "channel": "json",
                "world_json_bytes": serde_json::to_string(&world)?.len(),
            }))
        }
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
            vm.borrow_mut().recover().map_err(anyhow::Error::msg)?;
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

    /// 停止所有玩家线程，清空世界、失败缓存和 console。
    pub fn teardown(&mut self) {
        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.mod_realm = None;
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
