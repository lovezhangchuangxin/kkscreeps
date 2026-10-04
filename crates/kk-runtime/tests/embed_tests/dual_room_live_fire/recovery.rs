//! 真实双房结算故障、缓存重试及跳帧变体。
use super::*;

/// 后结算房失败时，前一房的结果也不得发布；重试一并提交两房且不重跑玩家代码。
/// 丢弃失败轮的场景另检查 Memory 未提前提交。
pub(crate) fn dual_room_failure_recovery_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 4, "owner": "p1" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 3000, "energyCapacity": 3000 }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "up1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 50 } }),
    )?;
    e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    const P1_CODE: &str = r#"
globalThis.n1 = (globalThis.n1 || 0) + 1;
Memory.k1 = globalThis.n1;
Game.creeps['up1'].upgradeController(Game.rooms['W1N1'].controller);
'p1'
"#;
    const P2_CODE: &str = r#"
globalThis.n2 = (globalThis.n2 || 0) + 1;
Memory.k2 = globalThis.n2;
const c = Game.creeps['har2'];
if (c.store.getFreeCapacity() > 0) c.harvest(c.room.find(FIND_SOURCES)[0]);
'p2'
"#;
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), P1_CODE.to_string());
    codes.insert("p2".to_string(), P2_CODE.to_string());
    // 轮 1：正常推进（n1=n2=1，W1N1 progress+1、W2N1 source−2）
    e.run_players(&codes)?;
    let (p0, s0, t0) = (
        controller_progress(&e, "W1N1"),
        source_energy(&e, "W2N1"),
        e.get_game_time(),
    );

    // 轮 2：房级失败注入（W2N1 名序在后——W1N1 已良好结算仍须回退）
    e.fail_next_settle_room_for_test("W2N1");
    let err = e.run_players(&codes).unwrap_err();
    assert!(err.to_string().contains("W2N1"), "房级故障注入命中: {err}");
    assert!(e.has_pending_round(), "失败轮候选缓存");
    assert_eq!(e.get_game_time(), t0, "时间不推进");
    assert_eq!(
        controller_progress(&e, "W1N1"),
        p0,
        "先结算房 W1N1 的良好结果随整体回退（两阶段跨房提交）"
    );
    assert_eq!(source_energy(&e, "W2N1"), s0, "失败房 W2N1 状态不变");

    // 轮 2 重试：只重结算——执行恰一次（n1/n2 保持 2，不重跑玩家）
    e.run_players(&codes)?;
    assert_eq!(
        controller_progress(&e, "W1N1"),
        p0 + 1,
        "重试后两房一并提交（W1N1）"
    );
    assert_eq!(
        source_energy(&e, "W2N1"),
        s0 - 2,
        "重试后两房一并提交（W2N1）"
    );
    assert_eq!(e.get_game_time(), t0 + 1, "重试恰推进一 tick");
    assert!(!e.has_pending_round(), "候选已消费");
    // 两轮执行后各沙箱计数器都应为 2；重试结算不能使它增加到 3。
    let mut probe = BTreeMap::new();
    probe.insert(
        "p1".to_string(),
        "JSON.stringify([globalThis.n1, Memory.k1])".to_string(),
    );
    probe.insert(
        "p2".to_string(),
        "JSON.stringify([globalThis.n2, Memory.k2])".to_string(),
    );
    let runs = e.run_players(&probe)?;
    for (p, label) in [("p1", "n1"), ("p2", "n2")] {
        let v: Vec<u64> = serde_json::from_str(runs[p].expr["value"].as_str().expect("JSON 串"))?;
        assert_eq!(v, vec![2, 2], "{label} 执行恰两次（轮1+轮2），失败轮未重跑");
    }

    // 再次失败后显式丢弃，再读 Memory.k 应仍为 2，证明失败候选未提前提交。
    e.fail_next_settle_for_test();
    e.run_players(&codes).unwrap_err();
    assert!(e.has_pending_round());
    e.discard_pending_round();
    assert!(!e.has_pending_round());
    let mut probe2 = BTreeMap::new();
    probe2.insert(
        "p1".to_string(),
        "JSON.stringify([globalThis.n1, Memory.k1])".to_string(),
    );
    probe2.insert(
        "p2".to_string(),
        "JSON.stringify([globalThis.n2, Memory.k2])".to_string(),
    );
    let runs = e.run_players(&probe2)?;
    for (p, label) in [("p1", "n1"), ("p2", "n2")] {
        let v: Vec<u64> = serde_json::from_str(runs[p].expr["value"].as_str().expect("JSON 串"))?;
        assert_eq!(
            v,
            vec![3, 2],
            "{label}：globalThis=3（含被丢弃的轮 3 执行，V8 不回滚），Memory 恒 2（失败轮候选未提交即被丢弃）"
        );
    }

    e.teardown();
    Ok(())
}

