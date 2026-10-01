//! 2B 批 P1 e2e：terminal.send 校验序错误码链（013 矩阵形态的内部锁——
//! 评审 3 P1 清偿：此前唯一锁是外部套件矩阵）+ 跨 tick 冷却 TIRED + 成功链
//! （送达/成本/交易视图映射/credits getter——P1 交付面原零覆盖）。
//! 官方对照：structures.js:714-745 / global-intents/market.js:15-95 /
//! game/market.js:196-233。同 tick 多笔 intents.set 同名覆盖（map 语义）
//! ——错误码链逐项独立调用，成功链恰留末笔。

use super::*;

pub(super) fn m2b2_terminal_send_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 6, "owner": "p1" },
                  { "name": "W3N1", "rcl": 5, "owner": "p1" },
                  { "name": "W5N1", "rcl": 6, "owner": "p2" }],
    }))?;
    // p1 主 terminal（校验链基准态：H 1000 / energy 100000 充裕）
    let t = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p1",
                      "store": {"energy": 100000, "H": 1000} }),
    )?)?;
    // 敌方 terminal（同房可见——NOT_OWNER 门）
    let foe = serde_json::to_string(&e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [26, 26], "structureType": "terminal", "owner": "p2",
                      "store": {"energy": 500, "H": 500} }),
    )?)?;
    // RCL 5 房的自家 terminal（terminal 配额 0 → inactive）
    let t_rcl5 = serde_json::to_string(&e.place_object(
        "W3N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p1",
                      "store": {"energy": 100000, "H": 1000} }),
    )?)?;
    // 目标 terminal（成功链）
    let t_dst = serde_json::to_string(&e.place_object(
        "W5N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p2",
                      "store": {"energy": 500} }),
    )?)?;
    e.tick(1)?;

    // ---- 第一段：无冷却前提下的错误码链（官方序：my → RCL → 房名 → 资源
    // → 保有 → 冷却 → 能量 → description；组合序由套件 013 矩阵 36 变体锁，
    // 此处锁单因错误码与门存在性）
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({foe}).send('H', 1, 'W5N1'),
                 Game.getObjectById({t_rcl5}).send('H', 1, 'W5N1'),
                 Game.getObjectById({t}).send('H', 1, 'INVALID'),
                 Game.getObjectById({t}).send('XXX', 1, 'W5N1'),
                 Game.getObjectById({t}).send('H', 9999, 'W5N1'),
                 Game.getObjectById({t}).send('H', 500, 'W5N1', 'x'.repeat(101)),
                 Game.getObjectById({t}).send('H', 500, 'W5N1', 'ok<desc') ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(-1), "敌方 terminal NOT_OWNER");
    assert_eq!(v[1], json!(-14), "RCL 5（配额 0）RCL_NOT_ENOUGH");
    assert_eq!(v[2], json!(-10), "非法房名 INVALID_ARGS");
    assert_eq!(v[3], json!(-10), "非法资源 INVALID_ARGS");
    assert_eq!(v[4], json!(-6), "保有不足 NOT_ENOUGH_RESOURCES");
    assert_eq!(
        v[5],
        json!(-10),
        "description 101 字符 INVALID_ARGS（保有/能量充足到 description 门）"
    );
    assert_eq!(
        v[6],
        json!(0),
        "末笔成功 OK（intents.set 同名覆盖——存活的是这笔）"
    );
    assert!(run.executed);

    // ---- 结算：H 500 送达 + 冷却挂 10
    e.tick(1)?;

    // ---- 第二段：冷却期跨 tick 二发 → TIRED（API 时刻读的是上轮结算落的
    // cooldownTime=10 > time）——同 tick 内二发不经此门（013 的
    // cooldownBeforeNotEnoughEnergyCost 组合由套件锁）
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.getObjectById({t}).send('H', 1, 'W5N1'),
                 Game.getObjectById({t}).store.H,
                 Game.getObjectById({t}).store.energy,
                 Game.getObjectById({t}).cooldown,
                 Game.getObjectById({t_dst}) === null,
                 Game.market.outgoingTransactions.length,
                 Game.market.outgoingTransactions[0].transactionId !== undefined,
                 Game.market.outgoingTransactions[0].sender,
                 Game.market.outgoingTransactions[0].recipient,
                 Game.market.outgoingTransactions[0].resourceType,
                 Game.market.outgoingTransactions[0].amount,
                 Game.market.outgoingTransactions[0].description,
                 Game.market.incomingTransactions.length,
                 Game.market.credits ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(-11), "冷却期 TIRED");
    assert_eq!(v[1], json!(500), "H 1000−500");
    let cost = (500.0_f64 * (1.0 - (-4.0_f64 / 30.0).exp())).ceil() as i64;
    assert_eq!(cost, 63, "公式锚 range=4");
    assert_eq!(v[2], json!(100000 - 63), "能量扣成本");
    // 冷却账：run1 执行于 t=1、其结算段挂 cooldownTime=1+10=11 并推进 t=2；
    // tick(1) → t=3；run2 于 t=3 读 11−3=8（embed 侧 runPlayer=执行+结算+
    // 推进一拍——与 suite 的 tick 语义差一拍，实测对齐）
    assert_eq!(v[3], json!(8), "cooldownTime(11) − 读时(3)");
    assert_eq!(v[4], json!(true), "视野外对象 getObjectById null");
    assert_eq!(v[5], json!(1), "出向交易 1 笔");
    assert_eq!(v[6], json!(true), "transactionId 串化存在");
    assert_eq!(v[7], json!({"username": "p1"}), "sender→{{username}}");
    assert_eq!(v[8], json!({"username": "p2"}), "recipient→{{username}}");
    assert_eq!(v[9], json!("H"), "resourceType");
    assert_eq!(v[10], json!(500), "amount");
    assert_eq!(v[11], json!("ok&lt;desc"), "description `<` 转义");
    assert_eq!(v[12], json!(0), "p1 无入向");
    assert_eq!(v[13], json!(0), "credits 缺省 0（P2 种子前）");

    // p2 视角：incoming 1 笔 + 送达量（fee=from：接收方能量不动）
    let run = e.run_player(
        "p2",
        r#"[ Game.rooms['W5N1'].terminal.store.H,
             Game.rooms['W5N1'].terminal.store.energy,
             Game.market.incomingTransactions.length,
             Game.market.incomingTransactions[0].amount,
             Game.market.outgoingTransactions.length ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(500), "目标收到 H");
    assert_eq!(v[1], json!(500), "接收方能量不动");
    assert_eq!(v[2], json!(1), "入向 1 笔");
    assert_eq!(v[3], json!(500), "amount");
    assert_eq!(v[4], json!(0), "p2 无出向");
    Ok(())
}
