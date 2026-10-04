//! 搬运和掉落的容量、库存与访问门禁。
use super::*;

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
        if let Some(RoomObject::Spawn(s)) = f.world.get_mut(target) {
            s.owner = target_owner.into();
        }
        let rampart = generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
        if let Some(RoomObject::Generic(g)) = f.world.get_mut(rampart) {
            let mut v: Value = serde_json::from_str(&g.data).unwrap();
            v["isPublic"] = serde_json::json!(rampart_public);
            g.data = v.to_string();
        }
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
