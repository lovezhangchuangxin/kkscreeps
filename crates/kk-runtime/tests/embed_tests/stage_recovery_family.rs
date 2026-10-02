//! 在 Market、Validate、Publish 阶段注入失败，验证世界保留、缓存重试和恢复状态。
//! 场景由 `embed_engine_suite` 顺序调用。

use kk_engine::RunState;
use kk_engine::TickStage;
use kk_engine::intent::IntentBatch;

use super::*;

pub(super) fn stage_recovery_family_impl() -> anyhow::Result<()> {
    let stages = [TickStage::Market, TickStage::Validate, TickStage::Publish]
        .map(|stage| (Some(stage), None));
    let steps = [
        kk_engine::WritebackStep::Market,
        kk_engine::WritebackStep::Cpu,
        kk_engine::WritebackStep::RawMemory,
        kk_engine::WritebackStep::Segments,
    ]
    .map(|step| (None, Some(step)));
    for (stage, step) in stages.into_iter().chain(steps) {
        let label = stage
            .map(|s| s.label())
            .unwrap_or_else(|| step.unwrap().label());
        let mut e = EmbedEngine::new();
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
        }))?;
        e.run_player("p1", "'warmup'")?;
        assert_eq!(e.get_game_time(), 1);

        // 轮 A：计数代码（globalThis 跨 tick 持久——执行次数取证通道）+
        // 段入口注入 → 整轮 Err、候选缓存、世界不动
        let before = e.player_state_for_test("p1").unwrap().clone();
        let counter = r#"
            globalThis.n = (globalThis.n || 0) + 1;
            RawMemory.set('{"saved":true}');
            RawMemory.segments[5] = 'candidate';
            RawMemory.setActiveSegments([5]);
            RawMemory.setPublicSegments([5]);
            RawMemory.setDefaultPublicSegment(5);
            globalThis.n
        "#;
        if let Some(stage) = stage {
            e.fail_next_settle_stage_for_test(stage);
        }
        if let Some(step) = step {
            e.fail_next_writeback_for_test(step);
        }
        let err = e.run_player("p1", counter).unwrap_err();
        assert!(err.to_string().contains(label), "{stage:?}: {err}");
        assert!(e.has_pending_round(), "{stage:?}: 失败轮候选已缓存");
        assert_eq!(e.get_game_time(), 1, "{stage:?}: 失败轮世界不动");
        assert_eq!(
            e.player_state_for_test("p1").unwrap(),
            &before,
            "{stage:?}: Memory、段内容及请求不得提前发布"
        );

        // 轮 B：原请求重试 → 消费缓存（expr 为执行 1 的候选）、代码不再执行
        let run = e.run_player("p1", counter)?;
        assert_eq!(
            run.expr["value"], 1,
            "{stage:?}: 重试返回缓存候选的 expr（代码不重跑——旧形态下重跑得 2）"
        );
        assert!(!e.has_pending_round(), "{stage:?}: 重试成功后缓存清空");
        let committed = e.player_state_for_test("p1").unwrap();
        assert_eq!(committed.raw_memory, r#"{"saved":true}"#);
        assert_eq!(committed.segments[&5], "candidate");
        assert_eq!(committed.active_request, vec![5]);
        assert_eq!(committed.public_ids, vec![5]);
        assert_eq!(committed.default_public, Some(5));
        assert_eq!(e.get_game_time(), 2, "{stage:?}: warmup 1 + 重试轮推进 1");
        // 执行次数终检：恰一次
        let run = e.run_player("p1", "[globalThis.n, Memory.saved, RawMemory.segments[5]]")?;
        assert_eq!(
            run.expr["value"],
            json!([1, true, "candidate"]),
            "{stage:?}: 代码只执行一次，下一轮读取已发布内存"
        );
    }
    Ok(())
}

