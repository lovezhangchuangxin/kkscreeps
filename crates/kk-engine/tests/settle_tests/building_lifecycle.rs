//! G08.a 建筑自定义字段与生命周期：完工默认值、生产×tick 同访顺序、
//! 字段写面校验与装配门。表示保持 Generic，字段经建筑目录校验。
use super::*;
use serde_json::json;

/// 带 u32 计数字段（默认 0）与可选 tick 槽的仪表建筑。
fn gauge(tick: bool) -> kk_mod_api::buildings::BuildingDescriptor {
    let key = kk_mod_api::Key::new("bl:gauge").unwrap();
    let mut lifecycle = BTreeSet::new();
    if tick {
        lifecycle.insert(kk_mod_api::Key::new("bl:gauge.tick").unwrap());
    }
    kk_mod_api::buildings::BuildingDescriptor {
        key,
        player_class: "Gauge".into(),
        cost: vec![("energy".into(), 100)],
        hits_max: 500,
        pathing: kk_mod_api::buildings::BuildingPathing::Obstacle,
        store_capacity: Some(200),
        production: if tick {
            Some(kk_mod_api::buildings::ProductionSpec {
                interval: 1,
                inputs: Vec::new(),
                outputs: vec![("energy".into(), 5)],
            })
        } else {
            None
        },
        fields: vec![
            kk_mod_api::FieldDescriptor {
                key: kk_mod_api::Key::new("bl:heat").unwrap(),
                schema: kk_mod_api::ValueSchema {
                    kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                        min: 0,
                        max: 1000,
                    }),
                    nullable: false,
                },
                default: kk_mod_api::FieldDefault::Value(json!(7)),
                visibility: kk_mod_api::Visibility::Public,
                binding: kk_mod_api::Key::new("bl:gauge.field.heat").unwrap(),
            },
            kk_mod_api::FieldDescriptor {
                key: kk_mod_api::Key::new("bl:label").unwrap(),
                schema: kk_mod_api::ValueSchema {
                    kind: kk_mod_api::ValueKind::String { max_bytes: 8 },
                    nullable: true,
                },
                // Missing 默认：完工不落键。
                default: kk_mod_api::FieldDefault::Missing,
                visibility: kk_mod_api::Visibility::Public,
                binding: kk_mod_api::Key::new("bl:gauge.field.label").unwrap(),
            },
        ],
        lifecycle_slots: lifecycle,
    }
}

fn gauge_contribution(tick: bool) -> kk_mod_api::Contribution {
    let building = gauge(tick);
    let mut bindings = BTreeSet::new();
    for field in &building.fields {
        bindings.insert(field.binding.clone());
    }
    let mut slots = Vec::new();
    if let Some(slot_key) = building.lifecycle_slots.iter().next() {
        let process = kk_mod_api::Key::new(format!("{slot_key}.process")).unwrap();
        bindings.insert(process.clone());
        slots.push(kk_mod_api::SlotDescriptor {
            key: slot_key.clone(),
            stage: kk_mod_api::Stage::ObjectTick,
            native_binding: process,
        });
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
        slots,
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: vec![building],
    }
}

#[test]
fn completion_writes_declared_field_defaults() {
    // Value 默认落键、Missing 默认不落键；原生键（store/storeCapacity 等）
    // 与既有完工口径一致。
    let plan = std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![gauge_contribution(false)],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    let mut f = Fixture::new(1, terrain_with(&[]));
    let plane = kk_engine::settle::Plane {
        links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
        stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
        combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
        resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
        buildings: std::rc::Rc::new(
            kk_mod_api::buildings::BuildingCatalog::from_plan(&plan).unwrap(),
        ),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    };
    // 直接驱动完工（公式与家族测试同口径：进度推进由 WORK 效果结算）。
    let worker = f.creep("w", "p1", 11, 10, &[BodyKind::Work], 200);
    let site = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 10,
                y: 10,
                structure_type: "bl:gauge".into(),
                progress: 95,
                progress_total: 100,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", worker, Action::Build, IntentData::target(site));
    f.settle_with_plane(b, plane.clone());
    let built = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => serde_json::from_str::<Value>(&g.data)
                .ok()
                .filter(|d| d["structureType"] == "bl:gauge")
                .map(|_| g.clone()),
            _ => None,
        })
        .expect("完工落成 Generic 结构");
    let data: Value = serde_json::from_str(&built.data).unwrap();
    assert_eq!(data["bl:heat"], json!(7), "Value 默认落键");
    assert!(data.get("bl:label").is_none(), "Missing 默认不落键");
    assert_eq!(data["hits"], json!(500), "原生耐久来自配方");
    assert_eq!(data["storeCapacity"], json!(200), "容量来自配方");
    let _ = plane;
}

