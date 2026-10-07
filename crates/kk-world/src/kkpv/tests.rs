use super::*;
use kk_core::Terrain;
use kk_schema::room::{
    BodyPart, ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj, ExtensionObj, FlagObj,
    GenericObj, Reservation, RuinObj, Sign, SourceObj, SpawnObj, SpawningJob, StorageObj, Store,
    TombstoneObj,
};
use serde_json::json;

fn key(name: &str) -> kk_mod_api::Key {
    kk_mod_api::Key::new(name).expect("测试键合法")
}

/// 挂载在 builtin:link 上的三权限扩展字段贡献。
fn ext_contribution() -> kk_mod_api::Contribution {
    use kk_mod_api::*;
    use std::collections::BTreeSet;
    let mount = |name: &str, schema: ValueSchema, visibility: Visibility| FieldMountDescriptor {
        target: TypeRef {
            key: key("builtin:link"),
            schema_version: 1,
        },
        field: FieldDescriptor {
            key: key(name),
            schema,
            default: FieldDefault::Missing,
            visibility,
            binding: key(&format!("{name}.impl")),
        },
    };
    let signed = || ValueSchema {
        kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 1000 }),
        nullable: false,
    };
    let text = || ValueSchema {
        kind: ValueKind::String { max_bytes: 64 },
        nullable: false,
    };
    Contribution {
        source: Source {
            namespace: "test".into(),
            api_version: 1,
            revision: "ext-fields".into(),
            code_digest: [0; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::from([
            key("test:charge.impl"),
            key("test:ownerNote.impl"),
            key("test:secret.impl"),
            key("test:ratio.impl"),
        ]),
        object_types: Vec::new(),
        field_mounts: vec![
            mount("test:charge", signed(), Visibility::Public),
            mount(
                "test:ratio",
                ValueSchema {
                    kind: ValueKind::Number(NumberDomain::Float {
                        min: 0.0,
                        max: 10.0,
                    }),
                    nullable: false,
                },
                Visibility::Public,
            ),
            mount("test:ownerNote", text(), Visibility::Owner),
            mount(
                "test:secret",
                ValueSchema {
                    kind: ValueKind::Bool,
                    nullable: false,
                },
                Visibility::ServerOnly,
            ),
        ],
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    }
}

fn catalog_fixture() -> kk_mod_api::extension::ExtensionCatalog {
    let plan = kk_mod_api::freeze_with_builtin(
        kk_mod_api::builtin::link(),
        vec![ext_contribution()],
        kk_mod_api::FreezeOptions::default(),
    )
    .expect("夹具计划可冻结");
    kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录提取成功")
}

fn store(energy: u32) -> Store {
    Store { energy, ghodium: 0 }
}

/// 覆盖全部 16 个内建对象族变体与两种查看者视野的对照夹具。
fn rich_world() -> World {
    let mut world = World::new(11);
    world.set_time_for_test(500).unwrap();
    let mut w1 = Terrain::plains();
    w1.normalize();
    world.register_room("W1N1", w1).unwrap();
    world.register_room("E2S2", Terrain::plains()).unwrap();
    let insert =
        |world: &mut World, room: &str, object| world.insert(room, object).expect("夹具对象可插入");
    insert(
        &mut world,
        "W1N1",
        RoomObject::Controller(ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level: 3,
            safe_mode: 0,
            safe_mode_available: 1,
            progress: 12_345,
            downgrade_time: Some(900),
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: true,
            reservation: Some(Reservation {
                user: "p2".into(),
                end_time: 700,
            }),
            sign: Some(Sign {
                user: "p1".into(),
                text: "签名".into(),
                time: 400,
                datetime: Some(1_700_000_000_000),
            }),
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Spawn(SpawnObj {
            id: 0,
            x: 10,
            y: 10,
            hits: 5000,
            off: false,
            name: "Spawn1".into(),
            owner: "p1".into(),
            store: store(220),
            spawning: Some(SpawningJob {
                name: "b1".into(),
                need_time: 3,
                spawn_time: 505,
                directions: vec![1, 5],
            }),
            notify_when_attacked: Some(true),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Extension(ExtensionObj {
            id: 0,
            x: 11,
            y: 10,
            hits: 1000,
            off: false,
            owner: "p1".into(),
            store: store(200),
            notify_when_attacked: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Storage(StorageObj {
            id: 0,
            x: 12,
            y: 10,
            hits: 10_000,
            owner: "p1".into(),
            store: store(900),
            notify_when_attacked: Some(false),
            store_extra: vec![kk_schema::room::ResourceStack {
                resource: "XUHO2".into(),
                amount: 40,
            }],
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Source(SourceObj {
            id: 0,
            x: 20,
            y: 20,
            energy: 2500,
            energy_capacity: 3000,
            next_regeneration_time: Some(600),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Creep(CreepObj {
            id: 0,
            x: 15,
            y: 15,
            fatigue: 2,
            age_time: 800,
            hits: 300,
            spawning: false,
            action_log: 0,
            name: "w1".into(),
            owner: "p1".into(),
            body: vec![
                BodyPart {
                    kind: 0,
                    boost: None,
                    hits: 100,
                },
                BodyPart {
                    kind: 4,
                    boost: Some("XUHO2".into()),
                    hits: 90,
                },
            ],
            store: store(50),
            store_extra: Vec::new(),
            say_message: Some("hi".into()),
            say_public: Some(true),
            notify_when_attacked: Some(false),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Creep(CreepObj {
            id: 0,
            x: 25,
            y: 25,
            fatigue: 0,
            age_time: 900,
            hits: 250,
            spawning: false,
            action_log: 0,
            name: "e1".into(),
            owner: "p2".into(),
            body: Vec::new(),
            store: store(0),
            store_extra: Vec::new(),
            say_message: None,
            say_public: None,
            notify_when_attacked: None,
        }),
    );
    let link = insert(
        &mut world,
        "W1N1",
        RoomObject::Generic(GenericObj {
            id: 0,
            x: 30,
            y: 30,
            data: r#"{"type":"structure","structureType":"link","owner":"p1","store":{"energy":120},"storeCapacityResource":{"energy":800},"cooldown":3,"hits":500,"hitsMax":500}"#.into(),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Tombstone(TombstoneObj {
            id: 0,
            x: 35,
            y: 35,
            death_time: 495,
            decay_time: 600,
            creep_id: Some(7),
            creep_name: Some("w0".into()),
            creep_ticks_to_live: 12,
            creep_body: vec![0, 4],
            creep_saying: Some("bye".into()),
            owner: Some("p1".into()),
            store: store(30),
            store_extra: Vec::new(),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Ruin(RuinObj {
            id: 0,
            x: 36,
            y: 35,
            structure_id: 9,
            structure_type: "spawn".into(),
            structure_hits_max: Some(5000),
            destroy_time: 498,
            decay_time: 598,
            owner: Some("p1".into()),
            store: store(300),
            store_extra: Vec::new(),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Energy(EnergyObj {
            id: 0,
            x: 40,
            y: 40,
            resource_type: 0,
            amount: 120,
            resource: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Energy(EnergyObj {
            id: 0,
            x: 41,
            y: 40,
            resource_type: 1,
            amount: 60,
            resource: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Flag(FlagObj {
            id: 0,
            x: 1,
            y: 1,
            name: "mine".into(),
            color: 1,
            secondary_color: 1,
            owner: Some("p1".into()),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Flag(FlagObj {
            id: 0,
            x: 2,
            y: 1,
            name: "theirs".into(),
            color: 2,
            secondary_color: 2,
            owner: Some("p2".into()),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
            id: 0,
            x: 6,
            y: 6,
            hits: 25_000,
            notify_when_attacked: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Road(kk_schema::room::RoadObj {
            id: 0,
            x: 7,
            y: 6,
            hits: 1_000,
            next_decay_time: 0,
            notify_when_attacked: Some(true),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Mineral(kk_schema::room::MineralObj {
            id: 0,
            x: 8,
            y: 6,
            mineral_type: "X".into(),
            mineral_amount: 45_000,
            density: 3,
            next_regeneration_time: Some(1_000),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Extractor(kk_schema::room::ExtractorObj {
            id: 0,
            x: 9,
            y: 6,
            hits: 500,
            owner: Some("p1".into()),
            cooldown: 7,
            notify_when_attacked: None,
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::Tombstone(TombstoneObj {
            id: 0,
            x: 34,
            y: 35,
            death_time: 493,
            decay_time: 599,
            creep_id: None,
            creep_name: None,
            creep_ticks_to_live: 0,
            creep_body: Vec::new(),
            creep_saying: None,
            owner: None,
            store: store(0),
            store_extra: Vec::new(),
        }),
    );
    insert(
        &mut world,
        "W1N1",
        RoomObject::ConstructionSite(ConstructionSiteObj {
            id: 0,
            x: 3,
            y: 1,
            structure_type: "extension".into(),
            progress: 100,
            progress_total: 300,
            owner: Some("p1".into()),
            name: None,
        }),
    );
    insert(
        &mut world,
        "E2S2",
        RoomObject::Spawn(SpawnObj {
            id: 0,
            x: 10,
            y: 10,
            hits: 5000,
            off: false,
            name: "SpawnE".into(),
            owner: "p2".into(),
            store: store(300),
            spawning: None,
            notify_when_attacked: None,
        }),
    );
    insert(
        &mut world,
        "E2S2",
        RoomObject::ConstructionSite(ConstructionSiteObj {
            id: 0,
            x: 2,
            y: 2,
            structure_type: "road".into(),
            progress: 0,
            progress_total: 300,
            owner: Some("p1".into()),
            name: None,
        }),
    );
    insert(
        &mut world,
        "E2S2",
        RoomObject::Flag(FlagObj {
            id: 0,
            x: 1,
            y: 2,
            name: "far".into(),
            color: 3,
            secondary_color: 3,
            owner: Some("p1".into()),
        }),
    );
    // 扩展值只写入权威状态；投影经目录裁切。
    let w1_state = world.rooms.get_mut("W1N1").expect("房间存在");
    w1_state.extensions.insert(
        link,
        std::collections::BTreeMap::from([
            ("test:charge".to_string(), json!(55)),
            ("test:ownerNote".to_string(), json!("仅主人")),
            ("test:secret".to_string(), json!(true)),
            ("test:ratio".to_string(), json!(1.5)),
        ]),
    );
    world
}

fn number_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    (a.as_i64(), a.as_u64(), a.as_f64()) == (b.as_i64(), b.as_u64(), b.as_f64())
}

fn values_equal(a: &Value, b: &Value, context: &str) {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            assert!(number_equal(a, b), "{context}: 数值 {a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{context}: 数组长度");
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                values_equal(x, y, &format!("{context}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len(), "{context}: 键数");
            for (k, v) in a {
                let other = b.get(k).unwrap_or_else(|| panic!("{context}: 缺键 {k}"));
                values_equal(v, other, &format!("{context}.{k}"));
            }
        }
        _ => assert_eq!(a, b, "{context}"),
    }
}

fn compare_worlds(
    world: &World,
    catalog: &kk_mod_api::extension::ExtensionCatalog,
    viewer: &str,
) -> usize {
    let plan_hash = [7u8; 32];
    let audience = [9u8; 32];
    let reference = world.world_json_for_viewer_with_extensions(viewer, 1000, catalog, None);
    let blocks = world
        .kkpv_room_blocks_for_viewer(viewer, catalog, None, plan_hash, audience)
        .expect("块输入构造成功");
    let mut total_block_bytes = 0usize;
    let reference_rooms: Vec<&String> = reference["rooms"]
        .as_object()
        .expect("rooms 是对象")
        .keys()
        .collect();
    let block_rooms: Vec<&String> = blocks.iter().map(|b| &b.room_name).collect();
    assert_eq!(reference_rooms, block_rooms, "{viewer}: 每注册房间一块");
    let mut objects_by_room: std::collections::BTreeMap<&str, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for object in reference["objects"].as_array().expect("objects 是数组") {
        objects_by_room
            .entry(object["room"].as_str().expect("room 键"))
            .or_default()
            .push(object);
    }
    for block in &blocks {
        let bytes = kk_schema::kkpv::encode(block, &kk_schema::kkpv::KkpvLimits::default())
            .expect("块编码成功");
        total_block_bytes += bytes.len();
        let view =
            KkpvView::decode(&bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("块解码成功");
        assert!(view.is_private(), "{viewer}: 桥产出私有块");
        assert_eq!(view.tick(), world.time);
        assert_eq!(view.generation(), world.generation());
        assert_eq!(view.plan_hash(), plan_hash);
        assert_eq!(view.audience_hash(), audience);
        let room = view.room_name();
        let reference_objects = objects_by_room.remove(room).unwrap_or_default();
        assert_eq!(
            view.object_count(),
            reference_objects.len(),
            "{viewer}/{room}: 对象数"
        );
        for object in reference_objects {
            let id = object["id"].as_str().expect("id 键");
            let oid = ObjId::parse(id).expect("id 形态");
            let index = view
                .object_index_by_id(oid.0)
                .unwrap_or_else(|| panic!("{viewer}/{room}: 块缺对象 {id}"));
            let materialized = record_json_from_view(&view, index).expect("物化成功");
            values_equal(object, &materialized, &format!("{viewer}/{room}/{id}"));
            assert_eq!(
                materialized["id"],
                json!(id),
                "{viewer}/{room}: id 由目录还原"
            );
            assert!(
                materialized.get("test:secret").is_none(),
                "{viewer}/{room}: ServerOnly 不进玩家数据"
            );
        }
        // 地形由公开束承载（见 public_terrain_blocks 对照测试），私有
        // 块不再携带地形段。
        assert!(view.terrain().is_none(), "{viewer}/{room}: 私有块不带地形");
    }
    assert!(
        objects_by_room.values().all(|v| v.is_empty()),
        "{viewer}: JSON 参考的对象房都有对应块"
    );
    total_block_bytes
}

/// 逐字段、缺失、null 与全部对象族的 KKPV/JSON 参考对照。
#[test]
fn kkpv_blocks_match_json_reference_per_field() {
    let world = rich_world();
    let plan = kk_mod_api::freeze_with_builtin(
        kk_mod_api::builtin::link(),
        vec![ext_contribution()],
        kk_mod_api::FreezeOptions::default(),
    )
    .expect("夹具计划可冻结");
    let catalog = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录提取成功");
    let block_bytes_p1 = compare_worlds(&world, &catalog, "p1");
    let block_bytes_p2 = compare_worlds(&world, &catalog, "p2");
    // Owner 权限对两名查看者分别裁切：p1 见 ownerNote，p2 不见。
    let for_p1 = world.world_json_for_viewer_with_extensions("p1", 0, &catalog, None);
    let link_p1 = for_p1["objects"]
        .as_array()
        .expect("数组")
        .iter()
        .find(|o| o["structureType"] == json!("link"))
        .expect("link 存在");
    assert_eq!(link_p1["ext"]["test:ownerNote"], json!("仅主人"));
    assert_eq!(link_p1["ext"]["test:charge"], json!(55));
    assert_eq!(link_p1["ext"]["test:ratio"], json!(1.5));
    assert!(link_p1["ext"].get("test:secret").is_none());
    let for_p2 = world.world_json_for_viewer_with_extensions("p2", 0, &catalog, None);
    let link_p2 = for_p2["objects"]
        .as_array()
        .expect("数组")
        .iter()
        .find(|o| o["structureType"] == json!("link"))
        .expect("p2 视野内可见敌方 link");
    assert!(link_p2["ext"].get("test:ownerNote").is_none());
    assert_eq!(link_p2["ext"]["test:charge"], json!(55));
    assert_eq!(link_p2["ext"]["test:ratio"], json!(1.5));
    // 记录对照测试里 p2 路径同样校验了 ext 裁切后的逐字段一致。
    let json_bytes = serde_json::to_string(&for_p1).expect("序列化").len();
    eprintln!(
        "KKPV 对照尺寸：p1 块合计 {block_bytes_p1} 字节，p2 块合计 {block_bytes_p2} 字节，JSON 参考 {json_bytes} 字节"
    );
}

/// KKPV 信封与 JSON 参考的房间级内容同形：去掉 objects 与逐房
/// terrain 后逐键相等（对象记录与地形分别由私有块和公开块承载），
/// 两名查看者的可见房集合与逐房裁切一致（能量/事件只在可见房出现）。
#[test]
fn kkpv_envelope_matches_json_reference_minus_objects() {
    let world = rich_world();
    for viewer in ["p1", "p2"] {
        let mut reference = world.world_json_for_viewer(viewer, 1000);
        let reference_map = reference.as_object_mut().expect("参考是对象");
        reference_map.remove("objects");
        if let Some(rooms) = reference_map
            .get_mut("rooms")
            .and_then(Value::as_object_mut)
        {
            for entry in rooms.values_mut() {
                if let Some(room) = entry.as_object_mut() {
                    room.remove("terrain");
                }
            }
        }
        let envelope = world.kkpv_envelope_for_viewer(viewer, 1000);
        assert_eq!(
            reference, envelope,
            "{viewer}: 信封与参考（去 objects 与地形）同形"
        );
    }
}

/// 公开地形块覆盖全部注册房间（含无对象房间），字节与 JSON 参考的
/// base64 地形载荷同源，且不随查看者或受众参数中查看者相关的部分
/// 变化——公开通道对全部玩家是同一份字节。
#[test]
fn kkpv_public_terrain_blocks_match_json_reference() {
    let world = rich_world();
    let reference = world.world_json_for_viewer("p1", 1000);
    let blocks = world.kkpv_public_terrain_blocks([1; 32], [2; 32]);
    assert_eq!(
        blocks.len(),
        world.rooms.len(),
        "公开块数 = 全部注册房间数（不按查看者裁切）"
    );
    for block in &blocks {
        assert!(!block.private, "公开块 flags 无私有位");
        assert!(block.objects.is_empty(), "公开地形块不带对象");
        assert_eq!(block.tick, world.time);
        assert_eq!(block.generation, world.generation());
        let payload = reference["rooms"][block.room_name.as_str()]["terrain"]["payload"]
            .as_str()
            .expect("JSON 参考的每注册房间都有地形载荷");
        let encoded = crate::projection::base64_encode(&block.terrain.clone().expect("地形"));
        assert_eq!(
            encoded,
            payload,
            "{room}: 公开块地形与 JSON 参考同源",
            room = block.room_name
        );
    }
    // 两次调用产出逐字节相同的公开束（确定编码，可跨查看者共享）。
    let terrain_of = |blocks: &[KkpvBlockInput]| -> Vec<(String, Vec<u8>)> {
        blocks
            .iter()
            .map(|b| {
                (
                    b.room_name.clone(),
                    b.terrain.clone().expect("公开块带地形"),
                )
            })
            .collect()
    };
    assert_eq!(
        terrain_of(&blocks),
        terrain_of(&world.kkpv_public_terrain_blocks([1; 32], [2; 32]))
    );
}

/// Generic 原生数据自带 ext 键时，目录扩展按 JSON 参考的覆盖语义
/// 合并，不因键冲突失败。
#[test]
fn generic_native_ext_key_is_overwritten_by_catalog() {
    let mut world = World::new(3);
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain).unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 5000,
                off: false,
                name: "S".into(),
                owner: "p1".into(),
                store: Store {
                    energy: 0,
                    ghodium: 0,
                },
                spawning: None,
                notify_when_attacked: None,
            }),
        )
        .expect("插入");
    world
        .insert(
            "W1N1",
            RoomObject::Generic(GenericObj {
                id: 0,
                x: 1,
                y: 1,
                data: r#"{"type":"structure","structureType":"link","ext":{"原生":1}}"#.into(),
            }),
        )
        .expect("插入");
    let link_id = world
        .rooms
        .get_mut("W1N1")
        .expect("房间")
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => Some(g.id),
            _ => None,
        })
        .expect("link id");
    world
        .rooms
        .get_mut("W1N1")
        .expect("房间")
        .extensions
        .insert(
            crate::world::ObjId(link_id),
            std::collections::BTreeMap::from([
                ("test:charge".to_string(), json!(55)),
                ("test:ratio".to_string(), json!(1.5)),
            ]),
        );
    let _ = link_id;
    let reference = world.world_json_for_viewer_with_extensions("p1", 0, &catalog_fixture(), None);
    let link_json = reference["objects"]
        .as_array()
        .expect("数组")
        .iter()
        .find(|o| o["structureType"] == json!("link"))
        .expect("link");
    assert_eq!(
        link_json["ext"],
        json!({"test:charge": 55, "test:ratio": 1.5}),
        "JSON 参考的原生 ext 被目录 ext 覆盖"
    );
    let blocks = world
        .kkpv_room_blocks_for_viewer("p1", &catalog_fixture(), None, [0; 32], [0; 32])
        .expect("ext 键冲突按覆盖语义合并，不是错误");
    let bytes =
        kk_schema::kkpv::encode(&blocks[0], &kk_schema::kkpv::KkpvLimits::default()).expect("编码");
    let view = KkpvView::decode(&bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("解码");
    let index = view
        .object_index_by_id(2)
        .expect("link 在块内（第二个插入对象 id=2）");
    assert_eq!(
        view.field(index, "ext"),
        Ok(Some(KkpvValue::Object(vec![
            ("test:charge".to_string(), KkpvValue::Int(55)),
            ("test:ratio".to_string(), KkpvValue::Float(1.5)),
        ])))
    );
}

/// 真实形状的私有块可与同版本公开块合并，overlay 对象可区分来源。
#[test]
fn world_block_merges_with_public_overlay() {
    let world = rich_world();
    let plan = kk_mod_api::freeze_with_builtin(
        kk_mod_api::builtin::link(),
        vec![ext_contribution()],
        kk_mod_api::FreezeOptions::default(),
    )
    .expect("冻结");
    let catalog = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录");
    let mut blocks = world
        .kkpv_room_blocks_for_viewer("p1", &catalog, None, [3u8; 32], [4u8; 32])
        .expect("块");
    let private_input = blocks.remove(0);
    let private_bytes =
        kk_schema::kkpv::encode(&private_input, &kk_schema::kkpv::KkpvLimits::default())
            .expect("编码");
    let public_input = world
        .kkpv_public_terrain_blocks([3u8; 32], [4u8; 32])
        .into_iter()
        .find(|b| b.room_name == private_input.room_name)
        .expect("公开地形块覆盖同一房间");
    let public_bytes =
        kk_schema::kkpv::encode(&public_input, &kk_schema::kkpv::KkpvLimits::default())
            .expect("编码");
    let private_view =
        KkpvView::decode(&private_bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("解码");
    let public_view =
        KkpvView::decode(&public_bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("解码");
    let merged = kk_schema::kkpv::merge(&public_view, &private_view).expect("合并");
    assert_eq!(merged.object_count(), private_view.object_count());
    assert!(merged.terrain().is_some(), "地形取公开块");
    for i in 0..merged.object_count() {
        assert!(merged.from_overlay(i), "对象全部来自 overlay");
    }
}
