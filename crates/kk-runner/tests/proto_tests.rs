//! 验证玩家专属线程的执行、回收、预算提交和中断后复用。
//!
//! 全部场景通过一个测试函数串行运行，避免进程级存活计数与销毁顺序计数
//! 被其他场景的沙箱污染。

use std::time::{Duration, Instant};

use serde_json::json;

use kk_runner::{
    ModuleTable, PlayerThreads, RunEpoch, SandboxConfig, TickOutcome, TickPlan, live_sandbox_count,
};

/// 用于验证报告原样返回派发轮次，不在测试线程中验证世界版本。
const EPOCH: RunEpoch = RunEpoch {
    tick: 1,
    generation: 0,
};

fn config() -> SandboxConfig {
    SandboxConfig {
        heap_limit_bytes: 64 * 1024 * 1024,
        ..SandboxConfig::default()
    }
}

fn counter_bot() -> ModuleTable {
    let mut t = ModuleTable::new();
    t.insert_js(
        "main",
        "module.exports.loop = function () { globalThis.__n = (globalThis.__n || 0) + 1; };",
    );
    t
}

fn hang_bot() -> ModuleTable {
    let mut t = ModuleTable::new();
    t.insert_js(
        "main",
        "module.exports.loop = function () { const t0 = Date.now(); while (true) {} };",
    );
    t
}

fn world() -> serde_json::Value {
    json!({
        "time": 1,
        "generation": 42,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "rooms": {},
        "objects": []
    })
}

#[test]
fn m0_thread_model_suite() -> anyhow::Result<()> {
    host_budget_job()?;
    pending_receipts_are_independent()?;
    queued_command_deadline()?;
    multi_player_ticks()?;
    idle_eviction_and_rebuild()?;
    out_of_order_eviction()?;
    crash_isolation()?;
    skip_penalty_recharge_channel()?;
    lifo_shutdown_clean()?;
    run_expr_watchdog()?;
    Ok(())
}

/// 验证两种线程命令的停跑回充：Tick 即时提交，RunPlayer 保留候选，
/// 随后 settle_bill 或 discard_bill 决定是否更新余额。
fn skip_penalty_recharge_channel() -> anyhow::Result<()> {
    let skip_cfg = || SandboxConfig {
        heap_limit_bytes: 64 * 1024 * 1024,
        initial_bucket: -40.0,
        ..SandboxConfig::default()
    };
    let mut pool = PlayerThreads::new();
    // 通道 A（Cmd::Tick）：停跑轮即时提交，bucket_after 与原即时落桶口径逐位
    // 一致（-40 → 报告 -20），intents 恒 Null
    pool.spawn_player(1, skip_cfg())?;
    let r = pool.tick(1, &world())?;
    assert_eq!(
        r.epoch,
        RunEpoch {
            tick: 1,
            generation: 42
        }
    );
    assert!(
        matches!(r.plan, TickPlan::SkipPenalty),
        "负桶停跑：{:?}",
        r.plan
    );
    assert_eq!(
        r.bucket_after, -20.0,
        "Tick 通道停跑轮即时并入回充（bucket_after=提交后值）"
    );
    assert!(r.intents.is_null(), "停跑轮不产出 intents");

    // 通道 B（Cmd::RunPlayer）：候选留协调器——kind=terminated、无计费候选、
    // bucket_after=提交前口径（-40，回充未并入）
    pool.spawn_player(2, skip_cfg())?;
    let rep = pool.run_player(2, &world(), "'never-runs'", EPOCH)?;
    assert_eq!(rep.epoch, EPOCH, "回执回带派发轮次标识（A1）");
    assert_eq!(
        rep.expr["kind"], "terminated",
        "停跑轮 expr 形态：{:?}",
        rep.expr
    );
    assert_eq!(rep.cpu_billed_ms, None, "停跑轮无计费候选");
    assert_eq!(
        rep.bucket_after, -40.0,
        "RunPlayer 通道 bucket_after=提交前口径（回充候选未并入）"
    );
    // 协调器提交臂：settle_bill 恰并入一次——随后 tick 应报告 −20 回充后再
    // 一轮到 0（若候选未被提交，bucket 仍 −40 → 报告 −20）
    pool.settle_bill(2)?;
    let r = pool.tick(2, &world())?;
    assert!(matches!(r.plan, TickPlan::SkipPenalty));
    assert_eq!(
        r.bucket_after, 0.0,
        "RunPlayer 候选经 settle_bill 提交（−40→−20）后本轮再回充到 0"
    );
    let r = pool.tick(2, &world())?;
    assert!(
        matches!(r.plan, TickPlan::Run { .. }),
        "回正后恢复执行：{:?}",
        r.plan
    );

    // 协调器回退臂（Cmd::DiscardBill）：候选丢弃后桶零痕迹——后续停跑轮照常
    // 即时并入（精确的丢弃语义由 sandbox 级 b06 用例锁定，此处锁通道连通）
    pool.spawn_player(3, skip_cfg())?;
    let rep = pool.run_player(3, &world(), "'x'", EPOCH)?;
    assert_eq!(rep.epoch, EPOCH, "回执回带派发轮次标识（A1，两通道一致）");
    assert_eq!(rep.expr["kind"], "terminated");
    pool.discard_bill(3)?;
    let r = pool.tick(3, &world())?;
    assert!(matches!(r.plan, TickPlan::SkipPenalty));
    assert_eq!(r.bucket_after, -20.0, "丢弃后新一轮照常一次回充");
    pool.shutdown_all();
    Ok(())
}

