//! capacity 域 power 施放的引擎级边界（效果注入构造的不可达状态）。
use super::*;

fn terrain_plain() -> kk_core::Terrain {
    kk_core::Terrain::plains()
}

/// OPERATE_EXTENSION 施放在目标带活跃 DISRUPT_TERMINAL 效果时静默拒绝
///（官方 usePower.js 分支门；DISRUPT_TERMINAL 施放链已接通，本测试仍以
/// 装填注入构造效果以便独立控制移除时机，移除后同轮施放恢复灌能作对照）。
#[test]
fn operate_extension_rejected_while_disrupt_terminal_active() {
    let mut f = Fixture::new(1, terrain_plain());
    f.controller_full(8, Some("p1"), |ctl| ctl.is_power_enabled = true);
    let storage = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 1000,
                    ghodium: 0,
                },
                notify_when_attacked: None,
                store_extra: vec![],
            }),
        )
        .unwrap();
    let ext = f
        .world
        .insert(
            "W1N1",
            RoomObject::Extension(kk_schema::room::ExtensionObj {
                id: 0,
                x: 21,
                y: 20,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let pc = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 19,
                y: 20,
                data: r#"{"type":"powerCreep","owner":"p1","powers":{"6":{"level":1}},"store":{"ops":300}}"#.into(),
            }),
        )
        .unwrap();
    f.world
        .edit_room("W1N1", |room| {
            room.effects.insert(
                storage,
                vec![kk_world::EffectEntry {
                    effect: Some(15),
                    power: 15,
                    level: 1,
                    end_time: 100,
                }],
            );
            Ok(())
        })
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.power = Some(6);
    d.id = Some(storage);
    b.add_object_intent("p1", pc, Action::UsePower, d);
    let events = f.settle(b);
    assert!(events.is_empty(), "无 EVENT_POWER");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.store.energy, 0, "未灌能"),
        _ => panic!(),
    }
    match f.obj(pc) {
        RoomObject::Generic(g) => {
            let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(v.pointer("/store/ops").unwrap(), 300, "不扣 ops");
            assert!(v.pointer("/powers/6/cooldownTime").is_none(), "不进冷却");
        }
        _ => panic!(),
    }
    // 对照：移除干扰效果后同款施放成功灌能 0.2 × 200 = 40。
    f.world
        .edit_room("W1N1", |room| {
            room.effects.remove(&storage);
            Ok(())
        })
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.power = Some(6);
    d.id = Some(storage);
    b.add_object_intent("p1", pc, Action::UsePower, d);
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.store.energy, 40),
        _ => panic!(),
    }
}
