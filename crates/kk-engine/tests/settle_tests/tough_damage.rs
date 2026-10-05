//! 强化 TOUGH 的伤害聚合、身体顺序、治疗与携带容量结算。

use super::*;

fn boost(f: &mut Fixture, id: ObjId, index: usize, resource: &str) {
    let Some(RoomObject::Creep(creep)) = f.world.get_mut(id) else {
        panic!("预期 creep");
    };
    creep.body[index].boost = Some(resource.into());
}

fn parts(f: &Fixture, id: ObjId) -> (u32, Vec<u32>) {
    let RoomObject::Creep(creep) = f.obj(id) else {
        panic!("预期存活 creep");
    };
    (
        creep.hits,
        creep.body.iter().map(|part| part.hits).collect(),
    )
}

#[test]
fn all_native_creep_attacks_use_tough_tiers_but_events_keep_raw_damage() {
    for (action, kind, raw) in [
        (Action::Attack, BodyKind::Attack, 30),
        (Action::RangedAttack, BodyKind::RangedAttack, 10),
        (Action::RangedMassAttack, BodyKind::RangedAttack, 10),
    ] {
        for (resource, percent) in [("GO", 70), ("GHO2", 50), ("XGHO2", 30)] {
            let mut f = Fixture::new(1, terrain_with(&[]));
            let a = f.creep("a", "p1", 10, 10, &[kind], 0);
            let b = f.creep("b", "p2", 10, 11, &[BodyKind::Tough, BodyKind::Move], 0);
            boost(&mut f, b, 0, resource);
            let mut batch = IntentBatch::default();
            batch.add_object_intent("p1", a, action, IntentData::target(b));
            let events = f.settle(batch);
            let loss = raw * percent / 100;
            assert_eq!(parts(&f, b), (200 - loss, vec![100 - loss, 100]));
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["data"]["damage"], raw);
            assert_eq!(events[0]["data"]["targetId"], b.as_str());
        }
    }
}

#[test]
fn partial_destroyed_and_late_tough_only_absorb_their_remaining_capacity() {
    for (front_hits, expected) in [(10, (90, vec![0, 90])), (0, (70, vec![0, 70]))] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let a = f.creep("a", "p1", 10, 10, &[BodyKind::Attack], 0);
        let b = f.creep("b", "p2", 10, 11, &[BodyKind::Tough, BodyKind::Move], 0);
        boost(&mut f, b, 0, "GHO2");
        if let Some(RoomObject::Creep(creep)) = f.world.get_mut(b) {
            creep.body[0].hits = front_hits;
            creep.hits = 100 + front_hits;
        }
        f.settle(attack_intent(a, b));
        assert_eq!(parts(&f, b), expected);
    }
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Attack; 5], 0);
    let b = f.creep(
        "b",
        "p2",
        10,
        11,
        &[BodyKind::Move, BodyKind::Tough, BodyKind::Move],
        0,
    );
    boost(&mut f, b, 1, "GHO2");
    f.settle(attack_intent(a, b));
    assert_eq!(
        parts(&f, b),
        (175, vec![0, 75, 100]),
        "前列 MOVE 先承伤 100"
    );
}

#[test]
fn same_tick_attackers_share_one_armor_capacity_and_later_ticks_use_damaged_body() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Attack; 5], 0);
    let c = f.creep("c", "p1", 11, 10, &[BodyKind::Attack; 5], 0);
    let b = f.creep(
        "b",
        "p2",
        10,
        11,
        &[BodyKind::Tough, BodyKind::Move, BodyKind::Move],
        0,
    );
    boost(&mut f, b, 0, "GHO2");
    let mut batch = attack_intent(a, b);
    batch.add_object_intent("p1", c, Action::Attack, IntentData::target(b));
    let events = f.settle(batch);
    assert_eq!(
        parts(&f, b),
        (100, vec![0, 0, 100]),
        "300 原始伤害只能减免一次 100"
    );
    assert_eq!(events.len(), 2);
    f.settle(attack_intent(a, b));
    assert!(
        f.world.get(b).is_none(),
        "下一轮已毁 TOUGH 不再减免 150 伤害"
    );
}

#[test]
fn heal_merges_after_reduction_can_rescue_lethal_damage_and_does_not_refresh_armor() {
    for (with_heal, alive) in [(false, false), (true, true)] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let b = f.creep("b", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Move], 0);
        boost(&mut f, b, 0, "GO");
        let a = f.creep("a", "p2", 10, 11, &[BodyKind::Attack; 10], 0);
        let mut batch = attack_intent_by("p2", a, b);
        if with_heal {
            let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal; 5], 0);
            batch.add_object_intent("p1", medic, Action::Heal, IntentData::target(b));
        }
        f.settle(batch);
        assert_eq!(f.world.get(b).is_some(), alive);
        if alive {
            assert_eq!(
                parts(&f, b),
                (3, vec![0, 3]),
                "200 - (300 - round(100/0.7*0.3)) + 60"
            );
        } else {
            assert_eq!(
                f.world
                    .room("W1N1")
                    .unwrap()
                    .objects
                    .values()
                    .filter(|o| matches!(o, RoomObject::Tombstone(_)))
                    .count(),
                1
            );
        }
    }
    let mut f = Fixture::new(1, terrain_with(&[]));
    let b = f.creep("b", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Move], 0);
    boost(&mut f, b, 0, "GHO2");
    if let Some(RoomObject::Creep(creep)) = f.world.get_mut(b) {
        creep.hits = 100;
        creep.body[0].hits = 0;
    }
    let a = f.creep("a", "p2", 10, 11, &[BodyKind::Attack], 0);
    let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal; 3], 0);
    let mut batch = attack_intent_by("p2", a, b);
    batch.add_object_intent("p1", medic, Action::Heal, IntentData::target(b));
    f.settle(batch);
    assert_eq!(
        parts(&f, b),
        (106, vec![6, 100]),
        "同轮治疗不能先复活 TOUGH 再减伤"
    );
}

#[test]
fn carry_capacity_drops_resources_only_after_actual_net_damage() {
    for (attacks, heals, expected_hits, expected_energy) in
        [(5, 0, 325, 100), (14, 0, 80, 50), (14, 27, 400, 100)]
    {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let a = f.creep("a", "p1", 10, 10, &vec![BodyKind::Attack; attacks], 0);
        let b = f.creep(
            "b",
            "p2",
            10,
            11,
            &[
                BodyKind::Tough,
                BodyKind::Tough,
                BodyKind::Carry,
                BodyKind::Carry,
            ],
            100,
        );
        boost(&mut f, b, 0, "GHO2");
        // 第一件 100 hits 可吸收 200 原始伤害；第二件不强化。
        let mut batch = attack_intent(a, b);
        if heals > 0 {
            let medic = f.creep("medic", "p2", 11, 11, &vec![BodyKind::Heal; heals], 0);
            batch.add_object_intent("p2", medic, Action::Heal, IntentData::target(b));
        }
        f.settle(batch);
        assert_eq!(parts(&f, b).0, expected_hits);
        assert_eq!(f.creep_state(b).3, expected_energy);
    }
}
