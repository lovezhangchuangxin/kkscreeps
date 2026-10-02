//! 测试通知指令清洗，以及 PlayerState 内存通知列表的跨 tick 保留和容量限制。

use super::*;
use serde_json::json;

/// 真实规则同时更新 GCL、创建资源对象和准备通知；失败重试须与直接成功整态一致。
#[test]
fn shared_candidates_retry_without_leaking_players_ids_or_notifications() {
    for fail_publish in [false, true] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let ctl = f.controller(4, Some("p1"));
        let worker = f.creep("up", "p1", 5, 6, &[BodyKind::Work, BodyKind::Carry], 50);
        let dropper = f.creep("drop", "p1", 8, 8, &[BodyKind::Carry], 10);
        f.world.register_room("W2N2", terrain_with(&[]));
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            "p1",
            worker,
            Action::UpgradeController,
            IntentData::target(ctl),
        );
        let mut drop = IntentData::bare();
        drop.amount = Some(5);
        drop.resource_type = Some("energy".into());
        batch.add_object_intent("p1", dropper, Action::Drop, drop);
        batch
            .users
            .entry("p1".into())
            .or_default()
            .notify
            .push(NotifyIntent {
                message: "once".into(),
                group_interval: Some(3),
            });
        let mut engine = kk_engine::Engine::new(f.world.clone());
        engine
            .players
            .entry("p1".into())
            .or_default()
            .rooms
            .insert("old".into());
        let before_players = engine.players.clone();
        let generation = engine.world.generation();
        let next = engine.world.next_obj_id();
        let before_room = engine
            .world
            .room("W1N1")
            .unwrap()
            .encode_snapshot(0, 1, next);
        let mut reference = kk_engine::Engine::new(f.world);
        reference.players = before_players.clone();
        let input = kk_engine::TickInput { now_ms: 42 };
        reference
            .tick_with_intents_with_input(batch.clone(), input)
            .unwrap();
        if fail_publish {
            engine.set_fail_stage_for_test(Some(kk_engine::TickStage::Publish));
        } else {
            engine.set_fail_room_for_test(Some("W2N2".into()));
        }
        assert!(
            engine
                .tick_with_intents_with_input(batch.clone(), input)
                .is_err()
        );
        assert_eq!(engine.players, before_players);
        assert_eq!(engine.world.generation(), generation);
        assert_eq!(engine.world.next_obj_id(), next);
        assert_eq!(
            engine
                .world
                .room("W1N1")
                .unwrap()
                .encode_snapshot(0, 1, next),
            before_room
        );
        engine.set_fail_stage_for_test(None);
        engine.set_fail_room_for_test(None);
        engine.tick_with_intents_with_input(batch, input).unwrap();
        assert_eq!(engine.players, reference.players);
        assert_eq!(engine.players["p1"].gcl, 1);
        assert_eq!(engine.players["p1"].notifications.len(), 1);
        assert_eq!(engine.players["p1"].rooms, BTreeSet::from(["W1N1".into()]));
        assert_eq!(engine.world.next_obj_id(), reference.world.next_obj_id());
        assert_eq!(engine.world.generation(), generation + 1);
        for (name, room) in engine.world.rooms() {
            assert_eq!(
                room.encode_snapshot(1, 1, engine.world.next_obj_id()),
                reference.world.room(name).unwrap().encode_snapshot(
                    1,
                    1,
                    reference.world.next_obj_id()
                )
            );
        }
        assert_eq!(engine.market, reference.market);
        assert_eq!(engine.last_events, reference.last_events);
    }
}

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
            // 数组消息以逗号连接，0/false 转为字符串，null 对应空串。
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
