//! 一实例一线程承载原型的集成测试（M0）。
//!
//! 与 sandbox_tests 同理：全部场景放在一个测试函数里串行执行，
//! 保证 V8 isolate 的 LIFO 销毁纪律不被并行测试打乱。

use std::time::{Duration, Instant};

use serde_json::json;

use kk_runner::{ModuleTable, PlayerThreads, SandboxConfig, TickOutcome, live_sandbox_count};

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
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "rooms": {},
        "objects": []
    })
}

#[test]
fn m0_thread_model_suite() -> anyhow::Result<()> {
    multi_player_ticks()?;
    idle_eviction_and_rebuild()?;
    crash_isolation()?;
    lifo_shutdown_clean()?;
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

    // 重建（无快照路径，M0 预算：应明显快于 1s）
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

/// 崩溃隔离：一个玩家死循环被硬中断，另一个玩家不受影响。
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

    for _ in 0..3 {
        let r1 = pool.tick(1, &world())?;
        assert!(
            matches!(r1.outcome, TickOutcome::Terminated),
            "死循环玩家被硬中断"
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
