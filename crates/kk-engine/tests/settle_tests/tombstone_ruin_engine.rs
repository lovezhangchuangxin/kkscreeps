//! B1 结构治理批自 settle_tests.rs 连续切块（不重排；原段落横幅注释随块
//! 保留；Fixture/helpers 经 `use super::*` 从 crate 根取用——142 个
//! #[test] 计数与原文序不变）。

use super::*;

// ---- B3：墓碑/废墟/死亡事件族 ----

/// 墓碑 store 逐资源锁值（_die.js:43-53 的 bodyResources 精确数值）：
/// boosted 件产出 boost 矿物（LAB_BOOST_MINERAL × lifeRate）+ 能量
/// （LAB_BOOST_ENERGY × lifeRate），每件能量 min(CREEP_PART_MAX_ENERGY,
/// cost × lifeRate)；携带 store 全资源并入（第二循环）。
#[test]
fn tombstone_body_recovery_and_boost_minerals() {
    // 自杀路径（suicide.js:14 → _die 缺省 dropRate 0.2）；age_time = 0+1500 ⇒
    // ticksToLive = 1500、无 CLAIM ⇒ lifeTime 1500、lifeRate = 0.2×1500/1500 = 0.2
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut bodylist = body(&[BodyKind::Work, BodyKind::Move]);
    bodylist[0].boost = Some("XUHO2".into());
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: bodylist,
                store: kk_schema::room::Store {
                    energy: 10,
                    ghodium: 25,
                },
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("suicide 产墓碑");
    // XUHO2 = 30×0.2 = 6；energy = 20×0.2 + min(125, 100×0.2) + min(125, 50×0.2)
    // = 4 + 20 + 10 = 34；携带 10 并入同槽 ⇒ 44；G = 携带 25
    assert_eq!(tomb.store.energy, 44, "回收 34 + 携带 10");
    assert_eq!(tomb.store.ghodium, 25, "携带 G 并入");
    assert_eq!(
        tomb.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "XUHO2".into(),
            amount: 6,
        }],
        "boost 矿物回收（LAB_BOOST_MINERAL × lifeRate）"
    );
    assert_eq!(tomb.creep_ticks_to_live, 1500, "ageTime − gameTime");
    assert_eq!(tomb.decay_time, 10, "2 部件 × TOMBSTONE_DECAY_PER_PART");
}

/// 同格 container 先承接（_die.js:36/45-59：container.hits>0 时按
/// CONTAINER_CAPACITY − 全槽合计 封顶，余量进墓碑——body 回收与携带两段
/// 各自过 container 门）。
#[test]
fn tombstone_container_deposit_first() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // container（Generic 透传）：store.energy=1900 ⇒ 余量 100
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 250000,
                    "store": { "energy": 1900 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    // 满寿自杀（fixture age_time 极大 ⇒ lifeRate 巨大 ⇒ move 件钳 125）：
    // 第一段回收 125 → container 吃 100 后满，25 进墓碑；
    // 第二段携带 150 → 余量 0，150 全进墓碑 ⇒ 175
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 150);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.store.energy),
            _ => None,
        })
        .expect("墓碑仍建（余量进槽）");
    assert_eq!(
        tomb, 175,
        "125 回收（container 吃 100）+ 150 携带（余量 0）"
    );
    match f.obj(container) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(
                data["store"]["energy"], 2000,
                "container 吃满 CONTAINER_CAPACITY"
            );
        }
        _ => panic!(),
    }
}

/// 墓碑衰减到点（tombstones/tick.js:5-16）：全 store 逐资源落地（同格异资源
/// 不并堆——_create-energy 按资源名键）、墓碑移除；新掉落不在本轮对象快照，
/// 当 tick 不衰减。
#[test]
fn tombstone_decay_dumps_store_per_resource() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let t = f
        .world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 10,
                y: 10,
                death_time: 0,
                decay_time: 1, // time=0 结算：0 >= decay_time-1 = 0 即到点
                creep_id: None,
                creep_name: Some("w1".into()),
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 7,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 5,
                }],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(t).is_none(), "衰减到点移除");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![
            ("energy".to_string(), 40),
            ("G".to_string(), 7),
            ("XUHO2".to_string(), 5),
        ],
        "三资源各成一堆（BTreeMap id 序 = 落地插入序：energy→G→extra）"
    );
}

