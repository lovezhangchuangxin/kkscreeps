//! 测试道路、容器和 rampart 衰减，以及新建结构的容量和初始时钟。

use super::*;

#[test]
fn cached_decay_gate_refreshes_owner_and_clock_without_losing_dynamic_fields() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let raw = r#"{ "type":"structure", "structureType":"rampart", "owner":"p1", "hits":5000, "hitsMax":1000000, "nextDecayTime":100, "custom":{"value":7} }"#;
    let id = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 12,
                y: 12,
                data: raw.into(),
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    let Some(RoomObject::Generic(g)) = f.world.get(id).map(|(_, o)| o) else {
        panic!("应存活")
    };
    assert_eq!(g.data.as_str(), raw, "未到期且上限不变时不改写 JSON");
    assert_eq!(g.data.identity().unwrap().owner.as_deref(), Some("p1"));
    f.world
        .edit_object(id, |object| {
            let RoomObject::Generic(g) = object else {
                panic!("应为 Generic")
            };
            let mut value: Value = serde_json::from_str(&g.data).unwrap();
            value["owner"] = "p2".into();
            g.data = value.to_string().into();
            Ok(())
        })
        .unwrap();
    f.settle(IntentBatch::default());
    let value = f.world.object_json_by_id(id).unwrap();
    assert_eq!(value["hitsMax"], 0, "归属变化后未到衰减点也同步上限");
    assert_eq!(value["hits"], 5000);
    assert_eq!(value["custom"]["value"], 7);
    let due = f.world.time() + 1;
    f.world
        .edit_object(id, |object| {
            let RoomObject::Generic(g) = object else {
                panic!("应为 Generic")
            };
            let mut value: Value = serde_json::from_str(&g.data).unwrap();
            value["nextDecayTime"] = due.into();
            g.data = value.to_string().into();
            Ok(())
        })
        .unwrap();
    f.settle(IntentBatch::default());
    let value = f.world.object_json_by_id(id).unwrap();
    assert_eq!(value["hits"], 4700, "时钟替换后在正确拍衰减");
    assert_eq!(value["custom"]["value"], 7);
}

#[test]
fn container_decay_drops_store_and_dies() {
    // 本例初始耐久为 5000，一次 CONTAINER_DECAY=5000 恰好使容器消失。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let id = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "hits": 5000, "nextDecayTime": 1,
                    "store": {"energy": 50, "U": 30},
                })
                .to_string()
                .into(),
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(id).is_none(), "hits 5000−5000≤0 → 移除");
    // 落地堆在 phase 8 新插入不入本轮（官方 lodash 键快照同语义）——本
    // tick 不受 energy 衰减相影响，amount 原值
    // 逐键校验 energy 与 U 的**量**——只断言「存在一个堆」区分不了全量落地
    // 与丢资源/少落。
    let mut energy_amt = None;
    let mut u_amt = None;
    for o in f.world.room("W1N1").unwrap().objects.values() {
        if let RoomObject::Energy(e) = o
            && e.x == 10
            && e.y == 10
        {
            match e.resource.as_deref() {
                // 既有编码：create_resource("energy") 落 res_code=0 + Some("energy")
                None | Some("energy") => energy_amt = Some(e.amount),
                Some("U") => u_amt = Some(e.amount),
                Some(_) => {}
            }
        }
    }
    assert_eq!(energy_amt, Some(50), "energy 全量落地（非仅存在）");
    assert_eq!(u_amt, Some(30), "U 同 tick 逐键落地");
}

#[test]
fn rampart_decay_and_hitsmax_sync() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // RCL 3 有主房：hitsMax 同步 RAMPART_HITS_MAX["3"]=1_000_000；一次衰减
    // 300 → 存活 + 重挂钟
    f.controller(3, Some("p1"));
    let id = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 12,
                y: 12,
                data: serde_json::json!({
                    "type": "structure", "structureType": "rampart",
                    "owner": "p1", "hits": 5000, "hitsMax": 5000,
                    "nextDecayTime": 1,
                })
                .to_string()
                .into(),
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => {
            let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(v["hits"], serde_json::json!(4700), "5000−300");
            assert_eq!(v["hitsMax"], serde_json::json!(1_000_000), "房主 rcl3 同步");
            assert_eq!(
                v["nextDecayTime"],
                serde_json::json!(100),
                "重挂钟（time 0 + RAMPART_DECAY_TIME）"
            );
        }
        other => panic!("应存活：{other:?}"),
    }
}

