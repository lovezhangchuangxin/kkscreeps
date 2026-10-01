//! 在 Market、Validate、Publish 阶段注入失败，验证世界保留、缓存重试和恢复状态。
//! 场景由 `embed_engine_suite` 顺序调用。

use kk_engine::RunState;
use kk_engine::TickStage;
use kk_engine::intent::IntentBatch;

use super::*;

pub(super) fn stage_recovery_family_impl() -> anyhow::Result<()> {
    for stage in [TickStage::Market, TickStage::Validate, TickStage::Publish] {
        let mut e = EmbedEngine::new();
        e.create_shard(&json!({
            "players": ["p1"],
            "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
        }))?;
        e.run_player("p1", "'warmup'")?;
        assert_eq!(e.get_game_time(), 1);

        // 轮 A：计数代码（globalThis 跨 tick 持久——执行次数取证通道）+
        // 段入口注入 → 整轮 Err、候选缓存、世界不动
        let counter = "globalThis.n = (globalThis.n || 0) + 1; globalThis.n";
        e.fail_next_settle_stage_for_test(stage);
        let err = e.run_player("p1", counter).unwrap_err();
        assert!(err.to_string().contains(stage.label()), "{stage:?}: {err}");
        assert!(e.has_pending_round(), "{stage:?}: 失败轮候选已缓存");
        assert_eq!(e.get_game_time(), 1, "{stage:?}: 失败轮世界不动");

        // 轮 B：原请求重试 → 消费缓存（expr 为执行 1 的候选）、代码不再执行
        let run = e.run_player("p1", counter)?;
        assert_eq!(
            run.expr["value"], 1,
            "{stage:?}: 重试返回缓存候选的 expr（代码不重跑——旧形态下重跑得 2）"
        );
        assert!(!e.has_pending_round(), "{stage:?}: 重试成功后缓存清空");
        assert_eq!(e.get_game_time(), 2, "{stage:?}: warmup 1 + 重试轮推进 1");
        // 执行次数终检：恰一次
        let run = e.run_player("p1", "globalThis.n")?;
        assert_eq!(run.expr["value"], 1, "{stage:?}: 玩家代码执行次数恰一次");
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
