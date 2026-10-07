use super::*;
use crate::settle::{Engine, Plane, TickState};
use kk_world::RoomState;
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
            terrain_passability: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan: engine.plan,
        },
    )
}

fn put(w: &mut Working, id: ObjId, kind: &str, spec: Value) {
    let ctx = kk_world::place::PlaceCtx {
        buildings: Default::default(),
        time: 0,
        links: w.plane.links,
        stores: w.plane.stores,
        controller: None,
        name_seq: id.0 as usize,
    };
    let mut object = kk_world::place::normalize(kind, &spec, &ctx).unwrap();
    object.set_id(id.0);
    w.room.objects.insert(id, object);
}

fn generic(w: &mut Working, id: ObjId, data: Value) {
    w.room.objects.insert(
        id,
        RoomObject::Generic(kk_schema::room::GenericObj {
            id: id.0,
            x: 10,
            y: 10,
            data: data.to_string(),
        }),
    );
}

#[test]
fn reserved_slots_and_remaining_shared_capacity_count_all_unreserved_resources() {
    let mut w = working();
    generic(
        &mut w,
        ObjId(1),
        json!({"store":{"energy":5,"U":3,"K":4},"storeCapacity":20,"storeCapacityResource":{"energy":10}}),
    );
    assert_eq!(
        inspect(&w, ObjId(1), "energy").unwrap(),
        ResourceView {
            amount: 5,
            capacity: Some(10),
            free: Some(5)
        }
    );
    assert_eq!(
        inspect(&w, ObjId(1), "U").unwrap(),
        ResourceView {
            amount: 3,
            capacity: Some(10),
            free: Some(3)
        }
    );
    assert_eq!(inspect(&w, ObjId(1), "Z").unwrap().free, Some(3));
    generic(
        &mut w,
        ObjId(2),
        json!({"store":{"U":10},"storeCapacity":10}),
    );
    assert_eq!(
        transfer(&mut w, ObjId(2), ObjId(1), "U", 4, 0).unwrap(),
        ResourceTransfer::default()
    );
    assert_eq!(
        transfer(&mut w, ObjId(2), ObjId(1), "U", 3, 0)
            .unwrap()
            .credited,
        3
    );
    assert_eq!(inspect(&w, ObjId(1), "Z").unwrap().free, Some(0));
}

#[test]
fn malformed_inventory_and_capacity_never_produce_a_partial_debit() {
    for data in [
        json!({"store":{"energy":-1},"storeCapacity":50}),
        json!({"store":{"energy":1.5},"storeCapacity":50}),
        json!({"store":{"energy":0},"storeCapacity":-1}),
        json!({"store":{"energy":0},"storeCapacity":"50"}),
        json!({"store":{"energy":0},"storeCapacity":"50","storeCapacityResource":{"energy":10}}),
        json!({"store":{"energy":0},"storeCapacity":9_007_199_254_740_992_i64}),
        json!({"store":{"energy":0},"storeCapacityResource":{"energy":-1}}),
        json!({"store":{"U":9_007_199_254_740_991_i64,"K":1},"storeCapacity":50}),
        json!({"store":{"U":9_007_199_254_740_991_i64,"energy":0},"storeCapacityResource":{"U":1,"energy":10}}),
    ] {
        let mut w = working();
        generic(
            &mut w,
            ObjId(1),
            json!({"store":{"energy":10},"storeCapacity":50}),
        );
        generic(&mut w, ObjId(2), data);
        let before = w.room.objects.clone();
        assert!(transfer(&mut w, ObjId(1), ObjId(2), "energy", 3, 1).is_err());
        assert_eq!(w.room.objects, before);
        assert!(w.events.is_empty());
    }
}

#[test]
fn tombstone_and_ruin_are_readable_sources_and_never_writable_destinations() {
    for kind in ["tombstone", "ruin"] {
        let mut w = working();
        put(
            &mut w,
            ObjId(1),
            kind,
            json!({"pos":[10,10],"store":{"energy":10,"U":5},"ticksToDecay":100}),
        );
        put(
            &mut w,
            ObjId(2),
            "creep",
            json!({"pos":[10,9],"name":"hauler","owner":"p1","body":["carry"],"store":{}}),
        );
        assert_eq!(
            inspect(&w, ObjId(1), "U").unwrap(),
            ResourceView {
                amount: 5,
                capacity: None,
                free: None
            }
        );
        assert_eq!(
            transfer(&mut w, ObjId(1), ObjId(2), "U", 5, 1).unwrap(),
            ResourceTransfer {
                debited: 5,
                credited: 4,
                lost: 1
            }
        );
        assert_eq!(inspect(&w, ObjId(1), "U").unwrap().amount, 0);
        assert_eq!(inspect(&w, ObjId(2), "U").unwrap().amount, 4);
        let before = w.room.objects.clone();
        assert!(transfer(&mut w, ObjId(2), ObjId(1), "U", 1, 0).is_err());
        assert_eq!(w.room.objects, before);
        assert_eq!(w.events.len(), 1);
    }
}

#[test]
fn total_loss_and_zero_requests_have_explicit_atomic_results() {
    let mut w = working();
    generic(
        &mut w,
        ObjId(1),
        json!({"store":{"energy":10},"storeCapacity":50}),
    );
    generic(
        &mut w,
        ObjId(2),
        json!({"store":{"energy":50},"storeCapacity":50}),
    );
    assert_eq!(
        transfer(&mut w, ObjId(1), ObjId(2), "energy", 0, 0).unwrap(),
        ResourceTransfer::default()
    );
    assert!(w.events.is_empty());
    assert_eq!(
        transfer(&mut w, ObjId(1), ObjId(2), "energy", 10, 10).unwrap(),
        ResourceTransfer {
            debited: 10,
            credited: 0,
            lost: 10
        }
    );
    assert_eq!(inspect(&w, ObjId(1), "energy").unwrap().amount, 0);
    assert_eq!(inspect(&w, ObjId(2), "energy").unwrap().amount, 50);
    assert_eq!(w.events[0]["data"]["loss"], 10);
}

#[test]
fn zero_independent_capacity_does_not_borrow_shared_space() {
    let mut w = working();
    generic(
        &mut w,
        ObjId(1),
        json!({"store":{"energy":0,"U":4},"storeCapacity":20,"storeCapacityResource":{"energy":0}}),
    );
    assert_eq!(
        inspect(&w, ObjId(1), "energy").unwrap(),
        ResourceView {
            amount: 0,
            capacity: Some(0),
            free: Some(0)
        }
    );
    assert_eq!(inspect(&w, ObjId(1), "K").unwrap().free, Some(16));
    generic(
        &mut w,
        ObjId(2),
        json!({"store":{"energy":5},"storeCapacity":10}),
    );
    assert_eq!(
        transfer(&mut w, ObjId(2), ObjId(1), "energy", 1, 0).unwrap(),
        ResourceTransfer::default()
    );
    assert!(w.events.is_empty());
}
