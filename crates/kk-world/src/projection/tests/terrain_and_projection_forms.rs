//! 检查地形缓存更新、资源与控制器投影，以及 Generic 对象查询。

use super::*;

#[test]
fn storage_named_resources_reach_all_projections() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    let storage = normalize(
        "structure",
        &json!({
            "pos": [10, 10], "structureType": "storage", "owner": "p1",
            "store": { "energy": 0, "G": 5, "U": 12, "XUHO2": 6, "Z": 0 },
        }),
        &PlaceCtx {
            buildings: Default::default(),
            links: crate::place::builtin_link_rules(),
            stores: crate::place::builtin_store_rules(),

            time: 0,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let id = world.insert("W1N1", storage).unwrap();
    let expected = json!({ "energy": 0, "G": 5, "U": 12, "XUHO2": 6 });
    assert_eq!(world.object_json_by_id(id).unwrap()["store"], expected);
    assert_eq!(world.world_json()["objects"][0]["store"], expected);
    assert_eq!(
        world.world_json_for_viewer("p1", 0)["objects"][0]["store"],
        expected
    );
    assert_eq!(
        world.find_in_room("W1N1", c::FIND_STRUCTURES, None)[0].1["store"],
        expected
    );
    assert_eq!(
        world.object_snapshot_by_id(id).unwrap()["store"],
        json!({ "G": 5, "U": 12, "XUHO2": 6 })
    );
}

/// 同一地形版本重复投影一致，替换地形后载荷和版本都须更新。
#[test]
fn terrain_payload_cache_epoch_invalidation() {
    let mut world = World::new(1);
    let mut t = Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t).unwrap();
    let first = world.world_json()["rooms"]["W1N1"]["terrain"]["payload"]
        .as_str()
        .unwrap()
        .to_string();
    // 稳态：同纪元多次投影，载荷逐字节一致（缓存命中路径）
    assert_eq!(
        world.world_json()["rooms"]["W1N1"]["terrain"]["payload"]
            .as_str()
            .unwrap(),
        first
    );
    // set_terrain（player_ticks==0 允许）⇒ 纪元递增 ⇒ 新载荷
    let mut t2 = Terrain::plains();
    t2.normalize();
    t2.as_mut_slice()[100] = kk_core::TERRAIN_MASK_WALL;
    world.set_terrain("W1N1", t2).unwrap();
    let second = world.world_json()["rooms"]["W1N1"]["terrain"]["payload"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(first, second, "纪元递增后必须重编码");
    let decoded = test_b64_decode(&second);
    assert_eq!(decoded[100], kk_core::TERRAIN_MASK_WALL, "新载荷反映新地形");
    assert_eq!(
        world.world_json()["rooms"]["W1N1"]["terrain"]["rev"],
        world.terrain_epoch()
    );
}

/// 原始沙箱 store 保留 energy=0、过滤额外零量资源，控制器总进度缺省为 0。
#[test]
fn store_and_progress_single_source_edges() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Spawn(kk_schema::room::SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 1,
                off: false,
                name: "S1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 5,
                },
                spawning: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 11,
                y: 11,
                death_time: 0,
                decay_time: 100,
                creep_id: None,
                creep_name: None,
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 2,
                    ghodium: 0,
                },
                // 零量栈（矿物耗尽的持久可达态——slots_sub 饱和到 0 不摘栈）
                store_extra: vec![
                    kk_schema::room::ResourceStack {
                        resource: "XUH2O".into(),
                        amount: 0,
                    },
                    kk_schema::room::ResourceStack {
                        resource: "ZK".into(),
                        amount: 7,
                    },
                ],
            }),
        )
        .unwrap();
    for (id, obj) in world.room("W1N1").unwrap().objects.iter() {
        let j = world.object_json_by_id(*id).unwrap();
        match obj {
            RoomObject::Spawn(_) => {
                assert_eq!(j["store"]["energy"], 0, "energy 恒投影（E=0）");
                assert_eq!(j["store"]["G"], 5, "G 正量入表");
            }
            RoomObject::Tombstone(_) => {
                assert_eq!(j["store"]["energy"], 2);
                assert_eq!(j["store"]["ZK"], 7, "正量 extra 入表");
                assert_eq!(
                    j["store"]["XUH2O"],
                    serde_json::Value::Null,
                    "零量 extra 不入表"
                );
            }
            _ => {}
        }
    }
    // progressTotal：0 级 → 0；2 级 → 45000；8 级 → 0（世界投影 0 兜底）
    for (level, expect) in [(0u8, 0), (2, 45000), (8, 0)] {
        let mut w2 = World::new(1);
        w2.register_room("W2N2", Terrain::plains()).unwrap();
        w2.insert(
            "W2N2",
            RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: None,
                owner_cleared: false,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        )
        .unwrap();
        let j = w2.world_json()["objects"][0].clone();
        assert_eq!(j["progressTotal"], expect, "level={level}");
    }
}

