//! 检查地形缓存更新、资源与控制器投影，以及 Generic 对象查询。

use super::*;

#[test]
fn storage_named_resources_reach_all_projections() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains());
    let storage = normalize(
        "structure",
        &json!({
            "pos": [10, 10], "structureType": "storage", "owner": "p1",
            "store": { "energy": 0, "G": 5, "U": 12, "XUHO2": 6, "Z": 0 },
        }),
        &PlaceCtx {
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
    world.register_room("W1N1", t);
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
    world.register_room("W1N1", Terrain::plains());
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
        w2.register_room("W2N2", Terrain::plains());
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
    world.register_room("W1N1", terrain);

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
        time: world.time,
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
    world.register_room("W1N1", terrain);
    let ctx = PlaceCtx {
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