#[test]
fn road_decay_terrain_ratios() {
    // 平原：100/tick 窗（clock=1 → 到点）；沼泽：×5=500
    let mut f = Fixture::new(1, terrain_with(&[(11, 11)]));
    let plain = f
        .world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 1000,
                next_decay_time: 1,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let swamp = f
        .world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 11,
                y: 11,
                hits: 1000,
                next_decay_time: 1,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    match (
        f.world.get(plain).map(|(_, o)| o),
        f.world.get(swamp).map(|(_, o)| o),
    ) {
        (Some(RoomObject::Road(a)), Some(RoomObject::Road(b))) => {
            assert_eq!(a.hits, 900, "平原 ROAD_DECAY_AMOUNT=100");
            assert_eq!(b.hits, 500, "沼泽 ×CONSTRUCTION_COST_ROAD_SWAMP_RATIO=5");
        }
        other => panic!("双路存活：{other:?}"),
    }
}

// 新建结构必须带初始衰减时钟；0 时钟立即触发衰减，会让新 rampart 消失，
// 或让新 container 提前损失耐久。来源：上游 creeps/build.js。

/// 取房间内某 structureType 的 Generic 对象 data（解析后）。
fn generic_data(f: &Fixture, structure_type: &str) -> Option<Value> {
    f.world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Generic(g) => {
                let v: Value = serde_json::from_str(&g.data).ok()?;
                (v.get("structureType").and_then(Value::as_str) == Some(structure_type))
                    .then_some(v)
            }
            _ => None,
        })
}

#[test]
fn built_rampart_gets_decay_clock() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let b = f.creep(
        "b1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        50,
    );
    let site = f.site(11, 10, "rampart", 0, 1, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::Build, IntentData::target(site));
    f.settle(batch);

    let data = generic_data(&f, "rampart").expect("rampart 落成");
    assert_eq!(data["hits"], serde_json::json!(1), "RAMPART_HITS");
    assert_eq!(
        data["nextDecayTime"],
        serde_json::json!(kk_core::constants::RAMPART_DECAY_TIME as u64),
        "官方 build.js:167 建成即挂 100 tick 钟"
    );
    f.settle(IntentBatch::default());
    assert!(
        generic_data(&f, "rampart").is_some(),
        "衰减窗前不得消失（缺钟实现会在本 tick 被打光移除）"
    );
}

#[test]
fn built_container_gets_capacity_and_decay_clock() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let b = f.creep(
        "b1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        50,
    );
    let site = f.site(11, 10, "container", 0, 1, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::Build, IntentData::target(site));
    f.settle(batch);

    let data = generic_data(&f, "container").expect("container 落成");
    assert_eq!(
        data["storeCapacity"],
        serde_json::json!(kk_core::constants::CONTAINER_CAPACITY),
        "官方 build.js:258 平面 storeCapacity"
    );
    assert_eq!(
        data["nextDecayTime"],
        serde_json::json!(kk_core::constants::CONTAINER_DECAY_TIME as u64),
        "官方 build.js:261 建成即挂窗钟"
    );
    assert_eq!(
        data["hits"],
        serde_json::json!(kk_core::constants::CONTAINER_HITS),
        "首 tick 不得误衰减"
    );
}

#[test]
fn rampart_hitsmax_syncs_before_decay_window() {
    // 将衰减时钟设在远期，验证未衰减时仍同步 hitsMax。
    // 如果同步放在到期检查之后，hits 不变但 hitsMax 会错误地停在旧值 5000。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 12,
                y: 12,
                data: serde_json::json!({
                    "type": "structure", "structureType": "rampart",
                    "owner": "p1", "hits": 5000, "hitsMax": 5000,
                    "nextDecayTime": 1_000_000,
                })
                .to_string()
                .into(),
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());

    let v = generic_data(&f, "rampart").expect("rampart 存活");
    assert_eq!(v["hits"], serde_json::json!(5000), "未到点不动 hits");
    assert_eq!(
        v["hitsMax"],
        serde_json::json!(1_000_000),
        "hitsMax 当 tick 随 rcl3 归一（RAMPART_HITS_MAX[\"3\"]）"
    );
    assert_eq!(
        v["nextDecayTime"],
        serde_json::json!(1_000_000),
        "远钟未到点不重挂"
    );
}

#[test]
fn road_decay_rearms_clock() {
    // 官方 roads/tick.js:25：衰减后重挂 nextDecayTime = gameTime + ROAD_DECAY_TIME
    let mut f = Fixture::new(1, terrain_with(&[]));
    let id = f
        .world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 1000,
                next_decay_time: 1,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Road(r)) => assert_eq!(
            r.next_decay_time,
            kk_core::constants::ROAD_DECAY_TIME as u64,
            "重挂钟（结算时 time 0 + ROAD_DECAY_TIME）"
        ),
        other => panic!("road 应存活：{other:?}"),
    }
}