/// 外部结算失败时没有玩家缓存，但引擎仍处于恢复状态。
/// 此时修改世界和市场都应被拒；重试结算或显式丢弃后允许继续使用。
pub(super) fn stage_recovery_escape_route_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.run_player("p1", "'warmup'")?;

    // 外部结算失败进入引擎恢复状态，但没有玩家候选缓存。
    e.fail_next_settle_stage_for_test(TickStage::Settle);
    assert!(e.settle_external(IntentBatch::default()).is_err());
    assert!(matches!(
        e.engine_run_state_for_test(),
        RunState::NeedsRecovery { .. }
    ));
    assert!(!e.has_pending_round(), "settle_external 不缓存候选");

    // 恢复期间分别检查对象摆放、市场订单注入和地形修改均被拒。
    // 市场修改不改变世界 generation，也必须由嵌入式接口阻止。
    let err = e
        .place_object(
            "W1N1",
            "structure",
            &json!({ "pos": [25, 25], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("恢复态"), "placeObject: {err}");
    let err = e
        .place_market_order(&json!({
            "roomName": "W1N1", "type": "sell", "resourceType": "energy",
            "price": 1.0, "totalAmount": 100
        }))
        .unwrap_err();
    assert!(
        err.to_string().contains("恢复态"),
        "placeMarketOrder: {err}"
    );
    let err = e.set_terrain("W1N1", vec![0; 2500]).unwrap_err();
    assert!(err.to_string().contains("恢复态"), "setTerrain: {err}");
    // 重试成功后引擎恢复为已提交状态，允许修改世界。
    e.run_player("p1", "'after-recovery'")?;
    assert!(matches!(
        e.engine_run_state_for_test(),
        RunState::Committed { .. }
    ));
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [26, 25], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
    )?;

    // 出路②：discard 显式放弃恢复（独立分片——pending 为 None 时仅 abandon）
    let mut e2 = EmbedEngine::new();
    e2.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e2.run_player("p1", "'warmup'")?;
    e2.fail_next_settle_stage_for_test(TickStage::Validate);
    assert!(e2.settle_external(IntentBatch::default()).is_err());
    assert!(matches!(
        e2.engine_run_state_for_test(),
        RunState::NeedsRecovery { .. }
    ));
    e2.discard_pending_round();
    assert!(
        matches!(e2.engine_run_state_for_test(), RunState::Ready),
        "丢弃后引擎回净态（abandon 成功）"
    );
    e2.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
    )?;
    e2.run_player("p1", "'ok'")?;
    Ok(())
}

/// 整轮故障钩子先返回错误时，阶段钩子须保留到下一次实际调用引擎。
pub(super) fn stage_hook_deferral_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.run_player("p1", "'warmup'")?;
    // 前置整轮注入 + 段级注入同置：本轮被前置钩子吞掉（错误不含段名）
    e.fail_next_settle_for_test();
    e.fail_next_settle_stage_for_test(TickStage::Settle);
    let err = e.run_player("p1", "'x'").unwrap_err();
    assert!(
        err.to_string().contains("世界结算失败"),
        "前置钩子先命中: {err}"
    );
    assert!(e.has_pending_round());
    // 重试：前置钩子已清（一次性），段钩子顺延命中（一次性）
    let err = e.run_player("p1", "'x'").unwrap_err();
    assert!(
        err.to_string().contains(TickStage::Settle.label()),
        "段钩子顺延至实际结算命中: {err}"
    );
    // 再重试：段钩子已清 → 成功
    e.run_player("p1", "'x'")?;
    assert!(matches!(
        e.engine_run_state_for_test(),
        RunState::Committed { .. }
    ));
    Ok(())
}

