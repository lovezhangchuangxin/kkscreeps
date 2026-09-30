//! M2B 扩面 P4：结构衰减三件（roads/containers/ramparts tick.js 结算）。
//! 官方对照：processor.js:365/:369/:389 的 phase 8 分派；
//! ramparts/tick.js（hitsMax 随房主 rcl 同步 + 300/100 衰减）；
//! roads/tick.js（100/1000 衰减、沼泽 ×5/墙 ×150 地形系数）；
//! containers/tick.js（5000/窗衰减——有主房窗 500/野房 100、死亡全量
//! store 落地、重挂钟）。

use super::*;

#[test]
fn container_decay_drops_store_and_dies() {
    // 有主房：窗 CONTAINER_DECAY_TIME_OWNED=500；一次衰减掉 CONTAINER_DECAY
    //=5000 ≥ 任何初始 hits → 死亡路径 + store 落地
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
                .to_string(),
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
                .to_string(),
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

// ---- 建造落成入口的衰减钟（官方 build.js:161-168 rampart / :255-263 container）----
//
// P4 引入 phase 8 衰减后，建造入口若不建钟，`nextDecayTime` 缺失会被判据
// `next != 0 && time < next-1` 放行（next==0 视为「已到点」）——建成
// rampart（hits=RAMPART_HITS=1）下一 tick 即被 RAMPART_DECAY_AMOUNT=300
// 打出负值移除，建成 container 首 tick 误掉 CONTAINER_DECAY=5000。官方
// 落成即挂钟（rampart 100 / container CONTAINER_DECAY_TIME）。

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
    // 官方 ramparts/tick.js:19-24：hitsMax 同步在衰减钟门**之外**、每 tick
    // 执行。本用例给远钟（未到点）→ hits 不动，但 hitsMax 必须当 tick 归一；
    // 若同步被放进门内（本轮修掉的回归形态），hitsMax 会停在 5000。
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
                .to_string(),
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

// ---- 建造落成入口的容量对 / 冷却钟（官方 build.js:222-233 lab / :265-274 nuker）----

/// 官方 build.js 对 lab 落**成对**的两键：平面 `storeCapacity`（energy+mineral
/// 合计 3000，build.js:230）与 per-resource `storeCapacityResource.energy`
///（LAB_ENERGY_CAPACITY=2000，:231）。只落平面会让 `Store.getCapacity()` 由
/// 官方 null 变 3000；只落 per-resource 会让 `energyCapacity` getter 读 0。
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

// ---- link 的 cooldown 计数器递减（官方 processor.js:378 links/tick.js）----
//
// link 与 lab/terminal/nuker/factory 的冷却**形态不同**（vanilla
// index.ts:1145-1158 明文）：link 存每 tick 递减计数器 `o.cooldown`，getter
// 直读 `o.cooldown || 0`。不实现递减则放置时写的冷却永不归零。

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
                .to_string(),
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
    // 官方：`if(object.cooldown > 0)` 才减，且减后钳 0——0 不再下降（不为负）。
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
