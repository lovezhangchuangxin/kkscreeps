//! G08.a 建筑字段写面：原生键只读、自定义键整集校验与合并语义。
use super::*;
use crate::settle::{Engine, Plane, TickState};
use kk_world::{ObjId, RoomState};
use std::collections::{BTreeMap, BTreeSet};

/// 冻结一个带字段的 bl:gauge 建筑，返回其贡献平面组件。
fn gauge_plan() -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    gauge_plan_with_fields(vec![
        field(
            "heat",
            1000,
            kk_mod_api::FieldDefault::Value(serde_json::json!(7)),
        ),
        field("cycles", 10_000, kk_mod_api::FieldDefault::Missing),
    ])
}

fn field(name: &str, max: u64, default: kk_mod_api::FieldDefault) -> kk_mod_api::FieldDescriptor {
    kk_mod_api::FieldDescriptor {
        key: kk_mod_api::Key::new(format!("bl:{name}")).unwrap(),
        schema: kk_mod_api::ValueSchema {
            kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned { min: 0, max }),
            nullable: false,
        },
        default,
        visibility: kk_mod_api::Visibility::Public,
        binding: kk_mod_api::Key::new(format!("bl:gauge.field.{name}")).unwrap(),
    }
}

fn gauge_plan_with_fields(
    fields: Vec<kk_mod_api::FieldDescriptor>,
) -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![gauge_contribution_with_fields(fields)],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    )
}

fn gauge_contribution_with_fields(
    fields: Vec<kk_mod_api::FieldDescriptor>,
) -> kk_mod_api::Contribution {
    let mut bindings = BTreeSet::new();
    for f in &fields {
        bindings.insert(f.binding.clone());
    }
    kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "bl".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings,
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: vec![kk_mod_api::buildings::BuildingDescriptor {
            key: kk_mod_api::Key::new("bl:gauge").unwrap(),
            player_class: "Gauge".into(),
            cost: vec![("energy".into(), 100)],
            hits_max: 500,
            pathing: kk_mod_api::buildings::BuildingPathing::Obstacle,
            store_capacity: Some(200),
            production: None,
            fields,
            lifecycle_slots: BTreeSet::new(),
        }],
    }
}

fn working_with_gauge() -> (Working, ObjId) {
    let plan = gauge_plan();
    let engine = Engine::default();
    let room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut w = Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            world_span: Plane::UNWRAPPED_SPAN,
            buildings: std::rc::Rc::new(
                kk_mod_api::buildings::BuildingCatalog::from_plan(&plan).unwrap(),
            ),
            links: engine.links,
            stores: engine.stores,
            combat: engine.combat,
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            static_passability: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan,
        },
    );
    let id = ObjId(2);
    let data = serde_json::json!({
        "type":"structure","structureType":"bl:gauge","owner":"p1",
        "hits":500,"hitsMax":500,"storeCapacity":200,"store":{"energy":0},
        "bl:heat":7,
    });
    w.room.objects.insert(
        id,
        RoomObject::Generic(kk_schema::room::GenericObj {
            id: id.0,
            x: 10,
            y: 10,
            data: data.to_string(),
        }),
    );
    (w, id)
}

fn record(w: &Working, id: ObjId) -> serde_json::Value {
    match w.room.objects.get(&id) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("非 Generic 结构：{other:?}"),
    }
}

#[test]
fn write_face_rejects_native_key_drift_and_enforces_custom_schema() {
    let (mut w, id) = working_with_gauge();
    let base = record(&w, id);

    // 原生键（无冒号）漂移即拒：耐久/库存/归属不经本写面。
    let mut native = base.clone();
    native["hits"] = serde_json::json!(499);
    native["bl:heat"] = serde_json::json!(8);
    let err = w.update_building_fields(id, &native).unwrap_err();
    assert!(err.contains("原生记录键"), "{err}");

    // 自定义键整集校验：缺失 cycles 拒（Missing 默认的键也须显式提交）；
    // 目录校验报去命名空间短名。
    let mut missing = base.clone();
    missing["bl:heat"] = serde_json::json!(8);
    let err = w.update_building_fields(id, &missing).unwrap_err();
    assert!(
        err.contains("缺少字段 cycles"),
        "整集校验点名缺失字段：{err}"
    );

    // 域外值拒（整集提交，字段按键序先过 cycles 的在场校验）。
    let mut out = base.clone();
    out["bl:heat"] = serde_json::json!(1001);
    out["bl:cycles"] = serde_json::json!(1);
    let err = w.update_building_fields(id, &out).unwrap_err();
    assert!(err.contains("heat"), "{err}");

    // 合法写：自定义键替换、原生键保持当前候选值。
    let mut valid = base.clone();
    valid["bl:heat"] = serde_json::json!(42);
    valid["bl:cycles"] = serde_json::json!(3);
    w.update_building_fields(id, &valid).unwrap();
    let merged = record(&w, id);
    assert_eq!(merged["bl:heat"], serde_json::json!(42));
    assert_eq!(merged["bl:cycles"], serde_json::json!(3));
    assert_eq!(merged["hits"], serde_json::json!(500), "原生键保持");
    assert_eq!(merged["store"], serde_json::json!({"energy": 0}));
}