/// 废墟衰减到点（ruins/tick.js）与墓碑同形：全 store 落地 + 移除。
#[test]
fn ruin_decay_dumps_store_and_removes() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let r = f
        .world
        .insert(
            "W1N1",
            RoomObject::Ruin(kk_schema::room::RuinObj {
                id: 0,
                x: 12,
                y: 12,
                structure_id: 99,
                structure_type: "container".into(),
                structure_hits_max: Some(250000),
                destroy_time: 0,
                decay_time: 1,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 30,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "LH".into(),
                    amount: 4,
                }],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(r).is_none(), "废墟衰减到点移除");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 12 && e.y == 12 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 30), ("LH".to_string(), 4)],
        "能量与 LH 各成一堆（落地插入序）"
    );
}

/// B3 评审 2/3 P1 曾锁"命名矿物堆拒拾"（双槽 schema 期）；B4 多资源槽清偿：
/// pickup.js:26-30 按堆 resourceType 入开放 store 对应键——XUHO2 堆拾入
/// 命名槽（store_extra），不再洗白进 energy；power 堆维持拒绝（power 无
/// 建模槽位，随 power 批放开——B3 登记口径不变）。
#[test]
fn pickup_mineral_pile_into_named_slot_and_power_still_rejected() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let pile = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                resource: Some("XUHO2".into()),
                amount: 6,
            }),
        )
        .unwrap();
    let power_pile = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 1,
                resource: None,
                amount: 8,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Carry], 0);
    // 同对象同名 intent 后者覆盖（官方语义）——两堆分两轮 settle
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Pickup, IntentData::target(pile));
    f.settle(batch);
    // XUHO2 堆整堆拾走（amount 6 ≤ 余量 50——堆移除）
    assert!(f.world.get(pile).is_none(), "矿物堆拾空即移除");
    match f.obj(c) {
        RoomObject::Creep(cr) => {
            assert_eq!(cr.store.energy, 0, "矿物不得洗白进 energy 槽");
            assert_eq!(cr.store.ghodium, 0);
            assert_eq!(
                cr.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 6,
                }],
                "拾入命名槽（官方 store[res] 键）"
            );
        }
        _ => panic!("非 creep"),
    }
    // power 堆维持拒绝（第二轮）：8 − 2 = 6（两 tick 各衰减 1，energy/tick.js
    // 全资源同式）
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Pickup, IntentData::target(power_pile));
    f.settle(batch);
    match f.obj(power_pile) {
        RoomObject::Energy(e) => {
            assert_eq!(kk_engine::settle::energy_resource_name(e), "power");
            assert_eq!(e.amount, 6, "power 堆不被拾走（两 tick 各 −1）");
        }
        _ => panic!("power 堆必须仍在"),
    }
}

/// B3 评审 3-2 锁值：container 余量的**跨资源竞争序**（官方 bodyResources 键
/// 序 = energy 先、矿物按 body 首现序——_die.js:38-76 两段循环间容量是活的）。
/// headroom=10 只够吃部分 energy：energy 先入 container、矿物全进墓碑。
#[test]
fn tombstone_container_cross_resource_competition() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // container 余量 10（1990/2000）
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 250000,
                    "store": { "energy": 1990 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    // body [Work(XUHO2), Move]、ttl=1500 自杀 → lifeRate=0.2×1500/1500=0.2：
    // energy 回收 = min(125,100×0.2) + 20×0.2 + min(125,50×0.2) = 20+4+10 = 34；
    // XUHO2 = 30×0.2 = 6。键序：energy 34 先过门（container 吃 10 → 墓碑 24、
    // 余量 0）→ XUHO2 6 全进墓碑
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: Some("XUHO2".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    match f.obj(container) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["store"]["energy"], 2000, "container 吃满");
            assert!(
                data["store"].get("XUHO2").is_none(),
                "键序 energy 先——余量 10 全被 energy 吃掉，矿物不进 container"
            );
        }
        _ => panic!(),
    }
    match f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        }) {
        Some(t) => {
            assert_eq!(t.store.energy, 24, "34−10 进墓碑");
            assert_eq!(
                t.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 6
                }],
                "矿物 6 全进墓碑（余量竞争可观察面锁值）"
            );
        }
        None => panic!("墓碑必须建"),
    }
}

