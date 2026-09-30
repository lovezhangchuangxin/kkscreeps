//! M2B 扩面 P1 全链 e2e：rangedAttack（API→intent→结算→下一 tick 可见）、
//! saying 载荷（本人/他人 isPublic 折叠 + 跨 tick 重置）、getActiveBodyparts、
//! notifyWhenAttacked（creep 通用段 + 投影判重门）。
//! 官方对照：creeps.js:626-677/589-592/996-1015/106-116、rangedAttack.js、
//! say.js:18-21。注意 run_players 一次调用 = 执行 + 结算 + 提交（推进一
//! tick）——双观察者断言须同 tick 并跑（saying 只存活下一 tick）。

use std::collections::BTreeMap;

use serde_json::Value;

use super::*;

pub(super) fn m2b_ranged_attack_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["ranged_attack", "tough"], "name": "atk" }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [12, 12], "owner": "p2", "body": ["tough", "tough"], "name": "vic" }),
    )?;

    let run = e.run_player(
        "p1",
        r#"
        const vic = Game.rooms['W1N1'].find(FIND_HOSTILE_CREEPS)[0];
        console.log('rc=' + Game.creeps.atk.rangedAttack(vic));
        console.log('gap=' + Game.creeps.atk.getActiveBodyparts(RANGED_ATTACK));
        console.log('gapX=' + Game.creeps.atk.getActiveBodyparts(ATTACK));
    "#,
    )?;
    assert!(run.executed);
    assert!(
        run.console.iter().any(|l| l.contains("rc=0")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("gap=1")));
    assert!(run.console.iter().any(|l| l.contains("gapX=0")));

    // 上一 tick 已结算：受攻者 hits 200−10=190；事件 EVENT_ATTACK(=1) 且
    // attackType=RANGED(=2)
    let vic = e
        .find_in_room("W1N1", 101)
        .into_iter()
        .find(|o| o["name"] == "vic")
        .unwrap()
        .clone();
    assert_eq!(vic["hits"], 190, "1×RANGED_ATTACK×10 延迟施加后");
    let run = e.run_player(
        "p1",
        "Game.rooms['W1N1'].getEventLog().filter((ev) => ev.event === 1).length",
    )?;
    assert_eq!(run.expr["value"], 1, "单 rangedAttack 事件");
    Ok(())
}

pub(super) fn m2b_saying_visibility_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "talker" }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [11, 10], "owner": "p2", "body": ["move"], "name": "watcher" }),
    )?;

    // tick1：公开说（超 10 字符截断）；tick2：双观察者同 tick 读
    e.run_player("p1", r#"Game.creeps.talker.say('hello world!!!', true);"#)?;
    // 双观察者同 tick 读（run_players 一次调用即结算一 tick——saying 只存活
    // 下一 tick，逐玩家串行会跨 tick）；p2 视角 talker 不在 Game.creeps，
    // 经 find(FIND_HOSTILE_CREEPS) 取
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), "Game.creeps.talker.saying".to_string());
    codes.insert(
        "p2".to_string(),
        "Game.rooms['W1N1'].find(FIND_HOSTILE_CREEPS)[0].saying".to_string(),
    );
    let runs = e.run_players(&codes)?;
    assert_eq!(runs["p1"].expr["value"], "hello worl", "本人恒见（截 10）");
    assert_eq!(runs["p2"].expr["value"], "hello worl", "他人公开可见");

    // tick3：私密说；tick4：本人见、他人 undefined；tick5：重置
    e.run_player("p1", r#"Game.creeps.talker.say('secret', false);"#)?;
    let runs = e.run_players(&codes)?;
    assert_eq!(runs["p1"].expr["value"], "secret", "本人恒见");
    assert_eq!(
        runs["p2"].expr["value"],
        Value::Null,
        "他人不可见（undefined→null）"
    );
    let runs = e.run_players(&codes)?;
    assert_eq!(runs["p1"].expr["value"], Value::Null, "下一 tick 载荷重置");
    Ok(())
}

pub(super) fn m2b_notify_when_attacked_creep_e2e_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "c" }),
    )?;
    let run = e.run_player(
        "p1",
        r#"
        console.log('r1=' + Game.creeps.c.notifyWhenAttacked(true));
        console.log('r2=' + Game.creeps.c.notifyWhenAttacked(true));
        console.log('r3=' + Game.creeps.c.notifyWhenAttacked(1));
    "#,
    )?;
    assert!(
        run.console.iter().any(|l| l.contains("r1=0")),
        "{:?}",
        run.console
    );
    assert!(run.console.iter().any(|l| l.contains("r2=0")));
    assert!(
        run.console.iter().any(|l| l.contains("r3=-10")),
        "非布尔 ERR_INVALID_ARGS"
    );
    // 上一 tick 已结算（通用段落库）→ 本 tick 判重门读到真值：同值不记、
    // 翻转记——API 返回码两种都 OK
    let run = e.run_player(
        "p1",
        r#"
        console.log('r4=' + Game.creeps.c.notifyWhenAttacked(true));
        Game.creeps.c.notifyWhenAttacked(false);
    "#,
    )?;
    assert!(run.console.iter().any(|l| l.contains("r4=0")));
    Ok(())
}
