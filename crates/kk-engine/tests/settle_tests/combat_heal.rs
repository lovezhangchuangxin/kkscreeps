//! 测试近战、修理、拆解与治疗，以及伤后身体效能和携带容量变化。

use super::*;

#[test]
fn safe_mode_blocks_hostile_attack_and_dismantle() {
    // attack.js:30-32 / dismantle.js:24-26：他人 safeMode 生效期拒（引擎层二次门）。
    // attack 与 dismantle 各自独立 settle 验证——两者同 creep 同 tick 会互相压制
    // （dismantle 在 attack 的压制表里），单一空事件断言无法归因到 attack 的门
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Work], 0);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
                id: 0,
                x: 10,
                y: 11,
                hits: 1000,
            }),
        )
        .unwrap();
    // 直接设置控制器的安全模式，单独验证攻击和拆解的结算检查。
    f.world
        .edit_object(ctl, |object| {
            if let Some(RoomObject::Controller(t)) = Some(object) {
                t.safe_mode = 1000;
            }
            Ok(())
        })
        .unwrap();
    let ext = extension_at(&mut f, 11, 10, 1000, 0);
    // attack 的门：单独 settle
    let events = f.settle(attack_intent(atk, ext));
    assert!(events.is_empty(), "safeMode：攻击拒");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    // dismantle 的门：单独 settle
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", atk, Action::Dismantle, IntentData::target(wall));
    let events2 = f.settle(batch);
    assert!(events2.is_empty(), "safeMode：拆解拒");
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 1000),
        _ => panic!(),
    }
}

#[test]
fn repair_effect_cost_and_clamps() {
    // repair.js:23-27：effect=min(功率,能量×100,缺口)；cost=min(存量,ceil(effect×0.01))
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ext = extension_at(&mut f, 12, 10, 900, 0);
    let w1 = f.creep("fixer", "p1", 11, 10, &[BodyKind::Work, BodyKind::Carry], 1);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_REPAIR);
    assert_eq!(events[0]["data"]["amount"], 100, "1×WORK×100");
    assert_eq!(events[0]["data"]["energySpent"], 1, "ceil(100×0.01)=1");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "钳到 hitsMax"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(w1).3, 0, "能量扣尽");

    // 缺口小于功率：effect 钳缺口，cost 仍按 ceil 计 1
    f.world
        .edit_object(ext, |object| {
            if let Some(RoomObject::Extension(e)) = Some(object) {
                e.hits = 950;
            }
            Ok(())
        })
        .unwrap();
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events2 = f.settle(b2);
    assert_eq!(events2.len(), 0, "无能量：整条拒绝（repair.js:11）");
    f.world
        .edit_object(ext, |object| {
            if let Some(RoomObject::Extension(e)) = Some(object) {
                e.hits = 950;
            }
            Ok(())
        })
        .unwrap();
    f.world
        .edit_object(w1, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.store.energy = 5;
            }
            Ok(())
        })
        .unwrap();
    let mut b3 = IntentBatch::default();
    b3.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events3 = f.settle(b3);
    assert_eq!(events3[0]["data"]["amount"], 50, "缺口 50 钳制");
    assert_eq!(events3[0]["data"]["energySpent"], 1, "ceil(50×0.01)=1");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(w1).3, 4);

    // 满血目标拒绝（repair.js:16 target.hits >= hitsMax）
    let mut b4 = IntentBatch::default();
    b4.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    assert!(f.settle(b4).is_empty(), "满血不修");

    // 无 WORK 部件：effect 0 → 拒（repair.js:40 `!boostedEffect`）
    let nowork = f.creep("nowork", "p1", 11, 10, &[BodyKind::Carry], 10);
    let mut b5 = IntentBatch::default();
    b5.add_object_intent("p1", nowork, Action::Repair, IntentData::target(ext));
    assert!(f.settle(b5).is_empty());
}