/// B3 评审 3-3 锁值：引擎产废墟对混合 store（Generic 开放资源面）的承接全链
/// ——container(energy+XUHO2) 击毁 → ruin.store 承接（含 store_extra）→
/// 衰减落地两堆。
#[test]
fn ruin_inherits_mixed_store_and_dumps_on_decay() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 1,
                    "store": { "energy": 20, "XUHO2": 8 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    let dis = f.creep("dis", "p1", 11, 10, &[BodyKind::Work], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(container));
    let events = f.settle(batch);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED,
        "击毁事件"
    );
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) => Some(r.clone()),
            _ => None,
        })
        .expect("废墟承接");
    assert_eq!(ruin.store.energy, 20);
    assert_eq!(
        ruin.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "XUHO2".into(),
            amount: 8
        }],
        "混合 store 的非 energy/G 资源进 store_extra"
    );
    // 加速衰减到点（放置面直改钟——decay 语义本身另有锁值）
    for o in f.world.room_mut("W1N1").unwrap().objects.values_mut() {
        if let RoomObject::Ruin(r) = o {
            r.decay_time = 1;
        }
    }
    f.settle(IntentBatch::default());
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 20), ("XUHO2".to_string(), 8)],
        "废墟衰减落地两堆（承接全链闭环）"
    );
}

/// B3 评审 3-4 锁值：CLAIM 部件的寿命分母（CREEP_CLAIM_LIFE_TIME=600——
/// _.any 不看 hits）。ttl=600 自杀 → lifeRate=0.2×600/600=0.2：回收
/// min(125,50×0.2)+min(125,600×0.2)=10+120=130（错分母 1500 会得 4+48=52）。
#[test]
fn claim_body_life_time_denominator() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 600,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Claim.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.store.energy),
            _ => None,
        })
        .expect("墓碑仍建");
    assert_eq!(tomb, 130, "CLAIM 分母 600：(50+600)×0.2");
}

/// B3 评审 3-6 边界：!decayTime（放置面 0）立即到点——官方
/// tombstones/ruins/tick.js:9 `!object.decayTime || ...` 的 falsy 分支。
#[test]
fn tombstone_zero_decay_time_dumps_immediately() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let t = f
        .world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 10,
                y: 10,
                death_time: 0,
                decay_time: 0,
                creep_id: None,
                creep_name: None,
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: None,
                store: kk_schema::room::Store {
                    energy: 12,
                    ghodium: 0,
                },
                store_extra: vec![],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(t).is_none(), "decayTime=0 falsy 立即到点");
    let energy = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some(e.amount),
            _ => None,
        })
        .sum::<u32>();
    assert_eq!(energy, 12);
}

/// structure.destroy() 的 room intent（destroy-structure.js：可建类型 +
/// controller 归本人 + 房内无敌方 creep）；事件为套件口径（OSS 无 push——
/// ROOM-EVENTLOG-027 期望有，登记报告）。
#[test]
fn destroy_structure_intent_paths() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ext = extension_at(&mut f, 15, 15, 1000, 80);
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext,
        },
    );
    let events = f.settle(b);
    assert!(f.world.get(ext).is_none(), "自家结构可毁");
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) => Some(r.clone()),
            _ => None,
        })
        .expect("destroy 产废墟");
    assert_eq!(ruin.structure_type, "extension");
    assert_eq!(ruin.store.energy, 80, "store 承接");
    // 官方 destroy-structure.js 无事件（GPT 复审 P1-4 清偿——套件 parity.json
    // 自认 vanilla 不产、expected 是文档推断；事件只在 _damage/_die 路径）
    assert_eq!(events.len(), 0, "destroy 不产事件（对齐官方）");

    // 门 1：房内有敌方 creep → 拒（destroy-structure.js:18）
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    f2.controller(1, Some("p1"));
    let ext2 = extension_at(&mut f2, 15, 15, 1000, 0);
    f2.creep("spy", "p2", 20, 20, &[BodyKind::Move], 0);
    let mut b2 = IntentBatch::default();
    b2.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext2,
        },
    );
    f2.settle(b2);
    assert!(f2.world.get(ext2).is_some(), "敌方 creep 在房拒毁");

    // 门 2：controller 非本人 → 拒（:14）
    let mut f3 = Fixture::new(3, terrain_with(&[]));
    f3.controller(1, Some("p2"));
    let ext3 = extension_at(&mut f3, 15, 15, 1000, 0);
    let mut b3 = IntentBatch::default();
    b3.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext3,
        },
    );
    f3.settle(b3);
    assert!(f3.world.get(ext3).is_some(), "非 controller 主人拒毁");

    // 门 3：非 CONSTRUCTION_COST 类型（source）→ 拒（:12）
    let mut f4 = Fixture::new(4, terrain_with(&[]));
    f4.controller(1, Some("p1"));
    let src4 = f4.source(30, 30, 3000);
    let mut b4 = IntentBatch::default();
    b4.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: src4,
        },
    );
    f4.settle(b4);
    assert!(
        f4.world.get(src4).is_some(),
        "source 无 CONSTRUCTION_COST 拒毁"
    );
}

