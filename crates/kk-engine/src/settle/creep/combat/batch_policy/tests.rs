//! 直接验证批量完整预检；不能靠整轮丢弃掩盖批内先写后报错。
use super::*;
use crate::settle::{Engine, Plane, TickState};
use kk_world::{RoomIndex, RoomState};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn working() -> Working {
    let engine = Engine::default();
    let room = RoomState::new("W1N1", kk_core::Terrain::plains());
    Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            buildings: Default::default(),
            links: engine.links,
            stores: engine.stores,
            combat: engine.combat,
            move_gate: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan: engine.plan,
        },
    )
}

fn put(w: &mut Working, id: u64, kind: &str, spec: Value) {
    let ctx = kk_world::place::PlaceCtx {
        buildings: Default::default(),
        time: 0,
        links: w.plane.links,
        stores: w.plane.stores,
        controller: None,
        name_seq: id as usize,
    };
    let mut object = kk_world::place::normalize(kind, &spec, &ctx).unwrap();
    object.set_id(id);
    w.room.objects.insert(ObjId(id), object);
    w.index = RoomIndex::build(&w.room.objects);
}

fn actors_and_structure() -> Working {
    let mut w = working();
    put(
        &mut w,
        2,
        "creep",
        json!({"name":"a","owner":"p1","pos":[10,10],"body":["ranged_attack"]}),
    );
    put(
        &mut w,
        3,
        "structure",
        json!({"pos":[30,30],"structureType":"container","hits":20,"store":{"energy":5}}),
    );
    put(
        &mut w,
        4,
        "creep",
        json!({"name":"b","owner":"p2","pos":[30,31],"body":["move"]}),
    );
    w
}

#[test]
fn invalid_late_batch_entry_does_not_destroy_an_earlier_structure() {
    for last in [
        (ObjId(99), 1),
        (ObjId(3), 1),
        (ObjId(4), -1),
        (ObjId(4), MAX_DAMAGE + 1),
    ] {
        let mut w = actors_and_structure();
        assert!(
            slot_combat_apply_damage_batch(
                &mut w,
                ObjId(2),
                &[(ObjId(3), 80), last],
                CombatAction::RangedMassAttack
            )
            .is_err()
        );
        assert_eq!(damage::object_hits(&w.room.objects[&ObjId(3)]), 20);
        assert_eq!(w.room.objects.len(), 3, "不能留下废墟或移除结构");
        assert!(w.events.is_empty());
        assert!(w.damage_to_apply.is_empty());
        assert_eq!(w.state.peek_next_id(), 100);
        assert_eq!(w.creep(ObjId(2)).unwrap().action_log, 0);
    }
}

#[test]
fn late_queue_overflow_does_not_apply_any_batch_entry() {
    let mut w = actors_and_structure();
    w.damage_to_apply.insert(ObjId(4), i64::MAX);
    assert!(
        slot_combat_apply_damage_batch(
            &mut w,
            ObjId(2),
            &[(ObjId(3), 80), (ObjId(4), 1)],
            CombatAction::RangedMassAttack
        )
        .is_err()
    );
    assert_eq!(damage::object_hits(&w.room.objects[&ObjId(3)]), 20);
    assert_eq!(w.damage_to_apply[&ObjId(4)], i64::MAX);
    assert!(w.events.is_empty());
    assert_eq!(w.state.peek_next_id(), 100);
}

#[test]
fn combat_pages_are_detached_and_continue_after_a_removed_cursor() {
    let mut w = actors_and_structure();
    put(
        &mut w,
        5,
        "structure",
        json!({"pos":[20,20],"structureType":"source"}),
    );
    let mut page = slot_combat_list(&w, "", 2).unwrap();
    assert_eq!(
        page.objects
            .iter()
            .map(|o| o.id.clone())
            .collect::<Vec<_>>(),
        vec![ObjId(2).as_str(), ObjId(3).as_str()]
    );
    assert_eq!(page.next, Some(ObjId(3).as_str()));
    page.objects[0].hits = 0;
    assert_eq!(w.creep(ObjId(2)).unwrap().hits, 100);
    w.room.objects.remove(&ObjId(3));
    let page = slot_combat_list(&w, &page.next.unwrap(), 2).unwrap();
    assert_eq!(page.objects.len(), 1);
    assert_eq!(page.objects[0].id, ObjId(4).as_str());
    assert!(page.next.is_none());
    assert!(slot_combat_list(&w, "", 0).is_err());
    assert!(slot_combat_list(&w, "bad-id", 2).is_err());
}

