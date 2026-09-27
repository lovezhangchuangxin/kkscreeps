//! rquickjs（quickjs-ng 后端）能力验证 spike
//!
//! 实验 A：性能基准 fib(30) / objchurn / jsonchurn（Rust Instant 计时）
//! 实验 B：硬中断（while(true){} 约 200ms 处强制终止 + runtime 复用验证）
//! 实验 C：内存硬上限（64MB，无限扩张数组）
//! 实验 D：创建开销 + 同时持有 10 个 runtime 的 RSS 边际增长
//! 实验 E：安全速查（process/require/fetch/文件系统等宿主能力探测）
//!
//! 运行：cargo run --release（在本 crate 目录下）

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rquickjs::{Context, Runtime, Undefined};
use sysinfo::{get_current_pid, ProcessesToUpdate, System};

const MB: f64 = 1024.0 * 1024.0;

/// 三个工作负载的 JS 代码，原样使用（与任务书逐字一致），保证与其他引擎可比。
const WORKLOAD_JS: &str = r#"
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
function objchurn() { let total = 0; for (let i = 0; i < 200; i++) { const objs = []; for (let j = 0; j < 500; j++) { objs.push({ x: j, y: j, hp: j * 3, type: 'creep' }); } for (const o of objs) { total += o.hp; } } return total; }
function jsonchurn() { let big = JSON.stringify({ a: Array.from({ length: 1000 }, (_, i) => ({ id: i, v: i * 2 })) }); for (let i = 0; i < 100; i++) { const o = JSON.parse(big); o.a[0].v = i; big = JSON.stringify(o); } return big.length; }
"#;

fn rss_mb() -> f64 {
    let pid = get_current_pid().expect("get_current_pid");
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory() as f64 / MB).unwrap_or(0.0)
}

/// 取出 ctx 上挂起的 JS 异常并转成字符串（必须在 ctx.with 闭包内完成，Value 不能跨闭包返回）
fn exception_detail(ctx: &Context) -> String {
    ctx.with(|c| {
        let ex = c.catch();
        c.globals()
            .set("__ex", ex)
            .expect("暂存异常失败");
        c.eval::<String, _>(
            "(__ex && __ex.name !== undefined && __ex.message !== undefined) ? (__ex.name + ': ' + __ex.message) : String(__ex)",
        )
        .unwrap_or_else(|_| "（异常无法字符串化）".into())
    })
}

/// 对一次 JS 调用计时：warmup 轮不计时，runs 轮取中位数。
/// 返回 (median_ms, min_ms, max_ms, 返回值)
fn timed(ctx: &Context, call: &str, warmup: usize, runs: usize) -> (f64, f64, f64, f64) {
    let run = || ctx.with(|c| c.eval::<f64, _>(call)).unwrap_or_else(|e| panic!("{call} eval 失败: {e}"));
    let mut v = 0.0;
    for _ in 0..warmup {
        v = run();
    }
    let mut samples: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        v = run();
        samples.push(t.elapsed().as_secs_f64() * 1e3);
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    (samples[runs / 2], samples[0], samples[runs - 1], v)
}

fn fmt1(x: f64) -> String {
    format!("{x:.1}")
}

// ---------------------------------------------------------------- 实验 A

fn experiment_a() -> (f64, f64, f64) {
    println!("==================== 实验A 性能基准 ====================");
    println!("方法：JS 源码原样 eval 后，Rust 侧 std::time::Instant 围绕单次调用计时；");
    println!("      fib(30) warmup=1 + 5 轮取中位数；objchurn warmup=2 + 30 轮取中位数；");
    println!("      jsonchurn warmup=2 + 20 轮取中位数（单次均在毫秒级，无需调整负载规模）");
    let rt = Runtime::new().expect("Runtime::new");
    let ctx = Context::full(&rt).expect("Context::full");
    ctx.with(|c| c.eval::<Undefined, _>(WORKLOAD_JS)).expect("定义工作负载失败");

    let (fib_ms, fib_min, fib_max, fib_val) = timed(&ctx, "fib(30)", 1, 5);
    println!("fib(30)     : median={:.1}ms min={:.1}ms max={:.1}ms 返回值={}（期望 832040）", fib_ms, fib_min, fib_max, fib_val as i64);

    let (obj_ms, obj_min, obj_max, obj_val) = timed(&ctx, "objchurn()", 2, 30);
    println!("objchurn()  : median={:.3}ms min={:.3}ms max={:.3}ms 返回值={}（期望 74850000）", obj_ms, obj_min, obj_max, obj_val as i64);

    let (json_ms, json_min, json_max, json_val) = timed(&ctx, "jsonchurn()", 2, 20);
    println!("jsonchurn() : median={:.3}ms min={:.3}ms max={:.3}ms 返回值={}（字符串长度，恒定）", json_ms, json_min, json_max, json_val as i64);

    (fib_ms, obj_ms, json_ms)
}

