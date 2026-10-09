//! 表示域与来源摘要直接预检；不依赖整轮丢弃掩盖部分写入。
use super::*;
use crate::settle::{Engine, Plane, TickState, damage};
use kk_schema::room::RoomObject;
use kk_world::RoomState;
use serde_json::json;
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
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            static_passability: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan: engine.plan,
        },
    )
}

#[test]
fn selected_damage_is_bounded_and_never_modifies_the_original_queue() {
    let mut w = working();
    let id = ObjId(2);
    w.damage_to_apply.insert(id, 3);
    let candidate = IncomingDamage::prepare(&w, id, 2).unwrap();
    assert_eq!(candidate.input().unattributed_damage, 3);
    candidate.set_damage(MAX_DAMAGE).unwrap();
    for invalid in [-1, MAX_DAMAGE + 1, i64::MAX] {
        assert!(candidate.set_damage(invalid).is_err());
        assert_eq!(candidate.damage(), MAX_DAMAGE);
    }
    assert_eq!(w.damage_to_apply[&id], 3);
    w.damage_to_apply.insert(id, MAX_DAMAGE + 1);
    assert!(IncomingDamage::prepare(&w, id, 1).is_err());
    assert_eq!(w.damage_to_apply[&id], MAX_DAMAGE + 1);
}

#[test]
fn source_digest_keeps_exact_total_and_rejects_event_queue_inconsistency() {
    let mut w = working();
    let id = ObjId(2);
    for source in 3..263 {
        w.push_event(
            c::EVENT_ATTACK,
            ObjId(source),
            json!({"targetId":id.as_str(),"damage":1,"attackType":c::EVENT_ATTACK_TYPE_MELEE}),
        );
    }
    w.push_event(
        c::EVENT_ATTACK,
        ObjId(3),
        json!({"targetId":ObjId(4).as_str(),"damage":999,"attackType":1}),
    );
    w.push_event(
        c::EVENT_ATTACK,
        ObjId(3),
        json!({"targetId":id.as_str(),"damage":0,"attackType":1}),
    );
    w.damage_to_apply.insert(id, 265);
    let candidate = IncomingDamage::prepare(&w, id, 265).unwrap();
    assert_eq!(candidate.input().sources.len(), MAX_SOURCES);
    assert_eq!(candidate.input().source_count, 260);
    assert!(candidate.input().sources_truncated);
    assert_eq!(candidate.input().unattributed_damage, 5);
    w.damage_to_apply.insert(id, 259);
    assert!(IncomingDamage::prepare(&w, id, 259).is_err());
    assert_eq!(w.events.len(), 262);
}

#[test]
fn pending_hits_uses_wide_arithmetic_for_large_damage_and_healing() {
    let mut w = working();
    let id = ObjId(2);
    let ctx = kk_world::place::PlaceCtx {
        buildings: Default::default(),
        time: 0,
        links: w.plane.links,
        stores: w.plane.stores,
        controller: None,
        name_seq: 2,
    };
    let mut object = kk_world::place::normalize(
        "creep",
        &json!({"name":"b","owner":"p1","pos":[10,10],"body":["move"]}),
        &ctx,
    )
    .unwrap();
    object.set_id(id.0);
    w.room.objects.insert(id, object);
    for raw in [0, i64::MAX] {
        w.damage_to_apply.insert(id, raw);
        w.heal_to_apply.insert(id, i64::MAX);
        assert!(damage::apply_pending_hits(&mut w, id));
        assert!(matches!(&w.room.objects[&id],RoomObject::Creep(c) if c.hits==100));
    }
}
