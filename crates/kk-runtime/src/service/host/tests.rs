//! 宿主行为测试：调度纯函数（虚拟时钟）、玩家错误继续、结算失败暂停
//! 且重试不重跑玩家、周期保存判定、持久化故障与停止流程。
use super::*;
use crate::service::identity::{self, ServiceIdentity};
use serde_json::json;
use std::path::PathBuf;

fn temp_dir(tag: &str) -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("kk-service-host-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn sample_identity() -> ServiceIdentity {
    ServiceIdentity {
        service_format_version: identity::SERVICE_FORMAT_VERSION,
        world_id: identity::generate_world_id(),
        world_seed: 3,
        scenario_sha256: "d".repeat(64),
        initialized: true,
    }
}

/// 单玩家引擎；模块计一次执行到 Memory.n，或按参数改为每拍抛错。
fn engine_with(ident: &ServiceIdentity, throwing: bool) -> anyhow::Result<EmbedEngine> {
    let mut engine = EmbedEngine::with_world_seed(ident.world_seed);
    engine.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}],
    }))?;
    let code = if throwing {
        "exports.loop = () => { throw new Error('boom'); };"
    } else {
        "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };"
    };
    let mut table = kk_runner::ModuleTable::new();
    table.insert_js("main", code);
    engine.set_player_modules("p1", table)?;
    Ok(engine)
}

fn host_with(
    tag: &str,
    throwing: bool,
    params: HostParameters,
) -> anyhow::Result<(ServiceHost, PathBuf, ServiceIdentity)> {
    let root = temp_dir(tag)?;
    let ident = sample_identity();
    let dir = ServiceDir::new(&root);
    dir.ensure_layout()?;
    let engine = engine_with(&ident, throwing)?;
    let entries = BTreeMap::from([("p1".to_string(), "main".to_string())]);
    let host = ServiceHost::new(
        engine,
        dir,
        ident.clone(),
        entries,
        params,
        RecoverySummary {
            selected: "cp-1.json".into(),
            publish_seq: 1,
            tick: 0,
            skipped_broken: 0,
        },
    )?;
    Ok((host, root, ident))
}

fn memory_n(host: &ServiceHost) -> i64 {
    host.engine
        .player_state("p1")
        .unwrap()
        .raw_memory
        .parse::<serde_json::Value>()
        .unwrap()["n"]
        .as_i64()
        .unwrap_or(0)
}

/// 虚拟时钟调度策略：网格锁定（拍执行耗时零漂移）、超时降速（不补拍
/// 不追积压）、重启无历史截止点。时间线以“已消费截止点”为锚，模拟
/// 驱动循环逐拍推进的实际输入序列。
#[test]
fn scheduler_locks_grid_and_never_catches_up() {
    let base = Instant::now();
    let interval = Duration::from_secs(1);
    // 首拍：无历史截止点 → now + interval。
    let d1 = next_deadline(None, base, interval);
    assert_eq!(d1, base + interval);
    // 拍 1 在 d1 触发、耗时 20ms 完成：下一截止点=网格点 d1+1s（执行
    // 耗时不累积进周期——连续快速拍周期恒为 interval）。
    let d2 = next_deadline(Some(d1), d1 + Duration::from_millis(20), interval);
    assert_eq!(d2, d1 + interval);
    let d3 = next_deadline(Some(d2), d2 + Duration::from_millis(5), interval);
    assert_eq!(d3, d2 + interval, "快速拍序列严格落在网格上");
    // 拍 3 超时（用时 1.6s > interval）：从完成时刻延后一个间隔，不回
    // 补错过的网格点、不突发追赶（区别于 Tokio interval 默认 Burst）。
    let late = d3 + Duration::from_millis(1600);
    let d4 = next_deadline(Some(d3), late, interval);
    assert_eq!(d4, late + interval);
    assert!(d4 > late, "新截止点必须晚于当前，不能立即补拍");
    // 超时后恢复快速拍：重新锚定网格。
    let d5 = next_deadline(Some(d4), d4 + Duration::from_millis(10), interval);
    assert_eq!(d5, d4 + interval);
    // 重启（无历史截止点）：停机期间的墙钟积压不产生待补拍数。
    assert_eq!(next_deadline(None, late, interval), late + interval);
}

