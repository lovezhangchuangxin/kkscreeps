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
/// prelude 按职责分五个段（共享仪表/Buffer/CJS/memory+世界投影/宿主通道），
/// 各段独立 IIFE、经 globalThis.__kkp 共享私有状态（末段删除）。
const PRELUDE_SEGMENTS: [(&str, &str); 5] = [
    ("kk-prelude-00-shared", include_str!("prelude/00_shared.js")),
    ("kk-prelude-10-buffer", include_str!("prelude/10_buffer.js")),
    ("kk-prelude-20-cjs", include_str!("prelude/20_cjs.js")),
    (
        "kk-prelude-30-memory-world",
        include_str!("prelude/30_memory_world.js"),
    ),
    ("kk-prelude-90-host", include_str!("prelude/90_host.js")),
];

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
static LIFO_VIOLATIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 进程内 LIFO 销毁纪律的违规次数（测试断言为零——告警不该被无视）。
/// 当前进程存活的 Sandbox 数（测试用：全部销毁后应归零）。
pub fn live_sandbox_count() -> usize {
    LIFO_REGISTRY.lock().unwrap().len()
}

pub fn lifo_violations() -> usize {
    LIFO_VIOLATIONS.load(Ordering::SeqCst)
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
    /// 本 tick 是否已硬中断（after_termination 已按整份结算）——end_tick 据此
    /// 跳过写回（官方：超时打断整个 run 含序列化）。注意与"正常结算完毕"区分：
    /// 正常 end_tick 之后此标志保持 false，tick 外的 run_expr+end_tick 仍可写回。
    terminated_this_tick: bool,
    /// 上一次 runLoop 捕获的玩家逻辑错误（run_loop 写入，take_last_loop_error 取走）。
    last_loop_error: Option<String>,
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

        // 注入顺序：lodash（结果放 __kkLodash）→ 常量平铺 → prelude 各段
        // （首段捕获 op、末段遮蔽 Deno 并删除 __kkp）。
        runtime.execute_script("kk-lodash", lodash_script())?;
        runtime.execute_script("kk-constants", constants_script())?;
        for (name, src) in PRELUDE_SEGMENTS {
            runtime.execute_script(name, src.to_string())?;
        }

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
            terminated_this_tick: false,
            last_loop_error: None,
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
                self.terminated_this_tick = false;
                self.runtime.v8_isolate().cancel_terminate_execution();
                // world 序列化在 arm 之前完成：arm 之后的任何失败路径都必须 disarm
                // （序列化 Value 理论不可达失败，防御性保证不变量成立）
                let world_json = serde_json::to_string(world)?;
                let deadline = Instant::now()
                    + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
                self.watchdog.arm(deadline);
                *self.shared.tick_start.lock().unwrap() = Some(Instant::now());
                self.shared.reset_console_gate();
                self.shared.near_heap_fired.store(false, Ordering::Relaxed);
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

    /// 执行玩家入口 `require('main').loop()`。入口执行完毕即 Completed——
    /// "执行到入口"≠"玩家代码无异常"：玩家逻辑错误由 runLoop 内部捕获，
    /// 经 `take_last_loop_error` 暴露（报告/验收统计据此区分）。
    pub fn run_loop(&mut self) -> anyhow::Result<TickOutcome> {
        let result = self.exec_json("globalThis.__kk.runLoop()");
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
                // execute_script 失败且非玩家逻辑错误（runLoop 内部已 try/catch）≈ 硬中断
                self.after_termination();
                Ok(TickOutcome::Terminated)
            }
        }
    }

    /// 取走上一次 runLoop 捕获的玩家逻辑错误（None = 正常完成或硬中断）。
    pub fn take_last_loop_error(&mut self) -> Option<String> {
        self.last_loop_error.take()
    }

    /// tick 结束：Memory 写回 + CPU 结算。
    /// **看门狗保持武装直到写回完成**（官方：Memory 序列化计入玩家的超时窗口与
    /// CPU 计费——玩家可经 `Memory.toJSON` 在写回阶段执行任意代码，不能 disarm
    /// 后再执行玩家可感代码）。
    /// 返回写回结果 JSON：`{ok:true}` | `{ok:false,error}`（按 screeps-ok 契约归为
    /// 玩家 runtime 错误）| `{ok:false,terminated:true}`（写回阶段被硬中断，已按
    /// 整份 tickLimit 善后结算）。Err 仅剩基础设施级故障。
    pub fn end_tick(&mut self) -> anyhow::Result<serde_json::Value> {
        // 官方语义：硬超时打断的是**整个 run（含 tick 末序列化）**——被中断的 tick
        // 不再写回，Memory 保持上一 tick 存档。硬中断后仍执行写回的话，玩家可经
        // Memory.toJSON 在无看门狗保护的阶段挂死线程
        if self.terminated_this_tick {
            return Ok(serde_json::json!({
                "ok": false, "terminated": true,
                "error": "本 tick 已硬中断，按官方语义跳过 Memory 写回",
            }));
        }
        // 迟到的看门狗 terminate 标志不得误伤写回（run 恰在 deadline 前完成时）——
        // 但 cancel 清掉的是**唯一**的超时信号：若该代 terminate 已投递（fired 已
        // 记录、同代不再重投），随后挂死的写回将没有第二次终止机会。因此 cancel
        // 后**重新武装到本 tick 的原 deadline**（由 tick_start 重算，与 begin_tick
        // 无漂移）：正常写回不受影响（旧标志已清），写回挂死由新代兜底——cancel
        // 的"防误伤"与"写回保持在武装窗口内"两个约束不再矛盾
        self.runtime.v8_isolate().cancel_terminate_execution();
        if let Some(t0) = *self.shared.tick_start.lock().unwrap() {
            let deadline = t0 + Duration::from_millis(self.budget.tick_limit().max(1.0) as u64);
            self.watchdog.arm(deadline);
        }
        let exec = self.exec_json("globalThis.__kk.endTick()");
        match exec {
            Ok(v) => {
                self.watchdog.disarm();
                self.budget.settle(self.cpu_used_ms()); // 含写回耗时（官方口径）
                *self.shared.tick_start.lock().unwrap() = None;
                Ok(v)
            }
            Err(_) => {
                // 写回阶段被硬中断（如玩家经 Memory.toJSON 挂死写回）：统一善后
                // （disarm、清标志、按整份 tickLimit 结算、恢复堆上限）
                self.after_termination();
                Ok(
                    serde_json::json!({ "ok": false, "terminated": true, "error": "写回阶段硬中断" }),
                )
            }
        }
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

    /// 独立 runExpr 通道（无 tick 生命周期）的看门狗编排：清遗留标志 → 武装
    /// 一个按官方公式现算的窗口（`min(maxPerTick, limit+bucket)`——不能读 stale
    /// 的 `tick_limit` 字段：新沙箱为 0、tick 过的为残留值）→ 求值 → 双路径
    /// disarm + 清标志（迟到的 terminate 不得残留到下一条命令）。
    /// 不带 begin/end_tick：无世界注入、**不动预算桶**（该通道的计费归 M2
    /// intent 通道定型；tick 上下文的善后结算走 `run_expr` 的 Err 路径）。
    pub fn run_expr_guarded(&mut self, code: &str) -> anyhow::Result<serde_json::Value> {
        let script = serde_json::to_string(code)?; // arm 前完成可失败步骤
        self.clear_terminate_flag();
        let window = self.budget.next_tick_limit().max(1.0);
        self.watchdog
            .arm(Instant::now() + Duration::from_millis(window as u64));
        let out = self.exec_json(format!("globalThis.__kk.runExpr({script})"));
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        match out {
            Ok(v) => Ok(v),
            Err(_) => {
                // 独立通道被掐断：恢复堆上限即可，不动桶、不碰 tick 生命周期状态
                self.restore_heap_limit();
                Ok(serde_json::json!({ "ok": false, "kind": "terminated", "error": "硬中断" }))
            }
        }
    }

    /// 硬中断善后：解除武装、清终止标志、结算（官方语义：扣整份 tickLimit，桶充足
    /// 不跳帧，负桶由 begin_tick 停跑）、恢复堆上限、清 tick_start。幂等；
    /// 本 tick 已由善后结算的，end_tick 不再重复结算。
    fn after_termination(&mut self) {
        self.watchdog.disarm();
        self.runtime.v8_isolate().cancel_terminate_execution();
        if !self.terminated_this_tick {
            self.budget.settle_terminated();
            self.terminated_this_tick = true;
        }
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

    /// V8 当前生效的堆上限。近堆回调的返回值会把它持久抬高（remove+re-add 恢复），
    /// 测试据此断言"触发后恢复到配置值"而非仅凭实例复用。
    pub fn heap_size_limit_bytes(&mut self) -> usize {
        self.runtime
            .v8_isolate()
            .get_heap_statistics()
            .heap_size_limit()
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
            LIFO_VIOLATIONS.fetch_add(1, Ordering::SeqCst);
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
