//! 通用仓库的容量竞争、实验室矿物槽切换与提取限制。
use super::*;
use serde_json::json;

fn warehouse(f: &mut Fixture, kind: &str, fields: Value) -> ObjId {
    let id = generic_structure(f, 10, 11, kind, "p1", 1000);
    f.world
        .edit_object(id, |object| {
            let RoomObject::Generic(g) = object else {
                unreachable!()
            };
            let mut data: Value = serde_json::from_str(&g.data).unwrap();
            data.as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            g.data = data.to_string();
            Ok(())
        })
        .unwrap();
    id
}

fn read(f: &Fixture, id: ObjId) -> Value {
    let RoomObject::Generic(g) = f.obj(id) else {
        unreachable!()
    };
    serde_json::from_str(&g.data).unwrap()
}

fn exchange(
    f: &mut Fixture,
    actor: ObjId,
    target: ObjId,
    action: Action,
    resource: &str,
    amount: i64,
) -> Vec<Value> {
    let mut data = IntentData::target(target);
    data.resource_type = Some(resource.into());
    data.amount = Some(amount);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", actor, action, data);
    f.settle(batch)
}

#[test]
fn shared_warehouses_clamp_mixed_resources_and_reverse_events() {
    for kind in ["container", "terminal", "factory", "customWarehouse"] {
        let capacity = match kind {
            "terminal" => 300_000,
            "factory" => 50_000,
            _ => 2000,
        };
        let mut f = Fixture::new(1, terrain_with(&[]));
        f.controller(8, Some("p1"));
        let target = warehouse(
            &mut f,
            kind,
            json!({"store": {"energy": capacity - 10}, "storeCapacity": capacity}),
        );
        let actor = f.creep_with_mineral("hauler", 10, 10, &[BodyKind::Carry], 0, "U", 30);
        let events = exchange(&mut f, actor, target, Action::Transfer, "U", 30);
        assert_eq!(read(&f, target)["store"]["U"], 10, "{kind} 共享余量");
        assert_eq!(events[0]["data"]["amount"], 10);
        let rejected = exchange(&mut f, actor, target, Action::Transfer, "U", 20);
        assert!(
            rejected.is_empty(),
            "{kind}: {rejected:?}, {:?}",
            read(&f, target)
        );
        let events = exchange(&mut f, actor, target, Action::Withdraw, "energy", 50);
        assert_eq!(read(&f, target)["store"]["energy"], capacity - 40);
        assert_eq!(events[0]["objectId"], target.as_str());
        assert_eq!(events[0]["data"]["amount"], 30, "携带已有 U20，余量30");
    }
}

#[test]
fn dedicated_slots_and_lab_mineral_binding_are_preserved() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let target = warehouse(
        &mut f,
        "lab",
        json!({"store": {"energy": 2000}, "storeCapacity": 5000, "storeCapacityResource": {"energy": 2000}}),
    );
    let actor = f.creep_with_mineral("hauler", 10, 10, &[BodyKind::Carry], 0, "U", 30);
    exchange(&mut f, actor, target, Action::Transfer, "U", 30);
    assert_eq!(read(&f, target)["storeCapacity"], Value::Null);
    assert_eq!(read(&f, target)["storeCapacityResource"]["U"], 3000);
    let other = f.creep_with_mineral("other", 9, 11, &[BodyKind::Carry], 0, "H", 20);
    assert!(exchange(&mut f, other, target, Action::Transfer, "H", 20).is_empty());
    exchange(&mut f, actor, target, Action::Withdraw, "U", 30);
    assert_eq!(read(&f, target)["storeCapacity"], 5000);
    assert_eq!(read(&f, target)["storeCapacityResource"]["U"], Value::Null);
    exchange(&mut f, other, target, Action::Transfer, "H", 20);
    assert_eq!(read(&f, target)["store"]["H"], 20);

    let mut f = Fixture::new(1, terrain_with(&[]));
    let target = warehouse(
        &mut f,
        "powerSpawn",
        json!({"store": {"energy": 5000, "power": 95}, "storeCapacity": 5100, "storeCapacityResource": {"energy": 5000, "power": 100}}),
    );
    let actor = f.creep_with_mineral("hauler", 10, 10, &[BodyKind::Carry], 0, "power", 10);
    exchange(&mut f, actor, target, Action::Transfer, "power", 10);
    assert_eq!(read(&f, target)["store"]["power"], 100);
    assert_eq!(read(&f, target)["store"]["energy"], 5000);
}