/// 驱动循环×纯函数的组合级时间线：模拟 run.rs 的 anchor/pending 接线
/// （pending=等待中截止点，命令打断保持；anchor=已消费截止点，网格锚），
/// 验证系统级周期=间隔本身。
#[test]
fn driver_wiring_yields_interval_period_for_fast_ticks() {
    let interval = Duration::from_millis(100);
    let tick_cost = Duration::from_millis(15);
    let base = Instant::now();
    let mut anchor: Option<Instant> = None;
    let mut now = base;
    let mut deadlines = Vec::new();
    for _ in 0..5 {
        // 接线约定：pending 为空才按锚重算；Deadline 消费后记锚。
        let deadline = next_deadline(anchor, now, interval);
        deadlines.push(deadline);
        anchor = Some(deadline); // 消费
        now = deadline + tick_cost;
    }
    for pair in deadlines.windows(2) {
        assert_eq!(
            pair[1] - pair[0],
            interval,
            "快速拍的截止点间距必须恒等于间隔（无执行耗时漂移）"
        );
    }
    // 超时拍后：下一截止点=完成时刻+间隔（周期放宽到间隔+耗时，但不
    // 追补）。
    let slow_cost = Duration::from_millis(150);
    let deadline = next_deadline(anchor, now, interval);
    anchor = Some(deadline);
    let after_slow = deadline + slow_cost;
    let next = next_deadline(anchor, after_slow, interval);
    assert_eq!(next, after_slow + interval);
}

/// 命令打断不滑动截止点：等待窗内到达的控制命令处理后继续等**同一**
/// pending 截止点——包括首拍（pending 已锚定，不因无消费历史而从命令
/// 处理完时刻重新起算；否则命令频率高于间隔时连首拍都能被无限推迟）。
/// 已消费后的窗口同样锁定 anchor+interval 网格点。
#[test]
fn command_interruptions_do_not_slide_deadlines() {
    let interval = Duration::from_millis(40);
    let base = Instant::now();
    // 与 run.rs 同构的驱动调度状态：打断期间 pending 保持锚定，循环
    // 回去后截止点直接取 pending（不经 next_deadline 重算）。
    let mut anchor: Option<Instant> = None;
    let mut pending: Option<Instant> = None;
    let deadline = match pending {
        Some(deadline) => deadline,
        None => {
            let deadline = next_deadline(anchor, base, interval);
            pending = Some(deadline);
            deadline
        }
    };
    let first = deadline;
    assert_eq!(first, base + interval);
    // 三条命令先后到达（各耗 1ms 处理）：打断后重等同一 pending——
    // 若接线改为打断即清 pending，重算会从命令处理完时刻再起算一个
    // 整间隔（首拍被命令流推迟），本断言即失败。
    for offset_ms in [5, 15, 30] {
        let deadline = match pending {
            Some(deadline) => deadline,
            None => unreachable!("打断期间 pending 必须保持锚定"),
        };
        assert_eq!(
            deadline, first,
            "命令打断后截止点不得滑动（offset={offset_ms}ms）"
        );
    }
    // 拍 1 在 first 消费执行；拍 2 网格锁定 first+interval，其等待窗内
    // 的命令打断同样不滑动。
    anchor = Some(first);
    let second = next_deadline(anchor, first + Duration::from_millis(2), interval);
    assert_eq!(second, first + interval);
    pending = Some(second);
    let deadline = match pending {
        Some(deadline) => deadline,
        None => unreachable!("打断期间 pending 必须保持锚定"),
    };
    assert_eq!(deadline, second);
    // 命令处理慢到越过网格点（second+interval 之后再 +1ms）：锚上网格
    // 点已过，按超时语义从当前时刻延后一个间隔。
    anchor = Some(second);
    let late = second + interval + Duration::from_millis(1);
    assert_eq!(next_deadline(anchor, late, interval), late + interval);
}

#[test]
fn high_frequency_commands_do_not_starve_ticks() {
    let interval = Duration::from_millis(100);
    let tick_cost = Duration::from_millis(5);
    let cmd_every = Duration::from_millis(20);
    let cmd_cost = Duration::from_millis(1);
    let base = Instant::now();
    let mut now = base;
    let mut anchor: Option<Instant> = None;
    let mut pending: Option<Instant> = None;
    let mut ticks = 0u32;
    let mut next_cmd = base + cmd_every;
    let end = base + Duration::from_millis(200);
    while now < end {
        let deadline = match pending {
            Some(deadline) => deadline,
            None => {
                let deadline = next_deadline(anchor, now, interval);
                pending = Some(deadline);
                deadline
            }
        };
        if next_cmd < deadline {
            // WaitOutcome::Command：处理命令，pending 不动。
            now = next_cmd + cmd_cost;
            next_cmd += cmd_every;
        } else {
            // WaitOutcome::Deadline：消费记锚、清 pending 并执行拍。
            anchor = Some(deadline);
            pending = None;
            now = deadline + tick_cost;
            ticks += 1;
        }
    }
    assert!(
        ticks >= 2,
        "高频命令流下 200ms 应至少执行 2 拍（网格 100/200ms），实际 {ticks}"
    );
}

