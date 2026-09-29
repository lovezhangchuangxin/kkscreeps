//! 连续切块 A：地形缓存/单源化输入类锁定/投影形状与 FIND/Generic 桶/墓碑废墟
//!（自 tests.rs 源文序 43-443 行区段，P4 拆分+终审再拆两段）。

use super::*;

/// 地形缓存行为（P3）：纪元内载荷确定（多次投影一致）；纪元递增后
/// 载荷随新地形重编码（无陈旧命中）。
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

/// 投影形状与 M0 契约对表：rooms 带 terrain 紧凑载荷、对象字段名对齐
/// prelude wrapObject、FIND 过滤两层一致、pos 组装。
/// P4 单源化的输入类锁定（评审 1 P2 清偿）：(energy=0, G>0) 的组装序
/// 重组面（世界投影 energy 恒在 + G 正量）；零量 extra 栈不出现；
/// progressTotal 的 0/中级/8 级折叠（世界投影 0 兜底）。
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
    // 地形紧凑协议（P3）：{rev, payload(b64)}；rev = 当前纪元，
    // payload 解回 2500 字节与 typed 地形逐字节一致（Rust 孪生——
    // JS 侧 b64ToBytes 解码器的对表责任由本测试承担）
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

/// tower 等结算范围外对象进 Generic 桶后仍对 FIND_* 可见（M0 接受面）。
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
