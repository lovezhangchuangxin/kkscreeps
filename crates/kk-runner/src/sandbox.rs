//! 持有一个玩家的 V8 实例，装配玩家 API，并生成指令、Memory 和计费候选。
//!
//! 看门狗按墙钟期限请求中断；近堆上限回调临时扩大堆上限以完成当前分配，
//! 同时请求中断，随后恢复配置上限。此限制针对 V8 堆，不覆盖所有堆外或宿主分配。
//!
//! 同一线程内的多个 V8 实例须按创建逆序销毁；本类型只检测并告警，
//! 调用方仍需保证顺序。字段顺序保证先回收看门狗，再销毁 runtime 和 tokio。
//! 每玩家一个线程的 PlayerThreads 可以回收任意玩家，无需跨线程逆序。

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use deno_core::v8;
use deno_core::{JsRuntime, RuntimeOptions};

use crate::budget::{BUCKET_CAP, Bucket, DEFAULT_CPU_LIMIT, MAX_PER_TICK, TickPlan};
use crate::modules::ModuleTable;
use crate::ops::{LogLine, Shared, SharedRef, kk_runner};
use crate::watchdog::Watchdog;
use serde_json::Value;

const LODASH_SRC: &str = include_str!("../assets/lodash-3.10.1.js");
const CONSTANTS_JSON: &str = include_str!("../assets/constants.json");
/// 按依赖顺序装配共享工具、模块加载、玩家 API 与宿主函数。
/// 各段通过临时全局 __kkp 共享闭包状态；Rust 捕获宿主函数句柄后删除 __kkp。
/// 因此 90_host 只能注册句柄，不能提前删除共享容器。
const PRELUDE_SEGMENTS: [(&str, &str); 16] = [
    ("kk-prelude-00-shared", include_str!("prelude/00_shared.js")),
    ("kk-prelude-10-buffer", include_str!("prelude/10_buffer.js")),
    ("kk-prelude-20-cjs", include_str!("prelude/20_cjs.js")),
    (
        "kk-prelude-30-game-registry",
        include_str!("prelude/30_game/_registry.js"),
    ),
    (
        "kk-prelude-30-game-kkpv",
        include_str!("prelude/30_game/kkpv.js"),
    ),
    (
        "kk-prelude-30-game-room-position",
        include_str!("prelude/30_game/room-position.js"),
    ),
    (
        "kk-prelude-30-game-room",
        include_str!("prelude/30_game/room.js"),
    ),
    (
        "kk-prelude-30-game-creep",
        include_str!("prelude/30_game/creep.js"),
    ),
    (
        "kk-prelude-30-game-creep-resources",
        include_str!("prelude/30_game/creep_resources.js"),
    ),
    (
        "kk-prelude-30-game-structures",
        include_str!("prelude/30_game/structures.js"),
    ),
    (
        "kk-prelude-30-game-structures-spawn",
        include_str!("prelude/30_game/structures_spawn.js"),
    ),
    (
        "kk-prelude-30-game-sites-flags",
        include_str!("prelude/30_game/sites-flags.js"),
    ),
    (
        "kk-prelude-30-game-resources",
        include_str!("prelude/30_game/resources.js"),
    ),
    (
        "kk-prelude-30-game-path-finder",
        include_str!("prelude/30_game/path-finder.js"),
    ),
    (
        "kk-prelude-30-game-game",
        include_str!("prelude/30_game/game.js"),
    ),
    ("kk-prelude-90-host", include_str!("prelude/90_host.js")),
];

/// 默认 V8 堆上限，单位字节；不等于整个玩家线程的内存上限。
const DEFAULT_HEAP_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub heap_limit_bytes: usize,
    pub cpu_limit: f64,
    pub bucket_cap: f64,
    pub max_per_tick: f64,
    pub initial_bucket: f64,
    /// 宿主按有效规则覆盖的玩家全局常量；默认为空，保持嵌入资产原值。
    /// 键须是常量表已有的名字，覆盖发生在平铺到 globalThis 之前。
    pub constant_overrides: std::collections::BTreeMap<String, serde_json::Value>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            heap_limit_bytes: DEFAULT_HEAP_LIMIT,
            cpu_limit: DEFAULT_CPU_LIMIT,
            bucket_cap: BUCKET_CAP,
            max_per_tick: MAX_PER_TICK,
            initial_bucket: BUCKET_CAP,
            constant_overrides: std::collections::BTreeMap::new(),
        }
    }
}

/// 一次玩家执行的结局。
#[derive(Debug)]
pub enum TickOutcome {
    /// 正常执行完毕（玩家逻辑自身的错误由 runLoop 结果 JSON 携带，不算这里）。
    Completed { used_ms: f64 },
    /// 执行被中断，包括超时、近堆上限和主动 halt。
    /// 此结果本身不提交费用；超时和近堆上限生成完整预算费用，halt 不计费。
    Terminated,
}

// ---------------------------------------------------------------- LIFO 纪律（每线程）