/// 中途故障保留旧持久状态，并拒绝重跑任何已经执行的玩家。
pub(super) fn execution_failure_blocks_redispatch_impl() -> anyhow::Result<()> {
    for failed in ["p1", "p2"] {
        let mut e = EmbedEngine::new();
        e.create_shard(&json!({"players":["p1","p2"], "rooms":[]}))?;
        e.set_player_concurrency(1)?;
        let before1 = e.player_state_for_test("p1").unwrap().clone();
        let before2 = e.player_state_for_test("p2").unwrap().clone();
        let code = "globalThis.n=(globalThis.n||0)+1; Memory.n=globalThis.n; globalThis.n";
        let codes = std::collections::BTreeMap::from([
            ("p1".into(), code.into()),
            ("p2".into(), code.into()),
        ]);
        e.fail_after_player_for_test(failed);
        assert!(
            e.run_players(&codes)
                .unwrap_err()
                .to_string()
                .contains("执行轮中止")
        );
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.player_state_for_test("p1").unwrap(), &before1);
        assert_eq!(e.player_state_for_test("p2").unwrap(), &before2);
        assert!(
            e.run_players(&codes)
                .unwrap_err()
                .to_string()
                .contains("先 discard_pending_round")
        );
        assert!(e.tick(1).is_err());
        assert!(e.settle_external(IntentBatch::default()).is_err());
        e.discard_pending_round();
        let read = std::collections::BTreeMap::from([
            ("p1".into(), "[globalThis.n||0,Memory.n||0]".into()),
            ("p2".into(), "[globalThis.n||0,Memory.n||0]".into()),
        ]);
        let runs = e.run_players(&read)?;
        assert_eq!(runs["p1"].expr["value"], json!([1, 0]));
        assert_eq!(
            runs["p2"].expr["value"],
            json!([if failed == "p2" { 1 } else { 0 }, 0])
        );
    }
    Ok(())
}

/// 第二个真实线程退出时不提交首个玩家的候选；显式重建从旧持久状态继续。
pub(super) fn lost_worker_rebuild_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.set_sandbox_config_for_test(
        "p1",
        kk_runner::SandboxConfig {
            initial_bucket: 7.0,
            ..Default::default()
        },
    );
    e.create_shard(&json!({"players":["p1","p2"],"rooms":[]}))?;
    let seed = "Memory.saved=true; RawMemory.segments[5]='seed'; RawMemory.setActiveSegments([5]); globalThis.oldGlobal=123";
    e.run_players(&std::collections::BTreeMap::from([
        ("p1".into(), seed.into()),
        ("p2".into(), seed.into()),
    ]))?;
    let before1 = e.player_state_for_test("p1").unwrap().clone();
    let before2 = e.player_state_for_test("p2").unwrap().clone();
    e.exit_worker_before_next_job_for_test("p2")?;
    let dirty = "globalThis.dirty=(globalThis.dirty||0)+1; Memory.saved=false; RawMemory.segments[5]='uncommitted'";
    let codes = std::collections::BTreeMap::from([
        ("p1".into(), dirty.into()),
        ("p2".into(), dirty.into()),
    ]);
    assert!(
        e.run_players(&codes)
            .unwrap_err()
            .to_string()
            .contains("未返回执行回执")
    );
    assert!(e.has_pending_round());
    assert_eq!(e.get_game_time(), 1);
    assert_eq!(e.player_state_for_test("p1").unwrap(), &before1);
    assert_eq!(e.player_state_for_test("p2").unwrap(), &before2);
    assert!(e.run_players(&codes).is_err());
    e.fail_next_worker_rebuild_for_test()?;
    assert!(
        e.rebuild_player_workers()
            .unwrap_err()
            .to_string()
            .contains("替代工作者")
    );
    assert!(e.has_pending_round(), "重建失败不能解除执行故障门");
    assert!(e.run_players(&codes).is_err());
    assert_eq!(e.get_game_time(), 1);
    assert_eq!(e.player_state_for_test("p1").unwrap(), &before1);
    assert_eq!(e.player_state_for_test("p2").unwrap(), &before2);
    e.rebuild_player_workers()?;
    assert!(!e.has_pending_round());
    assert_eq!(e.get_game_time(), 1);
    assert_eq!(e.player_state_for_test("p1").unwrap(), &before1);
    assert_eq!(e.player_state_for_test("p2").unwrap(), &before2);
    let read = "[globalThis.dirty||0,Memory.saved,RawMemory.segments[5],Game.cpu.bucket,globalThis.oldGlobal||0]";
    let runs = e.run_players(&std::collections::BTreeMap::from([
        ("p1".into(), read.into()),
        ("p2".into(), read.into()),
    ]))?;
    for (player, before) in [("p1", before1), ("p2", before2)] {
        let values = &runs[player].expr["value"];
        assert_eq!(values[0], 0);
        assert_eq!(values[1], true);
        assert_eq!(values[2], "seed");
        assert_eq!(values[3].as_f64(), Some(before.cpu.bucket()));
        assert_eq!(values[4], 0, "工作者重建丢弃全局变量");
    }
    assert_eq!(e.get_game_time(), 2);
    assert!(
        e.rebuild_player_workers().is_err(),
        "健康分片不得隐式清空全局状态"
    );
    Ok(())
}

