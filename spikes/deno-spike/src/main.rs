//! deno_core (rusty_v8) 能力验证 spike —— xscreeps 沙箱选型
//!
//! 实验总览（详见 docs/research/notes/spike-deno_core.md）：
//!  A 性能基准：fib(30) / objchurn / jsonchurn（Rust std::time::Instant 计时）
//!  B 硬中断：while(true){} + 看门狗线程 terminate_execution，验证宿主存活与 runtime 复用
//!  C 内存硬上限：heap_limits(0, 64MB) + near_heap_limit 回调内 terminate（子进程运行，防 V8 fatal OOM 波及）
//!  D 创建开销与常驻内存：JsRuntime::new 耗时；同时持有 10 个 runtime 的 RSS 边际增长
//!  E 安全速查：默认沙箱能碰到哪些宿主能力
//!  附加：startup snapshot 启动耗时对比；Extension 同步 op 调用开销

use deno_core::*;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

// ---------------------------------------------------------------- 常量

/// 实验A工作负载，JS 代码原样取自任务描述（保证三个引擎可比）。
const WORKLOAD_JS: &str = r#"
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
function objchurn() { let total = 0; for (let i = 0; i < 200; i++) { const objs = []; for (let j = 0; j < 500; j++) { objs.push({ x: j, y: j, hp: j * 3, type: 'creep' }); } for (const o of objs) { total += o.hp; } } return total; }
function jsonchurn() { let big = JSON.stringify({ a: Array.from({ length: 1000 }, (_, i) => ({ id: i, v: i * 2 })) }); for (let i = 0; i < 100; i++) { const o = JSON.parse(big); o.a[0].v = i; big = JSON.stringify(o); } return big.length; }
"#;

/// 实验C：无限扩张数组（在 64MB 堆上限 + 近堆回调终止的 runtime 中执行）。
const MEM_TEST_JS: &str = r#"
const a = [];
let n = 0;
for (;;) { a.push(new Array(10000).fill(1.5)); n += 10000; }
"#;

// ---------------------------------------------------------------- Extension 同步 op

/// 原始类型参数的同步 op（op2 fast 路径，免 serde）。
#[op2(fast)]
fn op_spike_add(a: u32, b: u32) -> u32 {
  a.wrapping_add(b)
}

/// serde 序列化参数的同步 op（官方 hello_world 示例同款）。
#[op2]
fn op_spike_sum(#[serde] nums: Vec<f64>) -> Result<f64, deno_error::JsErrorBox> {
  Ok(nums.iter().fold(0.0, |a, v| a + v))
}

// 注意：op 名不能与 deno_core 内置 op 撞名（内置已含 op_add 等），
// 撞名会在 ext:core/ops 合成模块里产生重复导出，触发 V8 CHECK 失败直接 abort 进程。
extension!(spike_ext, ops = [op_spike_add, op_spike_sum]);

// ---------------------------------------------------------------- 工具

fn rss_bytes() -> u64 {
  let pid = sysinfo::Pid::from_u32(std::process::id());
  let mut sys = System::new();
  sys.refresh_processes_specifics(
    ProcessesToUpdate::Some(std::slice::from_ref(&pid)),
    true,
    ProcessRefreshKind::nothing().with_memory(),
  );
  sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

/// 在 runtime 里执行 JS 并把结果反序列化为 JSON（serde_v8）。
/// 若结果是 JSON.stringify 产生的字符串则再解析一层。
fn eval_json(rt: &mut JsRuntime, code: &str) -> serde_json::Value {
  let g = rt
    .execute_script("<anon>", code.to_string())
    .expect("eval failed");
  deno_core::scope!(scope, rt);
  let local = v8::Local::new(scope, g);
  match serde_v8::from_v8(scope, local) {
    Ok(serde_json::Value::String(s)) => {
      serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))
    }
    Ok(v) => v,
    Err(e) => serde_json::json!(format!("<deserialize failed: {e:?}>")),
  }
}