#[test]
fn building_ticks_after_production_within_object_tick() {
    // 生产与 tick 同访：先结算生产周期，后派发生命周期槽——处理器读到
    // 的是本 tick 生产后的库存。无生产建筑（tick-only）也进入分发。
    for tick_only in [false, true] {
        let contribution = if tick_only {
            let mut c = gauge_contribution(true);
            c.buildings[0].production = None;
            c
        } else {
            gauge_contribution(true)
        };
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![contribution],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        let mut e = kk_engine::Engine::new(kk_world::World::new(3));
        let seen_store = std::rc::Rc::new(std::cell::RefCell::new(Vec::<i64>::new()));
        let seen_target = seen_store.clone();
        let observer = move |ctx: &kk_engine::settle::RoomCtx,
                             id: kk_world::ObjId,
                             _args: &Value|
              -> Result<(), String> {
            let w = ctx.borrow();
            let data: Value = match w.room.objects.get(&id) {
                Some(RoomObject::Generic(g)) => {
                    serde_json::from_str(&g.data).map_err(|e| e.to_string())?
                }
                _ => return Ok(()),
            };
            seen_target.borrow_mut().push(
                data.pointer("/store/energy")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
            );
            Ok(())
        };
        let processes = std::collections::BTreeMap::from([(
            kk_mod_api::Key::new("bl:gauge.tick.process").unwrap(),
            Box::new(observer) as kk_engine::settle::contrib::ExtSimple,
        )]);
        e.install_plan_with_ext(
            plan,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            processes,
        )
        .unwrap();
        e.world.register_room("W1N1", kk_core::Terrain::plains());
        e.world
            .insert(
                "W1N1",
                RoomObject::Generic(kk_schema::room::GenericObj {
                    id: 0,
                    x: 10,
                    y: 10,
                    data: json!({
                        "type":"structure","structureType":"bl:gauge","owner":"p1",
                        "hits":500,"hitsMax":500,"storeCapacity":200,
                        "store":{"energy":0},"bl:heat":7,
                    })
                    .to_string(),
                }),
            )
            .unwrap();
        e.tick_with_intents(IntentBatch::default()).unwrap();
        e.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(
            *seen_store.borrow(),
            if tick_only { vec![0, 0] } else { vec![5, 10] },
            "tick_only={tick_only}：先产后 tick"
        );
    }
}

#[test]
fn building_lifecycle_gate_rejects_wrong_shape() {
    // 形状门分两层：freeze 权威层（槽键派生/阶段/未声明槽——Rust 直构
    // 贡献与 JS 装载同门）与 install 纵深层（槽中心须自身 process——
    // freeze 不检查绑定中心，装配门拦住）；拒绝不改变已装配计划。
    for (case, needle) in [
        (0, "须为 bl:gauge.tick"),
        (1, "阶段为 ObjectTick"),
        (2, "未在 slots 中声明"),
        (3, "自身 process"),
    ] {
        let mut c = gauge_contribution(true);
        match case {
            0 => {
                let wrong = kk_mod_api::Key::new("bl:gauge.preTick").unwrap();
                c.slots[0].key = wrong.clone();
                c.buildings[0].lifecycle_slots = BTreeSet::from([wrong]);
            }
            1 => c.slots[0].stage = kk_mod_api::Stage::Market,
            2 => {
                let ghost = kk_mod_api::Key::new("bl:gauge.ghost.tick").unwrap();
                c.buildings[0].lifecycle_slots = BTreeSet::from([ghost]);
            }
            _ => {
                let wrong = kk_mod_api::Key::new("bl:gauge.other.process").unwrap();
                c.slots[0].native_binding = wrong.clone();
                c.bindings.insert(wrong);
            }
        }
        let mut e = kk_engine::Engine::new(kk_world::World::new(5));
        let hash = e.plan_hash();
        if case <= 2 {
            let error = kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![c],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(needle), "{error}");
            continue;
        }
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![c],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        let error = e
            .install_plan_with_ext(
                plan,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap_err();
        assert!(error.contains(needle), "{error}");
        assert_eq!(e.plan_hash(), hash, "拒绝不改变已装配计划");
    }
}
