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
            json!({"store": {"energy": 50}, "storeCapacity": 100, "effects": [{"power": 15, "level": 1, "endTime": 1}]}),
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

#[test]
fn transfer_clamps_to_target_capacity() {
    // transfer 钳制：spawn 容量 300
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = f
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 250,
                    ghodium: 0,
                },
                spawning: None,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 20, 21, &[BodyKind::Move, BodyKind::Carry], 100);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(spawn);
    d.amount = Some(100);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    // 250+100 > 300 → 只转 50
    assert_eq!(events[0]["data"]["amount"], 50);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 300),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn withdraw_only_from_stored_structures() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(src);
    d.amount = Some(10);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Withdraw, d);
    f.settle(b);
    // source 不可 withdraw（官方走 harvest）
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "withdraw 不动 source"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 0);
}

#[test]
fn transfer_into_extension_clamps_by_rcl() {
    // Extension 的能量容量按 RCL 计算，本例 RCL 3 为 50。
    // 来源：上游 extensions/tick.js 和 creeps/transfer.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ext = f
        .world
        .insert(
            "W1N1",
            RoomObject::Extension(kk_schema::room::ExtensionObj {
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        20,
        21,
        &[BodyKind::Move, BodyKind::Carry, BodyKind::Carry],
        60,
    );
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(ext);
    d.amount = Some(30);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["amount"], 10, "钳到容量差 50-40");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.store.energy, 50),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn drop_rejects_amount_over_holdings() {
    // drop.js:13：持有不足整条拒绝（官方不部分掉落）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 20);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(50);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    assert_eq!(f.creep_state(c).3, 20, "不足额：整条拒绝");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "无掉落"
    );
}

#[test]
fn energy_merge_saturates_at_u32_max() {
    // 掉落合并的受检运算：u32::MAX 堆 + 1 不 panic（debug）/不回绕丢数据（release），
    // 随后照常衰减 ceil(MAX/1000)
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                amount: u32::MAX,
                resource: None,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Move, BodyKind::Carry], 1);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(1);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    let decay = (u32::MAX as i64 + 999) / 1000; // ceil(4294967295/1000) = 4294968
    let expected = u32::MAX - decay as u32;
    let heap = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        })
        .unwrap();
    assert_eq!(heap, expected, "饱和合并后照常衰减（无 panic/回绕）");
}

#[test]
fn withdraw_respects_private_rampart_and_safe_mode() {
    for (target_owner, rampart_public, room_owner, safe_mode, allowed) in [
        ("p2", false, "p1", 0, false),
        ("p2", true, "p1", 0, true),
        ("p1", false, "p1", 0, true),
        ("p1", true, "p2", 10, false),
        ("p1", true, "p2", 0, true),
        ("p1", true, "p1", 10, true),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        f.controller_full(8, Some(room_owner), |ctl| ctl.safe_mode = safe_mode);
        let target = f.spawn("target", 10, 10, 100);
        f.world
            .edit_object(target, |object| {
                if let Some(RoomObject::Spawn(s)) = Some(object) {
                    s.owner = target_owner.into();
                }
                Ok(())
            })
            .unwrap();
        let rampart = generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
        f.world
            .edit_object(rampart, |object| {
                if let Some(RoomObject::Generic(g)) = Some(object) {
                    let mut v: Value = serde_json::from_str(&g.data).unwrap();
                    v["isPublic"] = serde_json::json!(rampart_public);
                    g.data = v.to_string();
                }
                Ok(())
            })
            .unwrap();
        let actor = f.creep("hauler", "p1", 11, 10, &[BodyKind::Carry], 0);
        let mut data = IntentData::target(target);
        data.amount = Some(10);
        data.resource_type = Some("energy".into());
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", actor, Action::Withdraw, data);
        let events = f.settle(batch);
        assert_eq!(
            f.creep_state(actor).3,
            if allowed { 10 } else { 0 },
            "target_owner={target_owner}, public={rampart_public}, room_owner={room_owner}, safe_mode={safe_mode}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e["event"] == kk_core::constants::EVENT_TRANSFER)
                .count(),
            usize::from(allowed)
        );
    }
}