#[test]
fn repair_wall_and_rampart_hits_max() {
    // build.js:192 墙 hitsMax=WALL_HITS_MAX(300M)；ramparts/tick.js:17 按 RCL 表
    let mut f = Fixture::new(1, terrain_with(&[]));
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
                id: 0,
                x: 12,
                y: 10,
                hits: 1,
            }),
        )
        .unwrap();
    let fixer = f.creep(
        "fixer",
        "p1",
        11,
        10,
        &[BodyKind::Work, BodyKind::Carry, BodyKind::Carry],
        100,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", fixer, Action::Repair, IntentData::target(wall));
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["amount"], 100, "300M 缺口不钳功率");
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 101),
        _ => panic!(),
    }

    // rampart：RCL 3 上限 1,000,000（RAMPART_HITS_MAX["3"]）
    f.controller(3, Some("p1"));
    let rampart = generic_structure(&mut f, 12, 11, "rampart", "p1", 999_900);
    let fixer2 = f.creep(
        "fixer2",
        "p1",
        11,
        11,
        &[BodyKind::Work, BodyKind::Carry],
        10,
    );
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", fixer2, Action::Repair, IntentData::target(rampart));
    let events2 = f.settle(b2);
    assert_eq!(events2[0]["data"]["amount"], 100, "缺口 100");
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 1_000_000, "修到 RCL 3 上限");
        }
        _ => panic!(),
    }
}

#[test]
fn dismantle_reflux_and_destroy() {
    // 拆解量取 WORK 功率与目标耐久的较小值，先回收能量再施加伤害。
    // 击毁后原结构库存进入废墟。来源：上游 creeps/dismantle.js 和 structures/_destroy.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let dis = f.creep(
        "dis",
        "p1",
        11,
        10,
        &[
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Carry,
            BodyKind::Carry,
        ],
        0,
    );
    let ext = extension_at(&mut f, 12, 10, 1000, 200);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(ext));
    let events = f.settle(b.clone());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_DISMANTLE
    );
    assert_eq!(events[0]["data"]["damage"], 250, "5×WORK×50");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 750),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(dis).3, 1, "回流 floor(250×0.005)=1");
    // 继续拆：750→500→250→0（第 4 tick 击毁，amount=250 回流 1）
    f.settle(b.clone());
    f.settle(b.clone());
    let events4 = f.settle(b.clone());
    // 结构击毁事件先于主攻击事件。来源：上游 intents/_damage.js。
    assert_eq!(events4.len(), 2);
    assert_eq!(
        events4[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events4[0]["data"]["type"], "extension");
    assert_eq!(events4[1]["data"]["damage"], 250, "amount=min(250,250)");
    assert!(f.world.get(ext).is_none(), "击毁");
    assert_eq!(f.creep_state(dis).3, 4, "四 tick 共回流 4");
    // 原结构库存进入废墟，不作为地面资源参与衰减。
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 12 && r.y == 10 => Some(r.clone()),
            _ => None,
        })
        .expect("拆毁产废墟");
    assert_eq!(ruin.structure_type, "extension");
    assert_eq!(ruin.store.energy, 200, "store 200 全量承接");
    assert_eq!(
        ruin.decay_time,
        3 + 500,
        "第 4 个结算 tick（time=3）+ RUIN_DECAY"
    );
}