#[test]
fn projection_shape_and_find() {
    let mut world = World::new(42);
    world.time = 55;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain).unwrap();

    let ctl = ControllerObj {
        id: 0,
        x: 1,
        y: 1,
        level: 2,
        safe_mode: 0,
        safe_mode_available: 1,
        progress: 10,
        downgrade_time: None,
        owner: Some("p1".into()),
        owner_cleared: false,
        is_power_enabled: false,
        reservation: None,
        sign: None,
        upgrade_blocked: None,
        safe_mode_cooldown: None,
    };
    let ctx = PlaceCtx {
        links: crate::place::builtin_link_rules(),
        stores: crate::place::builtin_store_rules(),

        time: world.time,
        buildings: Default::default(),
        controller: Some(&ctl),
        name_seq: 0,
    };
    let controller = normalize(
        "structure",
        &serde_json::json!({
            "pos": [1, 1], "structureType": "controller", "rcl": 2, "owner": "p1",
        }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", controller).unwrap();
    let source = normalize(
        "structure",
        &serde_json::json!({
            "pos": [10, 10], "structureType": "source",
        }),
        &ctx,
    )
    .unwrap();
    let source_id = world.insert("W1N1", source).unwrap();
    let creep = normalize(
        "creep",
        &serde_json::json!({
            "pos": [20, 20], "owner": "p1", "body": ["move", "work"], "name": "w1",
        }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", creep).unwrap();
    let j = world.world_json();
    assert_eq!(j["time"], 55);
    assert_eq!(j["rooms"]["W1N1"]["name"], "W1N1");
    // 地形载荷解码后须与房间的 2500 字节一致，rev 须为当前地形版本。
    let terrain_json = &j["rooms"]["W1N1"]["terrain"];
    assert_eq!(terrain_json["rev"], world.terrain_epoch());
    let decoded = test_b64_decode(terrain_json["payload"].as_str().unwrap());
    assert_eq!(
        decoded.as_slice(),
        world.room("W1N1").unwrap().terrain.as_slice()
    );

    // FIND 两层：104 按 energy>0（源 3000 满）
    let sources = world.find_in_room("W1N1", c::FIND_SOURCES_ACTIVE, None);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].0, source_id);
    assert_eq!(world.find_in_room("W1N1", c::FIND_CREEPS, None).len(), 1);
    // 中立面 MY_* 无视角不可判定（screeps-ok find.ts 不支持该面查询）
    assert_eq!(world.find_in_room("W1N1", c::FIND_MY_SPAWNS, None).len(), 0);
    assert_eq!(
        world
            .find_in_room("W1N1", c::FIND_MY_SPAWNS, Some("p1"))
            .len(),
        0
    );

    // 对象字段名对齐 prelude
    let (_, creep_json) = world.find_in_room("W1N1", c::FIND_CREEPS, None)[0].clone();
    assert_eq!(creep_json["name"], "w1");
    // age_time(=time+1500) − time
    assert_eq!(creep_json["ticksToLive"], 1500);
    assert_eq!(creep_json["pos"]["roomName"], "W1N1");
    assert_eq!(creep_json["body"][0]["type"], "move");

    // controller pos 查询
    assert_eq!(world.controller_pos("W1N1"), Some((1, 1)));
}

/// Generic tower 仍参与结构查询，专用墓碑参与墓碑查询。
#[test]
fn generic_objects_stay_visible_to_find() {
    let mut world = World::new(1);
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain).unwrap();
    let ctx = PlaceCtx {
        buildings: Default::default(),
        links: crate::place::builtin_link_rules(),
        stores: crate::place::builtin_store_rules(),

        time: 0,
        controller: None,
        name_seq: 0,
    };
    let tower = normalize(
        "structure",
        &serde_json::json!({
            "pos": [7, 7], "structureType": "tower", "owner": "p1",
        }),
        &ctx,
    )
    .unwrap();
    let tower_id = world.insert("W1N1", tower).unwrap();
    let tombstone = normalize(
        "tombstone",
        &serde_json::json!({ "pos": [8, 8], "store": { "energy": 30 } }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", tombstone).unwrap();

    let structs = world.find_in_room("W1N1", c::FIND_STRUCTURES, None);
    assert_eq!(structs.len(), 1, "tower 必须对 FIND_STRUCTURES 可见");
    assert_eq!(structs[0].0, tower_id);
    assert_eq!(structs[0].1["type"], "structure");
    assert_eq!(structs[0].1["structureType"], "tower");
    assert_eq!(
        world.find_in_room("W1N1", c::FIND_TOMBSTONES, None).len(),
        1
    );
    // 投影单对象可见
    let j = world.object_json_by_id(tower_id).unwrap();
    assert_eq!(j["structureType"], "tower");
    assert_eq!(j["pos"]["x"], 7);
}

/// Mod 对象的投影口径：不进内部全量 world_json、玩家视图、任何 FIND
/// 查询与 KKPV 私有块；查询快照（object_snapshot_by_id）是唯一读面。
#[test]
fn mod_objects_excluded_from_all_player_planes() {
    use crate::projection::matches_find;
    let mut world = crate::world::World::new(9);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 10,
                y: 12,
                type_key: "extmod:counter".into(),
                data: r#"{"owner":"p1","x":10,"y":12,"value":3}"#.into(),
            }),
        )
        .unwrap();
    let id = world.rooms["W1N1"].objects.keys().copied().next().unwrap();
    let full = world.world_json();
    assert!(
        full["objects"].as_array().unwrap().is_empty(),
        "内部全量投影不含 Mod 对象"
    );
    let viewer = world.world_json_for_viewer("p1", 0);
    assert!(viewer["objects"].as_array().unwrap().is_empty());
    // 全 FIND 常量范围 1..=250 内无命中（覆盖对象类 FIND 的实际取值区间）。
    for find in 1..=250 {
        assert!(
            world.find_in_room("W1N1", find, Some("p1")).is_empty(),
            "FIND {find} 命中了 Mod 对象"
        );
    }
    let bundle = world
        .kkpv_fresh_bundle_bytes_for_viewer(
            "p1",
            &kk_mod_api::extension::ExtensionCatalog::default(),
            None,
            [0; 32],
            [0; 32],
        )
        .unwrap();
    let blocks = kk_schema::kkpv::bundle_block_slices(&bundle).unwrap();
    assert_eq!(blocks.len(), 1);
    let view =
        kk_schema::kkpv::KkpvView::decode(blocks[0], &kk_schema::kkpv::KkpvLimits::default())
            .unwrap();
    assert_eq!(view.object_count(), 0, "KKPV 私有块不含 Mod 对象");
    // 查询快照读面唯一保留。
    let snap = world.object_snapshot_by_id(id).unwrap();
    assert_eq!(snap["kind"], "mod");
    // matches_find 对防御性空记录不命中（直接锁死兜底口径）。
    assert!(!matches_find(&serde_json::json!({}), 107, None));
}