#[test]
fn write_face_rejects_undeclared_and_fieldless_buildings() {
    let (mut w, id) = working_with_gauge();
    let base = record(&w, id);

    // proposed 新增的无冒号键（拼错的「原生样」键）点名拒绝——
    // 静默丢弃会掩盖作者侧 bug（看似生效实则没写）。
    let mut fake_native = base.clone();
    fake_native["disabled"] = serde_json::json!(true);
    let err = w.update_building_fields(id, &fake_native).unwrap_err();
    assert!(
        err.contains("disabled") && err.contains("不是原生记录键"),
        "{err}"
    );

    // 未声明键（命名空间外/拼错）在进入目录校验前点名拒绝。
    let mut alien = base.clone();
    alien["other:field"] = serde_json::json!(1);
    let err = w.update_building_fields(id, &alien).unwrap_err();
    assert!(
        err.contains("other:field") && err.contains("未在建筑中声明"),
        "{err}"
    );

    // 非建筑目标拒（link 不是建筑目录条目）。
    let link_id = ObjId(3);
    w.room.objects.insert(
        link_id,
        RoomObject::Generic(kk_schema::room::GenericObj {
            id: 3,
            x: 11,
            y: 10,
            data: serde_json::json!({
                "type":"structure","structureType":"link","owner":"p1",
                "hits":100,"hitsMax":100,"cooldown":0,
            })
            .to_string(),
        }),
    );
    let err = w
        .update_building_fields(link_id, &serde_json::json!({}))
        .unwrap_err();
    assert!(err.contains("不在建筑目录"), "{err}");

    // 无字段建筑只读。
    let plan = bare_plan();
    w.plane.buildings =
        std::rc::Rc::new(kk_mod_api::buildings::BuildingCatalog::from_plan(&plan).unwrap());
    let bare = kk_mod_api::Key::new("bl:bare").unwrap();
    let bare_id = ObjId(4);
    w.room.objects.insert(
        bare_id,
        RoomObject::Generic(kk_schema::room::GenericObj {
            id: 4,
            x: 12,
            y: 10,
            data: serde_json::json!({
                "type":"structure","structureType": bare.as_str(), "owner":"p1",
                "hits":500,"hitsMax":500,
            })
            .to_string(),
        }),
    );
    let err = w
        .update_building_fields(bare_id, &serde_json::json!({"type":"structure"}))
        .unwrap_err();
    assert!(err.contains("只读"), "{err}");
}

/// 无字段建筑（同命名空间第二条目）的冻结计划。
fn bare_plan() -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    let contribution = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "bl".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: vec![kk_mod_api::buildings::BuildingDescriptor {
            key: kk_mod_api::Key::new("bl:bare").unwrap(),
            player_class: "Bare".into(),
            cost: vec![("energy".into(), 100)],
            hits_max: 500,
            pathing: kk_mod_api::buildings::BuildingPathing::Obstacle,
            store_capacity: None,
            production: None,
            fields: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
        }],
    };
    std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    )
}