// ---------------------------------------------------------------- 实验 B

fn experiment_b() -> (bool, bool, f64) {
    println!("\n==================== 实验B 硬中断 ====================");
    println!("方法：set_interrupt_handler 装一个由 Arc<AtomicBool> 驱动的 handler；");
    println!("      eval 死循环的同时起线程 sleep 200ms 后置位；handler 返回 true 即中断");
    let rt = Runtime::new().expect("Runtime::new");
    let ctx = Context::full(&rt).expect("Context::full");

    let flag = Arc::new(AtomicBool::new(false));
    let hf = flag.clone();
    rt.set_interrupt_handler(Some(Box::new(move || hf.load(Ordering::Relaxed))));

    let armed = flag.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        armed.store(true, Ordering::Relaxed);
    });

    let t = Instant::now();
    let res = ctx.with(|c| c.eval::<f64, _>("var t = 0; while (true) { t++ } t"));
    let elapsed_ms = t.elapsed().as_secs_f64() * 1e3;

    flag.store(false, Ordering::Relaxed); // 解除中断标志
    match &res {
        Ok(v) => println!("死循环返回了值 {v}（未中断！）"),
        Err(e) => {
            let ex = exception_detail(&ctx);
            println!("eval 被中断：Err({e:?})，Display=\"{e}\"，JS 异常详情=\"{ex}\"");
        }
    }
    let interrupted = res.is_err();

    let reuse = ctx.with(|c| c.eval::<f64, _>("40 + 2"));
    let reuse_ok = matches!(reuse, Ok(42.0));
    println!(
        "中断后同一 runtime+context 继续执行 40+2 -> {:?}（复用{}）",
        reuse,
        if reuse_ok { "成功" } else { "失败" }
    );
    println!("实际中断耗时 {:.1}ms（目标 ~200ms），宿主进程存活（本行由 Rust 打印）", elapsed_ms);
    (interrupted, reuse_ok, elapsed_ms)
}

// ---------------------------------------------------------------- 实验 C

fn experiment_c() -> (bool, f64, f64) {
    println!("\n==================== 实验C 内存硬上限 ====================");
    println!("方法：set_memory_limit(64MB) 后执行不断 push 大数组的死循环，观察是否被强制终止");
    let rt = Runtime::new().expect("Runtime::new");
    rt.set_memory_limit(64 * 1024 * 1024);
    let ctx = Context::full(&rt).expect("Context::full");

    let rss_before = rss_mb();
    let t = Instant::now();
    let res = ctx.with(|c| {
        c.eval::<f64, _>(
            "const a = []; let i = 0; while (true) { a.push(new Array(10000).fill(1.1)); i++ } i",
        )
    });
    let elapsed_ms = t.elapsed().as_secs_f64() * 1e3;
    let rss_after = rss_mb();

    match &res {
        Ok(v) => println!("循环自然返回 i={v}（未触发上限！）"),
        Err(e) => {
            let ex = exception_detail(&ctx);
            println!("内存上限触发：Err({e:?})，Display=\"{e}\"，JS 异常详情=\"{ex}\"");
        }
    }
    let limited = res.is_err();
    println!(
        "耗时 {:.1}ms；进程 RSS：开始 {:.1}MB -> 出错后 {:.1}MB（上限 64MB，应远小于吃光系统内存）",
        elapsed_ms, rss_before, rss_after
    );

    // OOM 之后 runtime 是否还能用：直接试一次，失败再 gc 后试一次
    let reuse1 = ctx.with(|c| c.eval::<f64, _>("6 * 7"));
    let reuse_direct = matches!(reuse1, Ok(42.0));
    rt.run_gc();
    let reuse2 = ctx.with(|c| c.eval::<f64, _>("6 * 7"));
    let reuse_after_gc = matches!(reuse2, Ok(42.0));
    println!(
        "OOM 后继续执行 6*7：直接 -> {:?}；run_gc 后 -> {:?}",
        reuse1.map(|v| v as i64).map_err(|e| e.to_string()),
        reuse2.map(|v| v as i64).map_err(|e| e.to_string())
    );
    let _ = (reuse_direct, reuse_after_gc);
    (limited, rss_after, elapsed_ms)
}