#[test]
fn player_errors_continue_running() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "player-error",
        true,
        HostParameters {
            auto_save_ticks: 1000,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 3,
        },
    )?;
    let start = host.engine().get_game_time();
    for _ in 0..3 {
        let report = host.tick()?.expect("玩家错误不构成整轮故障");
        assert_eq!(report.player_errors.len(), 1, "每拍恰好一个玩家错误");
    }
    assert_eq!(host.ticks_total, 3);
    assert_eq!(host.engine.committed_rounds(), 3, "世界照常推进");
    let status = host.status();
    assert_eq!(status["recent_player_errors"].as_array().unwrap().len(), 3);
    assert_eq!(status["live_tick"], start + 3);
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

/// 结算失败暂停保持 tick；显式重试只消费缓存候选，玩家代码不重跑
/// （Memory.n 只加一），成功后恢复推进。
#[test]
fn settle_failure_pauses_and_retry_does_not_rerun_players() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "settle-retry",
        false,
        HostParameters {
            auto_save_ticks: 1000,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 3,
        },
    )?;
    let start = host.engine().get_game_time();
    host.tick()?.expect("首拍成功");
    assert_eq!(memory_n(&host), 1);
    // 结算失败：玩家已执行（候选缓存），世界未提交。
    host.engine().fail_next_settle_for_test();
    let paused = host.tick()?;
    assert!(paused.is_none(), "结算失败应转入暂停而不是推进");
    assert!(matches!(
        host.fault(),
        Some(Fault::Paused {
            kind: super::PauseKind::Settle,
            ..
        })
    ));
    assert!(!host.can_tick(), "暂停期间不得开始新拍");
    // 重试：复用缓存（不重跑玩家）——n 仍为 2 的候选只结算一次。
    let retried = host.retry()?.expect("重试成功提交缓存轮");
    assert!(retried.player_errors.is_empty());
    assert_eq!(memory_n(&host), 2, "失败拍只执行一次玩家代码");
    assert_eq!(
        host.engine.get_game_time(),
        start + 2,
        "失败拍经重试恰好提交一次"
    );
    assert!(host.can_tick(), "重试成功后恢复推进");
    // 后续拍正常。
    host.tick()?.expect("恢复后新拍成功");
    assert_eq!(memory_n(&host), 3);
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn periodic_save_policy_and_stop_flow() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "save-stop",
        false,
        HostParameters {
            auto_save_ticks: 3,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 2,
        },
    )?;
    let start = host.engine().get_game_time();
    // 墙钟条件：无脏拍不保存（无状态变化不重复写相同版本）。
    assert!(!host.should_save(9999.0));
    host.tick()?;
    // 脏拍 1 + 墙钟超阈值 → 到点；tick 阈值未到 + 墙钟未到 → 不到点。
    assert!(host.should_save(3601.0));
    assert!(!host.should_save(0.0));
    // tick 条件到点（脏拍 3）：即使墙钟基准为 0 也保存。
    host.tick()?;
    host.tick()?;
    assert!(host.should_save(0.0));
    let SaveOutcome::Saved { seq, tick } = host.save_now() else {
        anyhow::bail!("保存应成功");
    };
    assert_eq!((seq, tick), (1, start + 3));
    assert_eq!(host.status()["dirty_ticks"], 0);
    assert_eq!(host.status()["publish_seq"], 1);
    assert!(!host.should_save(0.0), "保存后脏拍归零");
    // 墙钟条件：脏拍 1 + 距基准超阈值。
    host.tick()?;
    assert!(host.should_save(3601.0));
    // 停止：关闭准入 → 再拍被拒 → 收尾保存（脏拍 1）→ 完成。
    host.request_stop();
    assert!(host.tick().is_err(), "Draining 拒绝开始新拍");
    match host.finish_stop() {
        StopOutcome::Completed {
            saved: Some((seq, tick)),
        } => {
            assert_eq!((seq, tick), (2, start + 4), "收尾保存覆盖未保存脏拍");
        }
        other => anyhow::bail!("应完成收尾保存: {other:?}"),
    }
    assert_eq!(host.phase(), Phase::Stopped);
    // 再停止：无脏拍，无事可做。
    match host.finish_stop() {
        StopOutcome::Completed { saved: None } => {}
        other => anyhow::bail!("二次停止应无保存: {other:?}"),
    }
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