#[test]
fn generic_withdraw_respects_special_targets_and_terminal_effect_expiry() {
    for kind in ["nuker", "powerBank", "terminal"] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let target = warehouse(
            &mut f,
            kind,
            json!({"store": {"energy": 50}, "storeCapacity": 100, "effects": [{"power": 15, "endTime": 1}]}),
        );
        let actor = f.creep("hauler", "p1", 10, 10, &[BodyKind::Carry], 0);
        assert!(exchange(&mut f, actor, target, Action::Withdraw, "energy", 10).is_empty());
        assert_eq!(read(&f, target)["store"]["energy"], 50);
        let events = exchange(&mut f, actor, target, Action::Withdraw, "energy", 10);
        assert_eq!(events.len(), usize::from(kind == "terminal"));
    }
}

#[test]
fn competing_transfers_share_candidate_capacity_and_private_rampart_blocks_withdraw() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let target = warehouse(
        &mut f,
        "container",
        json!({"store": {"energy": 1990}, "storeCapacity": 2000}),
    );
    let a = f.creep_with_mineral("a", 10, 10, &[BodyKind::Carry], 0, "U", 20);
    let b = f.creep_with_mineral("b", 9, 11, &[BodyKind::Carry], 0, "H", 20);
    let mut batch = IntentBatch::default();
    for (actor, resource) in [(a, "U"), (b, "H")] {
        let mut data = IntentData::target(target);
        data.resource_type = Some(resource.into());
        data.amount = Some(20);
        batch.add_object_intent("p1", actor, Action::Transfer, data);
    }
    let events = f.settle(batch);
    assert_eq!(events.len(), 1);
    let store = read(&f, target);
    assert_eq!(
        store["store"]
            .as_object()
            .unwrap()
            .values()
            .map(|n| n.as_i64().unwrap())
            .sum::<i64>(),
        2000
    );
    let before = store["store"]["energy"].clone();
    // 改为无主仓库，覆盖敌方私有 rampart，继承既有保护判定。
    f.world
        .edit_object(target, |object| {
            let RoomObject::Generic(g) = object else {
                unreachable!()
            };
            let mut data: Value = serde_json::from_str(&g.data).unwrap();
            data["owner"] = Value::Null;
            g.data = data.to_string();
            Ok(())
        })
        .unwrap();
    generic_structure(&mut f, 10, 11, "rampart", "p2", 1000);
    assert!(exchange(&mut f, a, target, Action::Withdraw, "energy", 10).is_empty());
    assert_eq!(read(&f, target)["store"]["energy"], before);
}

#[test]
fn newly_built_container_accepts_its_first_transfer() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let site = f.site(10, 11, "container", 4995, 5000, "p1");
    let actor = f.creep(
        "builder",
        "p1",
        10,
        10,
        &[BodyKind::Work, BodyKind::Carry],
        10,
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", actor, Action::Build, IntentData::target(site));
    f.settle(batch);
    let target = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, object)| match object {
            RoomObject::Generic(g)
                if serde_json::from_str::<Value>(&g.data).unwrap()["structureType"]
                    == "container" =>
            {
                Some(*id)
            }
            _ => None,
        })
        .expect("容器建造完成");
    assert_eq!(read(&f, target)["store"], json!({"energy": 0}));
    let events = exchange(&mut f, actor, target, Action::Transfer, "energy", 5);
    assert_eq!(events[0]["data"]["amount"], 5);
    assert_eq!(read(&f, target)["store"]["energy"], 5);
}
