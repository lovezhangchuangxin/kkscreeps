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
        expr: serde_json::Value::Null,
        cpu_billed_ms: None,
        raw_memory: None,
        segments_out: None,
        console: Vec::new(),
    }
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
