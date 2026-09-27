//! Sandbox：一个玩家的 deno_core 沙箱宿主（M0 种子实现）。
//!
//! 组装 spike 验证过的全部机制：
//! - 内存双闸门：`heap_limits` 硬上限 + `near_heap_limit` 回调内优雅 terminate
//!   （只设上限不设回调会 FatalOOM abort 整个进程——spike negctl 的阴性对照）；
//! - 看门狗：常驻线程 + 代际守卫，预算硬超时强制掐断，实例跨 tick 复用；
//! - 执行前 `cancel_terminate_execution()` 清遗留终止标志（双保险）；
//! - tokio current-thread 上下文守卫（V8 延迟任务需要）。
//!
//! ## LIFO 销毁纪律（V8 硬约束）
//! 同一进程内多个 isolate 必须按创建的**逆序**销毁，顺序错了进程直接崩溃。
//! 本类型用全局注册栈在 Drop 时检测违规并告警（测试断言栈为空）；
//! 正确用法由字段声明顺序结构性保证：`watchdog`（先 join 线程）→ `runtime` → `tokio`。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use deno_core::v8;
use deno_core::{JsRuntime, RuntimeOptions};

use crate::budget::{BUCKET_CAP, Bucket, DEFAULT_CPU_LIMIT, MAX_PER_TICK, TickPlan};
use crate::modules::ModuleTable;
use crate::ops::{LogLine, Shared, SharedRef, kk_runner};
use crate::watchdog::Watchdog;

const LODASH_SRC: &str = include_str!("../assets/lodash-3.10.1.js");
const CONSTANTS_JSON: &str = include_str!("../assets/constants.json");
const PRELUDE_JS: &str = include_str!("prelude.js");

/// 官方配额：单实例堆上限约 256MB。
const DEFAULT_HEAP_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub heap_limit_bytes: usize,
    pub cpu_limit: f64,
    pub bucket_cap: f64,
    pub max_per_tick: f64,
    pub initial_bucket: f64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            heap_limit_bytes: DEFAULT_HEAP_LIMIT,
            cpu_limit: DEFAULT_CPU_LIMIT,
            bucket_cap: BUCKET_CAP,
            max_per_tick: MAX_PER_TICK,
            initial_bucket: BUCKET_CAP,
        }
    }
}

/// 一次玩家执行的结局。
#[derive(Debug)]
pub enum TickOutcome {
    /// 正常执行完毕（玩家逻辑自身的错误由 runLoop 结果 JSON 携带，不算这里）。
    Completed { used_ms: f64 },
    /// 看门狗/内存闸门强制掐断（已按整份 tickLimit 计费；官方语义下桶充足不跳帧，
    /// 桶被扣成负数时由下一 begin_tick 的停跑规则处理）。
    Terminated,
}

// ---------------------------------------------------------------- LIFO 注册栈

static SEQ: AtomicU64 = AtomicU64::new(0);
static LIFO_REGISTRY: LazyLock<Mutex<Vec<u64>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// 当前进程存活的 Sandbox 数（测试用：全部销毁后应归零）。
pub fn live_sandbox_count() -> usize {
    LIFO_REGISTRY.lock().unwrap().len()
}

pub struct Sandbox {
    // 字段声明顺序 = Drop 顺序：先 join 看门狗线程，再销毁 JsRuntime，最后拆 tokio。
    watchdog: Watchdog,
    runtime: JsRuntime,
    tokio: tokio::runtime::Runtime,
    seq: u64,
    budget: Bucket,
    shared: SharedRef,
    /// 配置的堆硬上限（near-heap 触发放大后据此恢复）。
    heap_limit: usize,
}

