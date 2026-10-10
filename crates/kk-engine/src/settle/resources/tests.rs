use super::*;
use crate::settle::{Engine, Plane, TickState};
use kk_world::RoomState;
use std::collections::{BTreeMap, BTreeSet};

fn working() -> Working {
    let engine = Engine::default();
    let room = RoomState::new("W1N1", kk_core::Terrain::plains());
    Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            world_span: Plane::UNWRAPPED_SPAN,
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
            data: data.to_string().into(),
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

// ---- G09.a Mod 对象库存 ----

/// 声明 stmod:crate（store 容量 500）与资源 stmod:gem 的贡献平面。
fn mod_store_plane() -> Plane {
    let contribution =
        kk_mod_api::Contribution {
            source: kk_mod_api::Source {
                namespace: "stmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [9; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: BTreeSet::from([
                kk_mod_api::Key::new("stmod:crate.field.x").unwrap(),
                kk_mod_api::Key::new("stmod:crate.field.y").unwrap(),
            ]),
            field_mounts: Vec::new(),
            object_types: vec![kk_mod_api::ObjectTypeDescriptor {
                key: kk_mod_api::Key::new("stmod:crate").unwrap(),
                schema_version: 1,
                owner_field: None,
                position_fields: Some([
                    kk_mod_api::Key::new("stmod:x").unwrap(),
                    kk_mod_api::Key::new("stmod:y").unwrap(),
                ]),
                fields: vec![
                    kk_mod_api::FieldDescriptor {
                        key: kk_mod_api::Key::new("stmod:x").unwrap(),
                        schema: kk_mod_api::ValueSchema {
                            kind: kk_mod_api::ValueKind::Number(
                                kk_mod_api::NumberDomain::Unsigned { min: 0, max: 49 },
                            ),
                            nullable: false,
                        },
                        default: kk_mod_api::FieldDefault::Missing,
                        visibility: kk_mod_api::Visibility::Public,
                        binding: kk_mod_api::Key::new("stmod:crate.field.x").unwrap(),
                    },
                    kk_mod_api::FieldDescriptor {
                        key: kk_mod_api::Key::new("stmod:y").unwrap(),
                        schema: kk_mod_api::ValueSchema {
                            kind: kk_mod_api::ValueKind::Number(
                                kk_mod_api::NumberDomain::Unsigned { min: 0, max: 49 },
                            ),
                            nullable: false,
                        },
                        default: kk_mod_api::FieldDefault::Missing,
                        visibility: kk_mod_api::Visibility::Public,
                        binding: kk_mod_api::Key::new("stmod:crate.field.y").unwrap(),
                    },
                ],
                capabilities: Vec::new(),
                lifecycle_slots: BTreeSet::new(),
                visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                player_class: "Crate".into(),
                store_capacity: Some(500),
                hits_max: None,
                retaliation_milli: None,
            }],
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: vec![kk_mod_api::resources::ResourceDescriptor {
                key: kk_mod_api::Key::new("stmod:gem").unwrap(),
            }],
            buildings: Vec::new(),
        };
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    Plane {
        world_span: Plane::UNWRAPPED_SPAN,
        buildings: Default::default(),
        links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
        stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
        combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        static_passability: Default::default(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        contrib: std::rc::Rc::new(crate::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    }
}

fn mod_working() -> Working {
    let room = RoomState::new("W1N1", kk_core::Terrain::plains());
    Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        mod_store_plane(),
    )
}

fn carrier(w: &mut Working, id: ObjId, store: Value, capacity: i64) {
    w.room.objects.insert(
        id,
        RoomObject::Mod(kk_schema::room::ModObj {
            id: id.0,
            x: 10,
            y: 10,
            type_key: "stmod:crate".into(),
            data: json!({"x": 10, "y": 10, "store": store, "storeCapacity": capacity}).to_string(),
        }),
    );
}

#[test]
fn mod_object_store_inspects_capacity_and_transfers_both_directions() {
    let mut w = mod_working();
    carrier(
        &mut w,
        ObjId(1),
        json!({"energy": 100, "stmod:gem": 30}),
        500,
    );
    assert_eq!(
        inspect(&w, ObjId(1), "stmod:gem").unwrap(),
        ResourceView {
            amount: 30,
            capacity: Some(500),
            free: Some(370)
        }
    );
    carrier(&mut w, ObjId(2), json!({"energy": 0}), 50);
    // 精确转账协议不自动截取：请求 400 超出持有（100）与目标余量（50）
    // 均按零结果返回。
    assert_eq!(
        transfer(&mut w, ObjId(1), ObjId(2), "energy", 400, 0).unwrap(),
        ResourceTransfer::default()
    );
    let moved = transfer(&mut w, ObjId(1), ObjId(2), "energy", 50, 0).unwrap();
    assert_eq!(
        moved,
        ResourceTransfer {
            debited: 50,
            credited: 50,
            lost: 0
        }
    );
    let data = w.room.objects[&ObjId(2)].clone();
    let RoomObject::Mod(o) = data else {
        panic!("仍为 Mod 对象")
    };
    let payload: Value = serde_json::from_str(&o.data).unwrap();
    assert_eq!(
        payload["store"]["energy"],
        json!(50),
        "目标满载（余量恰 50）"
    );
    assert_eq!(payload["storeCapacity"], json!(50), "容量信封不随转账变化");
    // 反向提取回到容量内。
    let back = transfer(&mut w, ObjId(2), ObjId(1), "energy", 30, 0).unwrap();
    assert_eq!(back.credited, 30);
}

#[test]
fn mod_object_without_store_envelope_is_not_a_resource_endpoint() {
    let mut w = mod_working();
    w.room.objects.insert(
        ObjId(3),
        RoomObject::Mod(kk_schema::room::ModObj {
            id: 3,
            x: 10,
            y: 10,
            type_key: "stmod:crate".into(),
            data: json!({"x": 10, "y": 10}).to_string(),
        }),
    );
    assert!(
        inspect(&w, ObjId(3), "energy").is_err(),
        "无信封按未接线拒绝"
    );
}