/// 双房失败轮中，检查错误重试保留缓存、签名复用首次墙钟，以及两房一并提交。
pub(crate) fn dual_b06_variants_impl() -> anyhow::Result<()> {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now_ms = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2", "p3"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N1", "controller": false },
            { "name": "E1N1", "rcl": 1, "owner": "p3" },
        ],
    }))?;
    // p1 的 signer 与控制器相邻，p2 在野房采集，p3 已注册但不参与首次执行。
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 1], "name": "signer", "owner": "p1",
            "body": ["move"], "store": { "energy": 0 } }),
    )?;
    e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    let sign_code = "globalThis.n = (globalThis.n||0)+1; Game.creeps['signer'].signController(Game.rooms['W1N1'].controller, 'dual'); 'sign'";
    let harvest_code = "const c = Game.creeps['har2']; if (c.store.getFreeCapacity() > 0) c.harvest(c.room.find(FIND_SOURCES)[0]); 'harvest'";
    // 预热一轮（挡首跑装配耗时出 datetime 判窗）
    let mut warm = BTreeMap::new();
    for p in ["p1", "p2", "p3"] {
        warm.insert(p.to_string(), "'warmup'".to_string());
    }
    e.run_players(&warm)?;

    // 轮 A：p1 签名 + p2 采集（p3 不在 codes——pending 只含 {p1,p2}）→ 失败
    let first_attempt_ms = now_ms();
    e.fail_next_settle_for_test();
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), sign_code.to_string());
    codes.insert("p2".to_string(), harvest_code.to_string());
    let err = e.run_players(&codes).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    assert!(e.has_pending_round(), "失败轮候选缓存");

    // ①被拒重试：请求含 pending 外玩家（p3）→ bail 且缓存幸存
    let mut with_p3 = BTreeMap::new();
    for (p, c) in [("p1", sign_code), ("p2", harvest_code), ("p3", "'idle'")] {
        with_p3.insert(p.to_string(), c.to_string());
    }
    let rejected = e.run_players(&with_p3).unwrap_err();
    assert!(
        rejected.to_string().contains("p3"),
        "键集守卫拒绝 pending 外玩家: {rejected}"
    );
    assert!(
        e.has_pending_round(),
        "被拒请求不销毁缓存（B0-6 ①双玩家形态）"
    );

    // ②TickInput 复用：重试（{p1,p2} 合法子集）——datetime 应为首次尝试
    // 的记录值（早于重试起点），两房意图一并提交
    std::thread::sleep(Duration::from_millis(150));
    let retry_started_ms = now_ms();
    let s_before = source_energy(&e, "W2N1");
    e.run_players(&codes)?;
    assert_eq!(
        s_before - source_energy(&e, "W2N1"),
        2,
        "重试提交野房采集意图"
    );
    let run = e.run_player("p1", "JSON.stringify([globalThis.n, new Date(Game.rooms['W1N1'].controller.sign.datetime).getTime()])")?;
    let v: Vec<u64> = serde_json::from_str(run.expr["value"].as_str().expect("JSON 串"))?;
    assert_eq!(
        v[0], 1,
        "signer 计数恰 1（轮A 一次——预热轮为纯字面量代码不计数；失败轮未重跑）"
    );
    assert!(
        v[1] < retry_started_ms.saturating_sub(100),
        "datetime 用首次尝试的记录值（dt={} 早于重试起点 {}；重采样则不早于）",
        v[1],
        retry_started_ms
    );
    assert!(
        v[1] >= first_attempt_ms.saturating_sub(1_000),
        "datetime 合理性：不早于首轮起点附近（{first_attempt_ms}）"
    );

    e.teardown();
    Ok(())
}