#[test]
fn dismantle_redirects_to_rampart() {
    // dismantle.js:27-35：rampart 重定向后 amount 按重定向目标（rampart）的 hits 取
    let mut f = Fixture::new(1, terrain_with(&[]));
    let dis = f.creep("dis", "p1", 11, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let ext = extension_at(&mut f, 12, 10, 1000, 0);
    let rampart = generic_structure(&mut f, 12, 10, "rampart", "p2", 80);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(ext));
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["targetId"], rampart.as_str());
    assert_eq!(events[0]["data"]["damage"], 50, "1×WORK×50");
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 30, "80-50：rampart 存活");
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(dis).3, 0, "floor(50×0.005)=0 无回流");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "本体受保护"),
        _ => panic!(),
    }

    // 功率超过 rampart 血量：amount 钳到 80，恰击毁、回流 0
    let dis2 = f.creep(
        "dis2",
        "p1",
        11,
        12,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let rampart2 = generic_structure(&mut f, 12, 12, "rampart", "p2", 80);
    let ext2 = extension_at(&mut f, 12, 12, 1000, 0);
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", dis2, Action::Dismantle, IntentData::target(ext2));
    let events2 = f.settle(b2);
    // 先产生结构击毁事件，再产生拆解攻击事件；damage 从攻击事件读取。
    assert_eq!(events2.len(), 2);
    assert_eq!(
        events2[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events2[0]["data"]["type"], "rampart");
    assert_eq!(
        events2[1]["data"]["damage"], 80,
        "amount=min(100,80) 钳 rampart 血量"
    );
    assert!(f.world.get(rampart2).is_none(), "80-80=0：击毁");
    // 房间 events 保存的事件与返回事件顺序一致，均为 DESTROYED 在 ATTACK 前。
    let buffered: Vec<serde_json::Value> = f
        .world
        .room("W1N1")
        .unwrap()
        .events
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(buffered.len(), 2);
    assert_eq!(
        buffered[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED,
        "缓冲序 = 官方 _damage.js 序（:51 先于 :93）"
    );
    assert!(f.world.get(ext2).is_some(), "本体未动");
    assert_eq!(f.creep_state(dis2).3, 0, "floor(80×0.005)=0 无回流");
    // rampart 击毁同样产废墟（Generic 透传结构——存照按 data 面）
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 12 && r.y == 12 => Some(r.clone()),
            _ => None,
        })
        .expect("rampart 击毁产废墟");
    assert_eq!(ruin.structure_type, "rampart");
    assert_eq!(ruin.owner.as_deref(), Some("p2"), "存照主人（data.owner）");
    // 本房没有 controller，废墟中的 structure_hits_max 为 None。
    // 上游 rampart 按房主与 RCL 计算上限，本实现保留这一无控制器情况的差异。
    assert_eq!(ruin.structure_hits_max, None);
}

#[test]
fn attack_suppresses_harvest() {
    // intents.js 优先级图：attack 压制 harvest——同 tick 只执行 attack
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let enemy = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Tough, BodyKind::Tough],
        0,
    );
    let hybrid = f.creep(
        "hy",
        "p1",
        10,
        10,
        &[BodyKind::Attack, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", hybrid, Action::Attack, IntentData::target(enemy));
    b.add_object_intent("p1", hybrid, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert!(
        events
            .iter()
            .all(|e| e["event"] != kk_core::constants::EVENT_HARVEST),
        "harvest 被压制：无采集事件"
    );
    assert_eq!(creep_parts(&f, enemy).0, 170, "attack 照常执行");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "source 未被动"),
        _ => panic!(),
    }
}

#[test]
fn carry_death_drops_overflow_energy() {
    // tick.js:141-143 → _drop-resources-without-space：CARRY 部件损毁缩容 → 超容掉落
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f.creep(
        "haul",
        "p2",
        11,
        10,
        &[BodyKind::Carry, BodyKind::Carry],
        80,
    );
    f.settle(attack_intent(a, b));
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 80, "200-120");
    assert_eq!(parts, vec![0, 80], "首个 carry 报废");
    assert_eq!(f.creep_state(b).3, 50, "容量缩至 50");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(30), "超容 80-50 掉地（tick 阶段不衰减）");
}