#[test]
fn empty_batch_records_action_and_upper_bound_is_accepted() {
    let mut w = actors_and_structure();
    slot_combat_apply_damage_batch(&mut w, ObjId(2), &[], CombatAction::RangedMassAttack).unwrap();
    assert_eq!(
        w.creep(ObjId(2)).unwrap().action_log,
        crate::settle::creep::ACTION_RANGED_MASS
    );
    assert!(w.events.is_empty());
    let requests: Vec<_> = (10..266)
        .map(|id| {
            put(
                &mut w,
                id,
                "creep",
                json!({"name":format!("b{id}"),"owner":"p2","pos":[30,30],"body":["move"]}),
            );
            (ObjId(id), if id == 10 { MAX_DAMAGE } else { 0 })
        })
        .collect();
    slot_combat_apply_damage_batch(&mut w, ObjId(2), &requests, CombatAction::RangedMassAttack)
        .unwrap();
    assert_eq!(w.events.len(), 256);
    assert_eq!(w.damage_to_apply[&ObjId(10)], MAX_DAMAGE);
    let mut too_many = requests;
    too_many.push((ObjId(4), 1));
    assert!(
        slot_combat_apply_damage_batch(&mut w, ObjId(2), &too_many, CombatAction::RangedMassAttack)
            .is_err()
    );
    assert_eq!(w.events.len(), 256);
}

#[test]
fn retaliation_overflow_and_self_damage_are_checked_before_any_batch_mutation() {
    let mut w = actors_and_structure();
    let policy = super::super::RetaliationPolicy::new(&w, &[(ObjId(3), 7), (ObjId(2), 9)]).unwrap();
    w.damage_to_apply.insert(ObjId(2), i64::MAX - 20);
    assert!(
        super::slot_combat_apply_damage_batch_with_retaliation(
            &mut w,
            ObjId(2),
            &[(ObjId(3), 80), (ObjId(2), 5)],
            CombatAction::RangedMassAttack,
            Some(&policy),
        )
        .is_err()
    );
    assert_eq!(damage::object_hits(&w.room.objects[&ObjId(3)]), 20);
    assert_eq!(w.damage_to_apply[&ObjId(2)], i64::MAX - 20);
    assert!(w.events.is_empty());
    assert_eq!(w.state.peek_next_id(), 100);
}

#[test]
fn retaliation_table_validates_whole_input_and_numeric_representation() {
    let w = actors_and_structure();
    for requests in [
        vec![(ObjId(3), 7), (ObjId(99), 1)],
        vec![(ObjId(3), 7), (ObjId(3), 8)],
        vec![(ObjId(3), -1)],
        vec![(ObjId(3), MAX_DAMAGE + 1)],
        vec![(ObjId(3), 0); 257],
    ] {
        assert!(super::super::RetaliationPolicy::new(&w, &requests).is_err());
    }
    assert!(super::super::RetaliationPolicy::new(&w, &[(ObjId(3), MAX_DAMAGE)]).is_ok());
}

#[test]
fn native_retaliation_overflow_is_rejected_before_the_primary_hit() {
    let mut w = actors_and_structure();
    let policy = super::super::RetaliationPolicy::new(&w, &[(ObjId(3), 7)]).unwrap();
    w.damage_to_apply.insert(ObjId(2), i64::MAX);
    assert!(
        super::super::slot_combat_execute_with_retaliation(
            &mut w,
            ObjId(2),
            &crate::intent::IntentData::target(ObjId(3)),
            CombatAction::RangedAttack,
            &policy
        )
        .is_err()
    );
    assert_eq!(damage::object_hits(&w.room.objects[&ObjId(3)]), 20);
    assert!(w.events.is_empty());
}
