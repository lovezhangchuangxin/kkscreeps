//! 检查查询快照的类型、计时、资源、容量及缺省值。

use super::*;
use crate::place::{PlaceCtx, normalize};
use kk_core::Terrain;

fn world_with_room() -> World {
    let mut world = World::new(42);
    world.time = 10;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    world
}

fn ctx(world: &World) -> PlaceCtx<'_> {
    PlaceCtx {
        links: crate::place::builtin_link_rules(),

        time: world.time,
        controller: None,
        name_seq: 0,
    }
}

/// 快照省略零量资源，保留额外正量资源；控制器 0/8 级总进度返回 null。
#[test]
fn snapshot_store_and_progress_edges() {
    let mut world = World::new(1);
    world.register_room("W1N1", Terrain::plains());
    let spawn = world
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
    let tomb = world
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
    // energy=0 不进入查询 store；沙箱原始数据则保留该键。
    let spawn_snap = world.object_snapshot_by_id(spawn).unwrap();
    assert_eq!(
        spawn_snap["store"]["energy"],
        serde_json::Value::Null,
        "E=0 不入表"
    );
    assert_eq!(spawn_snap["store"]["G"], 5);
    let tomb_snap = world.object_snapshot_by_id(tomb).unwrap();
    assert_eq!(tomb_snap["store"]["energy"], 2);
    assert_eq!(tomb_snap["store"]["ZK"], 7, "正量 extra 入表");
    assert_eq!(
        tomb_snap["store"]["XUH2O"],
        serde_json::Value::Null,
        "零量 extra 不入表"
    );
    // progressTotal：0/8 级 Null（契约面）；2 级查表 45000
    for (level, expect) in [
        (0u8, serde_json::Value::Null),
        (2, json!(45000)),
        (8, serde_json::Value::Null),
    ] {
        let mut w2 = World::new(1);
        w2.register_room("W2N2", Terrain::plains());
        let id = w2
            .insert(
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
        assert_eq!(
            w2.object_snapshot_by_id(id).unwrap()["progressTotal"],
            expect,
            "level={level}"
        );
    }
}

#[test]
fn snapshot_contract_shapes() {
    let mut world = world_with_room();
    let creep = normalize(
        "creep",
        &serde_json::json!({
            "pos": [5, 5], "owner": "p1", "body": ["carry", "carry"],
            "store": { "energy": 0 },
        }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let creep_id = world.insert("W1N1", creep).unwrap();
    let snap = world.object_snapshot_by_id(creep_id).unwrap();
    assert_eq!(snap["kind"], "creep");
    assert_eq!(
        snap["store"].as_object().unwrap().len(),
        0,
        "0 值资源不出现"
    );
    assert_eq!(snap["storeCapacity"], 100, "CARRY×2 × 50");
    assert_eq!(
        snap["ticksToLive"], 1500,
        "默认寿命：age_time(=10+1500) − time=10"
    );
    assert!(snap["spawning"].is_boolean());

    // G 也须出现在查询 store 中，不能只保留 energy。
    let gh_creep = normalize(
        "creep",
        &serde_json::json!({
            "pos": [6, 6], "owner": "p1", "body": ["move"],
            "store": { "energy": 30, "G": 200 },
        }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 1,
        },
    )
    .unwrap();
    let gh_id = world.insert("W1N1", gh_creep).unwrap();
    let gh_snap = world.object_snapshot_by_id(gh_id).unwrap();
    assert_eq!(gh_snap["store"]["energy"], 30);
    assert_eq!(gh_snap["store"]["G"], 200, "ghodium 正量入快照");

    let spawn = normalize(
        "structure",
        &serde_json::json!({ "pos": [6, 6], "structureType": "spawn", "owner": "p1" }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let spawn_id = world.insert("W1N1", spawn).unwrap();
    let snap = world.object_snapshot_by_id(spawn_id).unwrap();
    assert_eq!(snap["kind"], "structure");
    assert_eq!(snap["structureType"], "spawn");
    // spawn 的查询总容量为 null，因为容量按资源分别限制。
    assert_eq!(snap["storeCapacity"], serde_json::Value::Null);
    assert_eq!(
        snap["spawning"],
        serde_json::Value::Null,
        "无孵化任务读 null"
    );

    let source = normalize(
        "structure",
        &serde_json::json!({ "pos": [10, 10], "structureType": "source" }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let source_id = world.insert("W1N1", source).unwrap();
    let snap = world.object_snapshot_by_id(source_id).unwrap();
    assert_eq!(
        snap["kind"], "source",
        "source 有独立契约 kind（非 structure）"
    );
    assert_eq!(
        snap["ticksToRegeneration"],
        serde_json::Value::Null,
        "未挂钟读 null"
    );
}

/// getter 换算形态：controller 的剩余值/8 级 null、source 挂钟后的剩余数。
#[test]
fn snapshot_getter_forms() {
    let mut world = world_with_room();
    let ctl = kk_schema::room::ControllerObj {
        id: 0,
        x: 1,
        y: 1,
        level: 8,
        safe_mode: 0,
        safe_mode_available: 1,
        progress: 0,
        downgrade_time: Some(500),
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

        time: world.time,
        controller: Some(&ctl),
        name_seq: 0,
    };
    let controller = normalize(
            "structure",
            &serde_json::json!({ "pos": [1, 1], "structureType": "controller", "rcl": 8, "owner": "p1", "downgradeTime": 500 }),
            &ctx,
        )
        .unwrap();
    let ctl_id = world.insert("W1N1", controller).unwrap();
    let snap = world.object_snapshot_by_id(ctl_id).unwrap();
    assert_eq!(snap["kind"], "structure");
    assert_eq!(
        snap["progressTotal"],
        serde_json::Value::Null,
        "8 级无进度条"
    );
    assert_eq!(snap["ticksToDowngrade"], 490, "绝对 500 − time 10");
    assert_eq!(
        snap["safeMode"],
        serde_json::Value::Null,
        "safe_mode=0 读 null"
    );
    assert_eq!(snap["isPowerEnabled"], false);

    // FIND_STRUCTURES 不含 source（官方 game.js:229-251 structureTypes 表）
    let source = normalize(
        "structure",
        &serde_json::json!({ "pos": [10, 10], "structureType": "source" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", source).unwrap();
    let structures = world.find_snapshots_in_room("W1N1", c::FIND_STRUCTURES);
    assert_eq!(
        structures.len(),
        1,
        "只有 controller，source 不入 FIND_STRUCTURES"
    );
    assert_eq!(structures[0]["structureType"], "controller");
    let sources = world.find_snapshots_in_room("W1N1", c::FIND_SOURCES);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["kind"], "source");
}

/// KH 强化 CARRY 容量翻倍，没有 CARRY 的 creep 查询容量为 null。
#[test]
fn snapshot_creep_capacity_boosts_and_null() {
    let mut world = world_with_room();
    let boosted = normalize(
        "creep",
        &serde_json::json!({
            "pos": [7, 7], "owner": "p1",
            "body": ["carry", "carry"], "boosts": { "1": "KH" },
        }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let id = world.insert("W1N1", boosted).unwrap();
    let snap = world.object_snapshot_by_id(id).unwrap();
    assert_eq!(
        snap["storeCapacity"], 150,
        "裸 CARRY 50 + KH boosted ×2 = 150"
    );

    let no_carry = normalize(
        "creep",
        &serde_json::json!({ "pos": [8, 8], "owner": "p1", "body": ["move", "work"] }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let id = world.insert("W1N1", no_carry).unwrap();
    let snap = world.object_snapshot_by_id(id).unwrap();
    assert_eq!(
        snap["storeCapacity"],
        serde_json::Value::Null,
        "无 CARRY 读 null"
    );
}

/// structure 通道的矿物须使用独立快照 kind，并被 FIND_MINERALS 查询识别。
#[test]
fn snapshot_mineral_and_tombstone_kind_mapping() {
    let mut world = world_with_room(); // time = 10
    let mineral = normalize(
        "structure",
        &serde_json::json!({
            "pos": [12, 12], "structureType": "mineral",
            "mineralType": "X", "mineralAmount": 2000, "density": 3,
            "ticksToRegeneration": 90,
        }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let id = world.insert("W1N1", mineral).unwrap();
    let snap = world.object_snapshot_by_id(id).unwrap();
    assert_eq!(
        snap["kind"], "mineral",
        "structure 通道的 mineral 进 typed 变体"
    );
    assert_eq!(snap["mineralType"], "X");
    assert_eq!(snap["mineralAmount"], 2000);
    assert_eq!(snap["density"], 3);
    assert_eq!(snap["ticksToRegeneration"], 90, "绝对钟 10+90 − time 10");
    assert_eq!(
        snap.as_object().unwrap().len(),
        7,
        "契约键面：kind/id/pos/mineralType/mineralAmount/density/ticksToRegeneration"
    );
    assert!(
        snap.get("structureType").is_none(),
        "mineral 契约无 structureType 键（vanilla snapshotMineral）"
    );
    assert_eq!(
        world.find_snapshots_in_room("W1N1", c::FIND_MINERALS).len(),
        1,
        "FIND_MINERALS 命中且 kind 正确"
    );
    // FIND_STRUCTURES 不含 mineral（官方 game.js:229-251 structureTypes 表）
    assert_eq!(
        world
            .find_snapshots_in_room("W1N1", c::FIND_STRUCTURES)
            .len(),
        0
    );

    let tombstone = normalize(
        "tombstone",
        &serde_json::json!({ "pos": [13, 13], "store": { "energy": 30 } }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let id = world.insert("W1N1", tombstone).unwrap();
    let snap = world.object_snapshot_by_id(id).unwrap();
    assert_eq!(snap["kind"], "tombstone", "直通道 tombstone（B3 typed）");
}

/// 平原、沼泽、墙地形的道路最大 hits 分别采用对应系数。
#[test]
fn road_hits_max_terrain_scaling() {
    let mut world = world_with_room();
    let mut terrain = kk_core::Terrain::plains();
    terrain.as_mut_slice()[25 * 50 + 10] |= kk_core::TERRAIN_MASK_SWAMP;
    terrain.as_mut_slice()[25 * 50 + 11] |= kk_core::TERRAIN_MASK_WALL;
    terrain.normalize();
    world.rooms.get_mut("W1N1").unwrap().terrain = terrain;
    let plain_road = normalize(
        "structure",
        &serde_json::json!({ "pos": [10, 24], "structureType": "road" }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let plain_id = world.insert("W1N1", plain_road).unwrap();
    let swamp_road = normalize(
        "structure",
        &serde_json::json!({ "pos": [10, 25], "structureType": "road" }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let swamp_id = world.insert("W1N1", swamp_road).unwrap();
    let wall_road = normalize(
        "structure",
        &serde_json::json!({ "pos": [11, 25], "structureType": "road" }),
        &PlaceCtx {
            links: crate::place::builtin_link_rules(),

            time: world.time,
            controller: None,
            name_seq: 0,
        },
    )
    .unwrap();
    let wall_id = world.insert("W1N1", wall_road).unwrap();
    assert_eq!(
        world.object_snapshot_by_id(plain_id).unwrap()["hitsMax"],
        5000
    );
    assert_eq!(
        world.object_snapshot_by_id(swamp_id).unwrap()["hitsMax"],
        25000,
        "沼泽 ×5"
    );
    assert_eq!(
        world.object_snapshot_by_id(wall_id).unwrap()["hitsMax"],
        750000,
        "墙 ×150"
    );
    // prelude 世界投影同源
    let wj = world.world_json();
    let roads: Vec<&serde_json::Value> = wj["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| o["structureType"] == "road")
        .collect();
    assert_eq!(roads.len(), 3);
    assert!(roads.iter().all(|o| o["hitsMax"].as_i64().unwrap() > 0));
}

/// flag 不进查询面（vanilla snapshotObject 无 flag 分支）。
#[test]
fn flags_not_in_query_plane() {
    let mut world = world_with_room();
    let c = ctx(&world);
    let flag = normalize(
        "flag",
        &serde_json::json!({ "pos": [3, 3], "name": "f1" }),
        &c,
    )
    .unwrap();
    let flag_id = world.insert("W1N1", flag).unwrap();
    assert!(world.object_snapshot_by_id(flag_id).is_none());
    assert!(
        world
            .find_snapshots_in_room("W1N1", c::FIND_FLAGS)
            .is_empty()
    );
}
/// 墓碑、废墟查询输出衰减剩余时间和正量资源，掉落资源保留显式资源名。
#[test]
fn tombstone_and_ruin_snapshot_contract() {
    let mut world = world_with_room(); // time = 10
    let tomb = {
        let c = ctx(&world);
        normalize(
            "tombstone",
            &serde_json::json!({
                "pos": [13, 13], "creepName": "w1", "deathTime": 5, "ticksToDecay": 500,
                "store": { "energy": 30, "XUHO2": 6 },
            }),
            &c,
        )
        .unwrap()
    };
    let tomb_id = world.insert("W1N1", tomb).unwrap();
    let snap = world.object_snapshot_by_id(tomb_id).unwrap();
    assert_eq!(snap["kind"], "tombstone");
    assert_eq!(snap["creepName"], "w1");
    assert_eq!(snap["deathTime"], 5);
    assert_eq!(
        snap["store"],
        serde_json::json!({ "energy": 30, "XUHO2": 6 }),
        "全资源正量（额外资源并入）"
    );
    assert_eq!(
        snap["ticksToDecay"], 495,
        "decayTime 505 − time 10（裸减法，vanilla 同式）"
    );
    assert_eq!(
        snap.as_object().unwrap().len(),
        7,
        "契约键面：kind/id/pos/creepName/deathTime/store/ticksToDecay"
    );

    let ruin = {
        let c = ctx(&world);
        normalize(
            "ruin",
            &serde_json::json!({
                "pos": [14, 14], "structureType": "container", "structureOwner": "p1",
                "destroyTime": 8, "ticksToDecay": 500, "store": { "energy": 700 },
            }),
            &c,
        )
        .unwrap()
    };
    let ruin_id = world.insert("W1N1", ruin).unwrap();
    let snap = world.object_snapshot_by_id(ruin_id).unwrap();
    assert_eq!(snap["kind"], "ruin");
    assert_eq!(snap["structureType"], "container");
    assert_eq!(snap["destroyTime"], 8);
    assert_eq!(snap["store"]["energy"], 700);
    assert_eq!(snap["ticksToDecay"], 498, "508 − 10");
    assert_eq!(snap.as_object().unwrap().len(), 7, "契约七键面");

    // FIND 面与命名资源堆
    assert_eq!(
        world
            .find_snapshots_in_room("W1N1", c::FIND_TOMBSTONES)
            .len(),
        1
    );
    assert_eq!(world.find_snapshots_in_room("W1N1", c::FIND_RUINS).len(), 1);
    let mineral_pile = {
        let c2 = ctx(&world);
        normalize(
            "resource",
            &serde_json::json!({ "pos": [15, 15], "resourceType": "XUHO2", "amount": 12 }),
            &c2,
        )
        .unwrap()
    };
    let pile_id = world.insert("W1N1", mineral_pile).unwrap();
    let snap = world.object_snapshot_by_id(pile_id).unwrap();
    assert_eq!(snap["kind"], "resource");
    assert_eq!(snap["resourceType"], "XUHO2", "命名槽透出资源名");
    assert_eq!(snap["amount"], 12);
}

/// 无主提取器的快照 owner 为 null，creep 的额外正量矿物进入 store。
#[test]
fn extractor_snapshot_and_creep_named_slot() {
    let mut world = world_with_room();
    let extractor = world
        .insert(
            "W1N1",
            RoomObject::Extractor(kk_schema::room::ExtractorObj {
                id: 0,
                x: 8,
                y: 8,
                hits: 500,
                owner: None,
                cooldown: 5,
                notify_when_attacked: Some(true),
            }),
        )
        .unwrap();
    let snap = world.object_snapshot_by_id(extractor).unwrap();
    assert_eq!(snap["kind"], "structure");
    assert_eq!(snap["structureType"], "extractor");
    assert_eq!(snap["hits"], 500);
    assert_eq!(snap["hitsMax"], c::EXTRACTOR_HITS);
    assert_eq!(snap["cooldown"], 5, "计数器原样（obj.cooldown || 0）");
    assert_eq!(snap["owner"], serde_json::Value::Null, "无主读 null");
    assert!(
        snap.get("notifyWhenAttacked").is_none(),
        "契约面无该键（vanilla ExtractorSnapshot）"
    );

    // creep 携带 U：快照 store 全资源正量
    let c = ctx(&world);
    let creep = normalize(
        "creep",
        &serde_json::json!({
            "pos": [9, 9], "owner": "p1", "body": ["carry", "move"],
            "store": { "U": 7 },
        }),
        &c,
    )
    .unwrap();
    let creep_id = world.insert("W1N1", creep).unwrap();
    let snap = world.object_snapshot_by_id(creep_id).unwrap();
    assert_eq!(
        snap["store"],
        serde_json::json!({ "U": 7 }),
        "命名槽并入正量表"
    );
}

#[test]
fn controller_owner_three_states() {
    let mk = |owner: Option<&str>, cleared: bool| {
        RoomObject::Controller(kk_schema::room::ControllerObj {
            id: 0,
            x: 1,
            y: 1,
            level: 1,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 0,
            downgrade_time: None,
            owner: owner.map(str::to_string),
            owner_cleared: cleared,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        })
    };
    let mut world = world_with_room();
    let owned = world.insert("W1N1", mk(Some("p1"), false)).unwrap();
    let cleared = world.insert("W1N1", mk(None, true)).unwrap();
    let virgin = world.insert("W1N1", mk(None, false)).unwrap();
    assert_eq!(world.object_snapshot_by_id(owned).unwrap()["owner"], "p1");
    assert_eq!(
        world.object_snapshot_by_id(cleared).unwrap()["owner"],
        serde_json::Value::Null,
        "降级归零哨兵投影 null"
    );
    assert!(
        world
            .object_snapshot_by_id(virgin)
            .unwrap()
            .get("owner")
            .is_none(),
        "从未拥有无键"
    );
}

#[test]
fn snapshot_store_capacity_family() {
    let mut world = world_with_room();

    let storage = {
        let c = ctx(&world);
        normalize(
            "structure",
            &serde_json::json!({
                "pos": [9, 9], "structureType": "storage", "owner": "p1",
                "store": { "energy": 700 }
            }),
            &c,
        )
        .unwrap()
    };
    let storage_id = world.insert("W1N1", storage).unwrap();
    let snap = world.object_snapshot_by_id(storage_id).unwrap();
    assert_eq!(snap["storeCapacity"], 1_000_000, "STORAGE_CAPACITY 数值");
    assert_eq!(snap["store"]["energy"], 700, "正量资源入表");

    let ext = {
        let c = ctx(&world);
        normalize(
            "structure",
            &serde_json::json!({ "pos": [9, 10], "structureType": "extension", "owner": "p1" }),
            &c,
        )
        .unwrap()
    };
    let ext_id = world.insert("W1N1", ext).unwrap();
    let snap = world.object_snapshot_by_id(ext_id).unwrap();
    assert_eq!(
        snap["storeCapacity"],
        serde_json::Value::Null,
        "extension 无参 getCapacity() 读 null"
    );

    let creep = {
        let c = ctx(&world);
        normalize(
            "creep",
            &serde_json::json!({
                "pos": [9, 11], "owner": "p1", "body": ["carry", "carry"], "name": "c9"
            }),
            &c,
        )
        .unwrap()
    };
    let creep_id = world.insert("W1N1", creep).unwrap();
    let snap = world.object_snapshot_by_id(creep_id).unwrap();
    assert_eq!(
        snap["storeCapacity"], 100,
        "CARRY×2 × 50（与 world_json 同源）"
    );
}

/// 控制器有效计时转为剩余值，过期或未设置时返回 null；签名不带 datetime。
#[test]
fn controller_b2_projection_fields() {
    let mk = |reservation, sign, blocked, cooldown| {
        RoomObject::Controller(kk_schema::room::ControllerObj {
            id: 0,
            x: 1,
            y: 1,
            level: 1,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 0,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation,
            sign,
            upgrade_blocked: blocked,
            safe_mode_cooldown: cooldown,
        })
    };
    let mut world = world_with_room(); // time = 10
    let full = world
        .insert(
            "W1N1",
            mk(
                Some(kk_schema::room::Reservation {
                    user: "p2".into(),
                    end_time: 510,
                }),
                Some(kk_schema::room::Sign {
                    user: "p1".into(),
                    text: "hi".into(),
                    time: 80,
                    datetime: None,
                }),
                Some(310),
                Some(50_010),
            ),
        )
        .unwrap();
    let expired = world
        .insert("W1N1", mk(None, None, Some(5), Some(5)))
        .unwrap();
    let empty = world.insert("W1N1", mk(None, None, None, None)).unwrap();
    let snap = world.object_snapshot_by_id(full).unwrap();
    assert_eq!(
        snap["reservation"],
        serde_json::json!({ "owner": "p2", "ticksToEnd": 500 }),
        "endTime 510 − time 10"
    );
    assert_eq!(
        snap["sign"],
        serde_json::json!({ "owner": "p1", "text": "hi", "time": 80 })
    );
    assert_eq!(snap["upgradeBlocked"], 300, "310 − 10");
    assert_eq!(snap["safeModeCooldown"], 50_000, "50010 − 10");
    let e = world.object_snapshot_by_id(expired).unwrap();
    assert_eq!(e["upgradeBlocked"], serde_json::Value::Null, "过期读 null");
    assert_eq!(
        e["safeModeCooldown"],
        serde_json::Value::Null,
        "过期读 null"
    );
    assert_eq!(
        e["reservation"],
        serde_json::Value::Null,
        "无预定读 null（active_remaining 家族外的对象形态字段）"
    );
    let n = world.object_snapshot_by_id(empty).unwrap();
    assert_eq!(n["reservation"], serde_json::Value::Null);
    assert_eq!(n["sign"], serde_json::Value::Null);
    assert_eq!(n["upgradeBlocked"], serde_json::Value::Null);
    assert_eq!(n["safeModeCooldown"], serde_json::Value::Null);
}