/// 重复字段键在 freeze 权威层拒绝（目录层门保留为纵深防御）：
/// 字段写面按目录路由自定义键，重复键会让校验载荷与存储键的映射
/// 失去单射。kk-mod-api 的 tests/freeze 另有完整反例。
#[test]
fn duplicate_field_keys_are_rejected_at_freeze() {
    let error = kk_mod_api::freeze_with_builtin(
        kk_mod_api::builtin::link(),
        vec![gauge_contribution_with_fields(vec![
            field("heat", 1000, kk_mod_api::FieldDefault::Missing),
            field("heat", 2000, kk_mod_api::FieldDefault::Missing),
        ])],
        kk_mod_api::FreezeOptions::default(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("建筑字段重复声明 bl:heat"), "{error}");
}

#[test]
fn building_record_schema_is_enforced_at_edit_restore_and_publication() {
    let mut engine = Engine::default();
    engine
        .install_plan(gauge_plan(), BTreeMap::new(), BTreeMap::new())
        .unwrap();
    engine
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let object = RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x: 10,
        y: 10,
        data: serde_json::json!({"type":"structure", "structureType":"bl:gauge", "bl:heat":7})
            .to_string(),
    });
    let id = engine.insert_object("W1N1", object).unwrap();
    let before = serde_json::to_value(engine.checkpoint().unwrap()).unwrap();
    for invalid in [serde_json::json!("bad"), serde_json::json!(1001)] {
        let edit = |room: &mut RoomState| {
            let RoomObject::Generic(g) = room.objects.get_mut(&id).unwrap() else {
                panic!()
            };
            let mut record: Value = serde_json::from_str(&g.data).unwrap();
            record["bl:heat"] = invalid.clone();
            g.data = record.to_string();
        };
        assert!(
            engine
                .edit_room("W1N1", |room| {
                    let RoomObject::Generic(g) = room.objects.get_mut(&id).unwrap() else {
                        panic!()
                    };
                    let mut record: Value = serde_json::from_str(&g.data).unwrap();
                    record["bl:heat"] = invalid.clone();
                    g.data = record.to_string();
                    Ok(())
                })
                .is_err()
        );
        // 构造绕过公共写面的候选，发布门必须独立拒绝。
        let mut txn = engine
            .prepare_tick(
                crate::intent::IntentBatch::default(),
                crate::TickInput {
                    now_ms: 0,
                    warmup: false,
                },
            )
            .unwrap();
        txn.rooms = engine
            .world
            .rooms()
            .iter()
            .map(|(name, room)| (name.clone(), room.clone()))
            .collect();
        edit(&mut txn.rooms[0].1);
        assert!(engine.validate_stage(&mut txn).is_err());
        let mut world = engine.world.clone();
        world
            .edit_room("W1N1", |room| {
                let RoomObject::Generic(g) = room.objects.get_mut(&id).unwrap() else {
                    panic!()
                };
                let mut record: Value = serde_json::from_str(&g.data).unwrap();
                record["bl:heat"] = invalid.clone();
                g.data = record.to_string();
                Ok(())
            })
            .unwrap();
        let mut saved = engine.checkpoint().unwrap();
        saved.world = world.checkpoint();
        assert!(engine.restore_checkpoint(saved).is_err());
        assert_eq!(
            serde_json::to_value(engine.checkpoint().unwrap()).unwrap(),
            before
        );
    }
}

/// 完整回写直接使用命名空间键，递归规范化不能触及输入或失败候选。
#[test]
fn qualified_fields_normalize_nested_values_only_after_complete_validation() {
    use kk_mod_api::{NumberDomain, ValueKind, ValueSchema};
    let float = ValueSchema {
        kind: ValueKind::Number(NumberDomain::Float {
            min: 0.0,
            max: 100.0,
        }),
        nullable: true,
    };
    let mut heat = field("heat", 1000, kk_mod_api::FieldDefault::Missing);
    heat.schema = ValueSchema {
        kind: ValueKind::Record {
            fields: BTreeMap::from([
                (
                    "samples".into(),
                    ValueSchema {
                        kind: ValueKind::Array {
                            element: Box::new(float.clone()),
                            max_len: 10,
                        },
                        nullable: false,
                    },
                ),
                (
                    "ratios".into(),
                    ValueSchema {
                        kind: ValueKind::Map {
                            value: Box::new(float),
                            max_len: 10,
                            max_key_bytes: 32,
                        },
                        nullable: false,
                    },
                ),
            ]),
            optional: BTreeSet::new(),
        },
        nullable: false,
    };
    let (mut w, id) = working_with_gauge();
    w.plane.buildings = std::rc::Rc::new(
        kk_mod_api::buildings::BuildingCatalog::from_plan(&gauge_plan_with_fields(vec![
            heat,
            field("cycles", 10000, kk_mod_api::FieldDefault::Missing),
        ]))
        .unwrap(),
    );
    let before = record(&w, id);
    let mut proposed = before.clone();
    proposed["bl:heat"] = serde_json::json!({"samples": [2, null], "ratios": {"x": 3}});
    assert!(
        w.update_building_fields(id, &proposed)
            .unwrap_err()
            .contains("缺少字段 cycles")
    );
    assert_eq!(record(&w, id), before);
    proposed["bl:cycles"] = serde_json::json!(1);
    let input = proposed.clone();
    w.update_building_fields(id, &proposed).unwrap();
    let actual = record(&w, id);
    assert!(
        actual["bl:heat"]["samples"][0]
            .as_number()
            .unwrap()
            .is_f64()
    );
    assert!(
        actual["bl:heat"]["ratios"]["x"]
            .as_number()
            .unwrap()
            .is_f64()
    );
    assert!(actual["bl:heat"]["samples"][1].is_null());
    assert_eq!(actual["hits"], before["hits"]);
    assert_eq!(actual["store"], before["store"]);
    assert!(actual.get("heat").is_none());
    assert_eq!(proposed, input);
}