fn median(mut v: Vec<f64>) -> f64 {
  v.sort_by(|a, b| a.partial_cmp(b).unwrap());
  v[v.len() / 2]
}

fn ms(d: Duration) -> f64 {
  d.as_secs_f64() * 1e3
}

fn us(d: Duration) -> f64 {
  d.as_secs_f64() * 1e6
}

// ---------------------------------------------------------------- 实验A：性能基准

struct BenchResult {
  fib30_ms: f64,
  objchurn_ms: f64,
  jsonchurn_ms: f64,
}

fn experiment_a() -> BenchResult {
  println!("== 实验A 性能基准（方法：预热 1 次后测 5 次取中位数；Rust Instant 包住 execute_script，含 Rust↔JS 边界开销） ==");
  let mut rt = JsRuntime::new(RuntimeOptions::default());
  rt.execute_script("workload", WORKLOAD_JS).unwrap();

  let sanity = eval_json(
    &mut rt,
    r#"JSON.stringify({fib30: fib(30), obj: objchurn(), json: jsonchurn()})"#,
  );
  println!("   校验: fib(30)={} objchurn={} jsonchurn={}", sanity["fib30"], sanity["obj"], sanity["json"]);

  let run_bench = |rt: &mut JsRuntime, code: &str| -> Vec<f64> {
    rt.execute_script("warm", code.to_string()).unwrap();
    let mut times = Vec::new();
    for _ in 0..5 {
      let t0 = Instant::now();
      rt.execute_script("bench", code.to_string()).unwrap();
      times.push(ms(t0.elapsed()));
    }
    times
  };

  let fib_times = run_bench(&mut rt, "fib(30)");
  println!("   fib(30)   5次={fib_times:?} ms -> 中位 {}", median(fib_times.clone()));
  let obj_times = run_bench(&mut rt, "objchurn()");
  println!("   objchurn  5次={obj_times:?} ms -> 中位 {}", median(obj_times.clone()));
  let json_times = run_bench(&mut rt, "jsonchurn()");
  println!("   jsonchurn 5次={json_times:?} ms -> 中位 {}", median(json_times.clone()));

  BenchResult {
    fib30_ms: median(fib_times),
    objchurn_ms: median(obj_times),
    jsonchurn_ms: median(json_times),
  }
}

// ---------------------------------------------------------------- 实验B：硬中断

struct InterruptResult {
  pass: bool,
  elapsed_ms: f64,
  err_message: String,
  reused_ok: bool,
}

fn experiment_b() -> InterruptResult {
  println!("== 实验B 硬中断（方法：主线程 execute_script(while(true){{}})，看门狗线程 200ms 后调用 v8 IsolateHandle::terminate_execution；IsolateHandle 为 Send+Sync，rusty_v8 官方跨线程终止路径） ==");
  let mut rt = JsRuntime::new(RuntimeOptions::default());
  let handle = rt.v8_isolate().thread_safe_handle();

  let watchdog = std::thread::spawn(move || {
    std::thread::sleep(Duration::from_millis(200));
    handle.terminate_execution()
  });
  let t0 = Instant::now();
  let res = rt.execute_script("spin", "while (true) {}");
  let elapsed = ms(t0.elapsed());
  let terminated_back = watchdog.join().expect("watchdog panicked");
  println!("   execute_script 用时 {elapsed:.1} ms 后返回（看门狗 terminate 返回值={terminated_back}），宿主线程存活");

  match res {
    Err(js_err) => {
      let msg = js_err.exception_message.clone();
      println!("   捕获中断错误: {msg:?}");
      // 验证 runtime 可复用：清终止标志后再执行新代码
      let iso = rt.v8_isolate();
      let still = iso.is_execution_terminating();
      iso.cancel_terminate_execution();
      let after = rt.execute_script("after-interrupt", "(6 * 7) + (typeof Array)");
      let reused_ok = match after {
        Ok(g) => {
          let mut rt2 = rt;
          deno_core::scope!(scope, rt2);
          let local = v8::Local::new(scope, g);
          let val: String = serde_v8::from_v8(scope, local).unwrap_or_default();
          println!("   中断后同一 runtime 继续执行新代码: 结果={val:?}（清标志前 is_execution_terminating={still}）");
          val == "42function" // typeof Array === 'function'
        }
        Err(e2) => {
          println!("   中断后 runtime 无法执行新代码: {e2:?}");
          false
        }
      };
      InterruptResult { pass: true, elapsed_ms: elapsed, err_message: msg, reused_ok }
    }
    Ok(_) => {
      println!("   while(true) 正常返回?! 中断失败");
      InterruptResult { pass: false, elapsed_ms: elapsed, err_message: "no-error".into(), reused_ok: false }
    }
  }
}

