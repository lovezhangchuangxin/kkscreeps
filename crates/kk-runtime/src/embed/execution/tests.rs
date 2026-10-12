use super::*;
use kk_engine::intent::{Action, IntentData, MarketIntent, NotifyIntent, RoomIntent};
use kk_world::ObjId;

fn market_order() -> MarketIntent {
    MarketIntent::CreateOrder {
        order_type: Some("sell".into()),
        resource_type: Some("token".into()),
        price_milli: Some(1000),
        total_amount: Some(100),
        room_name: None,
    }
}

fn result(intents: IntentBatch) -> RunResult {
    RunResult {
        // 故意伪造自报身份；宿主必须以注册表的键为准。
        player: "p2".into(),
        outcome: crate::RunOutcome::Executed,
        intents,
        intents_raw: serde_json::Value::Null,
        expr: serde_json::Value::Null,
        console_expr: None,
        cpu_billed_ms: None,
        raw_memory: None,
        ism_local: None,
        segments_out: None,
        console: Vec::new(),
    }
}

#[test]
fn host_intents_lose_to_player_wire_on_same_key_and_survive_on_others() {
    // S2.3 宿主意图合并语义：玩家 wire 对同 (对象, 意图键) 覆盖宿主注入
    // （结构性玩家优先），玩家未提交的宿主键保留；玩家自有意图族不受
    // 宿主注入影响。
    let mut wire = IntentBatch::default();
    wire.add_ext_intent(
        "p1",
        ObjId(1),
        kk_mod_api::Key::new("example:playerOnly").unwrap(),
        json!({"from": "player"}),
    );
    wire.add_ext_intent(
        "p1",
        ObjId(1),
        kk_mod_api::Key::new("example:shared").unwrap(),
        json!({"from": "player"}),
    );
    wire.add_object_intent("p1", ObjId(1), Action::Move, IntentData::bare());
    let host = vec![
        // 同键：宿主先注入，玩家 wire 覆盖。
        crate::HostIntent {
            player: "p1".into(),
            target: "kk1".into(),
            intent: "example:shared".into(),
            args: json!({"from": "host"}),
        },
        // 玩家未提交的键：宿主保留。
        crate::HostIntent {
            player: "p1".into(),
            target: "kk1".into(),
            intent: "example:hostOnly".into(),
            args: json!({"from": "host"}),
        },
        // 非 Mod 对象 ID 形非法（kk9f 越界十六进制字符？kkff 合法但对象
        // 不存在——构造层不查存在性，结算层负责；这里校验 wire 形合法即注入）。
    ];
    let batch =
        EmbedEngine::batch_with_host_intents(&BTreeMap::from([("p1".into(), result(wire))]), &host);
    let user = &batch.users["p1"];
    let ext = &user.ext[&ObjId(1)];
    assert_eq!(
        ext[&kk_mod_api::Key::new("example:shared").unwrap()],
        json!({"from": "player"}),
        "同键玩家覆盖宿主"
    );
    assert_eq!(
        ext[&kk_mod_api::Key::new("example:hostOnly").unwrap()],
        json!({"from": "host"}),
        "玩家未提交的宿主键保留"
    );
    assert_eq!(
        ext[&kk_mod_api::Key::new("example:playerOnly").unwrap()],
        json!({"from": "player"}),
        "玩家自有键不受影响"
    );
    assert!(
        user.objects.contains_key(&ObjId(1)),
        "玩家对象动作随合并保留"
    );
}

#[test]
fn registered_identity_filters_every_intent_family() {
    let mut intents = IntentBatch::default();
    for player in ["p1", "p2"] {
        intents.add_object_intent(player, ObjId(1), Action::Move, IntentData::bare());
        intents.add_room_intent(
            player,
            RoomIntent::RemoveFlag {
                room: "W1N1".into(),
                name: player.into(),
            },
        );
        intents.add_ext_intent(
            player,
            ObjId(1),
            kk_mod_api::Key::new("example:action").unwrap(),
            json!({"player": player}),
        );
        let user = intents.users.get_mut(player).unwrap();
        user.notify.push(NotifyIntent {
            message: player.into(),
            group_interval: None,
        });
        user.market.push(market_order());
    }
    let expected = intents.users["p1"].clone();
    let batch = EmbedEngine::batch_from_results(&BTreeMap::from([("p1".into(), result(intents))]));
    assert_eq!(batch.users, BTreeMap::from([("p1".into(), expected)]));

    // 不能将仅含他人指令的结果重新绑定为注册玩家的合法指令。
    let mut foreign_only = IntentBatch::default();
    foreign_only
        .users
        .entry("p2".into())
        .or_default()
        .market
        .push(market_order());
    assert!(
        EmbedEngine::batch_from_results(&BTreeMap::from([("p1".into(), result(foreign_only))]))
            .users
            .is_empty()
    );
}

#[test]
fn filtered_market_results_only_charge_registered_player() {
    let mut engine = kk_engine::Engine::default();
    for player in ["p1", "p2"] {
        engine
            .edit_players(|players| {
                players.entry(player.into()).or_default().money = 10_000_000;
                Ok(())
            })
            .unwrap();
    }
    let mut intents = IntentBatch::default();
    for player in ["p1", "p2"] {
        intents
            .users
            .entry(player.into())
            .or_default()
            .market
            .push(market_order());
    }
    let batch = EmbedEngine::batch_from_results(&BTreeMap::from([("p1".into(), result(intents))]));
    engine.tick_with_intents(batch).unwrap();
    assert_eq!(engine.market().orders.len(), 1);
    assert_eq!(engine.market().orders["mkt1"].user, "p1");
    assert_eq!(engine.players()["p1"].money, 9_995_000);
    assert_eq!(engine.players()["p2"].money, 10_000_000);
}