/// 独立表达式中的死循环应在预算窗口内中断，且线程仍可继续求值。
/// 默认窗口是 min(500, 20+10000)=500 毫秒；有限循环用于排除错误的 1 毫秒窗口。
fn run_expr_watchdog() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(7, config())?;
    // 有限循环应能在默认 500 毫秒窗口内完成，不能使用尚未初始化的缓存预算。
    let v = pool.run_expr(7, "let s = 0; for (let i = 0; i < 2e7; i++) s += i; s")?;
    assert_eq!(v["ok"], true, "合法重表达式不被误掐: {v:?}");
    let t0 = Instant::now();
    let v = pool.run_expr(7, "while (true) {}")?;
    let elapsed = t0.elapsed();
    assert_eq!(
        v.get("kind").and_then(serde_json::Value::as_str),
        Some("terminated"),
        "死循环被掐断: {v:?}"
    );
    // 允许中断处理和线程调度余量，但死循环应在 2 秒内返回。
    assert!(elapsed.as_millis() < 2000, "掐断及时（{elapsed:?}）");
    // 线程善后后可复用；且独立通道不动预算桶（后续 tick 不因本通道停跑）
    let v = pool.run_expr(7, "1 + 1")?;
    assert_eq!(v["value"], 2, "掐断后通道可继续服务");
    let report = pool.tick(7, &world())?;
    assert!(
        matches!(report.outcome, TickOutcome::Completed { .. }),
        "独立通道掐断不动桶——后续 tick 正常执行"
    );
    pool.shutdown_all();
    Ok(())
}

/// 8 玩家各跑 50 tick：计数器正确、报告齐全。
fn multi_player_ticks() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    for id in 0..8 {
        pool.spawn_player(id, config())?;
        pool.set_modules(id, counter_bot())?;
    }
    assert_eq!(pool.len(), 8);
    assert_eq!(live_sandbox_count(), 8, "8 个 isolate 存活");

    for _ in 0..50 {
        let reports = pool.tick_all(&world())?;
        assert_eq!(reports.len(), 8);
        for (_, r) in &reports {
            let r = r.as_ref().expect("tick 报告");
            assert!(
                matches!(r.outcome, TickOutcome::Completed { .. }),
                "全部玩家正常执行"
            );
        }
    }
    // 计数核对：每玩家的 __n 都应是 50
    for id in 0..8 {
        let v = pool.run_expr(id, "globalThis.__n")?;
        assert_eq!(v["value"], 50, "玩家 {id} 的 50 tick 全部执行");
    }
    pool.shutdown_all();
    Ok(())
}

