//! 控制器零量升级仍有动作与事件，完整门通过才进入官方零量路径。
use super::*;
use kk_core::constants as c;
use serde_json::json;

#[test]
fn upgrade_zero_work_still_logs_event_without_spending_energy_or_adding_gcl() {
    // engine 4.3.0 upgradeController.js:29-36、64-103：没有 buildEffect=0 拒绝。
    for level in [1, 8] {
        for damaged_work in [false, true] {
            let mut f = Fixture::new(1, terrain_with(&[]));
            let ctl = f.controller(level, Some("p1"));
            let body = if damaged_work {
                vec![BodyKind::Carry, BodyKind::Work]
            } else {
                vec![BodyKind::Carry]
            };
            let id = f.creep("zero", "p1", 6, 6, &body, 10);
            if damaged_work {
                f.world
                    .edit_object(id, |object| {
                        if let RoomObject::Creep(creep) = object {
                            creep.body[1].hits = 0;
                            creep.hits = 100;
                        }
                        Ok(())
                    })
                    .unwrap();
            }
            let events = f.settle(controller_intent(id, Action::UpgradeController, ctl));
            let upgrades: Vec<_> = events
                .iter()
                .filter(|e| e["event"] == c::EVENT_UPGRADE_CONTROLLER)
                .collect();
            assert_eq!(upgrades.len(), 1, "level={level}, damaged={damaged_work}");
            assert_eq!(upgrades[0]["data"], json!({"amount":0,"energySpent":0}));
            assert_eq!(ctl_of(&f).progress, 0);
            assert_eq!(f.players["p1"].gcl, 0);
            match f.obj(id) {
                RoomObject::Creep(creep) => {
                    assert_eq!(creep.store.energy, 10);
                    assert_ne!(
                        creep.action_log & u16::from(kk_engine::settle::creep::ACTION_UPGRADE),
                        0
                    );
                }
                _ => panic!(),
            }
        }
    }
}

#[test]
fn upgrade_zero_work_keeps_native_energy_ownership_range_and_blocked_gates() {
    for gate in ["energy", "owner", "range", "level", "blocked", "spawning"] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let ctl = f.controller_full(
            if gate == "level" { 0 } else { 1 },
            Some(if gate == "owner" { "p2" } else { "p1" }),
            |t| {
                if gate == "blocked" {
                    t.upgrade_blocked = Some(100);
                }
            },
        );
        let id = f.creep(
            "zero",
            "p1",
            if gate == "range" { 20 } else { 6 },
            6,
            &[BodyKind::Carry],
            if gate == "energy" { 0 } else { 10 },
        );
        if gate == "spawning" {
            f.world
                .edit_object(id, |object| {
                    if let RoomObject::Creep(creep) = object {
                        creep.spawning = true;
                    }
                    Ok(())
                })
                .unwrap();
        }
        let events = f.settle(controller_intent(id, Action::UpgradeController, ctl));
        assert!(
            !events
                .iter()
                .any(|e| e["event"] == c::EVENT_UPGRADE_CONTROLLER),
            "{gate}"
        );
        if let Some((_, RoomObject::Creep(creep))) = f.world.get(id) {
            assert_eq!(
                creep.action_log & u16::from(kk_engine::settle::creep::ACTION_UPGRADE),
                0,
                "{gate}"
            );
        }
    }
}

#[test]
fn upgrade_zero_work_is_rejected_after_rcl8_limit_is_exhausted() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(8, Some("p1"));
    let first = f.creep("first", "p1", 6, 6, &[BodyKind::Work; 16], 100);
    let zero = f.creep("zero", "p1", 6, 7, &[BodyKind::Carry], 10);
    let mut batch = controller_intent(first, Action::UpgradeController, ctl);
    batch.add_object_intent(
        "p1",
        zero,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    let events = f.settle(batch);
    let upgrades: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == c::EVENT_UPGRADE_CONTROLLER)
        .collect();
    assert_eq!(upgrades.len(), 1);
    assert_eq!(upgrades[0]["data"], json!({"amount":15,"energySpent":15}));
    assert_eq!(f.players["p1"].gcl, 15);
    if let RoomObject::Creep(creep) = f.obj(zero) {
        assert_eq!(
            creep.action_log & u16::from(kk_engine::settle::creep::ACTION_UPGRADE),
            0
        );
        assert_eq!(creep.store.energy, 10);
    }
}
