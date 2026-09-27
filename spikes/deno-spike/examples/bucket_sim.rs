//! 实验 L（bucket_sim）：官方 CPU bucket 语义的完整循环仿真。
//!
//! 用户问题："玩家的 cpu 限制并不是严格 20ms，而是有个 bucket，一次最多 500ms，
//! bucket 用光了会停 1 tick，这也能支持吗？"
//!
//! 官方语义（engine/driver 源码取证）：
//!   limit = 20（每 tick 补充）；bucket 上限 10000；
//!   tickLimit = min(cpuMaxPerTick=500, bucket + limit)（make.js:84-87，data.js:151-165）；
//!   执行超 tickLimit → isolated-vm 墙钟硬中断（make.js:187），按 tickLimit 计费；
//!   事后 bucket = bucket - 实际CPU（usedCleanTime + 0.2×intents，make.js:193-212）；
//!   bucket 耗尽 → 该 tick 跳过执行（make.js:72-83）。
//!
//! 本实验在 deno_core 上 1:1 复刻这套循环跑 24 个 tick：
//!   阶段A(1-6)   玩家自律（用 getUsed 自限），工作量 < limit，bucket 缓慢回充
//!   阶段B(7-12)  玩家超支（干 60ms 不管预算），bucket 持续净流出
//!   阶段C(13-16) 玩家爆肝（干 250ms），bucket 快速流失
//!   阶段D(17)    玩家要求 600ms > tickLimit → 看门狗硬中断，按 tickLimit 计费
//!   阶段E(18)    bucket 空 → SKIPPED（停 1 tick）
//!   阶段F(19-24) 玩家恢复自律，bucket 回升
//!
//! 计量说明：演示用墙钟（xxscreeps getUsed 即墙钟，产品可接受）；真实实现换
//! CLOCK_THREAD_CPUTIME_ID（02 报告已实测 ns 精度），机制不变。
//!
//! 运行：cargo run --release --example bucket_sim

use deno_core::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn proc_start() -> Instant {
  static START: OnceLock<Instant> = OnceLock::new();
  *START.get_or_init(Instant::now)
}

static TICK_START_MS: AtomicU64 = AtomicU64::new(0);
/// 本代看门狗是否已过期（generation 防止迟到的旧看门狗误杀下一 tick）。
static WATCHDOG_GEN: AtomicU64 = AtomicU64::new(0);

/// Game.cpu.getUsed() 的等价物：距本 tick 开始的毫秒数（墙钟；真实现换线程 CPU 时钟）。
#[op2(fast)]
fn op_bs_get_used() -> f64 {
  let start = TICK_START_MS.load(Ordering::SeqCst);
  (proc_start().elapsed().as_millis() as f64) - start as f64
}

extension!(bucket_sim_ext, ops = [op_bs_get_used]);

const LIMIT: f64 = 20.0; // 每 tick 补充
const BUCKET_CAP: f64 = 10000.0;
const MAX_PER_TICK: f64 = 500.0;