fn test_creep() -> kk_schema::room::CreepObj {
    kk_schema::room::CreepObj {
        id: 0,
        x: 5,
        y: 5,
        fatigue: 0,
        age_time: 800,
        hits: 300,
        spawning: false,
        action_log: 0,
        name: "w1".into(),
        owner: "p1".into(),
        body: vec![kk_schema::room::BodyPart {
            kind: 0,
            boost: None,
            hits: 100,
        }],
        store: kk_schema::room::Store::default(),
        store_extra: Vec::new(),
        say_message: None,
        say_public: None,
        notify_when_attacked: Some(false),
    }
}

/// 玩家面目录夹具：counter（room-visible，value 公开/treasury 归属者/
/// secret 服务端）与 vault（owner-collection）。
fn surface_fixture() -> kk_mod_api::player_surface::PlayerSurface {
    use kk_mod_api::player_surface::{PlayerSurface, SurfaceField, SurfaceType};
    use kk_mod_api::{Key, ObjectVisibility, Visibility};
    let surface_type =
        |key: &str, visibility: ObjectVisibility, fields: Vec<(&str, Visibility)>| SurfaceType {
            key: Key::new(key).unwrap(),
            find_const: kk_mod_api::player_surface::MOD_FIND_BASE,
            look_key: key.to_string(),
            player_class: "C".into(),
            visibility,
            owner_field: Some("owner".into()),
            fields: fields
                .into_iter()
                .map(|(name, visibility)| SurfaceField {
                    name: name.into(),
                    visibility,
                })
                .collect(),
        };
    PlayerSurface {
        types: vec![
            surface_type(
                "extmod:counter",
                ObjectVisibility::RoomVisible,
                vec![
                    ("owner", Visibility::Public),
                    ("value", Visibility::Public),
                    ("treasury", Visibility::Owner),
                    ("secret", Visibility::ServerOnly),
                ],
            ),
            surface_type(
                "extmod:vault",
                ObjectVisibility::OwnerCollection,
                vec![("owner", Visibility::Public), ("gold", Visibility::Public)],
            ),
        ],
        intents: Vec::new(),
    }
}