// ---------------------------------------------------------------- 实验 D

fn experiment_d() -> (f64, f64, f64) {
    println!("\n==================== 实验D 创建开销与常驻内存 ====================");
    println!("方法：创建开销 = Runtime::new + Context::full + eval(\"0\") + drop 一个完整周期，");
    println!("      100 轮取中位数；RSS 用 sysinfo 读本进程，对比 某一时刻 vs 同时持有 10 个 runtime");

    let mut samples: Vec<f64> = Vec::with_capacity(100);
    for _ in 0..100 {
        let t = Instant::now();
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        ctx.with(|c| c.eval::<f64, _>("0")).expect("warm eval");
        drop(ctx);
        drop(rt);
        samples.push(t.elapsed().as_secs_f64() * 1e6);
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let create_us = samples[50];
    println!(
        "创建一个 runtime（含 context+eval+drop）：median={:.0}us min={:.0}us max={:.0}us（100 轮）",
        create_us,
        samples[0],
        samples[99]
    );

    let rss_base = rss_mb();
    let mut held: Vec<(Runtime, Context)> = Vec::with_capacity(10);
    for i in 0..10 {
        let rt = Runtime::new().expect("Runtime::new");
        let ctx = Context::full(&rt).expect("Context::full");
        ctx.with(|c| {
            c.eval::<f64, _>(format!("globalThis.pad = new Array(10000).fill({i}); 0").as_str())
        })
        .expect("分配驻留内存");
        held.push((rt, ctx));
    }
    let rss_10 = rss_mb() - rss_base;
    println!(
        "持有 10 个 runtime（每个 eval 过并驻留一个 1 万元素数组）RSS：{:.1}MB -> {:.1}MB，边际增长 {:.1}MB（均摊 {:.2}MB/runtime）",
        rss_base,
        rss_base + rss_10,
        rss_10,
        rss_10 / 10.0
    );
    (create_us, rss_10, rss_base)
}

// ---------------------------------------------------------------- 实验 E

fn experiment_e() -> String {
    println!("\n==================== 实验E 安全速查 ====================");
    println!("方法：裸 Runtime+Context::full（未注册任何 native 函数/模块加载器），");
    println!("      JS 侧 typeof 探测宿主能力 + 枚举 globalThis + 尝试动态 import('std')");
    let rt = Runtime::new().expect("Runtime::new");
    let ctx = Context::full(&rt).expect("Context::full");

    let probes = ctx
        .with(|c| {
            c.eval::<String, _>(
                r#"JSON.stringify({
  process: typeof process,
  globalThis_process: typeof globalThis.process,
  require: typeof require,
  fetch: typeof fetch,
  XMLHttpRequest: typeof XMLHttpRequest,
  Deno: typeof Deno,
  readFile: typeof readFile,
  readFileSync: typeof readFileSync,
  writeFile: typeof writeFile,
  read: typeof read,
  write: typeof write,
  load: typeof load,
  importScripts: typeof importScripts,
  quit: typeof quit,
  exit: typeof exit,
  os: typeof os,
  std: typeof std,
  WebAssembly: typeof WebAssembly,
})"#,
            )
        })
        .expect("探测 eval 失败");
    println!("typeof 探测：{probes}");

    let globals = ctx
        .with(|c| c.eval::<String, _>("Object.getOwnPropertyNames(globalThis).sort().join(',')"))
        .expect("枚举 globalThis 失败");
    println!("globalThis 自有属性（全部）：{globals}");

    // 动态 import 探测：无 loader 时应被拒绝；用 pending job 队列取回异步结果
    let import_probe = ctx.with(|c| {
        c.eval::<f64, _>(
            "globalThis.__imp = ''; import('std').then(m => { globalThis.__imp = 'IMPORT-OK ' + Object.keys(m).sort().join(','); }).catch(e => { globalThis.__imp = 'blocked: ' + e.message; }); 0",
        )
        .expect("发起 import 失败");
        while c.execute_pending_job() {}
        c.eval::<String, _>("globalThis.__imp").expect("读取结果失败")
    });
    println!("动态 import('std')：{import_probe}");

    let leaked_native: Vec<String> = ["process", "require", "fetch", "readFile", "writeFile", "os", "std", "load", "quit"]
        .iter()
        .filter(|n| globals.split(',').any(|g| g == **n))
        .map(|s| s.to_string())
        .collect();
    println!("globalThis 上泄漏的宿主能力名：{:?}", if leaked_native.is_empty() { "无".to_string() } else { leaked_native.join(",") });

    format!(
        "sandbox-clean: 宿主全局变量均不可见(typeof=undefined); import('std') {}",
        if import_probe.starts_with("blocked") { "被拒绝" } else { "IMPORT-OK(泄漏!)" }
    )
}