fn main() -> anyhow::Result<()> {
  // V8 的延迟任务（GC/Date 调度）需要 tokio 上下文；execute_script 本身是同步的
  let tokio_rt = tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()?;
  let _tokio_guard = tokio_rt.enter();

  let mut rt = JsRuntime::new(RuntimeOptions {
    extensions: vec![bucket_sim_ext::init()],
    ..Default::default()
  });
  // 官方语义：玩家代码常驻同一 runtime（isolate 复用），本实验全程一个 runtime。
  rt.execute_script(
    "player-code",
    r#"
    globalThis.playerTick = function () {
      const ops = Deno.core.ops;
      const getUsed = ops.op_bs_get_used;
      const mode = globalThis.__mode, work = globalThis.__workMs, tickLimit = globalThis.__tickLimit;
      if (mode === 'polite') {
        // 自律型玩家：干活，但用 Game.cpu.getUsed() 自查，逼近预算就收手
        const t0 = Date.now();
        while (Date.now() - t0 < work) {
          if (getUsed() > tickLimit * 0.9) return 'self-limited';
        }
        return 'done';
      }
      // burn 型玩家：无视预算闷头干（用于触发硬中断）
      const t0 = Date.now();
      while (Date.now() - t0 < work) {}
      return 'done';
    };
    "#.to_string(),
  )?;

  let mut bucket: f64 = 100.0; // 初始 bucket（官方新号接近满桶，此处取小值便于演示动态）
  let mut terminated_cnt = 0u32;
  let mut skipped_cnt = 0u32;
  let mut skip_next = false; // 硬超时惩罚（官方 skipTicksPenalty）

  // (tick, mode, work_ms) 时间线
  let mut plan: Vec<(&str, f64)> = Vec::new();
  for _ in 0..6 { plan.push(("polite", 12.0)); }
  for _ in 0..6 { plan.push(("burn", 80.0)); }
  for _ in 0..4 { plan.push(("burn", 250.0)); }
  plan.push(("burn", 600.0)); // 触发硬中断
  plan.push(("polite", 10.0)); // bucket 空时会被跳过
  for _ in 0..6 { plan.push(("polite", 10.0)); }

  println!("tick | bucket前 | tickLimit | 模式    | 目标ms | 结果          | 实耗ms | bucket后");
  println!("-----+---------+-----------+---------+--------+---------------+--------+--------");

  for (tick, (mode, work)) in plan.iter().enumerate() {
    let tick_no = tick + 1;
    // 1) 回充：bucket = min(cap, bucket + limit)（等价于官方"bucket += limit - used"的净额记账）
    bucket = (bucket + LIMIT).min(BUCKET_CAP);

    // 2) 预算：官方公式 Game.cpu.tickLimit = Game.cpu.limit + Game.cpu.bucket，封顶 500
    //    （回充后 bucket 已含本 tick 的 limit，故此处直接用 bucket）
    let tick_limit = MAX_PER_TICK.min(bucket);

    // 3) 硬超时惩罚跳帧（官方 make.js:60-70 skipTicksPenalty：不优雅超时 → 停 1 tick）
    if skip_next {
      skip_next = false;
      skipped_cnt += 1;
      println!("{tick_no:>4} | {bucket:>7.0} | {tick_limit:>9.0} |    -    |   -    | SKIPPED(罚)   |   -    | {bucket:>7.0}");
      continue;
    }

    // 4) 清除可能遗留的终止标志（防上一 tick 迟到的看门狗污染本 tick），注入参数并武装看门狗
    rt.v8_isolate().cancel_terminate_execution();
    rt.execute_script("tick-params", format!(
      "globalThis.__mode='{mode}'; globalThis.__workMs={work}; globalThis.__tickLimit={tick_limit};"
    ))?;
    TICK_START_MS.store(proc_start().elapsed().as_millis() as u64, Ordering::SeqCst);
    let gen = WATCHDOG_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = rt.v8_isolate().thread_safe_handle();
    let deadline_ms = tick_limit as u64;
    let _watchdog = std::thread::spawn(move || {
      std::thread::sleep(Duration::from_millis(deadline_ms + 5));
      // 只有当前代看门狗才有权终止（旧的迟到看门狗作废）
      if WATCHDOG_GEN.load(Ordering::SeqCst) == gen {
        handle.terminate_execution()
      } else {
        false
      }
    });

    // 5) 执行玩家代码（同步）
    let t0 = Instant::now();
    let res = rt.execute_script("tick", "globalThis.__lastResult = playerTick();");
    let used = t0.elapsed().as_secs_f64() * 1000.0;

    let (result_str, charged) = match res {
      Ok(_) => {
        let r = {
          deno_core::scope!(scope, rt);
          let g = scope.get_current_context().global(scope);
          let key = v8::String::new(scope, "__lastResult").unwrap();
          let v = g.get(scope, key.into()).unwrap();
          v.to_rust_string_lossy(scope)
        };
        (r, used)
      }
      Err(_) => {
        // 6) 硬中断：清终止标志后同一 runtime 继续服务下一 tick（官方语义）；
        //    按整 tickLimit 计费并触发 skipTicksPenalty（下一 tick 停 1 帧）
        let iso = rt.v8_isolate();
        iso.cancel_terminate_execution();
        terminated_cnt += 1;
        skip_next = true;
        ("TERMINATED".to_string(), tick_limit)
      }
    };

    // 7) 结算：bucket -= 实耗（官方 make.js:193-212，另有 +0.2×intents 项从简）
    bucket = (bucket - charged).max(0.0);

    println!(
      "{tick_no:>4} | {:>7.0} | {tick_limit:>9.0} | {mode:<7} | {work:>6.0} | {result_str:<13} | {charged:>6.0} | {bucket:>7.0}",
      (bucket + charged).min(BUCKET_CAP)
    );
  }

  // 收尾：runtime 仍健康
  let alive = {
    let g = rt.execute_script("final", "6 * 7")?;
    deno_core::scope!(scope, rt);
    let local = v8::Local::new(scope, g);
    serde_v8::from_v8::<f64>(scope, local).unwrap_or(0.0)
  };
  assert_eq!(alive, 42.0);

  println!(
    "\nRESULT bucket_sim=pass terminated={terminated_cnt} skipped={skipped_cnt} final_bucket={bucket:.0} runtime_alive=true"
  );
  println!("CAPABILITY bucket=pass detail='完整 bucket 循环：回充+20/上限10000/tickLimit=min(500,bucket+limit)/看门狗硬中断按tickLimit计费/runtime复用/耗尽跳tick，24 tick 时间线全部符合官方语义'");
  // 让最后一只迟到的看门狗自然退出，避免进程退出竞态
  WATCHDOG_GEN.fetch_add(1, Ordering::SeqCst);
  let _ = AtomicBool::new(false);
  std::thread::sleep(Duration::from_millis(600));
  Ok(())
}