/// Lab 同时保存总容量与按资源能量容量；缺任一项会改变玩家 Store 的返回值。
/// 数值来自 LAB_ENERGY_CAPACITY 和 LAB_MINERAL_CAPACITY，分别为 2000 和 3000。
/// 来源：上游 creeps/build.js 的 lab 初始化。
#[test]
fn built_lab_gets_paired_capacity() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let b = f.creep(
        "b1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        50,
    );
    let site = f.site(11, 10, "lab", 0, 1, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::Build, IntentData::target(site));
    f.settle(batch);

    let data = generic_data(&f, "lab").expect("lab 落成");
    assert_eq!(
        data["storeCapacity"],
        serde_json::json!(
            kk_core::constants::LAB_ENERGY_CAPACITY + kk_core::constants::LAB_MINERAL_CAPACITY
        ),
        "平面容量 = energy + mineral"
    );
    assert_eq!(
        data["storeCapacityResource"]["energy"],
        serde_json::json!(kk_core::constants::LAB_ENERGY_CAPACITY),
        "per-resource 容量必须与平面容量成对落"
    );
}

/// 官方 build.js:265-274：nuker 落成即带 cooldownTime（时间戳形态，非 link 的
/// 计数器）——`gameTime + NUKER_COOLDOWN`。
#[test]
fn built_nuker_gets_cooldown_time() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let b = f.creep(
        "b1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        50,
    );
    let site = f.site(11, 10, "nuker", 0, 1, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::Build, IntentData::target(site));
    f.settle(batch);

    let data = generic_data(&f, "nuker").expect("nuker 落成");
    assert_eq!(
        data["storeCapacityResource"]["energy"],
        serde_json::json!(kk_core::constants::NUKER_ENERGY_CAPACITY)
    );
    assert_eq!(
        data["storeCapacityResource"]["G"],
        serde_json::json!(kk_core::constants::NUKER_GHODIUM_CAPACITY)
    );
    assert_eq!(
        data["cooldownTime"],
        serde_json::json!(kk_core::constants::NUKER_COOLDOWN as u64),
        "落成即挂冷却钟（结算时 time 0 + NUKER_COOLDOWN）"
    );
}

/// 插入一个 Generic link（放置面形态：structureType=link + 计数器 cooldown）。
fn insert_link(f: &mut Fixture, cooldown: u64) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 13,
                y: 13,
                data: serde_json::json!({
                    "type": "structure", "structureType": "link",
                    "owner": "p1", "cooldown": cooldown,
                    "store": {"energy": 0},
                    "storeCapacity": kk_core::constants::LINK_CAPACITY,
                    "storeCapacityResource": {"energy": kk_core::constants::LINK_CAPACITY},
                    "hits": kk_core::constants::LINK_HITS,
                    "hitsMax": kk_core::constants::LINK_HITS_MAX,
                })
                .to_string()
                .into(),
            }),
        )
        .unwrap()
}

#[test]
fn link_cooldown_decrements_each_tick() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let id = insert_link(&mut f, 7);
    for expected in [6u64, 5, 4] {
        f.settle(IntentBatch::default());
        let v = generic_data(&f, "link").expect("link 存活");
        assert_eq!(
            v["cooldown"],
            serde_json::json!(expected),
            "每 tick 递减（官方 links/tick.js:11-22）"
        );
        let _ = id;
    }
}

#[test]
fn link_cooldown_stops_at_zero() {
    // Cooldown 为 0 时不再递减，不写负数。来源：上游 links/tick.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    insert_link(&mut f, 1);
    f.settle(IntentBatch::default());
    assert_eq!(
        generic_data(&f, "link").unwrap()["cooldown"],
        serde_json::json!(0)
    );
    f.settle(IntentBatch::default());
    assert_eq!(
        generic_data(&f, "link").unwrap()["cooldown"],
        serde_json::json!(0),
        "钳 0，不落负值"
    );
}

#[test]
fn build_progress_and_completion() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "constructedWall", 0, 1, "p1");
    // 1 work → 5/tick（BUILD_POWER=5）；墙成本 1 → 一次完成
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    let events = f.settle(b);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_BUILD);
    assert_eq!(events[0]["data"]["incomplete"], false);
    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&site),
        "工地移除"
    );
    let wall = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructedWall(w) => Some(w),
            _ => None,
        })
        .unwrap();
    assert_eq!(wall.hits, 1, "官方落成墙 hits=WALL_HITS(1)");
    assert_eq!(f.creep_state(c).3, 9, "耗 1 能量");
}

#[test]
fn road_on_swamp_costs_more_to_build() {
    // 本例直接放置 progress_total=1500 的道路工地，验证建造使用保存的总进度。
    // 不覆盖工地创建时的沼泽倍率计算。
    let mut f = Fixture::new(1, terrain_with(&[(12, 10)]));
    let site = f.site(12, 10, "road", 0, 1500, "p1");
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    let events = f.settle(b);
    assert_eq!(
        events[0]["data"]["incomplete"], true,
        "5 能量 < 1500 不完成"
    );
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => assert_eq!(s.progress, 5),
        _ => panic!(),
    }
}