// ---------------------------------------------------------------- 实验C：内存硬上限（子进程）

fn experiment_c_child() -> i32 {
  // 在子进程中运行：即使 V8 走到 FatalProcessOutOfMemory 也不影响父进程打印结果
  println!("MEMTEST_BEGIN heap_limit_bytes={}", 64 * 1024 * 1024);
  let create_params = v8::CreateParams::default().heap_limits(0, 64 * 1024 * 1024);
  let mut rt = JsRuntime::new(RuntimeOptions {
    create_params: Some(create_params),
    ..Default::default()
  });
  let cb_handle = rt.v8_isolate().thread_safe_handle();
  let fired = Arc::new(AtomicUsize::new(0));
  let fired_in_cb = fired.clone();
  rt.add_near_heap_limit_callback(move |current_limit, _initial| {
    fired_in_cb.fetch_add(1, Ordering::SeqCst);
    // deno_core 自身测试（runtime/tests/misc.rs test_heap_limit_cb）同款模式：
    // 回调内触发 terminate，返回放大后的上限让当前分配得以完成，
    // 随即因终止标志在下次中断检查点展开 —— 避免 V8 FatalProcessOutOfMemory 直接 abort 进程
    cb_handle.terminate_execution();
    current_limit * 2
  });

  // RSS 峰值采样线程
  let peak = Arc::new(AtomicU64::new(0));
  let peak2 = peak.clone();
  let stop = Arc::new(AtomicBool::new(false));
  let stop2 = stop.clone();
  let sampler = std::thread::spawn(move || {
    while !stop2.load(Ordering::Relaxed) {
      peak2.fetch_max(rss_bytes(), Ordering::Relaxed);
      std::thread::sleep(Duration::from_millis(5));
    }
  });

  let t0 = Instant::now();
  let res = rt.execute_script("memtest", MEM_TEST_JS);
  let elapsed = ms(t0.elapsed());
  stop.store(true, Ordering::Relaxed);
  sampler.join().unwrap();
  let peak_mb = peak.load(Ordering::Relaxed) as f64 / 1048576.0;

  let stats = rt.v8_isolate().get_heap_statistics();
  match res {
    Err(e) => {
      println!(
        "MEMTEST_RESULT terminated=true elapsed_ms={elapsed:.1} peak_rss_mb={peak_mb:.1} cb_fired={} heap_used_mb={:.1} heap_limit_mb={:.1} err={:?}",
        fired.load(Ordering::SeqCst),
        stats.used_heap_size() as f64 / 1048576.0,
        stats.heap_size_limit() as f64 / 1048576.0,
        e.exception_message
      );
      0
    }
    Ok(_) => {
      println!("MEMTEST_RESULT terminated=false elapsed_ms={elapsed:.1} peak_rss_mb={peak_mb:.1} cb_fired={}", fired.load(Ordering::SeqCst));
      1
    }
  }
}

