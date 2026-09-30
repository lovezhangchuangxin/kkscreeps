//! embed 层停跑回归锁（双房批 O3）：负桶停跑（SkipPenalty）走 embed
//! 全链——结局盲提交语义的回归锁（B0-6 评 2 P2-1 登记的残余风险：未来
//! 给 commit_players 遍历加结局过滤会静默丢 Terminated 玩家的回充候选）。
//!
//! 驱动面：`set_sandbox_config_for_test`（create_shard 前注入
//! `initial_bucket` 负值）；断言面：`player_bucket_for_test`（桶只活在
//! 沙箱线程内，经 Cmd::Bucket 通道读出——读取的是已落桶权威值，不含
//! pending 候选）。官方停跑数值口径（make.js:77）：负桶 begin_tick
//! 即 Skip、回充**先按原桶严格比较** `bucket < −2×limit ? −2×limit :
//! bucket+limit`（双房批外部复审 P0 勘误：原 `(bucket+limit).max(−2×
//! limit)`「先加再钳」在 [−3×limit,−2×limit) 窗与官方分歧）；默认
//! limit=20（cpu_limit）。

use std::collections::BTreeMap;

use serde_json::json;

use kk_runtime::EmbedEngine;

/// 停跑全链与结局盲提交（场景①②）：p1 正常跑 / p2 深负桶停跑同轮混合
/// ——同轮 commit 同时处置 p1 计费与 p2 回充（结局盲）；桶轨迹
/// −2000 → 提交后 −40（−2×limit 地板）→ −20 → 0 → 第四轮恢复执行。
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
    assert_eq!(e.get_game_time(), 1, "停跑轮世界照常推进");
    // 结局盲提交：p2 的回充候选已落桶（−2000 → −2×limit=−40 地板）
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

/// 失败臂的候选丢弃（场景③）：停跑轮回充候选随统一提交点回退——结算
/// 失败桶不动、显式丢弃不落桶；重试/新轮提交后候选才生效。
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

    // 结算失败：p2 已停跑（候选暂存沙箱）→ 整轮 Err → 桶不动
    e.fail_next_settle_for_test();
    assert!(e.run_players(&codes).is_err(), "结算失败整轮回退");
    assert_eq!(
        e.player_bucket_for_test("p2")?,
        -2000.0,
        "失败轮桶不动（回充候选未提交）"
    );
    assert!(e.has_pending_round());

    // 重试臂（评审 3 P2-2 清偿）：不丢弃直接重试缓存轮——commit_players
    // 须把**首跑暂存的**回充候选并入（若 Err 路径被加「善后清理」误
    // discard 沙箱候选，重试提交 no-op、桶滞 −2000，此断言即红）
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

    // 显式丢弃臂：再注入失败（候选重挂）→ 丢弃 → 桶不动（B0-6
    // discard_bill 同点清两类候选）
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

/// 复审 P0 的 embed 级边界取证：窗口桶（−41 ∈ [−3×limit, −2×limit)）
/// 提交后钳回地板 −40（官方），旧错误式将得 −21——embed 全链数值锁。
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

    // 轮 1：−41 破地板 → 官方钳回 −40（旧式 −21）
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