/// 命名矿物 Z 已占满容量，采得的 U 因超容掉落，energy 和 Z 不变。
/// 来源：上游 creeps/harvest.js 的 calcResources 与矿物 drop 分支。
#[test]
fn harvest_mixed_holding_overflow_drops_mined_mineral() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 70_000, 3, None);
    f.extractor(10, 10, Some("p1"), 0);
    // 容量 50 已被 Z 占满（命名槽）
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 11,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Work, BodyKind::Work, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(batch);
    // 全槽 sum = 50(Z)+2(U 产出)=52 > 50 → 掉 2 U（intent 相掉落同 tick 衰减
    // ceil(2/1000)=1 → 堆 1）；Z 槽不动、energy 不动
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(cc.store.energy, 0, "energy 槽不动（只掉本 tick 产出资源）");
            assert_eq!(
                cc.store_extra,
                vec![
                    kk_schema::room::ResourceStack {
                        resource: "Z".into(),
                        amount: 50
                    },
                    kk_schema::room::ResourceStack {
                        resource: "U".into(),
                        amount: 0
                    },
                ],
                "Z 槽不动；U 产出全掉但键留 0（官方 store[U]=0 落库同构）"
            );
        }
        _ => panic!(),
    }
    let u_pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e)
                if e.x == 10 && e.y == 11 && e.resource.as_deref() == Some("U") =>
            {
                Some(e.amount)
            }
            _ => None,
        });
    assert_eq!(
        u_pile,
        Some(1),
        "掉 2 U 同 tick 衰减 1 → 堆 1（官方出生 tick 衰减一致）"
    );
}

/// Z 已占满容量，source 采集只增加 energy 槽；按全仓超容量掉落 energy。
/// 不能将全仓总量写入 energy 槽，否则会复制已持有的矿物。
#[test]
fn harvest_source_mixed_holding_overflow_drops_energy_only() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f
        .world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 10,
                y: 10,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    // 容量 50 已被 Z 占满
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 11,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Work, BodyKind::Work, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    f.settle(batch);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(
                cc.store.energy, 0,
                "采 4 → 全槽 54>50 → 掉 min(4, 4)=4，energy 归 0"
            );
            assert_eq!(
                cc.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50
                }],
                "Z 槽不动（只掉本 tick 产出的 energy）"
            );
        }
        _ => panic!(),
    }
    // 掉 4 energy 同 tick 衰减 ceil(4/1000)=1 → 堆 3
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 11 => Some(e.amount),
            _ => None,
        });
    assert_eq!(pile, Some(3), "掉 4 同 tick 衰减 1");
}

/// CARRY 损毁后按 RESOURCES_ALL 顺序掉落全部资源中的超量部分。
/// 本例先掉 energy，再掉 Z；对象 tick 中产生的掉落堆本轮不衰减。
#[test]
fn carry_death_drops_overflow_mixed_resources() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "haul".into(),
                owner: "p2".into(),
                body: body(&[BodyKind::Carry, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 60,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    f.settle(attack_intent(a, b));
    // 容量 100→50（毁一 carry）；总持有 40e+60Z=100 > 50 → 掉 50：
    // energy 40 全掉 → Z 补 10 → creep 留 Z 50
    assert_eq!(f.creep_state(b).3, 0, "energy 全掉");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 40), ("Z".to_string(), 10)],
        "序贯掉落 energy 先 Z 补足（tick 相不衰减）"
    );
    match f.obj(b) {
        RoomObject::Creep(cc) => assert_eq!(
            cc.store_extra,
            vec![kk_schema::room::ResourceStack {
                resource: "Z".into(),
                amount: 50
            }],
            "Z 留 50"
        ),
        _ => panic!(),
    }
}

#[test]
fn attack_target_validation_rejections() {
    // attack.js 各前置门：自指（:18）、射程（:21-23）、spawning 目标（:24-26）、
    // 无 hits 目标（:27-29）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    let far = f.creep("far", "p2", 20, 20, &[BodyKind::Tough], 0);
    let src = f.source(10, 11, 3000);
    // spawning 目标：直插 spawning=true 的 creep
    let chick = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: true,
                action_log: 0,
                name: "chick".into(),
                owner: "p2".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let e1 = f.settle(attack_intent(a, a));
    assert!(e1.is_empty(), "自指拒");
    let e2 = f.settle(attack_intent(a, far));
    assert!(e2.is_empty(), "射程外拒");
    let e3 = f.settle(attack_intent(a, src));
    assert!(e3.is_empty(), "source 无 hits 不可攻");
    let e4 = f.settle(attack_intent(a, chick));
    assert!(e4.is_empty(), "spawning 目标拒");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000),
        _ => panic!(),
    }
}