fn experiment_c_parent() -> (bool, String) {
  println!("== 实验C 内存硬上限（方法：子进程运行 heap_limits(0,64MB)+near_heap_limit 回调终止，父进程 60s 超时保护；JS 无限 push 大数组） ==");
  let exe = std::env::current_exe().expect("current exe");
  let mut child = std::process::Command::new(exe)
    .arg("memtest")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .spawn()
    .expect("spawn memtest child");

  let deadline = Instant::now() + Duration::from_secs(60);
  let status = loop {
    match child.try_wait().expect("try_wait") {
      Some(status) => break status,
      None => {
        if Instant::now() > deadline {
          let _ = child.kill();
          println!("   子进程超时 60s（内存上限未生效？），已 kill");
          return (false, "child timed out after 60s, killed".into());
        }
        std::thread::sleep(Duration::from_millis(50));
      }
    }
  };
  let text = child
    .wait_with_output()
    .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    .unwrap_or_default();
  for line in text.lines().filter(|l| l.starts_with("MEMTEST")) {
    println!("   [child] {line}");
  }
  let result_line = text.lines().find(|l| l.starts_with("MEMTEST_RESULT")).unwrap_or("no result line");
  let pass = text.contains("terminated=true") && status.success();
  (
    pass,
    format!("child_exit={:?}; {result_line}", status.code()),
  )
}

// ---------------------------------------------------------------- 实验D：创建开销与 RSS

struct CreateResult {
  first_create_us: f64,
  median_create_us: f64,
  rss_1_mb: f64,
  rss_10_mb: f64,
  per_runtime_mb: f64,
}

fn experiment_d() -> CreateResult {
  println!("== 实验D 创建开销与常驻内存（方法：首次创建含 V8 平台初始化；随后建 20 次取中位数。RSS 用 sysinfo 读本进程，drop 掉此前 runtime 后测基线，再分别持 1 / 10 个 runtime，每个建后执行小脚本触碰页面） ==");
  let t0 = Instant::now();
  let mut first = JsRuntime::new(RuntimeOptions::default());
  let first_us = us(t0.elapsed());
  first.execute_script("touch", "globalThis.x = [1,2,3].reduce((a,b)=>a+b,0)").unwrap();
  println!("   首次 JsRuntime::new（含 V8 平台初始化）: {first_us:.0} us");

  let mut times = Vec::new();
  for _ in 0..20 {
    let t = Instant::now();
    let mut rt = JsRuntime::new(RuntimeOptions::default());
    rt.execute_script("touch", "1+1").unwrap();
    times.push(us(t.elapsed()));
    drop(rt);
  }
  println!(
    "   后续 20 次 create+小脚本: 中位 {:.0} us, min {:.0}, max {:.0}",
    median(times.clone()),
    times.iter().cloned().fold(f64::INFINITY, f64::min),
    times.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
  );

  drop(first);
  std::thread::sleep(Duration::from_millis(200)); // 等后台任务安顿
  let base = rss_bytes();
  println!("   基线 RSS: {:.1} MB", base as f64 / 1048576.0);

  let make = |i: usize| {
    let mut rt = JsRuntime::new(RuntimeOptions::default());
    rt.execute_script(
      "touch",
      format!("globalThis.tag = {i}; globalThis.arr = new Array(8192).fill({i}); arr.length"),
    )
    .unwrap();
    rt
  };

  let one = make(1);
  let rss1 = rss_bytes();
  let d1 = rss1.saturating_sub(base) as f64 / 1048576.0;

  let mut all = vec![one];
  for i in 2..=10 {
    all.push(make(i));
  }
  std::thread::sleep(Duration::from_millis(200));
  let rss10 = rss_bytes();
  let d10 = rss10.saturating_sub(base) as f64 / 1048576.0;
  let per = (rss10.saturating_sub(rss1)) as f64 / 9.0 / 1048576.0;
  println!("   持 1 个 runtime:  RSS Δ = {d1:.1} MB");
  println!("   持 10 个 runtime: RSS Δ = {d10:.1} MB；第 2..10 个平均边际 {per:.1} MB/个");

  // rusty_v8 硬约束：OwnedIsolate 创建即 Enter、drop 即 Exit，
  // 多 isolate 必须【逆序（LIFO）】销毁，否则 V8 FatalProcessOutOfMemory/HandleScope 崩溃。
  // （实测正序 drop 会触发 "Fatal error in v8::HandleScope::CreateHandle()" 进程 abort）
  println!("   按 LIFO 逆序销毁 10 个 runtime ...");
  while let Some(rt) = all.pop() {
    drop(rt);
  }
  let post = rss_bytes();
  println!("   全部销毁后 RSS 回落至 Δ={:.1} MB", post.saturating_sub(base) as f64 / 1048576.0);

  CreateResult {
    first_create_us: first_us,
    median_create_us: median(times),
    rss_1_mb: d1,
    rss_10_mb: d10,
    per_runtime_mb: per,
  }
}