/// 事件缓冲随房持久化（官方 saveRoomEventLog 每 tick 整体覆写——
/// processor.js:497）：有事件的 tick 落字符串数组、无事件的 tick 清空。
#[test]
fn room_events_buffer_overwritten_per_tick() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Work, BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    let stored = f.world.room("W1N1").unwrap().events.clone();
    assert_eq!(stored.len(), 1, "事件随房落账");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored[0]).unwrap(),
        events[0],
        "缓冲是结算事件的序列化原文"
    );
    f.settle(IntentBatch::default());
    assert!(
        f.world.room("W1N1").unwrap().events.is_empty(),
        "无事件 tick 覆写为空（官方 hset 整体覆写）"
    );
}

// ---- 完整闭环（同 tick 采+升） ----

#[test]
fn engine_closed_loop_harvest_and_upgrade() {
    // source 与 controller 相邻放置：worker 每 tick 同时采 4、升 4
    // （upgradeController 不在 harvest 的压制表——官方同 tick 双动作）
    let mut f = Fixture::new(9, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.source(6, 6, 3000);
    let src = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Source(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        7,
        7,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );

    for _ in 0..10 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
        f.settle(b);
    }
    // 净存量断言随 carry 容量调整
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.progress, 20, "10 tick × 2（UPGRADE_CONTROLLER_POWER=1）")
        }
        _ => panic!(),
    }
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000 - 40, "采 4/tick × 10"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 20, "采 40 升 20，净存 20");
    assert_eq!(f.players["p1"].gcl, 20);
}

// ---- 评审回归测试（每条 high 修复一条锁） ----

#[test]
fn stationary_creep_blocks_movement() {
    // H1：非获准移动的 creep 是障碍（对穿的另一半）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let _still = f.creep("still", "p1", 11, 10, &[BodyKind::Move], 0);
    let mover = f.creep("mover", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(mover, 3)); // RIGHT → (11,10) 有静止 creep
    assert_eq!(f.creep_state(mover), (10, 10, 0, 0), "静止 creep 挡路");
}

#[test]
fn swap_with_third_contender_rates() {
    // H2：对穿者受压优先（rate1=100）——A↔B 对穿时，抢 A 目标格的 C 应被 A 压制
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move], 0);
    let c = f.creep("c", "p1", 12, 10, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 3); // a: (10,10)→(11,10)（b 让位）
    let mut d = IntentData::bare();
    d.direction = Some(7);
    batch.add_object_intent("p1", b, Action::Move, d); // b: (11,10)→(10,10)
    let mut d2 = IntentData::bare();
    d2.direction = Some(7);
    batch.add_object_intent("p1", c, Action::Move, d2); // c: (12,10)→(11,10) 抢 a 的目标
    f.settle(batch);
    // 官方：a 的 rate1=100（有竞争者从其目标格移出）必胜 c
    assert_eq!(f.creep_state(a).0, 11, "受压对穿者 a 胜出");
    assert_eq!(f.creep_state(b).0, 10);
    assert_eq!(f.creep_state(c).0, 12, "c 被 a 压制原地");
}

