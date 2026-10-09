//! 验证 CPU 桶为负时的跳帧惩罚和待提交回充。
//!
//! 跳帧玩家虽然不运行代码，仍须在世界成功结算后提交回充；失败时保留候选供重试。
//! 测试在创建分片前设置初始桶，再读取宿主的已提交桶值。默认 CPU limit 为 20。
//! 回充先判断原桶是否小于 -2 * limit：低于时设为该下限，否则加 limit。

use std::collections::BTreeMap;

use serde_json::json;

use kk_runtime::EmbedEngine;

/// 正常玩家与跳帧玩家同轮处理，验证跳帧玩家仍提交回充。
/// 其桶从 -2000 到 -40、-20、0，第四轮恢复执行。
pub(super) fn stop_run_commit_blind_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    // 须在 create_shard 之前注入（spawn 消费）
    let cfg = kk_runner::SandboxConfig {
        initial_bucket: -2000.0,
        ..Default::default()
    };
    e.set_sandbox_config_for_test("p2", cfg);
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
        ],
    }))?;

    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), "'p1'".to_string());
    codes.insert("p2".to_string(), "'p2'".to_string());

    // 轮 1：p2 停跑（SkipPenalty → Terminated），p1 正常——同轮成功提交
    let runs = e.run_players(&codes)?;
    assert_eq!(
        runs["p1"].expr["value"], "p1",
        "p1 正常执行（混合轮不受停跑玩家影响）"
    );
    assert!(!runs["p2"].executed, "p2 停跑（代码未执行）");
    assert_eq!(
        runs["p2"].expr["kind"], "terminated",
        "p2 结局分类 Terminated"
    );
    assert_eq!(e.get_game_time(), 2, "停跑轮世界照常推进");
    // 跳帧玩家也要提交回充，桶从 -2000 变为 -2 * limit，即 -40。
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -40.0,
        "回充候选经 commit_players 并入（结局盲）"
    );

    // 轮 2/3：−40 → −20 → 0（每轮回充 +limit）
    e.run_players(&codes)?;
    assert_eq!(e.player_bucket_for_test("p2")?, -20.0, "第二轮回充 −20");
    e.run_players(&codes)?;
    assert_eq!(e.player_bucket_for_test("p2")?, 0.0, "第三轮回充至 0");

    // 轮 4：桶非负 → 恢复执行
    let runs = e.run_players(&codes)?;
    assert!(runs["p2"].executed, "第四轮恢复执行（桶回正）");
    assert_eq!(runs["p2"].expr["value"], "p2", "p2 代码真实运行");

    e.teardown();
    Ok(())
}

/// 失败轮不提交回充；重试须保留并提交首次候选，显式丢弃则不改变桶值。
pub(super) fn stop_run_failure_and_discard_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    let cfg = kk_runner::SandboxConfig {
        initial_bucket: -2000.0,
        ..Default::default()
    };
    e.set_sandbox_config_for_test("p2", cfg);
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
        ],
    }))?;
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), "'a'".to_string());
    codes.insert("p2".to_string(), "'b'".to_string());

    // 结算失败：p2 已停跑（候选尚未发布）→ 整轮 Err → 桶不动
    e.fail_next_settle_for_test();
    assert!(e.run_players(&codes).is_err(), "结算失败整轮回退");
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -2000.0,
        "失败轮桶不动（回充候选未提交）"
    );
    assert!(e.has_pending_round());

    // 重试须提交首次保留的回充候选；若失败后误丢弃，桶会停在 -2000。
    let runs = e.run_players(&codes)?;
    assert!(
        !runs["p2"].executed,
        "重试轮消费缓存（不重执行，p2 仍停跑形态）"
    );
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -40.0,
        "重试提交并入首跑暂存的回充候选（−2×limit 地板）"
    );

    // 再次失败后丢弃候选，不应改变已经提交的桶值。
    e.fail_next_settle_for_test();
    assert!(e.run_players(&codes).is_err());
    e.discard_pending_round();
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -40.0,
        "丢弃后桶不动（本轮候选已清，重试并入的 −40 保持）"
    );

    // 新一轮：−40 仍停跑 → 成功提交 → 回充至 −20
    let runs = e.run_players(&codes)?;
    assert!(!runs["p2"].executed, "负桶新轮仍停跑");
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -20.0,
        "成功轮候选并入（−40 → −20）"
    );

    e.teardown();
    Ok(())
}

/// 桶为 -41、limit 为 20 时，先比较原桶应得到 -40；
/// 若先加 limit 再取下限，会错误地得到 -21。
pub(super) fn stop_run_floor_window_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    let cfg = kk_runner::SandboxConfig {
        initial_bucket: -41.0,
        ..Default::default()
    };
    e.set_sandbox_config_for_test("p1", cfg);
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), "'x'".to_string());

    // -41 低于 -2 * limit，直接设为 -40；不能先加 limit 再取下限。
    let runs = e.run_players(&codes)?;
    assert!(!runs["p1"].executed, "负桶停跑");
    assert_eq!(
        e.player_bucket_for_test("p1")?,
        -40.0,
        "窗口桶钳回 −2×limit 地板（旧式为 −21）"
    );
    // 轮 2/3：−40 → −20 → 0 → 恢复执行（tickLimit=20）
    e.run_players(&codes)?;
    assert_eq!(e.player_bucket_for_test("p1")?, -20.0);
    e.run_players(&codes)?;
    assert_eq!(e.player_bucket_for_test("p1")?, 0.0);
    let runs = e.run_players(&codes)?;
    assert!(runs["p1"].executed, "回正恢复执行");

    e.teardown();
    Ok(())
}