#[test]
fn mod_objects_project_with_visibility_cut() {
    let mut world = crate::world::World::new(11);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    // p1 的 creep 授予房间视野（Mod 对象自身不授予）。
    world
        .insert("W1N1", kk_schema::room::RoomObject::Creep(test_creep()))
        .unwrap();
    world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Creep(kk_schema::room::CreepObj {
                owner: "p2".into(),
                name: "w2".into(),
                ..test_creep()
            }),
        )
        .unwrap();
    let (counter_id, vault_id) = (
        world
            .insert(
                "W1N1",
                kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                    id: 0,
                    x: 10,
                    y: 12,
                    type_key: "extmod:counter".into(),
                    data: r#"{"owner":"p1","value":3,"treasury":9,"secret":"k"}"#.into(),
                }),
            )
            .unwrap(),
        world
            .insert(
                "W1N1",
                kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                    id: 0,
                    x: 20,
                    y: 22,
                    type_key: "extmod:vault".into(),
                    data: r#"{"owner":"p2","gold":7}"#.into(),
                }),
            )
            .unwrap(),
    );
    let surface = surface_fixture();
    let catalog = kk_mod_api::extension::ExtensionCatalog::default();

    // 归属者：公开+归属字段可见，secret 裁掉；vault 归属 p2，p1 不可见。
    let view = world.world_json_for_viewer_with_extensions("p1", 0, &catalog, Some(&surface));
    let objects = view["objects"].as_array().unwrap();
    let counter = objects
        .iter()
        .find(|o| o["modType"] == "extmod:counter")
        .unwrap();
    assert_eq!(counter["type"], "mod");
    assert_eq!(counter["id"], counter_id.as_str());
    assert_eq!(counter["room"], "W1N1");
    assert_eq!(counter["x"], 10);
    assert_eq!(counter["pos"]["roomName"], "W1N1");
    assert_eq!(counter["value"], 3);
    assert_eq!(counter["treasury"], 9);
    assert!(counter.get("secret").is_none(), "ServerOnly 字段不进投影");
    assert!(
        objects.iter().all(|o| o["modType"] != "extmod:vault"),
        "OwnerCollection 对象对非归属者不可见"
    );

    // 他人（p2 亦有 creep 授予房间视野）：treasury 裁掉、value 保留。
    let other = world.world_json_for_viewer_with_extensions("p2", 0, &catalog, Some(&surface));
    let counter = other["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["modType"] == "extmod:counter")
        .unwrap();
    assert_eq!(counter["value"], 3);
    assert!(counter.get("treasury").is_none(), "Owner 字段仅归属者可见");
    // vault 归属 p2：他人视角（p1）不可见已由上方断言；归属者视角可见。
    let owner = world.world_json_for_viewer_with_extensions("p2", 0, &catalog, Some(&surface));
    let vault = owner["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == vault_id.as_str())
        .unwrap();
    assert_eq!(vault["gold"], 7);

    // 无视野玩家（p3）：RoomVisible 对象与 OwnerCollection 他人对象都不可见。
    let stranger = world.world_json_for_viewer_with_extensions("p3", 0, &catalog, Some(&surface));
    assert!(
        stranger["objects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o.get("modType").is_none()),
        "房间不可见时 Mod 对象不出投影"
    );

    // 无目录（vanilla 口径）：不进投影。
    let plain = world.world_json_for_viewer_with_extensions("p1", 0, &catalog, None);
    assert!(
        plain["objects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|o| o.get("modType").is_none()),
        "无玩家面目录时 Mod 对象不进投影"
    );
}

/// 管理读面与玩家面的 ServerOnly 分工：object_snapshot_by_id 是操作员
/// 排障面，可以整体读出字段（含 ServerOnly）；玩家投影永不携带。分工
/// 由设计契约固定，防止管理面被误当作投影参照实现。
#[test]
fn admin_snapshot_face_reads_server_only_player_projection_does_not() {
    let mut world = crate::world::World::new(12);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world
        .insert("W1N1", kk_schema::room::RoomObject::Creep(test_creep()))
        .unwrap();
    let counter_id = world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 10,
                y: 12,
                type_key: "extmod:counter".into(),
                data: r#"{"owner":"p1","value":3,"treasury":9,"secret":"k"}"#.into(),
            }),
        )
        .unwrap();

    let snap = world.object_snapshot_by_id(counter_id).unwrap();
    assert_eq!(snap["kind"], "mod");
    assert_eq!(snap["typeKey"], "extmod:counter");
    assert_eq!(snap["fields"]["value"], 3);
    assert_eq!(
        snap["fields"]["secret"], "k",
        "管理读面可以读出 ServerOnly 字段"
    );

    let surface = surface_fixture();
    let catalog = kk_mod_api::extension::ExtensionCatalog::default();
    let view = world.world_json_for_viewer_with_extensions("p1", 0, &catalog, Some(&surface));
    let counter = view["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["modType"] == "extmod:counter")
        .unwrap();
    assert!(
        counter.get("secret").is_none(),
        "玩家投影不携带 ServerOnly 字段"
    );
}

#[test]
fn owner_collection_in_invisible_room_matches_across_channels() {
    // 房间对归属者不可见（无任何内建视野授予对象）时，OwnerCollection
    // 对象按归属者集合语义仍对归属者导出——两条通道都必须如此（曾因
    // KKPV 侧门序错误出现 JSON 可见/KKPV 不可见的分叉）。
    let mut world = crate::world::World::new(13);
    world.register_room("W2N2", Terrain::plains()).unwrap();
    let vault_id = world
        .insert(
            "W2N2",
            kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 3,
                y: 4,
                type_key: "extmod:vault".into(),
                data: r#"{"owner":"p9","gold":11}"#.into(),
            }),
        )
        .unwrap();
    let surface = surface_fixture();
    let catalog = kk_mod_api::extension::ExtensionCatalog::default();

    let json_view = world.world_json_for_viewer_with_extensions("p9", 0, &catalog, Some(&surface));
    let record = json_view["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == vault_id.as_str())
        .expect("JSON 通道对不可见房的归属者导出 OwnerCollection 对象");
    assert_eq!(record["gold"], 11);

    let bundle = world
        .kkpv_fresh_bundle_bytes_for_viewer("p9", &catalog, Some(&surface), [5; 32], [6; 32])
        .unwrap();
    let view = kk_schema::kkpv::bundle_block_slices(&bundle)
        .unwrap()
        .into_iter()
        .map(|bytes| {
            kk_schema::kkpv::KkpvView::decode(bytes, &kk_schema::kkpv::KkpvLimits::default())
                .unwrap()
        })
        .find(|v| v.room_name() == "W2N2")
        .expect("不可见房也有归属者的私有块");
    let index = (0..view.object_count())
        .find(|i| view.object_type(*i) == "extmod:vault")
        .expect("KKPV 通道同样导出（门序与 JSON 一致）");
    let decoded = crate::kkpv::record_json_from_view(&view, index).unwrap();
    // mod 记录的 pos 是 mod_record_json 的显式字段，两通道逐键一致。
    assert_eq!(decoded.as_object().unwrap(), record.as_object().unwrap());
}