static SEQ: AtomicU64 = AtomicU64::new(0);
static LIVE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LIFO_VIOLATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
// 本线程存活实例的创建顺序，用于检测逆序销毁要求。
// 其他线程的实例不属于这个栈；跨线程销毁顺序不影响本线程的 Enter/Exit。
thread_local! {
    static THREAD_ORDER: std::cell::RefCell<Vec<u64>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 当前进程存活的 Sandbox 数（测试用：全部销毁后应归零）。
pub fn live_sandbox_count() -> usize {
    LIVE_COUNT.load(Ordering::SeqCst)
}

/// 线程内 LIFO 销毁纪律的违规次数（测试断言为零——告警不该被无视）。
pub fn lifo_violations() -> usize {
    LIFO_VIOLATIONS.load(Ordering::SeqCst)
}

pub struct Sandbox {
    // 字段声明顺序 = Drop 顺序：先 join 看门狗线程，再销毁 JsRuntime，最后拆
    // tokio。host（v8::Global 句柄）在 isolate 销毁后才 drop——v8 150 对已销毁
    // isolate 的 Global drop 是显式安全 no-op（handle.rs "Globals may be dropped
    // after their host isolate is disposed"）。
    watchdog: Watchdog,
    runtime: JsRuntime,
    tokio: tokio::runtime::Runtime,
    /// Rust 捕获的宿主函数句柄，不通过玩家全局名称查找生命周期入口。
    host: HostFns,
    seq: u64,
    budget: Bucket,
    command_deadline: Option<Instant>,
    shared: SharedRef,
    /// 配置的堆硬上限（near-heap 触发放大后据此恢复）。
    heap_limit: usize,
    /// 本 tick 是否已中断；end_tick 据此跳过写回。
    /// 正常 end_tick 不设置此标志，费用提交与此标志分别管理。
    terminated_this_tick: bool,
    /// 上一次 runLoop 捕获的玩家逻辑错误（run_loop 写入，take_last_loop_error 取走）。
    last_loop_error: Option<String>,
    /// 等待 take_intents 取走的本 tick 指令；中断或写回失败时丢弃。
    /// 协调器将其转换为 IntentBatch，再交给世界结算。
    pending_intents: Option<Value>,
    /// 本 tick 待提交的费用，单位毫秒：正常结束为线程 CPU 用量加 0.2×指令数，
    /// 超时或近堆上限中断为完整预算。RunJob 导出给宿主；独立诊断通道本地提交或丢弃。
    /// 新的 begin_tick 会清除尚未处理的候选，调用方须先处理上一轮结果。
    pending_bill: Option<f64>,
    /// 负余额停跑时暂存的回充增量，与计费候选互斥。
    /// settle_bill 应用回充，discard_bill 丢弃；新的 begin_tick 清除旧候选。
    pending_recharge: Option<f64>,
}

impl Sandbox {
    pub fn new(config: SandboxConfig) -> anyhow::Result<Self> {
        crate::cpu_clock::thread_cpu_time()?;
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let _guard = tokio.enter();

        let create_params = v8::CreateParams::default().heap_limits(0, config.heap_limit_bytes);
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![kk_runner::init()],
            create_params: Some(create_params),
            ..Default::default()
        });

        let shared: SharedRef = Arc::new(Shared::default());
        let host = {
            let _guard = tokio.enter();
            runtime.op_state().borrow_mut().put(ModuleTable::new());
            runtime.op_state().borrow_mut().put(shared.clone());
            // Game.cpu.halt 在玩家线程内设置 V8 终止标志；后续根据 halt_requested 选择计费方式。
            runtime
                .op_state()
                .borrow_mut()
                .put(runtime.v8_isolate().thread_safe_handle());

            // 先加载 lodash 和常量，再装配 prelude、捕获宿主函数句柄，最后删除临时 __kkp。
            runtime.execute_script("kk-lodash", lodash_script())?;
            runtime.execute_script("kk-constants", constants_script(&config.constant_overrides))?;
            for (name, src) in PRELUDE_SEGMENTS {
                runtime.execute_script(name, src.to_string())?;
            }
            let host = HostFns::capture(&mut runtime)?;
            runtime.execute_script("kk-host-unlink", "delete globalThis.__kkp".to_string())?;
            host
        };

        let watchdog = Watchdog::spawn(runtime.v8_isolate().thread_safe_handle());

        let seq = SEQ.fetch_add(1, Ordering::SeqCst);
        let mut sb = Self {
            watchdog,
            runtime,
            tokio,
            host,
            seq,
            command_deadline: None,
            budget: Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            ),
            shared,
            heap_limit: config.heap_limit_bytes,
            terminated_this_tick: false,
            last_loop_error: None,
            pending_intents: None,
            pending_bill: None,
            pending_recharge: None,
        };
        sb.install_near_heap_callback();
        // 所有可能失败的初始化都完成后再登记实例，避免 ? 早退留下没有实例的计数和栈记录。
        THREAD_ORDER.with(|s| s.borrow_mut().push(seq));
        LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(sb)
    }

    /// 接近 V8 堆上限时请求中断，并返回临时扩大后的上限。
    /// V8 会保留新上限，必须在执行结束后通过 restore_heap_limit 恢复，
    /// 否则连续触发会不断放大配置上限。回调不覆盖 TypedArray 等全部堆外分配。
    fn install_near_heap_callback(&mut self) {
        let handle = self.runtime.v8_isolate().thread_safe_handle();
        let shared = self.shared.clone();
        self.runtime
            .add_near_heap_limit_callback(move |current_limit, _initial| {
                shared.near_heap_fired.store(true, Ordering::Relaxed);
                handle.terminate_execution();
                current_limit * 2
            });
    }

    /// 近堆回调触发后，以配置值请求恢复上限并重新注册回调。
    /// V8 生效上限还受当前堆状态影响，不保证恰好等于配置值。
    /// 诊断标志保留至下一次 begin_tick，便于判断本轮是否触发过。
    fn restore_heap_limit(&mut self) {
        if self.shared.near_heap_fired.load(Ordering::Relaxed) {
            self.runtime
                .remove_near_heap_limit_callback(self.heap_limit);
            self.install_near_heap_callback();
        }
    }

    /// 替换 Rust 模块表，并重建 JavaScript require 缓存。
    /// 缓存重置失败也已替换模块表，此操作不提供失败回滚。
    pub fn set_modules(&mut self, table: ModuleTable) -> anyhow::Result<()> {
        {
            let state = self.runtime.op_state();
            state.borrow_mut().put(table);
        }
        // 失败时清除 V8 终止标志并恢复堆上限；此调用不生成计费候选。
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.reset_modules,
            &[],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    /// 设置下一 tick 的 RawMemory 初值，通过捕获的宿主函数写入闭包状态。
    ///
    /// 调用方须在 begin_tick 前注入；此处不检查时序。tick 中途注入不会清除
    /// 已解析的 RawMemory._parsed，后续写回可能用旧解析值覆盖新数据。
    pub fn set_raw_memory(&mut self, data: &str) -> anyhow::Result<()> {
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.set_raw_memory,
            &[data.to_string()],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    /// 注入 RawMemory、活动 segment 窗口（`[[id,str],...]` JSON）和 foreignSegment。
    /// foreignSegment 的 null 字符串表示无数据；须在 begin_tick 前调用。
    pub fn set_memory_state(
        &mut self,
        raw: &str,
        segments_json: &str,
        foreign_json: &str,
    ) -> anyhow::Result<()> {
        call_host_unit(
            &self.tokio,
            &mut self.runtime,
            &self.host.set_memory_state,
            &[
                raw.to_string(),
                segments_json.to_string(),
                foreign_json.to_string(),
            ],
        )
        .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    pub fn set_command_deadline(&mut self, deadline: Instant) -> anyhow::Result<()> {
        if deadline <= Instant::now() {
            anyhow::bail!("玩家命令在执行前已过期");
        }
        // 先封住旧代次的投递，再清掉已投递的标志；内存注入早于 begin_tick。
        self.watchdog.disarm();
        self.clear_terminate_flag();
        self.command_deadline = Some(deadline);
        self.watchdog.arm(deadline);
        Ok(())
    }
    pub fn clear_command_deadline(&mut self) {
        self.watchdog.disarm();
        self.clear_terminate_flag();
        self.command_deadline = None;
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    pub fn bucket(&self) -> f64 {
        self.budget.bucket()
    }

    pub fn tick_limit(&self) -> f64 {
        self.budget.tick_limit()
    }

    /// 制定预算计划并注入世界数据，不提交桶余额。
    /// SkipPenalty 表示负余额停跑；回充已暂存，仍需 settle_bill 或 discard_bill。
    pub fn begin_tick(&mut self, world: &serde_json::Value) -> anyhow::Result<TickPlan> {
        let plan = self.budget.begin_tick();
        self.begin_tick_plan(world, None, plan)
    }

    /// [`Sandbox::begin_tick`] 的 KKPV 通道形态：`world` 是不含对象数组的
    /// 世界信封，对象记录由沙箱从 `bundle` 块束解码。
    pub fn begin_tick_kkpv(
        &mut self,
        world: &serde_json::Value,
        bundle: &[u8],
    ) -> anyhow::Result<TickPlan> {
        let plan = self.budget.begin_tick();
        self.begin_tick_plan(world, Some(bundle), plan)
    }

    /// 使用宿主制定的预算；本地桶只作为本次执行与计费的工作副本。
    pub fn begin_tick_with_budget(
        &mut self,
        world: &serde_json::Value,
        budget: &kk_core::cpu::RunBudget,
    ) -> anyhow::Result<TickPlan> {
        self.budget = budget.bucket().clone();
        self.begin_tick_plan(world, None, budget.plan().clone())
    }

    /// [`Sandbox::begin_tick_with_budget`] 的 KKPV 通道形态。
    pub fn begin_tick_kkpv_with_budget(
        &mut self,
        world: &serde_json::Value,
        bundle: &[u8],
        budget: &kk_core::cpu::RunBudget,
    ) -> anyhow::Result<TickPlan> {
        self.budget = budget.bucket().clone();
        self.begin_tick_plan(world, Some(bundle), budget.plan().clone())
    }

    fn begin_tick_plan(
        &mut self,
        world: &serde_json::Value,
        kkpv_bundle: Option<&[u8]>,
        plan: TickPlan,
    ) -> anyhow::Result<TickPlan> {
        // 调用方应已提交或丢弃上一轮候选；清除残留，避免它混入当前 tick。
        self.pending_bill = None;
        self.pending_recharge = None;
        match plan {
            // 停跑分支不注入世界或执行玩家代码；清除上一轮指令，只暂存本轮回充。
            TickPlan::SkipPenalty => {
                self.pending_intents = None;
                self.pending_recharge = Some(self.budget.skip_recharge());
                Ok(TickPlan::SkipPenalty)
            }
            TickPlan::Run { .. } => {
                // 清除可能遗留的终止标志与上一 tick 未消费的 intents
                //（消费方漏取不得把旧 intent 泄漏进新 tick），然后武装看门狗
                self.terminated_this_tick = false;
                self.pending_intents = None;
                self.runtime.v8_isolate().cancel_terminate_execution();
                // 将沙箱预算作为独立参数交给 beginTick，替换世界数据中的 CPU 占位值，
                // 避免为了更新 CPU 字段克隆整个世界数据。
                let cpu_json = {
                    // 在 begin_tick 采样 V8 堆统计，字段名使用 snake_case。
                    // JS getHeapStatistics 返回此快照；externally_allocated_size 无对应统计，固定为 0。
                    let hs = self.runtime.v8_isolate().get_heap_statistics();
                    serde_json::json!({
                        "limit": self.budget.limit,
                        "tickLimit": self.budget.tick_limit(),
                        "bucket": self.budget.bucket(),
                        "heap": {
                            "total_heap_size": hs.total_heap_size(),
                            "total_heap_size_executable": hs.total_heap_size_executable(),
                            "total_physical_size": hs.total_physical_size(),
                            "total_available_size": hs.total_available_size(),
                            "used_heap_size": hs.used_heap_size(),
                            "heap_size_limit": hs.heap_size_limit(),
                            "malloced_memory": hs.malloced_memory(),
                            "peak_malloced_memory": hs.peak_malloced_memory(),
                            "does_zap_garbage": hs.does_zap_garbage() as u32,
                            "externally_allocated_size": 0,
                        },
                    })
                    .to_string()
                };
                // 先序列化世界数据再启动看门狗；启动后的失败路径必须解除期限。
                let world_json = serde_json::to_string(world)?;
                let deadline = Instant::now()
                    + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
                *self.shared.thread_cpu_start.lock().unwrap() =
                    Some(crate::cpu_clock::thread_cpu_time()?);
                let deadline = self
                    .command_deadline
                    .map_or(deadline, |command| command.min(deadline));
                self.watchdog.arm(deadline);
                *self.shared.tick_start.lock().unwrap() = Some(Instant::now());
                self.shared.reset_console_gate();
                // 先恢复堆上限再清诊断标志，否则会丢失近堆回调曾扩大上限的信息。
                self.restore_heap_limit();
                self.shared.near_heap_fired.store(false, Ordering::Relaxed);
                self.shared.halt_requested.store(false, Ordering::Relaxed);
                // 世界注入也受本 tick 期限约束；失败时生成完整预算费用并清理执行状态。
                // 参数通过 V8 值传递（字符串或 ArrayBuffer），不拼接进 JavaScript 源码。
                // KKPV 通道的块束在此完成每 isolate 私有拷贝，不共享宿主内存。
                match kkpv_bundle {
                    Some(bundle) => call_host_kkpv_begin(
                        &self.tokio,
                        &mut self.runtime,
                        &self.host.begin_tick_kkpv,
                        &world_json,
                        bundle,
                        &cpu_json,
                    ),
                    None => call_host_unit(
                        &self.tokio,
                        &mut self.runtime,
                        &self.host.begin_tick,
                        &[world_json, cpu_json],
                    ),
                }
                .inspect_err(|_| self.after_termination())?;
                Ok(TickPlan::Run {
                    tick_limit: self.budget.tick_limit(),
                })
            }
        }
    }

    /// 执行玩家入口 `require('main').loop()`。入口执行完毕即 Completed——
    /// "执行到入口"≠"玩家代码无异常"：玩家逻辑错误由 runLoop 内部捕获，
    /// 经 `take_last_loop_error` 暴露（报告/验收统计据此区分）。
    pub fn run_loop(&mut self) -> anyhow::Result<TickOutcome> {
        let result = call_host_json(&self.tokio, &mut self.runtime, &self.host.run_loop, &[]);
        match result {
            Ok(v) => {
                self.last_loop_error = match v.get("ok").and_then(serde_json::Value::as_bool) {
                    Some(false) => v
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    _ => None,
                };
                Ok(TickOutcome::Completed { used_ms: 0.0 })
            }
            Err(_) => {
                // runLoop 内部捕获普通玩家异常；逃出宿主调用的错误按中断处理。
                // halt_requested 区分主动 halt，此情况不生成费用。
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(TickOutcome::Terminated)
            }
        }
    }

    /// 处理主动 halt：解除期限、清除终止标志、恢复堆上限并跳过本轮写回。
    /// 不生成费用，不销毁长期 V8 实例；已收集的日志保留，并追加 CPU halted。
    /// 来源：上游 driver/lib/runtime/make.js 的 didHaltByUserRequest 分支，
    /// 保留沙箱和已收日志的差异见 docs/designs/compatibility.md。
    fn after_halt(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        if !self.terminated_this_tick {
            self.pending_bill = None;
            self.terminated_this_tick = true;
            self.shared.push_console_line("error", "CPU halted");
        }
        self.restore_heap_limit();
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    /// 取走最近一次 runLoop 捕获的玩家逻辑错误；取走后清空。
    /// 中断分支不更新该字段，调用方应每轮消费，避免残留上一轮错误。
    pub fn take_last_loop_error(&mut self) -> Option<String> {
        self.last_loop_error.take()
    }

    /// 执行 Memory 写回并生成指令与计费候选，不提交桶余额。
    ///
    /// 写回可能调用玩家 Memory.toJSON，必须继续受看门狗保护。返回 JSON
    /// 表示写回成功、普通错误或中断；中断费用由处理函数暂存，调用方随后决定是否提交。
    pub fn end_tick(&mut self) -> anyhow::Result<serde_json::Value> {
        // 硬中断后的 Memory 保持上一 tick 的存档。
        if self.terminated_this_tick {
            // 中断后不调用 JS 写回函数，也不保留本轮指令；费用候选已由中断处理生成。
            self.pending_intents = None;
            return Ok(serde_json::json!({
                "ok": false, "terminated": true,
                "error": "本 tick 已硬中断，按官方语义跳过 Memory 写回",
            }));
        }
        // 清除执行入口结束后可能迟到的 V8 终止标志，再为写回登记新代次。
        // 期限根据保存的 tick_start 加预算重算，不重新给写回一份完整预算。
        // 若不重新登记，旧代次可能已经触发，Memory.toJSON 死循环就无法再次中断。
        self.runtime.v8_isolate().cancel_terminate_execution();
        if let Some(t0) = *self.shared.tick_start.lock().unwrap() {
            let deadline = t0 + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
            let deadline = self
                .command_deadline
                .map_or(deadline, |command| command.min(deadline));
            self.watchdog.arm(deadline);
        }
        let exec = call_host_json(&self.tokio, &mut self.runtime, &self.host.end_tick, &[]);
        match exec {
            Ok(v) => {
                self.watchdog.disarm();
                // intentsList 带出与宿主计费（官方双轨：沙箱内 intents.cpu 供
                // getUsed 即时显示 runtime.js:202-204；宿主按最终清单重算
                // 0.2×count 并入扣桶 make.js:193-194——排除 say/pull）。
                // 硬中断 tick 不进此分支（Err 路径丢弃 intents 并按整份结算，
                // 官方 isolate 级故障同款 make.js:279-294）
                let (intents, billed) = self.take_endtick_intents(&v);
                // 普通写回错误也丢弃指令，避免只提交玩家 tick 的部分结果。
                // 仍按当前已用时间和收集到的指令数生成费用候选。
                let intents = if v.get("ok").and_then(Value::as_bool) == Some(false) {
                    None
                } else {
                    intents
                };
                self.pending_intents = intents;
                // 线程 CPU 用量包含写回耗时；此处只暂存，世界结算成功后由调用方提交。
                self.pending_bill = Some(self.cpu_used_ms()? + 0.2 * billed as f64);
                // 即使宿主调用成功返回，也恢复可能被近堆回调扩大的上限。
                self.restore_heap_limit();
                *self.shared.tick_start.lock().unwrap() = None;
                *self.shared.thread_cpu_start.lock().unwrap() = None;
                Ok(v)
            }
            Err(_) => {
                // 写回被中断时丢弃指令并清理执行状态；主动 halt 无费用，其他中断计完整预算。
                self.pending_intents = None;
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(
                    serde_json::json!({ "ok": false, "terminated": true, "error": "写回阶段硬中断" }),
                )
            }
        }
    }

    /// 截取 endTick 的 intentsList，并计算保留清单的计费条数。
    /// 最多保留 50,000 个顶层键，包括对象 id、room/global 和 notify 等通道。
    /// 该限制不约束嵌套数组长度或 JSON 字节数；超限截断并添加日志提示。
    fn take_endtick_intents(&mut self, v: &Value) -> (Option<Value>, usize) {
        const MAX_INTENT_OBJECTS: usize = 50_000;
        let Some(list) = v.get("intentsList") else {
            return (None, 0);
        };
        let free: &[&str] = &["say", "pull"];
        // 官方 intentsCount（make.js:100-106）：对象级按动作名、数组通道
        //（room/global）按元素数计；排除 say/pull
        let count_billed = |map: &serde_json::Map<String, Value>| {
            map.values()
                .map(|a| match a {
                    // 顶层数组通道（notify——push 全局形态）：官方 make.js:98-107
                    // intentsCount 对数组 for-in 逐元素 +1（索引串不中免费表）
                    Value::Array(items) => items.len(),
                    a => a
                        .as_object()
                        .map(|inner| {
                            inner
                                .iter()
                                .filter(|(k, _)| !free.contains(&k.as_str()))
                                .map(|(_, v)| v.as_array().map_or(1, Vec::len))
                                .sum::<usize>()
                        })
                        .unwrap_or(0),
                })
                .sum::<usize>()
        };
        let objects = list.as_object().map(|m| m.len()).unwrap_or(0);
        if objects > MAX_INTENT_OBJECTS {
            let mut truncated = serde_json::Map::new();
            if let Some(map) = list.as_object() {
                for (k, val) in map.iter().take(MAX_INTENT_OBJECTS) {
                    truncated.insert(k.clone(), val.clone());
                }
            }
            self.shared.push_console_line(
                "error",
                &format!("intent 上行闸门：对象数 {objects} 超上限 {MAX_INTENT_OBJECTS}，已截断"),
            );
            let billed = count_billed(&truncated);
            return (Some(Value::Object(truncated)), billed);
        }
        let billed = list.as_object().map(count_billed).unwrap_or(0);
        (Some(list.clone()), billed)
    }

    /// 取走本 tick 的 intentsList（embed 结算通道消费；None = 无或已取）。
    pub fn take_intents(&mut self) -> Option<Value> {
        self.pending_intents.take()
    }

    /// 当前计费候选的只读视图（EvalReport 带出给协调器/测试）。
    pub fn pending_bill(&self) -> Option<f64> {
        self.pending_bill
    }

    /// 读取尚未提交的停跑回充增量。
    pub fn pending_recharge(&self) -> Option<f64> {
        self.pending_recharge
    }

    /// 提交本轮费用或停跑回充，随后消费候选；没有候选时无操作。
    /// 调用方应在世界结算成功后调用。停跑回充提交时按当前桶余额重新计算。
    pub fn settle_bill(&mut self) {
        // Run 产生费用，SkipPenalty 产生回充；同轮只能有一个，否则会重复增加 limit。
        debug_assert!(
            self.pending_bill.is_none() || self.pending_recharge.is_none(),
            "计费候选与停跑回充候选不得同轮共存（Skip/Run 分支互斥）"
        );
        if let Some(bill) = self.pending_bill.take() {
            self.budget.settle(bill);
        }
        if self.pending_recharge.take().is_some() {
            self.budget.apply_recharge();
        }
    }

    /// 丢弃费用与回充候选，不修改桶余额。
    /// 此操作不恢复 V8 全局或闭包状态；下一轮应重新注入已提交的 Memory。
    pub fn discard_bill(&mut self) {
        self.pending_bill = None;
        self.pending_recharge = None;
    }

    /// 求值一段玩家表达式（screeps-ok 适配器的 runPlayer 通道），恰好占用一个 tick 的
    /// 执行语义由调用方（engine/embed）负责编排。
    pub fn run_expr(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        match call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.run_expr,
            &[code.to_string()],
        ) {
            Ok(v) => Ok(v),
            Err(_) => {
                // 表达式中的主动 halt 也使用零计费的中断处理。
                if self.shared.halt_requested.load(Ordering::Acquire) {
                    self.after_halt();
                } else {
                    self.after_termination();
                }
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 在独立看门狗窗口中求值，不注入世界、不执行 tick 写回、不修改预算桶。
    /// 窗口按当前余额计算，至少 1 毫秒；成功或失败后均解除期限、清标志并恢复堆上限。
    /// 需要完整 tick 语义时，由调用方使用 begin_tick、run_expr 和 end_tick。
    pub fn run_expr_guarded(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        // arm 前完成可失败步骤（句柄通道参数为值传递，无脚本构造步骤）
        self.clear_terminate_flag();
        let window = self.budget.next_tick_limit().max(1.0);
        self.watchdog
            .arm(Instant::now() + Duration::from_millis(window as u64));
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.run_expr,
            &[code.to_string()],
        );
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        match out {
            Ok(v) => {
                // 独立表达式也可能触发近堆回调，成功返回后仍需恢复上限。
                self.restore_heap_limit();
                Ok(v)
            }
            Err(_) => {
                // 独立通道被掐断：恢复堆上限即可，不动桶、不碰 tick 生命周期状态
                self.restore_heap_limit();
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 解除期限、清终止标志、恢复堆上限，并暂存完整预算费用、清除计时起点。
    /// 同一 tick 重复调用不会再次生成费用；真正扣桶由 settle_bill 完成。
    fn after_termination(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        if !self.terminated_this_tick {
            // 费用只暂存，提交点再修改桶余额。
            self.pending_bill = Some(self.budget.tick_limit().ceil());
            self.terminated_this_tick = true;
        }
        self.restore_heap_limit();
        *self.shared.tick_start.lock().unwrap() = None;
        *self.shared.thread_cpu_start.lock().unwrap() = None;
    }

    /// 清除 tick 外调用可能遗留的终止标志，并恢复近堆回调扩大的上限。
    /// 不修改计费候选，也不解除看门狗期限。
    fn clear_terminate_flag(&mut self) {
        self.runtime.v8_isolate().cancel_terminate_execution();
        self.restore_heap_limit();
    }

    /// 看门狗句柄（测试驱动真实看门狗线程的代际守卫用）。
    pub fn watchdog(&self) -> &Watchdog {
        &self.watchdog
    }

    /// 近堆回调是否在本 tick 触发过（诊断：区分内存闸门与看门狗兜底路径）。
    pub fn near_heap_fired(&self) -> bool {
        self.shared.near_heap_fired.load(Ordering::Relaxed)
    }

    /// V8 当前生效的堆上限。近堆回调的返回值会把它持久抬高（remove+re-add 恢复），
    /// 测试据此断言"触发后恢复到配置值"而非仅凭实例复用。
    pub fn heap_size_limit_bytes(&mut self) -> usize {
        self.runtime
            .v8_isolate()
            .get_heap_statistics()
            .heap_size_limit()
    }

    /// 本 tick 在执行线程上消耗的 CPU 毫秒数，不含指令费用。
    pub fn cpu_used_ms(&self) -> anyhow::Result<f64> {
        let start = *self.shared.thread_cpu_start.lock().unwrap();
        match start {
            Some(start) => Ok(crate::cpu_clock::thread_cpu_time()?
                .saturating_sub(start)
                .as_secs_f64()
                * 1000.0),
            None => Ok(0.0),
        }
    }

    pub fn drain_console(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut *self.shared.console.lock().unwrap())
    }

    pub fn drain_missing_api(&mut self) -> anyhow::Result<serde_json::Value> {
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.drain_missing,
            &[],
        );
        // 诊断导出没有看门狗窗口；成功或失败后都恢复近堆回调可能扩大的上限。
        // missing 的属性名由玩家访问产生，输出量可能较大。
        self.restore_heap_limit();
        out.inspect_err(|_| self.clear_terminate_flag())
    }

    pub fn drain_stub_calls(&mut self) -> anyhow::Result<serde_json::Value> {
        let out = call_host_json(
            &self.tokio,
            &mut self.runtime,
            &self.host.drain_stub_calls,
            &[],
        );
        self.restore_heap_limit();
        out.inspect_err(|_| self.clear_terminate_flag())
    }
}

// ---------------------------------------------------------------- 宿主通道私有句柄

/// 持有装配时捕获的宿主函数句柄。
/// Rust 在全部 prelude 执行后捕获 __kkp.host，再删除临时全局 __kkp，
/// 后续生命周期调用通过句柄完成，不查找玩家可替换的全局属性。
struct HostFns {
    begin_tick: v8::Global<v8::Function>,
    begin_tick_kkpv: v8::Global<v8::Function>,
    run_loop: v8::Global<v8::Function>,
    run_expr: v8::Global<v8::Function>,
    end_tick: v8::Global<v8::Function>,
    reset_modules: v8::Global<v8::Function>,
    set_raw_memory: v8::Global<v8::Function>,
    set_memory_state: v8::Global<v8::Function>,
    drain_missing: v8::Global<v8::Function>,
    drain_stub_calls: v8::Global<v8::Function>,
}

impl HostFns {
    /// 从 `__kkp.host` 捕获全部句柄（prelude 装配后、`__kkp` 删除前）。
    fn capture(runtime: &mut JsRuntime) -> anyhow::Result<Self> {
        // 局部捕获宏：host 对象按键取函数 → 校验为函数 → 提升为全局句柄
        //（失败直接从 capture 早退——宏在函数体内展开）
        macro_rules! grab {
            ($scope:expr, $host:expr, $key:literal) => {{
                let key = match v8::String::new_from_utf8(
                    $scope,
                    $key.as_bytes(),
                    v8::NewStringType::Normal,
                ) {
                    Some(k) => k,
                    None => return Err(anyhow::anyhow!("键名构造失败")),
                };
                let f = match $host.get($scope, key.into()) {
                    Some(f) => f,
                    None => return Err(anyhow::anyhow!("prelude 宿主容器缺 {}", $key)),
                };
                let f = match v8::Local::<v8::Function>::try_from(f) {
                    Ok(f) => f,
                    Err(_) => return Err(anyhow::anyhow!("prelude 宿主容器 {} 非函数", $key)),
                };
                v8::Global::new($scope, f)
            }};
        }
        deno_core::scope!(scope, runtime);
        let global = scope.get_current_context().global(scope);
        let name = v8::String::new_from_utf8(scope, b"__kkp", v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
        let kkp = global
            .get(scope, name.into())
            .ok_or_else(|| anyhow::anyhow!("prelude 未注册 __kkp"))?;
        let kkp =
            v8::Local::<v8::Object>::try_from(kkp).map_err(|_| anyhow::anyhow!("__kkp 非对象"))?;
        let host_key = v8::String::new_from_utf8(scope, b"host", v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
        let host = kkp
            .get(scope, host_key.into())
            .ok_or_else(|| anyhow::anyhow!("prelude 未注册 __kkp.host"))?;
        let host = v8::Local::<v8::Object>::try_from(host)
            .map_err(|_| anyhow::anyhow!("__kkp.host 非对象"))?;
        Ok(Self {
            begin_tick: grab!(scope, host, "beginTick"),
            begin_tick_kkpv: grab!(scope, host, "beginTickKkpv"),
            run_loop: grab!(scope, host, "runLoop"),
            run_expr: grab!(scope, host, "runExpr"),
            end_tick: grab!(scope, host, "endTick"),
            reset_modules: grab!(scope, host, "resetModules"),
            set_raw_memory: grab!(scope, host, "setRawMemory"),
            set_memory_state: grab!(scope, host, "setMemoryState"),
            drain_missing: grab!(scope, host, "drainMissing"),
            drain_stub_calls: grab!(scope, host, "drainStubCalls"),
        })
    }
}

/// 调用捕获的函数，将字符串参数作为 V8 值传入，再解析返回的 JSON。
/// JavaScript 异常或 V8 中断返回 Err；参数不作为源码拼接。
fn call_host_json(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<serde_json::Value> {
    let text = call_host_string(tokio, runtime, f, args)?;
    Ok(serde_json::from_str(&text)?)
}

/// [`call_host_json`] 的无返回值形态（只关心成功/失败）。
fn call_host_unit(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<()> {
    call_host_string(tokio, runtime, f, args).map(|_| ())
}

/// 调用捕获的宿主函数，参数为（世界信封字符串、块束 ArrayBuffer、CPU
/// 字符串）。块束字节装入新建 ArrayBuffer——这是一次每 isolate 私有
/// 拷贝，不共享宿主内存；参数不作源码拼接。异常与中断的处理与
/// [`call_host_string`] 一致。
fn call_host_kkpv_begin(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    world_json: &str,
    bundle: &[u8],
    cpu_json: &str,
) -> anyhow::Result<()> {
    let _guard = tokio.enter();
    deno_core::scope!(scope, runtime);
    v8::tc_scope!(let scope, scope);
    let func = f.open(scope);
    let recv: v8::Local<v8::Value> = v8::undefined(scope).into();
    let world = v8::String::new_from_utf8(scope, world_json.as_bytes(), v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
    let cpu = v8::String::new_from_utf8(scope, cpu_json.as_bytes(), v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
    let store = v8::ArrayBuffer::new_backing_store_from_vec(bundle.to_vec()).make_shared();
    let buffer = v8::ArrayBuffer::with_backing_store(scope, &store);
    let argv: [v8::Local<v8::Value>; 3] = [world.into(), buffer.into(), cpu.into()];
    match func.call(scope, recv, &argv) {
        Some(_) => Ok(()),
        None => {
            let msg = if scope.is_execution_terminating() {
                "硬中断（terminate）".to_string()
            } else {
                scope
                    .exception()
                    .map(|e| e.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "无异常对象".into())
            };
            Err(anyhow::anyhow!("宿主通道调用失败: {msg}"))
        }
    }
}

fn call_host_string(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<String> {
    let _guard = tokio.enter();
    deno_core::scope!(scope, runtime);
    // TryCatch 域：调用抛异常/被 terminate 时在此取异常文本（deno_core
    // scoped_call_with_args 同款模式）
    v8::tc_scope!(let scope, scope);
    let func = f.open(scope);
    let recv: v8::Local<v8::Value> = v8::undefined(scope).into();
    let mut argv = Vec::with_capacity(args.len());
    for a in args {
        let s = v8::String::new_from_utf8(scope, a.as_bytes(), v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
        argv.push(s.into());
    }
    let result = match func.call(scope, recv, &argv) {
        Some(v) => v,
        None => {
            let msg = if scope.is_execution_terminating() {
                "硬中断（terminate）".to_string()
            } else {
                scope
                    .exception()
                    .map(|e| e.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "无异常对象".into())
            };
            return Err(anyhow::anyhow!("宿主通道调用失败: {msg}"));
        }
    };
    Ok(result.to_rust_string_lossy(scope))
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        THREAD_ORDER.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack.last() != Some(&self.seq) {
                // 销毁顺序不满足本线程创建栈时记录并告警；告警本身不能修复 V8 的 Enter/Exit 顺序。
                eprintln!(
                    "kk-runner: 线程内销毁顺序违反 LIFO（seq={}，栈顶={:?}）。V8 的 isolate Enter/Exit 栈是线程局部的，同线程多实例必须按创建逆序销毁！",
                    self.seq,
                    stack.last()
                );
                LIFO_VIOLATIONS.fetch_add(1, Ordering::SeqCst);
                stack.retain(|s| *s != self.seq);
            } else {
                stack.pop();
            }
        });
    }
}

fn lodash_script() -> String {
    // lodash 3.10.1（npm 主入口，官方引擎 webpack 用的同一构建）。
    // 在 IIFE 里提供 module/exports（CJS 检测）并压掉 AMD；取 CJS 导出，回退 UMD 根挂载。
    let mut s = String::with_capacity(LODASH_SRC.len() + 200);
    s.push_str("(function(){\nvar module = { exports: {} }; var exports = module.exports; var define = undefined;\n");
    s.push_str(LODASH_SRC);
    s.push_str(
        "\nvar out = (module.exports && Object.keys(module.exports).length > 0) ? module.exports : globalThis._;\n\
         globalThis.__kkLodash = out;\n})();",
    );
    s
}

fn constants_script(overrides: &std::collections::BTreeMap<String, serde_json::Value>) -> String {
    // 将嵌入的常量 JSON 构造成对象，叠加宿主按有效规则提供的覆盖，
    // 再平铺至玩家全局；API 各段按需捕获常量。
    let extra = serde_json::to_string(overrides).expect("常量覆盖可序列化");
    format!(
        "(function(){{ var C = {CONSTANTS_JSON}; var O = {extra}; for (var k in O) {{ if (!Object.prototype.hasOwnProperty.call(C, k)) throw new Error('常量覆盖键不在常量表: ' + k); C[k] = O[k]; }} for (var k in C) globalThis[k] = C[k]; }})();"
    )
}

#[cfg(test)]
mod command_deadline_tests {
    use super::*;

    #[test]
    fn command_boundaries_clear_delivered_termination() -> anyhow::Result<()> {
        let mut sandbox = Sandbox::new(SandboxConfig::default())?;
        // 模拟上一代看门狗已经投递，但宿主调用成功返回的交错。
        sandbox.runtime.v8_isolate().terminate_execution();
        sandbox.set_command_deadline(Instant::now() + Duration::from_secs(1))?;
        sandbox.set_memory_state("{}", "[]", "null")?;
        // 解除期限必须同时清掉已投递标志，后续管理命令也应能复用实例。
        sandbox.runtime.v8_isolate().terminate_execution();
        sandbox.clear_command_deadline();
        sandbox.set_memory_state("{}", "[]", "null")?;
        Ok(())
    }
}