/// 实际工作者在门闩处同时开始，完成一个后才补第三个；世界结果与串行参考相同。
pub(super) fn bounded_parallel_players_impl() -> anyhow::Result<()> {
    fn make(limit: usize) -> anyhow::Result<EmbedEngine> {
        let mut e = EmbedEngine::new();
        for player in ["p1", "p2", "p3"] {
            e.set_sandbox_config_for_test(
                player,
                kk_runner::SandboxConfig {
                    cpu_limit: 1000.0,
                    ..Default::default()
                },
            );
        }
        e.create_shard(&json!({"players":["p1","p2","p3"],"rooms":[
            {"name":"W1N1","owner":"p1","rcl":3},
            {"name":"W2N1","owner":"p2","rcl":3},
            {"name":"W3N1","owner":"p3","rcl":3}
        ]}))?;
        for (room, owner) in [("W1N1", "p1"), ("W2N1", "p2"), ("W3N1", "p3")] {
            e.place_object(
                room,
                "structure",
                &json!({"pos":[2,2],"structureType":"source",
                "energy":3000,"energyCapacity":3000}),
            )?;
            e.place_object(
                room,
                "creep",
                &json!({"pos":[2,3],"owner":owner,"name":owner,
                "body":["work","carry","move"],"store":{"energy":0}}),
            )?;
        }
        e.set_player_concurrency(limit)?;
        Ok(e)
    }
    let mut parallel = make(2)?;
    let mut serial = make(1)?;
    let first = parallel.gate_next_job_for_test("p1")?;
    let second = parallel.gate_next_job_for_test("p2")?;
    let third = parallel.gate_next_job_for_test("p3")?;
    let observer = std::thread::spawn(move || -> anyhow::Result<()> {
        let timeout = std::time::Duration::from_secs(1);
        first.started.recv_timeout(timeout)?;
        second.started.recv_timeout(timeout)?;
        assert!(
            matches!(
                third.started.try_recv(),
                Err(std::sync::mpsc::TryRecvError::Empty)
            ),
            "第三个任务不能超出上限开始"
        );
        first.release.send(())?;
        third.started.recv_timeout(timeout)?;
        second.release.send(())?;
        third.release.send(())?;
        Ok(())
    });
    let code = "Memory.n=(Memory.n||0)+1; RawMemory.segments[5]='same'; RawMemory.setActiveSegments([5]); Game.notify('same'); for (const c of Object.values(Game.creeps)) { c.harvest(c.room.find(FIND_SOURCES)[0]); } Memory.n";
    let codes = std::collections::BTreeMap::from([
        ("p1".into(), code.into()),
        ("p2".into(), code.into()),
        ("p3".into(), code.into()),
    ]);
    let a = parallel.run_players(&codes)?;
    observer.join().unwrap()?;
    let b = serial.run_players(&codes)?;
    for player in ["p1", "p2", "p3"] {
        assert_eq!(a[player].expr, b[player].expr);
        assert_eq!(
            parallel.player_state_for_test(player),
            serial.player_state_for_test(player)
        );
    }
    for room in ["W1N1", "W2N1", "W3N1"] {
        assert_eq!(
            parallel.find_in_room(room, kk_core::constants::FIND_STRUCTURES),
            serial.find_in_room(room, kk_core::constants::FIND_STRUCTURES)
        );
    }
    for room in ["W1N1", "W2N1", "W3N1"] {
        let a = parallel.find_in_room(room, kk_core::constants::FIND_CREEPS);
        assert_eq!(
            a,
            serial.find_in_room(room, kk_core::constants::FIND_CREEPS)
        );
        assert_eq!(a[0]["store"]["energy"], 2, "真实采集指令须在本轮生效");
        assert_eq!(
            parallel.find_in_room(room, kk_core::constants::FIND_SOURCES),
            serial.find_in_room(room, kk_core::constants::FIND_SOURCES)
        );
    }
    assert_eq!(parallel.get_game_time(), serial.get_game_time());
    Ok(())
}
