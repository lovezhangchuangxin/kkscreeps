//! A1 批（mod-plane-batches A1 判据）：段边界故障注入的 embed 全链——
//! 失败轮世界不动、候选缓存、重试不重跑玩家代码。取 Market/Validate/
//! Publish 三段（settle_market 之后或入口——市场快照桥本体由引擎级测试
//! 锁定，本链验证段级注入→恢复态→重试经 embed 全链）。串行链 impl
//!（顶层唯一 #[test] 在 crate 根）。

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

/// A1 评审清偿（R2/R3 P1）：恢复态与 embed pending 的两门分叉场景——
/// settle_external 失败不缓存 pending、引擎落 NeedsRecovery。验证：
/// 恢复期间世界/市场变异被 no_open_round 拒（place_market_order 的市场域
/// 漏洞随之封死）；出路①重试结算（世界未变 ⇒ 引擎门放行）、出路②
/// discard 显式放弃恢复。此前形态：变异放行 → 代际 bump → 引擎门与
/// abandon 双拒 → 永久楔死。
pub(super) fn stage_recovery_escape_route_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    e.run_player("p1", "'warmup'")?;

    // settle_external 失败：引擎恢复态、embed 无 pending（两门分叉）
    e.fail_next_settle_stage_for_test(TickStage::Settle);
    assert!(e.settle_external(IntentBatch::default()).is_err());
    assert!(matches!(
        e.engine_run_state_for_test(),
        RunState::NeedsRecovery { .. }
    ));
    assert!(!e.has_pending_round(), "settle_external 不缓存候选");

    // 恢复期间变异被拒（三个 no_open_round 调用点逐一——place_market_order
    // 是 P1-b 市场域漏洞的原始向量，单独改回旧门时此臂红）
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
    // 出路①：重试结算（世界未变 ⇒ 引擎门放行）→ 恢复为净态，变异解禁
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

/// A1 评审清偿（R3 P2）：段级钩子的顺延语义——整轮前置注入
/// （fail_next_settle）bail 于 settle_world 之前时不吞段钩子（与房级钩子
/// 的评 2 P2-1 清偿口径一致）：段注入留存至下一次实际结算命中。
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
