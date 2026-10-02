//! 检查归属、容量、控制器计时、玩家查询与房间能量统计。

use super::*;

/// 墓碑与废墟使用 user 主人字段，沙箱投影须保留内容并导出所在可见房间的对象。
#[test]
fn tombstone_and_ruin_world_projection_and_vision() {
    let mut world = World::new(42);
    world.time = 100;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    world.register_room("W2N2", Terrain::plains());
    let tomb = world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 10,
                y: 11,
                death_time: 95,
                decay_time: 130,
                creep_id: Some(7),
                creep_name: Some("w1".into()),
                creep_ticks_to_live: 5,
                creep_body: vec![0, 1],
                creep_saying: None,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 30,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 6,
                }],
            }),
        )
        .unwrap();
    let ruin = world
        .insert(
            "W1N1",
            RoomObject::Ruin(kk_schema::room::RuinObj {
                id: 0,
                x: 12,
                y: 11,
                structure_id: 9,
                structure_type: "spawn".into(),
                structure_hits_max: Some(5000),
                destroy_time: 98,
                decay_time: 598,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 300,
                    ghodium: 4,
                },
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let j = world.object_json_by_id(tomb).unwrap();
    assert_eq!(j["type"], "tombstone");
    assert_eq!(j["user"], "p1");
    assert_eq!(j["deathTime"], 95);
    assert_eq!(j["decayTime"], 130);
    assert_eq!(j["ticksToDecay"], 30, "getter 折叠（decayTime − time）");
    assert_eq!(j["creepId"], "kk0000000000000007", "u64 转官方字符串 id");
    assert_eq!(j["creepName"], "w1");
    assert_eq!(j["creepTicksToLive"], 5);
    assert_eq!(j["creepBody"], serde_json::json!(["move", "work"]));
    assert_eq!(j["store"]["energy"], 30);
    assert_eq!(j["store"]["XUHO2"], 6, "额外资源并入 store 投影");
    assert!(
        j.get("creepSaying").is_none(),
        "无遗言不落键（官方 undefined）"
    );

    let r = world.object_json_by_id(ruin).unwrap();
    assert_eq!(r["type"], "ruin");
    assert_eq!(r["destroyTime"], 98);
    assert_eq!(r["ticksToDecay"], 498);
    assert_eq!(r["structure"]["id"], "kk0000000000000009");
    assert_eq!(r["structure"]["type"], "spawn");
    assert_eq!(
        r["structure"]["hits"], 0,
        "存照 hits 恒 0（_destroy.js:29）"
    );
    assert_eq!(r["structure"]["hitsMax"], 5000);
    assert_eq!(r["structure"]["user"], "p1");
    assert_eq!(r["user"], "p1", "顶层 user 双落（官方两处同写）");
    assert_eq!(r["store"]["energy"], 300);
    assert_eq!(r["store"]["G"], 4);

    // 中立查询分别识别墓碑、废墟，不将它们计为结构。
    assert_eq!(
        world.find_in_room("W1N1", c::FIND_TOMBSTONES, None).len(),
        1
    );
    assert_eq!(world.find_in_room("W1N1", c::FIND_RUINS, None).len(), 1);
    assert_eq!(
        world.find_in_room("W1N1", c::FIND_STRUCTURES, None).len(),
        0
    );

    // 有主墓碑和废墟使房间可见，其余房间仍提供地形。
    let viewer = world.world_json_for_viewer("p1", 0);
    let rooms = viewer["rooms"].as_object().unwrap();
    assert!(rooms["W1N1"].get("terrain").is_some(), "墓碑授视野");
    assert!(rooms["W2N2"].get("terrain").is_some(), "地形面恒在");
    let visible_objects = viewer["objects"].as_array().unwrap();
    assert_eq!(visible_objects.len(), 2, "可见房内对象全量（墓碑+废墟）");
}

#[test]
fn controller_owner_three_states() {
    let mk = |owner: Option<&str>, cleared: bool| {
        RoomObject::Controller(ControllerObj {
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
    let mut world = World::new(42);
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let owned = world.insert("W1N1", mk(Some("p1"), false)).unwrap();
    let cleared = world.insert("W1N1", mk(None, true)).unwrap();
    let virgin = world.insert("W1N1", mk(None, false)).unwrap();
    assert_eq!(world.object_json_by_id(owned).unwrap()["owner"], "p1");
    assert_eq!(
        world.object_json_by_id(cleared).unwrap()["owner"],
        serde_json::Value::Null,
        "降级归零哨兵投影 null"
    );
    assert!(
        world
            .object_json_by_id(virgin)
            .unwrap()
            .get("owner")
            .is_none(),
        "从未拥有无键"
    );
}

/// 检查容量字段的两种表示，以及孵化任务未指定 directions 时省略该键。
#[test]
fn store_capacity_projection_family() {
    let mut world = World::new(7);
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let ctl = ControllerObj {
        id: 0,
        x: 5,
        y: 5,
        level: 7,
        safe_mode: 0,
        safe_mode_available: 0,
        progress: 0,
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

        time: world.time,
        controller: Some(&ctl),
        name_seq: 0,
    };
    world
        .insert("W1N1", RoomObject::Controller(ctl.clone()))
        .unwrap();
    let spawn = normalize(
        "spawn",
        &serde_json::json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
        &ctx,
    )
    .unwrap();
    let spawn_id = world.insert("W1N1", spawn).unwrap();
    let ext = normalize(
        "structure",
        &serde_json::json!({ "pos": [21, 20], "structureType": "extension", "owner": "p1" }),
        &ctx,
    )
    .unwrap();
    let ext_id = world.insert("W1N1", ext).unwrap();
    let storage = normalize(
        "structure",
        &serde_json::json!({ "pos": [22, 20], "structureType": "storage", "owner": "p1" }),
        &ctx,
    )
    .unwrap();
    let storage_id = world.insert("W1N1", storage).unwrap();
    let creep = normalize(
        "creep",
        &serde_json::json!({
            "pos": [23, 20], "owner": "p1", "body": ["carry", "carry", "move"], "name": "c1"
        }),
        &ctx,
    )
    .unwrap();
    let creep_id = world.insert("W1N1", creep).unwrap();

    let spawn_j = world.object_json_by_id(spawn_id).unwrap();
    assert_eq!(spawn_j["storeCapacityResource"]["energy"], 300);
    assert!(
        spawn_j.get("storeCapacity").is_none(),
        "spawn 无数值容量键（官方落库无 storeCapacity）"
    );
    assert_eq!(spawn_j["off"], false, "off 标记投影（data(id).off 回查面）");
    // 没有孵化任务时 spawning 为 null。
    assert_eq!(spawn_j["spawning"], serde_json::Value::Null);

    let ext_j = world.object_json_by_id(ext_id).unwrap();
    assert_eq!(
        ext_j["storeCapacityResource"]["energy"], 100,
        "RCL 7 的 extension 容量（EXTENSION_ENERGY_CAPACITY[7]）"
    );
    assert_eq!(ext_j["off"], false);

    let storage_j = world.object_json_by_id(storage_id).unwrap();
    assert_eq!(storage_j["storeCapacity"], 1_000_000);
    assert!(
        storage_j.get("storeCapacityResource").is_none(),
        "storage 无 per-resource 容量（通用商店）"
    );

    let creep_j = world.object_json_by_id(creep_id).unwrap();
    assert_eq!(creep_j["storeCapacity"], 100, "CARRY×2 × 50");

    // 手写孵化任务（带 directions）经 world_json 全量投影
    if let Some(RoomObject::Spawn(s)) = world.room_mut("W1N1").unwrap().objects.get_mut(&spawn_id) {
        s.spawning = Some(kk_schema::room::SpawningJob {
            name: "child".into(),
            need_time: 6,
            spawn_time: 6,
            directions: vec![3, 5],
        });
    }
    let spawn_j = world.object_json_by_id(spawn_id).unwrap();
    assert_eq!(spawn_j["spawning"]["name"], "child");
    assert_eq!(spawn_j["spawning"]["needTime"], 6);
    assert_eq!(spawn_j["spawning"]["spawnTime"], 6);
    assert_eq!(spawn_j["spawning"]["directions"], serde_json::json!([3, 5]));
    // 未指定 directions 时省略字段。
    if let Some(RoomObject::Spawn(s)) = world.room_mut("W1N1").unwrap().objects.get_mut(&spawn_id) {
        s.spawning = Some(kk_schema::room::SpawningJob {
            name: "child".into(),
            need_time: 6,
            spawn_time: 6,
            directions: vec![],
        });
    }
    let spawn_j = world.object_json_by_id(spawn_id).unwrap();
    assert!(
        spawn_j["spawning"].get("directions").is_none(),
        "空 directions 缺键（官方 Spawning.directions === undefined）"
    );
}

/// 沙箱保留控制器绝对计时和预订、签名记录；未设置的可选字段返回 null。
#[test]
fn controller_b2_projection_fields() {
    let mk = |reservation, sign, blocked, cooldown| {
        RoomObject::Controller(ControllerObj {
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
    let mut world = World::new(42);
    world.time = 100;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let full = world
        .insert(
            "W1N1",
            mk(
                Some(kk_schema::room::Reservation {
                    user: "p2".into(),
                    end_time: 600,
                }),
                Some(kk_schema::room::Sign {
                    user: "p1".into(),
                    text: "hi".into(),
                    time: 80,
                    datetime: None,
                }),
                Some(400),
                Some(50_500),
            ),
        )
        .unwrap();
    let empty = world.insert("W1N1", mk(None, None, None, None)).unwrap();
    let j = world.object_json_by_id(full).unwrap();
    assert_eq!(
        j["reservation"],
        serde_json::json!({ "user": "p2", "endTime": 600 }),
        "原始 DB 形态（getter 侧换算 ticksToEnd）"
    );
    assert_eq!(
        j["sign"],
        serde_json::json!({ "user": "p1", "text": "hi", "time": 80 })
    );
    assert!(
        j["sign"].get("datetime").is_none(),
        "墙钟不落盘不投影（灰区表登记）"
    );
    assert_eq!(j["upgradeBlocked"], 400, "绝对 tick 原样");
    assert_eq!(j["safeModeCooldown"], 50_500, "绝对 tick 原样");
    let e = world.object_json_by_id(empty).unwrap();
    assert_eq!(e["reservation"], serde_json::Value::Null);
    assert_eq!(e["sign"], serde_json::Value::Null);
    assert_eq!(e["upgradeBlocked"], serde_json::Value::Null);
    assert_eq!(e["safeModeCooldown"], serde_json::Value::Null);
}

/// 同房多个主人时按 viewer 区分归属；玩家查询排除孵化 creep，中立查询保留。
#[test]
fn find_per_viewer_semantics() {
    let mut world = World::new(3);
    world.time = 10;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let ctx = PlaceCtx {
        links: crate::place::builtin_link_rules(),

        time: world.time,
        controller: None,
        name_seq: 0,
    };
    // p1：普通 creep + 孵化雏形 + spawn + extension；p2：creep + spawn；
    // 工地 p1 一个
    let creep1 = normalize(
        "creep",
        &serde_json::json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "c1" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", creep1).unwrap();
    let ghost = normalize(
        "creep",
        &serde_json::json!({ "pos": [11, 10], "owner": "p1", "body": ["move"], "name": "g1" }),
        &ctx,
    )
    .unwrap();
    let ghost_id = world.insert("W1N1", ghost).unwrap();
    // place 对 spawning 硬编码 false——insert 后直改（孵化雏形形态）
    if let Some(RoomObject::Creep(g)) = world.room_mut("W1N1").unwrap().objects.get_mut(&ghost_id) {
        g.spawning = true;
    }
    let creep2 = normalize(
        "creep",
        &serde_json::json!({ "pos": [12, 10], "owner": "p2", "body": ["move"], "name": "c2" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", creep2).unwrap();
    let spawn1 = normalize(
            "spawn",
            &serde_json::json!({ "pos": [13, 10], "structureType": "spawn", "owner": "p1", "name": "s1" }),
            &ctx,
        )
        .unwrap();
    world.insert("W1N1", spawn1).unwrap();
    let spawn2 = normalize(
            "spawn",
            &serde_json::json!({ "pos": [14, 10], "structureType": "spawn", "owner": "p2", "name": "s2" }),
            &ctx,
        )
        .unwrap();
    world.insert("W1N1", spawn2).unwrap();
    let ext1 = normalize(
        "structure",
        &serde_json::json!({ "pos": [15, 10], "structureType": "extension", "owner": "p1" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", ext1).unwrap();
    let tower2 = normalize(
        "structure",
        &serde_json::json!({ "pos": [16, 10], "structureType": "tower", "owner": "p2" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", tower2).unwrap();
    let wall = normalize(
        "structure",
        &serde_json::json!({ "pos": [17, 10], "structureType": "constructedWall" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", wall).unwrap();
    let site1 = normalize(
        "constructionSite",
        &serde_json::json!({ "pos": [18, 10], "structureType": "road", "owner": "p1" }),
        &ctx,
    )
    .unwrap();
    world.insert("W1N1", site1).unwrap();

    // 清除归属的控制器不应进入己方或敌方结构集合。
    let cleared_ctl = RoomObject::Controller(ControllerObj {
        id: 0,
        x: 2,
        y: 1,
        level: 1,
        safe_mode: 0,
        safe_mode_available: 0,
        progress: 0,
        downgrade_time: None,
        owner: None,
        owner_cleared: true,
        is_power_enabled: false,
        reservation: None,
        sign: None,
        upgrade_blocked: None,
        safe_mode_cooldown: None,
    });
    world.insert("W1N1", cleared_ctl).unwrap();
    let count = |f: i64, v: Option<&str>| world.find_in_room("W1N1", f, v).len();
    // FIND_CREEPS：玩家面滤雏形（game.js:18 !i.spawning）；中立面含雏形
    assert_eq!(count(c::FIND_CREEPS, Some("p1")), 2, "c1+c2，雏形被滤");
    assert_eq!(count(c::FIND_CREEPS, None), 3, "中立面含孵化雏形");
    // MY/HOSTILE creep（game.js:18-19）
    assert_eq!(count(c::FIND_MY_CREEPS, Some("p1")), 1);
    assert_eq!(count(c::FIND_MY_CREEPS, Some("p2")), 1);
    assert_eq!(count(c::FIND_HOSTILE_CREEPS, Some("p1")), 1, "p2 的 c2");
    assert_eq!(
        count(c::FIND_HOSTILE_CREEPS, Some("p2")),
        1,
        "p1 的 c1（雏形滤）"
    );
    // MY/HOSTILE spawns（game.js:22-23）
    assert_eq!(count(c::FIND_MY_SPAWNS, Some("p1")), 1);
    assert_eq!(count(c::FIND_HOSTILE_SPAWNS, Some("p1")), 1);
    // MY/HOSTILE structures（game.js:24-25）：spawn+extension+tower 归属
    // 分野；wall 无 owner（my undefined）与降级清零 controller（owner:
    // null 哨兵）两边都不进
    assert_eq!(count(c::FIND_MY_STRUCTURES, Some("p1")), 2, "s1+ext1");
    assert_eq!(count(c::FIND_MY_STRUCTURES, Some("p2")), 2, "s2+tower");
    assert_eq!(
        count(c::FIND_HOSTILE_STRUCTURES, Some("p1")),
        2,
        "s2+tower；wall 无 owner/cleared ctl 的 null owner 都不进"
    );
    assert_eq!(count(c::FIND_HOSTILE_STRUCTURES, Some("p2")), 2, "s1+ext1");
    // MY/HOSTILE construction sites（game.js:26-27）
    assert_eq!(count(c::FIND_MY_CONSTRUCTION_SITES, Some("p1")), 1);
    assert_eq!(count(c::FIND_HOSTILE_CONSTRUCTION_SITES, Some("p2")), 1);
    assert_eq!(count(c::FIND_MY_CONSTRUCTION_SITES, Some("p2")), 0);
    // 中立面 MY_*：不可判定 → 空
    assert_eq!(count(c::FIND_MY_CREEPS, None), 0);
    assert_eq!(count(c::FIND_HOSTILE_STRUCTURES, None), 0);
}

/// 房间能量只统计查看者的非 off spawn；内部全量投影则统计所有主人。
#[test]
fn energy_per_viewer_projection() {
    let mut world = World::new(3);
    world.time = 1;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let ctx = PlaceCtx {
        links: crate::place::builtin_link_rules(),

        time: world.time,
        controller: None,
        name_seq: 0,
    };
    let mk_spawn = |name: &str, owner: &str, energy: i64| {
        normalize(
            "spawn",
            &serde_json::json!({ "pos": [10, 10], "structureType": "spawn", "owner": owner,
                    "name": name, "store": { "energy": energy } }),
            &ctx,
        )
        .unwrap()
    };
    world.insert("W1N1", mk_spawn("s1", "p1", 100)).unwrap();
    let off_id = world.insert("W1N1", mk_spawn("s1off", "p1", 50)).unwrap();
    // place 对 off 硬编码 false——insert 后直改（_calc_spawns 的 off 标记）
    if let Some(RoomObject::Spawn(s)) = world.room_mut("W1N1").unwrap().objects.get_mut(&off_id) {
        s.off = true;
    }
    world.insert("W1N1", mk_spawn("s2", "p2", 200)).unwrap();
    let j1 = world.world_json_for_viewer("p1", 0);
    // p1 只见自家非 off spawn：100/300（off 的 50 不计、p2 的 200 不计）
    assert_eq!(j1["rooms"]["W1N1"]["energyAvailable"], 100);
    assert_eq!(j1["rooms"]["W1N1"]["energyCapacityAvailable"], 300);
    let j2 = world.world_json_for_viewer("p2", 0);
    assert_eq!(j2["rooms"]["W1N1"]["energyAvailable"], 200);
    assert_eq!(j2["rooms"]["W1N1"]["energyCapacityAvailable"], 300);
    // 内部全量投影统计所有非 off spawn。
    let jn = world.world_json();
    assert_eq!(jn["rooms"]["W1N1"]["energyAvailable"], 300);
    assert_eq!(jn["rooms"]["W1N1"]["energyCapacityAvailable"], 600);
}

/// 矿物不属于 FIND_STRUCTURES，提取器属于结构；creep 额外资源须进入 store。
#[test]
fn mineral_and_extractor_projection_and_find() {
    let mut world = World::new(42);
    world.time = 100;
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    world.register_room("W2N2", Terrain::plains());
    let mineral = world
        .insert(
            "W1N1",
            RoomObject::Mineral(kk_schema::room::MineralObj {
                id: 0,
                x: 10,
                y: 10,
                mineral_type: "U".into(),
                mineral_amount: 35_000,
                density: 2,
                next_regeneration_time: Some(150),
            }),
        )
        .unwrap();
    let extractor = world
        .insert(
            "W1N1",
            RoomObject::Extractor(kk_schema::room::ExtractorObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 400,
                owner: Some("p1".into()),
                cooldown: 3,
                notify_when_attacked: Some(true),
            }),
        )
        .unwrap();
    let hauler = world
        .insert(
            "W1N1",
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: 1500,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![kk_schema::room::BodyPart {
                    kind: 2,
                    hits: 100,
                    boost: None,
                }],
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "U".into(),
                    amount: 12,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();

    let j = world.object_json_by_id(mineral).unwrap();
    assert_eq!(j["type"], "structure");
    assert_eq!(
        j["structureType"], "mineral",
        "FIND 装配面（game.js:329-345 独立分支）"
    );
    assert_eq!(j["mineralType"], "U");
    assert_eq!(j["mineralAmount"], 35_000);
    assert_eq!(j["density"], 2);
    assert_eq!(j["ticksToRegeneration"], 50, "getter 折叠 150−100");
    // 无钟折叠 0（prelude ?? 0 兜底口径——与 source 同形）
    if let RoomObject::Mineral(mm) = world.get_mut(mineral).unwrap() {
        mm.next_regeneration_time = None;
    }
    assert_eq!(
        world.object_json_by_id(mineral).unwrap()["ticksToRegeneration"],
        0
    );

    let e = world.object_json_by_id(extractor).unwrap();
    assert_eq!(e["structureType"], "extractor");
    assert_eq!(e["hits"], 400);
    assert_eq!(e["hitsMax"], c::EXTRACTOR_HITS, "build.js:218 hitsMax");
    assert_eq!(e["cooldown"], 3, "计数器原样（getter o.cooldown || 0）");
    assert_eq!(e["owner"], "p1");
    assert_eq!(e["notifyWhenAttacked"], true);

    // FIND：mineral 只进 FIND_MINERALS；extractor 进 FIND_STRUCTURES 且按
    // 归属进 MY_*（116 臂的两键判定）
    assert_eq!(world.find_in_room("W1N1", c::FIND_MINERALS, None).len(), 1);
    assert_eq!(
        world.find_in_room("W1N1", c::FIND_STRUCTURES, None).len(),
        1
    );
    assert_eq!(
        world
            .find_in_room("W1N1", c::FIND_MY_STRUCTURES, Some("p1"))
            .len(),
        1,
        "extractor 归 p1"
    );
    assert_eq!(
        world
            .find_in_room("W1N1", c::FIND_MY_STRUCTURES, Some("p2"))
            .len(),
        0
    );

    // creep 的命名矿物正量投影（store getter 全资源口径）
    let h = world.object_json_by_id(hauler).unwrap();
    assert_eq!(h["store"]["U"], 12);
    assert_eq!(h["store"]["energy"], 0, "energy 恒投影（既有形状）");

    // 视野：仅有一枚有主 extractor 的房间对其主人可见
    let viewer = world.world_json_for_viewer("p1", 0);
    let rooms = viewer["rooms"].as_object().unwrap();
    assert!(
        rooms["W1N1"].get("terrain").is_some(),
        "extractor 主人保住视野"
    );
    assert!(rooms["W2N2"].get("terrain").is_some(), "地形面恒在");

    // 无主 extractor：owner 缺键（投影无 owner ⇒ my undefined）
    let neutral = world
        .insert(
            "W2N2",
            RoomObject::Extractor(kk_schema::room::ExtractorObj {
                id: 0,
                x: 5,
                y: 5,
                hits: 500,
                owner: None,
                cooldown: 0,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let ne = world.object_json_by_id(neutral).unwrap();
    assert!(ne.get("owner").is_none(), "无主不落 owner 键");
}

/// 传入的累计 GCL 点数原样投影，等级换算交给 prelude。
#[test]
fn gcl_points_projection() {
    let mut world = World::new(1);
    let mut terrain = Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    let j = world.world_json_for_viewer("p1", 1_000_000);
    assert_eq!(j["gcl"], 1_000_000);
    assert_eq!(j["user"], "p1");
    assert_eq!(world.world_json()["gcl"], 0);
}
