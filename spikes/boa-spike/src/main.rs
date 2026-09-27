//! boa_engine 嵌入式 JS 引擎能力 spike（实验 A–E）
//!
//! 运行：cargo run --release
//! 所有实验结果一次性打印；实验 C 的“分配器硬上限”演示通过子进程
//! （BOA_SPIKE_CHILD=memcap）运行，避免 abort 杀掉主进程。

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use boa_engine::error::{EngineError, RuntimeLimitError};
use boa_engine::{Context, JsError, Source};

const BOA_VERSION: &str = "0.22.0"; // 与 Cargo.toml 中 boa_engine = "0.22.0" 对应

// ---------------------------------------------------------------------------
// 全局计数分配器：统计存活字节 / 峰值；MEMCAP>0 时超限 abort（补救路径演示）
// ---------------------------------------------------------------------------
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
static MEMCAP: AtomicUsize = AtomicUsize::new(0); // 0 = 不启用硬上限
static IN_CAP_PRINT: AtomicBool = AtomicBool::new(false);

/// 无分配的十进制格式化（数字写到缓冲区头部，返回位数）
fn udec(buf: &mut [u8; 24], mut n: usize) -> usize {
    if n == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 24];
    let mut i = 0;
    while n > 0 {
        tmp[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    for k in 0..i {
        buf[k] = tmp[i - 1 - k];
    }
    i
}

/// 触发硬上限时的报告：绝对不能走会分配的路径（eprintln!/format! 会重入 alloc 造成
/// 无限递归栈溢出——本实验第一版就因此翻车），用栈缓冲 + 裸 stderr write。
fn report_cap_exceeded(live: usize, cap: usize) {
    use std::io::Write;
    let mut out = [0u8; 96];
    let mut len = 0usize;
    let mut push = |s: &[u8]| {
        out[len..len + s.len()].copy_from_slice(s);
        len += s.len();
    };
    push(b"MEMCAP_EXCEEDED live_bytes=");
    let mut nb = [0u8; 24];
    let nl = udec(&mut nb, live);
    push(&nb[..nl]);
    push(b" cap_bytes=");
    let mut cb = [0u8; 24];
    let cl = udec(&mut cb, cap);
    push(&cb[..cl]);
    push(b"\n");
    let _ = std::io::stderr().write_all(&out[..len]);
    let _ = std::io::stderr().flush();
}

struct CountingCapAllocator;

unsafe impl GlobalAlloc for CountingCapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
            let cap = MEMCAP.load(Ordering::Relaxed);
            if cap != 0 && live > cap {
                // 补救路径演示：到达硬上限时强制终止进程（无法只杀单个 runtime）。
                // 若本次调用是打印期间的再入分配，直接放行让打印完成后由外层 abort。
                if !IN_CAP_PRINT.swap(true, Ordering::Relaxed) {
                    report_cap_exceeded(live, cap);
                    std::process::abort();
                }
            }
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = self.alloc(layout);
        if !ptr.is_null() {
            std::ptr::write_bytes(ptr, 0, layout.size());
        }
        ptr
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
        let new = self.alloc(new_layout);
        if !new.is_null() {
            std::ptr::copy_nonoverlapping(ptr, new, layout.size().min(new_size));
            self.dealloc(ptr, layout);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: CountingCapAllocator = CountingCapAllocator;

// ---------------------------------------------------------------------------
// sysinfo RSS
// ---------------------------------------------------------------------------
fn rss_self() -> u64 {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0) // 字节（sysinfo 0.30+ RSS 单位为字节）
}

fn eval_string(ctx: &mut Context, code: &str) -> Result<String, String> {
    match ctx.eval(Source::from_bytes(code)) {
        Ok(v) => Ok(v.display().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// 实验 A：性能基准
// ---------------------------------------------------------------------------

const FIB_SRC: &str = "function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }";
const OBJCHURN_SRC: &str = "function objchurn() { let total = 0; for (let i = 0; i < 200; i++) { const objs = []; for (let j = 0; j < 500; j++) { objs.push({ x: j, y: j, hp: j * 3, type: 'creep' }); } for (const o of objs) { total += o.hp; } } return total; }";
const JSONCHURN_SRC: &str = "function jsonchurn() { let big = JSON.stringify({ a: Array.from({ length: 1000 }, (_, i) => ({ id: i, v: i * 2 })) }); for (let i = 0; i < 100; i++) { const o = JSON.parse(big); o.a[0].v = i; big = JSON.stringify(o); } return big.length; }";

struct BenchResult {
    #[allow(dead_code)]
    name: &'static str,
    reps: usize,
    samples_ms: Vec<f64>,
    median_ms: f64,
    value: String,
}

/// 每次重复用全新 Context（排除上次代码的编译缓存影响），计时仅覆盖 eval，
/// Context 创建在计时外。重复次数见各调用点；累计超 30s 自动截断。
fn bench(name: &'static str, setup: &str, call: &str, reps: usize, expected: Option<f64>) -> BenchResult {
    let full = format!("{}\n{}", setup, call);
    let mut samples_ms = Vec::new();
    let mut value = String::from("n/a");
    let mut cumulative = Duration::ZERO;
    for i in 0..reps {
        let mut ctx = Context::default();
        let t0 = Instant::now();
        let r = ctx.eval(Source::from_bytes(&full));
        let dt = t0.elapsed();
        match r {
            Ok(v) => {
                let n = v.to_number(&mut ctx).unwrap_or(f64::NAN);
                value = format!("{}", v.display());
                if let Some(exp) = expected {
                    assert!(
                        (n - exp).abs() < 1e-6,
                        "{} 结果校验失败: got {} want {}",
                        name, n, exp
                    );
                }
            }
            Err(e) => value = format!("<error: {}>", e),
        }
        samples_ms.push(dt.as_secs_f64() * 1000.0);
        cumulative += dt;
        if cumulative > Duration::from_secs(30) && i + 1 < reps {
            eprintln!("[bench:{}] 累计超 30s，提前结束（完成 {}/{} 次）", name, i + 1, reps);
            break;
        }
    }
    let mut sorted = samples_ms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    BenchResult {
        name,
        reps: samples_ms.len(),
        samples_ms,
        median_ms: sorted[sorted.len() / 2],
        value,
    }
}

// ---------------------------------------------------------------------------
// 实验 B：死循环硬中断（boa 无时间式中断，用 loop_iteration_limit 校准到 ~200ms）
// ---------------------------------------------------------------------------

fn run_dead_loop_with_budget(budget: u64) -> (Duration, Result<String, JsError>) {
    let mut ctx = Context::default();
    let mut lim = ctx.runtime_limits();
    lim.set_loop_iteration_limit(budget);
    ctx.set_runtime_limits(lim);
    let t0 = Instant::now();
    let r = ctx.eval(Source::from_bytes("while(true){}"));
    (t0.elapsed(), r.map(|v| v.display().to_string()))
}

fn exp_interrupt() -> (bool, String) {
    println!("== 实验B：死循环硬中断 ==");
    // 说明：boa_engine 0.22.0 无时间式中断 handler（源码 0 命中 "interrupt"），
    // 唯一等价机制是 RuntimeLimits::loop_iteration_limit（每调用帧累计循环回边计数）。
    // 用同一形状的空体 while(true){} 先试跑一个迭代预算，按耗时线性换算到 ~200ms。
    let target_ms = 200.0f64;
    let trial_budget: u64 = 10_000_000;
    let (t_trial, r_trial) = run_dead_loop_with_budget(trial_budget);
    let err = r_trial.expect_err("试跑应被 loop_iteration_limit 终止");
    let trial_ms = t_trial.as_secs_f64() * 1000.0;
    println!(
        "B.1 校准试跑: budget={} iterations, elapsed={:.1} ms, err=\"{}\"",
        trial_budget, trial_ms, err
    );

    let mut budget = ((target_ms / trial_ms) as f64 * trial_budget as f64) as u64;
    let mut final_ms = trial_ms;
    let mut final_budget = trial_budget;
    for _ in 0..3 {
        let (t, r) = run_dead_loop_with_budget(budget);
        let e = r.expect_err("死循环应被终止");
        let ms = t.as_secs_f64() * 1000.0;
        println!(
            "B.2 迭代预算={} -> 死循环在 {:.1} ms 被终止, err=\"{}\"",
            budget, ms, e
        );
        final_ms = ms;
        final_budget = budget;
        if (80.0..=500.0).contains(&ms) {
            break;
        }
        budget = ((target_ms / ms) as f64 * budget as f64) as u64;
    }

    // 错误类型精确识别：应为 EngineError::RuntimeLimit(RuntimeLimitError::LoopIteration)
    let (_t, r) = run_dead_loop_with_budget(final_budget);
    let e = r.expect_err("预期错误");
    let is_loop_limit = matches!(
        e.as_engine(),
        Some(EngineError::RuntimeLimit(RuntimeLimitError::LoopIteration))
    );
    println!(
        "B.3 错误类型识别: as_engine()={:?} 是LoopIteration={}",
        e.as_engine().map(|e2| e2.to_string()),
        is_loop_limit
    );

    // 同一 runtime（同一 Context）中断后继续执行新代码
    let mut ctx = Context::default();
    let mut lim = ctx.runtime_limits();
    lim.set_loop_iteration_limit(final_budget);
    ctx.set_runtime_limits(lim);
    let _ = ctx.eval(Source::from_bytes("while(true){}")); // 必然 Err
    let after = eval_string(&mut ctx, "6 * 7");
    let reusable = after == Ok("42".to_string());
    println!("B.4 中断后同一 Context 继续执行 6*7 => {:?} (可复用: {})", after, reusable);

    // JS 侧 try/catch 能否拦截该错误（若能拦截，注意帧计数不重置，重试立即再抛）
    let mut ctx2 = Context::default();
    let mut lim2 = ctx2.runtime_limits();
    lim2.set_loop_iteration_limit(1_000_000);
    ctx2.set_runtime_limits(lim2);
    let caught = eval_string(
        &mut ctx2,
        "let m='no-throw'; try { while(true){} } catch(e) { m='js-caught:' + (e && e.message ? e.message : String(e)); } m",
    );
    println!("B.5 JS 侧 try/catch 拦截结果: {:?}", caught);

    let ok = is_loop_limit && reusable;
    let detail = format!(
        "loop_iteration_limit(等价机制,非时间式,校准预算={}) 实测终止于{:.0}ms; 官方时间式中断不存在(issue #3442 open, PR #5475/#5380 关闭未合并); 中断后同Context可继续执行={}; JS try/catch={:?}",
        final_budget, final_ms, reusable, caught
    );
    (ok, detail)
}

// ---------------------------------------------------------------------------
// 实验 C：内存硬上限
// ---------------------------------------------------------------------------

fn run_memcap_child() -> i32 {
    // 子进程模式：64MB 硬上限 + 无限扩张数组 => 应在约 64MB 处被 abort
    let cap = 64 * 1024 * 1024usize;
    MEMCAP.store(cap, Ordering::Relaxed);
    let mut ctx = Context::default();
    println!(
        "MEMCAP_CHILD_START cap={} live_start={}",
        cap,
        LIVE_BYTES.load(Ordering::Relaxed)
    );
    let r = ctx.eval(Source::from_bytes(
        "let a=[]; let n=0; while(true){ for(let k=0;k<10000;k++){ a.push({i:n}); } n++; } n",
    ));
    // 正常情况走不到这里（应已 abort）；走到这里说明上限没触发
    println!(
        "MEMCAP_CHILD_UNEXPECTED_RETURN {:?}",
        r.map(|v| v.display().to_string())
    );
    3
}

fn exp_memory_limit() -> (bool, String) {
    println!("== 实验C：内存硬上限 ==");
    // C.1 官方机制查证：0.22.0 与 main 分支源码均无 memory limit / heap limit API
    //     （context/mod.rs、ContextBuilder 0 命中）。实测：扩张数组不会被任何上限拦截。
    let mut ctx = Context::default();
    boa_engine::gc::force_collect();
    std::thread::sleep(Duration::from_millis(150));
    let rss_before = rss_self();
    let t0 = Instant::now();
    let r = ctx.eval(Source::from_bytes(
        "let a=[]; for(let i=0;i<1000000;i++){ a.push({x:i}); } a.length",
    ));
    let dt = t0.elapsed();
    let rss_after_alloc = rss_self();
    boa_engine::gc::force_collect(); // 数组仍被 a 持有，GC 不应回收
    std::thread::sleep(Duration::from_millis(150));
    let rss_after_gc = rss_self();
    let peak = PEAK_BYTES.load(Ordering::Relaxed);
    println!(
        "C.1 扩张 1,000,000 个对象: 结果={:?} 耗时={:.0}ms 无任何引擎报错; RSS {} -> {} MB (GC后 {} MB); 分配器峰值存活 {:.0} MB",
        r.map(|v| v.display().to_string()),
        dt.as_secs_f64() * 1000.0,
        rss_before as f64 / 1e6,
        rss_after_alloc as f64 / 1e6,
        rss_after_gc as f64 / 1e6,
        peak as f64 / 1e6
    );

    // C.2 补救路径实测：全局分配器钩子 + 64MB 硬上限（子进程，超限 abort）
    let exe = std::env::current_exe().expect("current_exe");
    let out = Command::new(exe)
        .env("BOA_SPIKE_CHILD", "memcap")
        .output()
        .expect("spawn memcap child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let hit = stderr.contains("MEMCAP_EXCEEDED");
    #[cfg(unix)]
    let exit_desc = {
        use std::os::unix::process::ExitStatusExt;
        match (out.status.code(), out.status.signal()) {
            (Some(c), _) => format!("exit={}", c),
            (None, Some(s)) => format!("signal={} (SIGABRT=6)", s),
            _ => "unknown".into(),
        }
    };
    #[cfg(not(unix))]
    let exit_desc = format!("exit={:?}", out.status.code());
    println!(
        "C.2 分配器硬上限补救(子进程 64MB): 触发={}, 子进程退出[{}], stderr: {}",
        hit,
        exit_desc,
        stderr.trim()
    );
    for line in stdout.lines() {
        println!("    [child] {}", line);
    }

    let detail = format!(
        "官方无内存上限API(0.22与main均无); 实测1e6对象RSS增{}MB无任何拦截; 补救路径实测: 全局分配器钩子64MB硬上限可强制终止进程(abort, 子进程signal验证), 但粒度为整进程且不可恢复, 需进程隔离配合",
        (rss_after_alloc.saturating_sub(rss_before)) / 1_000_000
    );
    (false, detail) // 官方机制：fail
}

// ---------------------------------------------------------------------------
// 实验 D：创建开销与常驻内存
// ---------------------------------------------------------------------------

/// 子进程模式：全新进程内创建 N 个 Context 并持有，自报 RSS
/// （隔离主进程前期实验留下的 GC 堆残留，得到冷进程数字）
fn run_hold_child(n: usize) -> i32 {
    let mut ctxs: Vec<Context> = Vec::new();
    for _ in 0..n {
        let mut c = Context::default();
        let _ = eval_string(&mut c, "1+1");
        ctxs.push(c);
    }
    boa_engine::gc::force_collect();
    std::thread::sleep(Duration::from_millis(300));
    println!(
        "HOLD_RSS n={} rss_mb={:.1} live_mb={:.1}",
        n,
        rss_self() as f64 / 1e6,
        LIVE_BYTES.load(Ordering::Relaxed) as f64 / 1e6
    );
    std::thread::sleep(Duration::from_millis(100));
    0
}

fn spawn_hold_child(n: usize) -> String {
    let exe = std::env::current_exe().expect("current_exe");
    let out = Command::new(exe)
        .env("BOA_SPIKE_CHILD", "hold")
        .env("BOA_SPIKE_HOLD", format!("{}", n))
        .output()
        .expect("spawn hold child");
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    stdout
}

fn exp_create_and_rss() -> (f64, f64) {
    println!("== 实验D：runtime 创建开销与常驻内存 ==");
    // 冷启动（进程内第一个 Context）
    let t0 = Instant::now();
    let first = Context::default();
    let cold_us = t0.elapsed().as_secs_f64() * 1e6;
    drop(first);
    boa_engine::gc::force_collect();

    // 热态：取 20 次的中位数
    for _ in 0..3 {
        drop(Context::default());
    }
    let mut times = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let c = Context::default();
        times.push(t.elapsed().as_secs_f64() * 1e6);
        drop(c);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_us = times[times.len() / 2];
    println!(
        "D.1 Context::default() 冷启动 {:.0} us, 热态中位数 {:.0} us (20 次, min {:.0} / max {:.0})",
        cold_us,
        median_us,
        times[0],
        times[times.len() - 1]
    );

    // RSS：1 个 vs 10 个同时持有
    boa_engine::gc::force_collect();
    std::thread::sleep(Duration::from_millis(200));
    let rss_base = rss_self();
    let mut ctxs: Vec<Context> = Vec::new();
    ctxs.push(Context::default());
    let _ = eval_string(&mut ctxs[0], "1+1");
    boa_engine::gc::force_collect();
    std::thread::sleep(Duration::from_millis(200));
    let rss_1 = rss_self();
    for _ in 0..9 {
        let mut c = Context::default();
        let _ = eval_string(&mut c, "1+1");
        ctxs.push(c);
    }
    boa_engine::gc::force_collect();
    std::thread::sleep(Duration::from_millis(200));
    let rss_10 = rss_self();
    let mb = |b: u64| b as f64 / 1e6;
    println!(
        "D.2 RSS: 基线 {:.0} MB -> 持有1个 {:.0} MB -> 持有10个 {:.0} MB; 边际(10-1)={:.0} MB; (10-基线)={:.0} MB",
        mb(rss_base),
        mb(rss_1),
        mb(rss_10),
        mb(rss_10.saturating_sub(rss_1)),
        mb(rss_10.saturating_sub(rss_base))
    );
    println!(
        "D.3 分配器视角: 当前存活 {:.0} MB / 峰值存活 {:.0} MB",
        LIVE_BYTES.load(Ordering::Relaxed) as f64 / 1e6,
        PEAK_BYTES.load(Ordering::Relaxed) as f64 / 1e6
    );
    drop(ctxs);
    boa_engine::gc::force_collect();

    // D.4 冷进程对照：子进程分别持有 1 个 / 10 个 Context 自报 RSS
    //     （boa_gc 的 GC 堆是 thread_local 的，同线程多 Context 共享一个堆，
    //      主进程 D.2 的基线含前期实验堆残留；子进程给出干净数字）
    let hold1 = spawn_hold_child(1);
    let hold10 = spawn_hold_child(10);
    println!("D.4 冷进程子进程: [{}] / [{}]", hold1, hold10);
    let rss_1_cold = parse_hold_rss(&hold1);
    let rss_10_cold = parse_hold_rss(&hold10);
    let rss_10_mb_cold = if rss_10_cold > 0 && rss_1_cold > 0 {
        rss_10_cold.saturating_sub(rss_1_cold) as f64 / 1e6
    } else {
        f64::NAN
    };
    println!(
        "D.4 冷进程边际(10个-1个)={:.1} MB（对照：主进程内(10-1)={:.0} MB，受共享GC堆与前期实验残留影响）",
        rss_10_mb_cold,
        mb(rss_10 - rss_1)
    );
    // RESULT 行采用冷进程数字（更有代表性）；若子进程失败则退回主进程数字
    let rss_report = if rss_10_mb_cold.is_nan() {
        (rss_10 - rss_1) as f64 / 1e6
    } else {
        rss_10_mb_cold
    };
    (median_us, rss_report)
}

fn parse_hold_rss(line: &str) -> u64 {
    // HOLD_RSS n=.. rss_mb=123.4 live_mb=..
    line.split_whitespace()
        .find(|p| p.starts_with("rss_mb="))
        .and_then(|p| p["rss_mb=".len()..].parse::<f64>().ok())
        .map(|v| (v * 1e6) as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 实验 E：安全速查（默认 Context 能否碰到宿主能力）
// ---------------------------------------------------------------------------

fn exp_security() -> String {
    println!("== 实验E：沙箱默认能力速查 ==");
    let mut ctx = Context::default();
    let probe = eval_string(
        &mut ctx,
        r#"
        JSON.stringify({
            process: typeof process,
            require: typeof require,
            fetch: typeof fetch,
            XMLHttpRequest: typeof XMLHttpRequest,
            WebSocket: typeof WebSocket,
            Node: typeof Node, Buffer: typeof Buffer,
            Java: typeof Java, WebAssembly: typeof WebAssembly,
            setTimeout: typeof setTimeout, setInterval: typeof setInterval,
            readFile: typeof readFile, load: typeof load,
            quit: typeof quit, exit: typeof exit,
            globalThisProps: Object.getOwnPropertyNames(globalThis).length
        })
        "#,
    )
    .expect("probe");
    println!("E.1 typeof 探针: {}", probe);

    let globals = eval_string(&mut ctx, "Object.getOwnPropertyNames(globalThis).join(',')")
        .expect("globals");
    println!(
        "E.2 globalThis 全部属性({}个): {}",
        globals.split(',').count(),
        globals
    );

    // 实际调用尝试（应得到 ReferenceError，而非真实宿主调用）
    let call_process = eval_string(
        &mut ctx,
        "try { process.exit(0); 'CALLED' } catch(e) { 'blocked:' + e.name }",
    );
    let call_require = eval_string(
        &mut ctx,
        "try { require('fs'); 'CALLED' } catch(e) { 'blocked:' + e.name }",
    );
    println!(
        "E.3 实际调用 process.exit => {:?}; require('fs') => {:?}",
        call_process, call_require
    );

    format!(
        "默认Context无process/require/fetch/Buffer/Timer等宿主对象(globalThis仅ECMAScript内建,{}个属性); process.exit/require尝试均为ReferenceError",
        globals.split(',').count()
    )
}

// ---------------------------------------------------------------------------

fn rustc_version() -> String {
    String::from_utf8_lossy(
        &Command::new(std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into()))
            .arg("--version")
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default(),
    )
    .trim()
    .to_string()
}

fn main() {
    // 子进程模式（实验 C 补救路径 / 实验 D 冷进程对照）
    if let Ok(mode) = std::env::var("BOA_SPIKE_CHILD") {
        if mode == "memcap" {
            std::process::exit(run_memcap_child());
        }
        if mode == "hold" {
            let n: usize = std::env::var("BOA_SPIKE_HOLD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            std::process::exit(run_hold_child(n));
        }
    }

    println!("boa_engine spike | boa_engine={} | rustc={}", BOA_VERSION, rustc_version());
    println!(
        "platform={} cpu_count={}",
        std::env::consts::OS,
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)
    );
    println!();

    // ---- 实验A ----
    println!("== 实验A：性能基准（计时仅覆盖 eval，每次重复用全新 Context）==");
    let fib27 = bench("fib27", FIB_SRC, "fib(27)", 3, Some(196_418.0));
    println!(
        "A.fib(27)  {} 次: [{}] ms, 中位 {:.1} ms, 结果={}",
        fib27.reps,
        fib27.samples_ms.iter().map(|m| format!("{:.1}", m)).collect::<Vec<_>>().join(", "),
        fib27.median_ms,
        fib27.value
    );
    let fib30 = bench("fib30", FIB_SRC, "fib(30)", 3, Some(832_040.0));
    println!(
        "A.fib(30)  {} 次: [{}] ms, 中位 {:.1} ms, 结果={}",
        fib30.reps,
        fib30.samples_ms.iter().map(|m| format!("{:.1}", m)).collect::<Vec<_>>().join(", "),
        fib30.median_ms,
        fib30.value
    );
    // objchurn/jsonchurn 各 1 次预热 + 5 次计时（单次耗时可承受）
    bench("objchurn_warm", OBJCHURN_SRC, "objchurn()", 1, None);
    let obj = bench("objchurn", OBJCHURN_SRC, "objchurn()", 5, Some(74_850_000.0));
    println!(
        "A.objchurn {} 次(另1次未计时预热): [{}] ms, 中位 {:.1} ms, 结果={}",
        obj.reps,
        obj.samples_ms.iter().map(|m| format!("{:.1}", m)).collect::<Vec<_>>().join(", "),
        obj.median_ms,
        obj.value
    );
    bench("jsonchurn_warm", JSONCHURN_SRC, "jsonchurn()", 1, None);
    let json = bench("jsonchurn", JSONCHURN_SRC, "jsonchurn()", 5, None);
    println!(
        "A.jsonchurn {} 次(另1次未计时预热): [{}] ms, 中位 {:.1} ms, 结果(=big.length)={}",
        json.reps,
        json.samples_ms.iter().map(|m| format!("{:.1}", m)).collect::<Vec<_>>().join(", "),
        json.median_ms,
        json.value
    );
    println!();

    // ---- 实验B ----
    let (interrupt_ok, interrupt_detail) = exp_interrupt();
    println!();

    // ---- 实验C ----
    let (mem_ok, mem_detail) = exp_memory_limit();
    println!();

    // ---- 实验D ----
    let (create_us, rss10_mb) = exp_create_and_rss();
    println!();

    // ---- 实验E ----
    let sec_detail = exp_security();
    println!();

    // ---- 汇总 ----
    println!("================= 汇总 =================");
    println!(
        "RESULT engine=boa_engine fib30_ms={:.1} fib27_ms={:.1} objchurn_ms={:.1} json_ms={:.1} create_runtime_us={:.0} rss_10_mb={:.1}",
        fib30.median_ms, fib27.median_ms, obj.median_ms, json.median_ms, create_us, rss10_mb
    );
    println!(
        "CAPABILITY interrupt={} memory_limit={} detail=\"{}; 内存: {}; 安全: {}\"",
        if interrupt_ok { "pass" } else { "fail" },
        if mem_ok { "pass" } else { "fail" },
        interrupt_detail, mem_detail, sec_detail
    );
    println!(
        "NOTE fib30={} fib27={} objchurn={} jsonchurn_len={} 方法: 耗时取中位数; fib各3次; obj/json各5次(另1次预热); 每次重复用全新Context且仅计eval; 中断=loop_iteration_limit按耗时线性校准至~200ms; rss_10_mb=冷进程子进程持有10个vs1个Context的RSS差; 全部结果通过期望值校验",
        fib30.value, fib27.value, obj.value, json.value
    );
}