/// 空闲逐出：销毁后注册栈回基线；同 id 重建可继续服务（重建耗时可接受）。
fn idle_eviction_and_rebuild() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(100, config())?;
    pool.set_modules(100, counter_bot())?;
    pool.tick(100, &world())?;

    let evicted = pool.evict_idle(Duration::ZERO);
    assert_eq!(evicted, vec![100], "空闲玩家被逐出");
    assert_eq!(live_sandbox_count(), 0, "isolate 已销毁");
    assert!(pool.is_empty());

    // 当前重新执行 prelude 装配，不使用 V8 启动快照；检查重建时间低于测试阈值。
    let t0 = Instant::now();
    pool.spawn_player(100, config())?;
    pool.set_modules(100, counter_bot())?;
    let rebuild_ms = t0.elapsed().as_millis();
    pool.tick(100, &world())?;
    let v = pool.run_expr(100, "globalThis.__n")?;
    assert_eq!(v["value"], 1, "重建后继续服务");
    assert!(
        rebuild_ms < 1500,
        "实例重建 <1.5s（实测 {rebuild_ms}ms，M0 无快照；M2 快照后 <1ms）"
    );
    pool.shutdown_all();
    Ok(())
}

/// 回收中间空闲玩家，验证较晚创建的活跃玩家不会阻挡它，且存活玩家能继续求值。
/// 每个线程只有一个 V8 实例，因此跨线程回收不受同线程逆序销毁约束。
fn out_of_order_eviction() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(1, config())?;
    pool.spawn_player(2, config())?;
    pool.spawn_player(3, config())?;
    // B 保持空闲：A/C 的 last_command 在 sleep 后刷新，B 仍是 spawn 时刻。
    // sleep 300ms/阈值 150ms 给宿主线程停顿留 ~145ms 余量（CI 高负载下
    // run_expr 返回到 evict 检查之间被调度停顿不得把 A/C 一并逐出）
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(pool.run_expr(1, "1 + 1")?["value"], 2);
    assert_eq!(pool.run_expr(3, "1 + 1")?["value"], 2);

    let evicted = pool.evict_idle(Duration::from_millis(150));
    assert_eq!(
        evicted,
        vec![2],
        "只有中间空闲的 B 被逐出（全表扫描，非栈顶）"
    );
    assert_eq!(pool.len(), 2, "活跃的 A/C 不受影响");
    assert_eq!(live_sandbox_count(), 2, "B 的 isolate 已销毁");

    // 乱序销毁后存活实例正常求值
    assert_eq!(pool.run_expr(1, "2 + 2")?["value"], 4, "A 在乱序销毁后正常");
    assert_eq!(pool.run_expr(3, "2 + 2")?["value"], 4, "C 在乱序销毁后正常");

    // 全表回收（剩余全部空闲）后干净退出；每线程纪律零违规
    assert_eq!(pool.evict_idle(Duration::ZERO).len(), 2, "剩余实例全表回收");
    assert_eq!(live_sandbox_count(), 0, "全部 isolate 销毁");
    assert_eq!(
        kk_runner::lifo_violations(),
        0,
        "每线程 LIFO 纪律零违规（跨线程乱序不计违规）"
    );
    Ok(())
}

/// 崩溃隔离：一个玩家死循环被硬中断，另一个玩家不受影响。
/// 按 plan 区分真实硬中断与跳帧 tick（官方语义：桶充足时硬中断不跳帧，
/// 此配置下三轮全部是 Run+Terminated——若跳帧逻辑回归为"每中断必跳一帧"，
/// 第 2 轮的 plan 会变成 SkipPenalty 而被此断言抓住）。
fn crash_isolation() -> anyhow::Result<()> {
    let hang_cfg = SandboxConfig {
        max_per_tick: 60.0,
        initial_bucket: 60.0,
        ..config()
    };
    let mut pool = PlayerThreads::new();
    pool.spawn_player(1, hang_cfg)?;
    pool.set_modules(1, hang_bot())?;
    pool.spawn_player(2, config())?;
    pool.set_modules(2, counter_bot())?;

    for round in 1..=3 {
        let r1 = pool.tick(1, &world())?;
        assert!(
            matches!(r1.outcome, TickOutcome::Terminated),
            "死循环玩家被硬中断（第 {round} 轮）"
        );
        assert!(
            matches!(r1.plan, TickPlan::Run { .. }),
            "桶充足时硬中断 tick 的 plan 仍是 Run（官方：不跳帧），实际 {:?}",
            r1.plan
        );
        let r2 = pool.tick(2, &world())?;
        assert!(
            matches!(r2.outcome, TickOutcome::Completed { .. }),
            "邻居玩家不受影响"
        );
    }
    pool.shutdown_all();
    Ok(())
}