#[test]
fn mod_records_match_across_json_and_kkpv_channels() {
    let mut world = crate::world::World::new(12);
    world.register_room("W1N1", Terrain::plains()).unwrap();
    world
        .insert("W1N1", kk_schema::room::RoomObject::Creep(test_creep()))
        .unwrap();
    world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
                id: 0,
                x: 10,
                y: 12,
                type_key: "extmod:counter".into(),
                data: r#"{"owner":"p1","value":3,"treasury":9,"secret":"k"}"#.into(),
            }),
        )
        .unwrap();
    let surface = surface_fixture();
    let catalog = kk_mod_api::extension::ExtensionCatalog::default();

    let json_record =
        world.world_json_for_viewer_with_extensions("p1", 0, &catalog, Some(&surface))["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["modType"] == "extmod:counter")
            .cloned()
            .unwrap();
    let bundle = world
        .kkpv_fresh_bundle_bytes_for_viewer("p1", &catalog, Some(&surface), [1; 32], [2; 32])
        .unwrap();
    let bytes = &kk_schema::kkpv::bundle_block_slices(&bundle).unwrap()[0];
    let view = kk_schema::kkpv::KkpvView::decode(bytes, &kk_schema::kkpv::KkpvLimits::default())
        .expect("块解码成功");
    assert!(
        (0..view.object_count()).any(|i| view.object_type(i) == "extmod:counter"),
        "KKPV 块含 Mod 对象（桶键为类型键）"
    );
    let mod_index = (0..view.object_count())
        .find(|i| view.object_type(*i) == "extmod:counter")
        .expect("视图中有 counter 类型对象");
    let decoded = crate::kkpv::record_json_from_view(&view, mod_index)
        .unwrap_or_else(|e| panic!("解码失败：{e}"));
    // 两通道逐键一致（含裁切后的字段集合与信封键）。
    let expected: serde_json::Map<String, serde_json::Value> =
        json_record.as_object().unwrap().clone();
    assert_eq!(decoded, serde_json::Value::Object(expected));
}