// ---------------------------------------------------------------- 实验E：安全速查

fn experiment_e() {
  println!("== 实验E 安全速查（默认 RuntimeOptions，不加任何宿主扩展） ==");
  let mut rt = JsRuntime::new(RuntimeOptions::default());
  let probe = eval_json(
    &mut rt,
    r#"JSON.stringify({
      process: typeof process,
      require: typeof require,
      fetch: typeof fetch,
      Deno: typeof Deno,
      DenoCore: (typeof Deno === 'object' && Deno.core) ? 'object' : typeof Deno,
      DenoCoreOps: (typeof Deno === 'object' && Deno.core && Deno.core.ops) ? Object.keys(Deno.core.ops).sort().join(',') : 'n/a',
      XMLHttpRequest: typeof XMLHttpRequest,
      WebSocket: typeof WebSocket,
      WebAssembly: typeof WebAssembly,
      SharedArrayBuffer: typeof SharedArrayBuffer,
      globals: Object.getOwnPropertyNames(globalThis).sort().join(',')
    })"#,
  );
  let obj = probe.as_object().expect("probe json").clone();
  for (k, v) in &obj {
    if k != "globals" {
      println!("   {k:16} = {v}");
    }
  }
  println!("   globalThis 属性 = {}", obj["globals"].as_str().unwrap_or(""));
}

// ---------------------------------------------------------------- 附加：snapshot 与 同步 op

fn snapshot_experiment() {
  println!("== 附加实验 startup snapshot（方法：build.rs 里 create_snapshot 预热全局 fib/SNAPSHOT_PRELOADED；运行时对比 无/有 snapshot 的 JsRuntime::new 中位耗时（各 10 次），并验证预热状态随 snapshot 恢复） ==");
  static SNAPSHOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/snapshot.bin"));

  let mut cold = Vec::new();
  for _ in 0..10 {
    let t = Instant::now();
    let rt = JsRuntime::new(RuntimeOptions::default());
    cold.push(us(t.elapsed()));
    drop(rt);
  }
  let cold_med = median(cold.clone());

  let mut warm = Vec::new();
  for _ in 0..10 {
    let t = Instant::now();
    let rt = JsRuntime::new(RuntimeOptions {
      startup_snapshot: Some(SNAPSHOT),
      ..Default::default()
    });
    warm.push(us(t.elapsed()));
    drop(rt);
  }
  let warm_med = median(warm.clone());
  println!("   无 snapshot: 10 次中位 {cold_med:.0} us；有 snapshot: {warm_med:.0} us（加速 {:.2}x）", cold_med / warm_med);

  let mut rt = JsRuntime::new(RuntimeOptions {
    startup_snapshot: Some(SNAPSHOT),
    ..Default::default()
  });
  let v = eval_json(
    &mut rt,
    r#"JSON.stringify({ preloaded: (typeof globalThis.SNAPSHOT_PRELOADED !== 'undefined') && SNAPSHOT_PRELOADED === true, fib20: (typeof fib === 'function') ? fib(20) : null, regex: (typeof WARM_REGEX === 'object') })"#,
  );
  println!("   snapshot 恢复验证: {v}");
}