/// LIFO 收尾：shutdown_all 后注册栈干净。
fn lifo_shutdown_clean() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    for id in 0..4 {
        pool.spawn_player(id, config())?;
    }
    pool.shutdown_all();
    assert_eq!(live_sandbox_count(), 0, "全部 isolate 按逆序销毁");
    Ok(())
}

/// 每次使用宿主快照，即使线程工作桶发生变化也不能改写下一轮预算。
fn host_budget_job() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(1, config())?;
    let committed = kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, 7.0);
    for _ in 0..2 {
        let report = pool.run_job(
            1,
            kk_runner::RunJob {
                terrain: None,
                kkpv_bundle: None,
                kkpv_public: None,
                world: world(),
                code: "[Game.cpu.bucket,Game.cpu.tickLimit,Memory.saved]".into(),
                pre_expr: None,
                epoch: EPOCH,
                attempt: 1,
                deadline: None,
                raw_memory: r#"{"saved":42}"#.into(),
                segments_json: "[]".into(),
                ism_json: "null".into(),
                foreign_json: "null".into(),
                budget: kk_core::cpu::RunBudget::new(&committed),
            },
        )?;
        let report = report.report;
        assert_eq!(report.epoch, EPOCH);
        assert_eq!(report.expr["value"], json!([7, 27, 42]));
        assert!(report.cpu_billed_ms.unwrap() >= 0.0);
        assert_eq!(pool.bucket(1)?, 7.0);
        pool.settle_bill(1)?;
        assert_eq!(pool.bucket(1)?, 7.0, "RunJob 报告后不留待提交桶账");
    }
    assert_eq!(committed.bucket(), 7.0);
    Ok(())
}

/// 先等待第二条命令，再读取第一条迟到回执；结果仍与各自 token 对应。
fn pending_receipts_are_independent() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(1, config())?;
    let job = |attempt| kk_runner::RunJob {
        terrain: None,
        kkpv_bundle: None,
        kkpv_public: None,
        world: world(),
        code: "globalThis.n=(globalThis.n||0)+1;globalThis.n".into(),
        pre_expr: None,
        epoch: EPOCH,
        attempt,
        deadline: None,
        raw_memory: "{}".into(),
        segments_json: "[]".into(),
        ism_json: "null".into(),
        foreign_json: "null".into(),
        budget: kk_core::cpu::RunBudget::new(&kk_core::cpu::Bucket::default()),
    };
    let first = pool.dispatch_job(1, job(1))?;
    let second = pool.dispatch_job(1, job(2))?;
    let first_token = first.token;
    let second_token = second.token;
    let second = second.wait()?;
    assert_eq!(second.token, second_token);
    assert_eq!(second.report.expr["value"], 2);
    let first = first.wait()?;
    assert_eq!(first.token, first_token);
    assert_eq!(first.report.expr["value"], 1);
    assert_ne!(first.token, second.token);
    Ok(())
}

fn queued_command_deadline() -> anyhow::Result<()> {
    let mut pool = PlayerThreads::new();
    pool.spawn_player(1, config())?;
    let gate = pool.gate_next_job_for_test(1)?;
    let job = |attempt, code: &str| kk_runner::RunJob {
        terrain: None,
        kkpv_bundle: None,
        kkpv_public: None,
        world: world(),
        code: code.into(),
        pre_expr: None,
        epoch: EPOCH,
        attempt,
        deadline: None,
        raw_memory: "{}".into(),
        segments_json: "[]".into(),
        ism_json: "null".into(),
        foreign_json: "null".into(),
        budget: kk_core::cpu::RunBudget::new(&kk_core::cpu::Bucket::default()),
    };
    let first = pool.dispatch_job(1, job(1, "globalThis.n=1"))?;
    gate.started.recv_timeout(Duration::from_secs(1))?;
    let mut expired = job(2, "globalThis.n=99");
    expired.deadline = Some(Instant::now() + Duration::from_millis(30));
    let second = pool.dispatch_job(1, expired)?;
    assert!(second.wait().is_err());
    gate.release.send(())?;
    first.wait()?;
    let third = pool.run_job(1, job(3, "globalThis.n"))?;
    assert_eq!(third.report.expr["value"], 1, "队列中过期的命令不得执行");
    Ok(())
}