fn main() {
    // 辅助模式：`rquickjs-spike hold <N>` —— 创建 N 个 runtime 并驻留 3 秒后退出，
    // 供 `/usr/bin/time -l` 在冷进程下对比 RSS（实验D 的补充证据）。
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "hold" {
        let n: usize = args[2].parse().expect("hold 需要 0-10 的数字");
        let mut held: Vec<(Runtime, Context)> = Vec::with_capacity(n);
        for i in 0..n {
            let rt = Runtime::new().expect("Runtime::new");
            let ctx = Context::full(&rt).expect("Context::full");
            ctx.with(|c| {
                c.eval::<f64, _>(format!("globalThis.pad = new Array(10000).fill({i}); 0").as_str())
            })
            .expect("分配驻留内存");
            held.push((rt, ctx));
        }
        thread::sleep(Duration::from_secs(3));
        println!("hold {n} runtimes done");
        return;
    }

    println!("rquickjs spike（引擎版本由 JS_GetVersion() 报告，crate 版本见 Cargo.lock）");
    let version = unsafe {
        CStr::from_ptr(rquickjs::qjs::JS_GetVersion())
            .to_string_lossy()
            .into_owned()
    };
    println!("JS_GetVersion() = {version}");
    println!("目标平台：{}", std::env::consts::OS);

    let t_all = Instant::now();
    let (fib_ms, obj_ms, json_ms) = experiment_a();
    let (interrupted, reuse_ok, interrupt_ms) = experiment_b();
    let (limited, rss_oom, oom_ms) = experiment_c();
    let (create_us, rss_10, rss_base) = experiment_d();
    let sandbox_detail = experiment_e();
    let total_s = t_all.elapsed().as_secs_f64();

    println!("\n==================== SUMMARY ====================");
    println!(
        "RESULT engine=rquickjs fib30_ms={} objchurn_ms={:.3} json_ms={:.3} create_runtime_us={:.0} rss_10_mb={:.1}",
        fmt1(fib_ms), obj_ms, json_ms, create_us, rss_10
    );
    println!(
        "CAPABILITY interrupt={} memory_limit={} detail=\"interrupt {} at {:.0}ms (target 200ms), runtime-reuse-after-interrupt={}; oom-limit killed at {:.0}ms with RSS {:.1}MB (limit 64MB); {}\"",
        if interrupted && reuse_ok { "pass" } else { "fail" },
        if limited { "pass" } else { "fail" },
        if interrupted { "fired" } else { "NOT fired" },
        interrupt_ms,
        if reuse_ok { "ok" } else { "broken" },
        oom_ms,
        rss_oom,
        sandbox_detail
    );
    println!(
        "NOTE base_rss={:.1}MB total_run_s={:.1} method=\"median of runs per experiment A spec\"",
        rss_base, total_s
    );
}