/// 发布失败 → PersistenceFault：冻结推进、retry 拒绝、消息携带目标
/// 文件实证；权限恢复后停止收尾的最后一次保存机会成功。
#[test]
#[cfg(unix)]
fn publish_failure_freezes_as_persistence_fault() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let (mut host, root, _ident) = host_with(
        "persist-fault",
        false,
        HostParameters {
            auto_save_ticks: 1000,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 3,
        },
    )?;
    host.tick()?;
    // 只读检查点目录：临时文件创建失败（rename 之前，旧版本不受影响）。
    let checkpoints = root.join("checkpoints");
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o500))?;
    let SaveOutcome::Failed { reason } = host.save_now() else {
        anyhow::bail!("保存应失败");
    };
    assert!(
        reason.contains("目标文件实证"),
        "消息应携带目标文件实证状态: {reason}"
    );
    assert!(matches!(host.fault(), Some(Fault::Persistence { .. })));
    assert!(!host.can_tick(), "持久化故障冻结推进");
    assert!(host.retry().is_err(), "retry 不可用于持久化故障");
    // 权限恢复后，停止收尾是最后一次保存机会：成功即 Completed；
    // 若仍失败则如实 SaveFailed（不把未耐久当已保存）。
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o755))?;
    match host.finish_stop() {
        StopOutcome::Completed {
            saved: Some((seq, _)),
        } => assert_eq!(seq, 1),
        other => anyhow::bail!("权限恢复后收尾保存应成功: {other:?}"),
    }
    assert_eq!(host.status()["dirty_ticks"], 0);
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

/// 执行故障（玩家线程/协议）：暂停且不自动重派；显式重试先重建工作者
/// 再完成该拍。
#[test]
fn execution_fault_pauses_and_retry_rebuilds_workers() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "exec-fault",
        false,
        HostParameters {
            auto_save_ticks: 1000,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 3,
        },
    )?;
    host.tick()?.expect("首拍成功");
    // 真实线程退出：回执通道断开 → 执行故障（非结算失败）。
    host.engine().exit_worker_before_next_job_for_test("p1")?;
    let paused = host.tick()?;
    assert!(paused.is_none(), "执行故障应转入暂停");
    assert!(matches!(
        host.fault(),
        Some(Fault::Paused {
            kind: super::PauseKind::Execution,
            ..
        })
    ));
    assert!(!host.can_tick());
    // 显式重试：重建工作者后完成拍（不自动重派之外的操作者动作）。
    let retried = host.retry()?.expect("重建后重试应提交");
    assert!(retried.player_errors.is_empty());
    assert!(host.can_tick());
    host.tick()?.expect("重建后继续运行");
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

/// 存在未处置失败轮时停止：不擅自保存/丢弃，报告最后耐久 tick。
#[test]
fn stop_with_failed_round_reports_last_durable_tick() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "stop-failed-round",
        false,
        HostParameters {
            auto_save_ticks: 1000,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 3,
        },
    )?;
    host.tick()?;
    let (seq1, durable_tick) = match host.save_now() {
        SaveOutcome::Saved { seq, tick } => (seq, tick),
        SaveOutcome::Failed { reason } => anyhow::bail!("{reason}"),
    };
    assert_eq!(seq1, 1);
    host.engine().fail_next_settle_for_test();
    assert!(host.tick()?.is_none(), "转入暂停");
    host.request_stop();
    match host.finish_stop() {
        StopOutcome::BlockedByFailedRound { last_durable_tick } => {
            assert_eq!(
                last_durable_tick, durable_tick,
                "报告最后耐久 tick 而不是当前候选"
            );
        }
        other => anyhow::bail!("应被失败轮阻塞: {other:?}"),
    }
    // 缓存轮仍在（未丢弃），供操作者显式处置。
    assert!(host.engine().has_pending_round());
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

/// 保留策略随保存执行：超过 keep 的旧完整版本出窗删除。
#[test]
fn saves_enforce_keep_and_advance_sequences() -> anyhow::Result<()> {
    let (mut host, root, _ident) = host_with(
        "keep",
        false,
        HostParameters {
            auto_save_ticks: 1,
            auto_save_wall_secs: 3600,
            keep_checkpoints: 2,
        },
    )?;
    for expected_seq in 1..=3 {
        host.tick()?;
        let SaveOutcome::Saved { seq, .. } = host.save_now() else {
            anyhow::bail!("保存应成功");
        };
        assert_eq!(seq, expected_seq);
    }
    let remaining: Vec<u64> = checkpoints::list_seqs(&root.join("checkpoints"))?
        .into_iter()
        .collect();
    assert_eq!(remaining, vec![2, 3], "保留最近 2 个完整版本");
    assert_eq!(host.status()["next_publish_seq"], 4);
    std::fs::remove_dir_all(&root)?;
    Ok(())
}