#[test]
fn lighter_creep_wins_contested_cell() {
    // H3：rate4 = moves/体重——轻 creep 压过重 creep（平手 rate1 时）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let heavy = f.creep(
        "heavy",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    let light = f.creep("light", "p1", 12, 12, &[BodyKind::Move], 0);
    let mut batch = move_intent(heavy, 4); // → (11,11)
    let mut d = IntentData::bare();
    d.direction = Some(8);
    batch.add_object_intent("p1", light, Action::Move, d); // → (11,11)
    f.settle(batch);
    // light rate4=1/1 > heavy 1/2 → light 胜
    assert_eq!(
        (f.creep_state(light).0, f.creep_state(light).1),
        (11, 11),
        "轻者胜"
    );
    assert_eq!(f.creep_state(heavy).0, 10, "重者被压制");
}

#[test]
fn road_on_swamp_costs_one() {
    // H5：路上的沼泽按路走（rate 1 而非 10）
    let mut f = Fixture::new(1, terrain_with(&[(10, 11)]));
    f.world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                notify_when_attacked: None,
                id: 0,
                x: 10,
                y: 11,
                hits: 5000,
                next_decay_time: 9999,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle(move_intent(c, 5)); // → (10,11) 沼泽+路
    // rate 1：+1×1 后恢复 -2 → 0
    assert_eq!(f.creep_state(c).2, 0, "路上的沼泽按路走");
}

#[test]
fn road_on_swamp_costs_more_to_build() {
    // H6：沼泽上的路 progressTotal ×5（300→1500，建工地时定格存值）
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

#[test]
fn birth_blocked_by_stationary_creep_until_it_leaves() {
    // 深评 H2：出生占位对任何 creep（友敌皆算）都算被占——'creep' ∈
    // OBSTACLE_OBJECT_TYPES（_born-creep.js:12-15）；被占时推迟重试、雏形
    // 留在 spawn 格，让开后才出生。blocker 的逃离方向必须真正可走（远离
    // spawn），否则测试测不到"被占"阶段
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    // 方向 1 的格 (20,19) 站一个静止 creep
    let blocker = f.creep("blocker", "p1", 20, 19, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    d.directions = vec![1]; // 只允许方向 1（被 block）
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    // 推 5 tick 到出生日（spawn_time=6，time>=5 触发）
    for _ in 0..5 {
        f.settle(IntentBatch::default());
    }
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .expect("雏形存在");
    assert!(chick.2, "被占：不出世");
    assert_eq!(
        (chick.0, chick.1),
        (20, 20),
        "雏形留在 spawn 格（不与 blocker 同格）"
    );
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().map(|j| j.spawn_time),
            Some(6),
            "出生失败推迟一 tick 重试"
        ),
        _ => panic!(),
    }
    // blocker 真正走开（方向 4 → (21,20)，远离 spawn）；spawn tick 在 blocker
    // 的 creep tick 之前（id 序）→ 当 tick 仍失败、下一 tick 出生
    f.settle(move_intent(blocker, 4));
    f.settle(IntentBatch::default());
    let child = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .expect("出生成功");
    assert!(!child.2);
    assert_eq!((child.0, child.1), (20, 19), "blocker 让开后出生在方向 1");
}

#[test]
fn fatigue_recovers_from_persisted_value() {
    // H4：累加器从持久 fatigue 播种——静止恢复每 tick -2×MOVE
    let mut f = Fixture::new(1, terrain_with(&[(10, 11), (10, 12)]));
    // 3 work + 1 move：沼泽移动 fatigue = 3×10 - 2 = 28
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[
            BodyKind::Move,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );
    f.settle(move_intent(c, 5)); // → (10,11) 沼泽
    assert_eq!(f.creep_state(c).2, 28, "3×10 - 恢复 2");
    // 静止恢复：28 → 26 → 24（不是归零）
    f.settle(IntentBatch::default());
    assert_eq!(f.creep_state(c).2, 26, "从持久值恢复 -2");
    f.settle(IntentBatch::default());
    assert_eq!(f.creep_state(c).2, 24);
}