#[test]
fn zero_power_attack_still_logs() {
    // 官方引擎层无部件预检（那是 API 层的事）：无 ATTACK 部件的攻击 intent
    // 仍产 actionLog 与 0 伤害事件（_damage.js:59-93 无条件段）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("weak", "p1", 10, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let b = f.creep("victim", "p2", 11, 10, &[BodyKind::Tough], 0);
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["damage"], 0);
    assert_eq!(creep_parts(&f, b).0, 100, "0 伤害不进伤害路径");
    let (log_a, log_b) = match (f.obj(a), f.obj(b)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED as u16);
}

// 损毁部件下一 tick 不再贡献功率；上游瞬态 _oldHits 的处理见
// settle/damage.rs::distribute_hits 的说明。

#[test]
fn destroyed_attack_part_loses_power_next_tick() {
    // t1：B [attack,attack,tough,tough] 受击 120 → 首个 attack 报废；
    // t2：B 的反击功率 = 1×30（hits>0 计数）而非 2×30；t3：两个 attack 全灭 → 无反击
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    // t1：双方满编——反击 2×30=60
    let events1 = f.settle(attack_intent(a, b));
    assert_eq!(
        events1[0]["data"]["damage"], 60,
        "受损当 tick 部件尚未灭：反击 60"
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(
        (hits, parts),
        (280, vec![0, 80, 100, 100]),
        "首个 attack 报废"
    );
    // t2：反击功率 = 存活 attack 部件数 ×30 = 30（不是 _oldHits 冻结的 60）
    let events2 = f.settle(attack_intent(a, b));
    assert_eq!(events2[0]["data"]["damage"], 30, "损毁部件下一 tick 失效");
    assert_eq!(
        events2[1]["data"]["damage"], 120,
        "攻击者未受损：主伤害不变"
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(
        (hits, parts),
        (160, vec![0, 0, 60, 100]),
        "两个 attack 均灭"
    );
    // t3：无存活 attack 部件 → 无反击事件
    let events3 = f.settle(attack_intent(a, b));
    assert_eq!(events3.len(), 1, "仅主事件");
    assert_eq!(events3[0]["data"]["damage"], 120);
}

#[test]
fn destroyed_work_part_repairs_at_half_power_next_tick() {
    // repair.js:23 的 (hits>0||_oldHits>0) 同族：WORK 部件损毁后下一 tick
    // 修复功率按存活部件数计（_oldHits 已被 removeHidden 剥离）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let fixer = f.creep(
        "fixer",
        "p1",
        11,
        10,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        100,
    );
    let enemy = f.creep("atk", "p2", 10, 10, &[BodyKind::Attack; 4], 0);
    // t1：fixer 受击 120 → 300-120=180 → [0,80,100]（首个 work 报废）
    f.settle(attack_intent_by("p2", enemy, fixer));
    let (hits, parts) = creep_parts(&f, fixer);
    assert_eq!((hits, parts), (180, vec![0, 80, 100]));
    // t2：修复功率 = 1×REPAIR_POWER = 100（不是 _oldHits 冻结的 200）
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
                id: 0,
                x: 12,
                y: 10,
                hits: 1,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", fixer, Action::Repair, IntentData::target(wall));
    let events = f.settle(b);
    assert_eq!(
        events[0]["data"]["amount"], 100,
        "损毁的 WORK 部件下一 tick 不计功率"
    );
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 101),
        _ => panic!(),
    }
}

#[path = "combat_heal/healing.rs"]
mod healing;