impl Sandbox {
    pub fn new(config: SandboxConfig) -> anyhow::Result<Self> {
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
        runtime.op_state().borrow_mut().put(ModuleTable::new());
        runtime.op_state().borrow_mut().put(shared.clone());

        let watchdog = Watchdog::spawn(runtime.v8_isolate().thread_safe_handle());

        // 注入顺序：lodash（结果放 __kkLodash）→ 常量平铺 → prelude（捕获 op、遮蔽 Deno）。
        runtime.execute_script("kk-lodash", lodash_script())?;
        runtime.execute_script("kk-constants", constants_script())?;
        runtime.execute_script("kk-prelude", PRELUDE_JS.to_string())?;

        let seq = SEQ.fetch_add(1, Ordering::SeqCst);
        LIFO_REGISTRY.lock().unwrap().push(seq);

        let mut sb = Self {
            watchdog,
            runtime,
            tokio,
            seq,
            budget: Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            ),
            shared,
            heap_limit: config.heap_limit_bytes,
        };
        sb.install_near_heap_callback();
        Ok(sb)
    }

    /// 双闸门之二：接近堆上限回调内 terminate（返回放大后的上限让当前分配完成，
    /// 随即在中断检查点展开——spike 实验 C 验证的 deno_core 官方测试同款模式）。
    /// 触发即置诊断标志；放大的上限由 `restore_heap_limit`（硬中断善后）恢复——
    /// V8 会把回调返回值持久写为新的堆上限，不恢复则"触发 OOM→再触发"可指数级架空配置。
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

    /// 恢复堆硬上限（仅在 near-heap 触发过后执行：remove 会顺带把上限恢复为参数值，
    /// re-add 重新武装回调——deno_core 官方测试同款 remove+re-add 模式）。
    /// 诊断标志保留到下一 begin_tick 复位（"本 tick 是否触发过"语义，测试可观察）。
    fn restore_heap_limit(&mut self) {
        if self.shared.near_heap_fired.load(Ordering::Relaxed) {
            self.runtime
                .remove_near_heap_limit_callback(self.heap_limit);
            self.install_near_heap_callback();
        }
    }

    /// 注册（或替换）玩家模块表；替换时清空 require 缓存。
    pub fn set_modules(&mut self, table: ModuleTable) -> anyhow::Result<()> {
        {
            let state = self.runtime.op_state();
            state.borrow_mut().put(table);
        }
        // 失败也清掉可能遗留的终止标志，沙箱不留"中毒"状态（tick 外路径不结算预算）
        self.exec_script("globalThis.__kk.resetModules()")
            .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    /// 注入玩家的 RawMemory 初值（官方：每 tick 从存档恢复）。
    pub fn set_raw_memory(&mut self, data: &str) -> anyhow::Result<()> {
        let script = format!(
            "(function(){{ const s = {}; globalThis.RawMemory.set(s); }})()",
            serde_json::to_string(data)?
        );
        self.exec_script(script)
            .inspect_err(|_| self.clear_terminate_flag())?;
        Ok(())
    }

    pub fn bucket(&self) -> f64 {
        self.budget.bucket()
    }

    pub fn tick_limit(&self) -> f64 {
        self.budget.tick_limit()
    }

    /// tick 开始：预算结算 + 世界注入。返回 SkipPenalty 时本 tick 不执行玩家代码。
    pub fn begin_tick(&mut self, world: &serde_json::Value) -> anyhow::Result<TickPlan> {
        match self.budget.begin_tick() {
            TickPlan::SkipPenalty => Ok(TickPlan::SkipPenalty),
            TickPlan::Run { .. } => {
                // 清除可能遗留的终止标志，然后武装看门狗
                self.runtime.v8_isolate().cancel_terminate_execution();
                let deadline = Instant::now()
                    + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
                self.watchdog.arm(deadline);
                *self.shared.tick_start.lock().unwrap() = Some(Instant::now());
                self.shared.reset_console_gate();
                self.shared.near_heap_fired.store(false, Ordering::Relaxed);
                let world_json = serde_json::to_string(world)?;
                // beginTick 世界注入也在武装窗口内，硬中断（看门狗/near-heap）同样可能
                // 打在它身上——失败必须走统一的硬中断善后，不留"中毒"状态
                self.exec_script(format!("globalThis.__kk.beginTick({world_json})"))
                    .inspect_err(|_| self.after_termination())?;
                Ok(TickPlan::Run {
                    tick_limit: self.budget.tick_limit(),
                })
            }
        }
    }

    /// 执行玩家入口 `require('main').loop()`。
    pub fn run_loop(&mut self) -> anyhow::Result<TickOutcome> {
        let result = self.exec_json("globalThis.__kk.runLoop()");
        match result {
            Ok(_) => Ok(TickOutcome::Completed { used_ms: 0.0 }),
            Err(_) => {
                // execute_script 失败且非玩家逻辑错误（runLoop 内部已 try/catch）≈ 硬中断
                self.after_termination();
                Ok(TickOutcome::Terminated)
            }
        }
    }

    /// tick 结束：Memory 写回 + CPU 结算。
    /// 结算与 tick_start 清理**不因脚本错误跳过**——endTick 的玩家侧错误（如 Memory
    /// 不可序列化）已在 prelude 内归类处理（console.error + 跳过写回），这里返回的
    /// Err 只可能是基础设施级故障。
    pub fn end_tick(&mut self, used_ms: f64) -> anyhow::Result<()> {
        self.watchdog.disarm();
        let exec = self.exec_script("globalThis.__kk.endTick()");
        self.budget.settle(used_ms);
        *self.shared.tick_start.lock().unwrap() = None;
        exec
    }

    /// 求值一段玩家表达式（screeps-ok 适配器的 runPlayer 通道），恰好占用一个 tick 的
    /// 执行语义由调用方（engine/embed）负责编排。
    pub fn run_expr(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        let script = serde_json::to_string(code)?;
        match self.exec_json(format!("globalThis.__kk.runExpr({script})")) {
            Ok(v) => Ok(v),
            Err(_) => {
                self.after_termination();
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 硬中断善后：解除武装、清终止标志、结算（官方语义：扣整份 tickLimit，桶充足
    /// 不跳帧，负桶由 begin_tick 停跑）、恢复堆上限、清 tick_start。幂等。
    fn after_termination(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        self.budget.settle_terminated();
        self.restore_heap_limit();
        *self.shared.tick_start.lock().unwrap() = None;
    }

    /// tick 外路径（set_modules/set_raw_memory 等）失败的轻量善后：只清终止标志。
    fn clear_terminate_flag(&mut self) {
        self.runtime.v8_isolate().cancel_terminate_execution();
    }

    /// 看门狗句柄（测试驱动真实看门狗线程的代际守卫用）。
    pub fn watchdog(&self) -> &Watchdog {
        &self.watchdog
    }

    /// 近堆回调是否在本 tick 触发过（诊断：区分内存闸门与看门狗兜底路径）。
    pub fn near_heap_fired(&self) -> bool {
        self.shared.near_heap_fired.load(Ordering::Relaxed)
    }

    /// 当前 tick 已用的墙钟毫秒（M0 口径；正式实现换线程 CPU 时钟）。
    pub fn cpu_used_ms(&self) -> f64 {
        self.shared
            .tick_start
            .lock()
            .unwrap()
            .map(|t0| t0.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }

    pub fn drain_console(&mut self) -> Vec<LogLine> {
        std::mem::take(&mut *self.shared.console.lock().unwrap())
    }

    pub fn drain_missing_api(&mut self) -> anyhow::Result<serde_json::Value> {
        self.exec_json("globalThis.__kk.drainMissing()")
            .inspect_err(|_| self.clear_terminate_flag())
    }

    pub fn drain_stub_calls(&mut self) -> anyhow::Result<serde_json::Value> {
        self.exec_json("globalThis.__kk.drainStubCalls()")
            .inspect_err(|_| self.clear_terminate_flag())
    }

    fn exec_script(&mut self, code: impl Into<String>) -> anyhow::Result<()> {
        let _guard = self.tokio.enter();
        self.runtime
            .execute_script("kk", code.into())
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("execute_script: {e}"))
    }

    /// 执行脚本并把返回值（JSON 字符串）解析为 Value。
    fn exec_json(&mut self, code: impl Into<String>) -> anyhow::Result<serde_json::Value> {
        let _guard = self.tokio.enter();
        let global = self
            .runtime
            .execute_script("kk", code.into())
            .map_err(|e| anyhow::anyhow!("execute_script: {e}"))?;
        deno_core::scope!(scope, self.runtime);
        let local = v8::Local::new(scope, global);
        let text = local.to_rust_string_lossy(scope);
        Ok(serde_json::from_str(&text)?)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let mut registry = LIFO_REGISTRY.lock().unwrap();
        if registry.last() != Some(&self.seq) {
            // V8 硬约束被违反——不 panic（Drop 里不能），大声告警并把自己从栈里摘除。
            eprintln!(
                "kk-runner: 销毁顺序违反 LIFO（seq={}，栈顶={:?}）。V8 要求 isolate 按创建逆序销毁！",
                self.seq,
                registry.last()
            );
            registry.retain(|s| *s != self.seq);
        } else {
            registry.pop();
        }
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

fn constants_script() -> String {
    // 官方形态：常量深拷贝后平铺为裸全局（game.js:435），玩家可覆盖（不用 freeze，
    // freeze 的语义差异已登记为待验证项）。
    format!("(function(){{ var C = {CONSTANTS_JSON}; for (var k in C) globalThis[k] = C[k]; }})();")
}