fn sync_op_experiment() {
  println!("== 附加实验 Extension 同步 op（op2 宏，Rust 函数暴露给 JS 的 Deno.core.ops.*） ==");
  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![spike_ext::init()],
    ..Default::default()
  });
  let ok = eval_json(
    &mut rt,
    r#"JSON.stringify({ add: Deno.core.ops.op_spike_add(2, 40), sum: Deno.core.ops.op_spike_sum([1,2,3,4,5]) })"#,
  );
  println!("   正确性: {ok}");

  let t0 = Instant::now();
  let r = eval_json(
    &mut rt,
    r#"(function(){ let s = 0; for (let i = 0; i < 1000000; i++) s += Deno.core.ops.op_spike_add(i, 1); return s; })()"#,
  );
  let add_ns = t0.elapsed().as_secs_f64() * 1e9 / 1e6;
  println!("   op_spike_add(u32,u32) 1e6 次连续调用: 求和={r}, 平均 {add_ns:.0} ns/call");

  let t0 = Instant::now();
  let r2 = eval_json(
    &mut rt,
    r#"(function(){ let s = 0; const a = [1.5, 2.5]; for (let i = 0; i < 100000; i++) s += Deno.core.ops.op_spike_sum(a); return s; })()"#,
  );
  let sum_ns = t0.elapsed().as_secs_f64() * 1e9 / 1e5;
  println!("   op_spike_sum(serde Vec<f64>) 1e5 次连续调用: 求和={r2}, 平均 {sum_ns:.0} ns/call");
}

// ---------------------------------------------------------------- main

fn main() {
  // deno_core 的 GC 压力场景会 post delayed foreground tasks，需要 tokio runtime context
  //（deno_core 自身测试 runtime/tests/misc.rs 同款做法）
  let tokio_rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap();
  let _tokio_guard = tokio_rt.enter();

  if std::env::args().nth(1).as_deref() == Some("memtest") {
    std::process::exit(experiment_c_child());
  }

  println!(
    "deno-spike | deno_core={} deno_v8(v8)={} sysinfo={} deno_error={} tokio={}",
    env!("SPIKE_VER_DENO_CORE"),
    env!("SPIKE_VER_DENO_V8"),
    env!("SPIKE_VER_SYSINFO"),
    env!("SPIKE_VER_DENO_ERROR"),
    env!("SPIKE_VER_TOKIO"),
  );
  println!("v8 engine version: {}", v8::V8::get_version());

  let bench = experiment_a();
  let interrupt = experiment_b();
  let create = experiment_d();
  experiment_e();
  snapshot_experiment();
  sync_op_experiment();
  let (mem_pass, mem_detail) = experiment_c_parent();

  println!();
  println!("================= 汇总 =================");
  println!(
    "RESULT engine=deno_core(rusty_v8) fib30_ms={:.1} objchurn_ms={:.1} json_ms={:.1} create_runtime_us={:.0} rss_10_mb={:.1}",
    bench.fib30_ms, bench.objchurn_ms, bench.jsonchurn_ms, create.median_create_us, create.rss_10_mb
  );
  println!(
    "CAPABILITY interrupt={} memory_limit={} detail=\"interrupt: {:.1}ms 终止, err='{}', runtime复用={}; memory: {}\"",
    if interrupt.pass { "pass" } else { "fail" },
    if mem_pass { "pass" } else { "fail" },
    interrupt.elapsed_ms,
    interrupt.err_message.replace('\'', "`"),
    interrupt.reused_ok,
    mem_detail
  );
  println!(
    "EXTRA first_create_us={:.0} rss_1_mb={:.1} per_runtime_mb={:.1} interrupt_reuse={}",
    create.first_create_us, create.rss_1_mb, create.per_runtime_mb, interrupt.reused_ok
  );
  println!("NOTE fib_n=30 方法=预热1次后5次取中位(Rust Instant 含边界开销)");
}
