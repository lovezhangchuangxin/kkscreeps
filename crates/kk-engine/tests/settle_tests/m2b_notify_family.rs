//! M2B 扩面 P3：Game.notify 全局 intent 通道（引擎面）。
//! 官方对照：runtime.js:78-86（intents.push 全局通道——list 顶层按动作名
//! 键的数组，非对象 id 路由）、common/lib/system.js:17（清洗 schema
//! `notify: {message: 'userText', groupInterval: 'number'}`）、
//! utils.js storeIntents :411-414（notify 分支）。
//! 落账面 = PlayerState.notifications（官方 users.notifications 集合——
//! 邮件通知，游戏内不可观测）；房循环外每玩家恰一次（tick_with_intents）。

use super::*;
use serde_json::json;

#[test]
fn notify_intent_from_json_parses_and_sanitizes() {
    let list = json!({
        "notify": [
            { "message": "hello", "groupInterval": 5 },
            { "message": 42 },
            { "groupInterval": "7" },
            { "message": null },
            { "message": [1, 2] },
            { "message": [0, false, null, 1] },
            { "message": { "a": 1 } },
        ],
        "room": {},
    });
    let batch = IntentBatch::from_json("p1", &list);
    let u = batch.users.get("p1").expect("玩家条目");
    assert_eq!(
        u.notify,
        vec![
            NotifyIntent {
                message: "hello".into(),
                group_interval: Some(5)
            },
            NotifyIntent {
                message: "42".into(),
                group_interval: None
            },
            // 缺 message 容空串；groupInterval 字符串走 parseInt 前缀规则（官方 number 面）
            NotifyIntent {
                message: String::new(),
                group_interval: Some(7)
            },
            NotifyIntent {
                message: String::new(),
                group_interval: None
            },
            // 数组消息 join(",")——元素普通 ToString（0/false 照常字符串化、
            // null 空槽→空串；清偿复审阻断项锁定）
            NotifyIntent {
                message: "1,2".into(),
                group_interval: None
            },
            NotifyIntent {
                message: "0,false,,1".into(),
                group_interval: None
            },
            NotifyIntent {
                message: "[object Object]".into(),
                group_interval: None
            },
        ],
        "清洗：message userText/groupInterval number；room 空对象不产 intent"
    );
}

#[test]
fn notify_intents_persist_to_player_state_with_cap() {
    let mut world = World::new(1);
    world.register_room("W1N1", terrain_with(&[]));
    let mut engine = kk_engine::settle::Engine::new(world);

    let mut items = Vec::new();
    for i in 0..150 {
        items.push(json!({ "message": format!("n{i}"), "groupInterval": i }));
    }
    let list = json!({ "notify": items });
    let batch = IntentBatch::from_json("p1", &list);
    engine.tick_with_intents(batch).unwrap();

    let state = engine.players.get("p1").expect("玩家状态");
    assert_eq!(state.notifications.len(), 100, "有界保留最近 100 条");
    assert_eq!(state.notifications[0].message, "n50", "最旧被淘汰");
    assert_eq!(state.notifications[99].message, "n149");
    assert_eq!(state.notifications[0].group_interval, Some(50));
    assert_eq!(state.notifications[0].tick, 0, "落账 tick=结算时世界时间");

    // 第二 tick 追加（官方集合只增；边界滑动）
    let list = json!({ "notify": [{ "message": "more" }] });
    engine
        .tick_with_intents(IntentBatch::from_json("p1", &list))
        .unwrap();
    let state = engine.players.get("p1").unwrap();
    assert_eq!(state.notifications.len(), 100);
    assert_eq!(state.notifications[99].message, "more");
    assert_eq!(state.notifications[99].tick, 1, "第二 tick 落账 tick=1");
}
