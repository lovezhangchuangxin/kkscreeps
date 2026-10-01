//! 验证 terminal.send 的错误码、冷却、运输成本和交易视图，以及市场订单生命周期。
//! 同一 tick 的同名意图会覆盖，因此成功运输只保留最后一笔请求。

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
    // 同房敌方 terminal 可见，但发送请求应返回 NOT_OWNER。
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

    // 逐项检查无冷却时的校验错误；每次只构造一个非法条件。
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

    // 下一 tick 再次发送时读取上一轮提交的冷却时间，应返回 TIRED。
    // 同 tick 的第二次调用尚未结算，不能检验这个冷却限制。
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
    assert_eq!(
        v[13],
        json!(10_000_000),
        "credits=种子 10M（P2 createShard 落地——vanilla adapter 同位）"
    );

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

/// 检查 Game.market 属性描述符、严格模式赋值，以及创建订单、交易和随附订单信息。
pub(super) fn m2b2_market_book_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 6, "owner": "p1" },
                  { "name": "W5N1", "rcl": 6, "owner": "p2" }],
    }))?;
    let _t1 = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p1",
                      "store": {"energy": 100000} }),
    )?;
    let _t2 = e.place_object(
        "W5N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "terminal", "owner": "p2",
                      "store": {"energy": 100000, "H": 1000} }),
    )?;
    let oid = serde_json::to_string(&e.place_market_order(&json!({
        "owner": "p2", "type": "sell", "resourceType": "H",
        "price": 2.5, "totalAmount": 1000, "roomName": "W5N1", "active": true,
    }))?)?;
    e.tick(1)?;

    // 四个 getter 可枚举、无 setter、不可配置，严格模式赋值会抛 TypeError；
    // 方法属性则可枚举、可配置、可写。
    let run = e.run_player(
        "p1",
        r#"[ ...['credits','incomingTransactions','outgoingTransactions','orders'].map(k => {
              const d = Object.getOwnPropertyDescriptor(Game.market, k);
              return d && d.enumerable === true && d.get !== undefined
                && d.set === undefined && d.configurable === false;
            }),
            Object.getOwnPropertyDescriptor(Game.market, 'createOrder').writable === true,
            Object.getOwnPropertyDescriptor(Game.market, 'createOrder').configurable === true,
            Object.getOwnPropertyDescriptor(Game.market, 'calcTransactionCost').enumerable === true,
            (() => { 'use strict';
                     try { Game.market.credits = 5; return false; }
                     catch (err) { return err instanceof TypeError; } })() ]"#,
    )?;
    let v = &run.expr["value"];
    for i in 0..4 {
        assert_eq!(
            v[i],
            json!(true),
            "getter {i} 描述符 enumerable+get-only+不可配置"
        );
    }
    assert_eq!(v[4], json!(true), "方法字面量 writable");
    assert_eq!(
        v[5],
        json!(true),
        "方法字面量 configurable（清偿复审 nit 补）"
    );
    assert_eq!(v[6], json!(true), "方法字面量 enumerable");
    assert_eq!(v[7], json!(true), "严格赋值 getter 抛 TypeError");

    // ---- createOrder→deal 全链（p1 建买单收 H；p2 的卖单在簿）
    let run = e.run_player(
        "p1",
        &format!(
            r#"[ Game.market.credits,
                 Game.market.createOrder('buy', 'H', 3, 100, 'W1N1'),
                 Game.market.deal({oid}, 100, 'W1N1') ]"#
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(10_000_000), "种子 10M credits");
    assert_eq!(v[1], json!(0), "createOrder OK");
    assert_eq!(v[2], json!(0), "deal OK");

    e.tick(1)?;
    let run = e.run_player(
        "p1",
        r#"[ Game.rooms['W1N1'].terminal.store.H,
             Game.market.orders && Object.keys(Game.market.orders).length,
             Game.market.incomingTransactions.length,
             Game.market.incomingTransactions[0].order,
             Game.market.outgoingTransactions.length,
             Game.market.credits ]"#,
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], json!(100), "买入 H 100 送达");
    assert_eq!(v[1], json!(1), "自己的买单 1 张");
    assert_eq!(v[2], json!(1), "入向交易");
    assert_eq!(
        v[3],
        json!({"id": oid.trim_matches('"'), "type": "sell", "price": 2.5}),
        "order 附带 id/type/price÷1000"
    );
    assert_eq!(v[4], json!(0), "无出向");
    // credits：10M − createOrder fee ceil(3000×100×0.05)=15000毫=15 − dealCost
    // 100×2500毫=250000毫=250 → 9999735
    assert_eq!(v[5], json!(9_999_735), "fee+dealCost 双扣");
    Ok(())
}
