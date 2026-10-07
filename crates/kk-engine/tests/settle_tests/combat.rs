//! 测试近战、修理、拆解与治疗，以及伤后身体效能和携带容量变化。

use super::*;

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

mod healing;
mod melee;
mod ranged;
mod ranges;
mod tough;

#[test]
fn heal_target_validation_rejections() {
    // heal.js 各前置门：spawning 施疗者（:11-13）、非 creep 目标（:17-19）、
    // spawning 目标（:17-19）、射程外（:21-23）、他人 safeMode（:25-27）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let medic = f.creep("medic", "p1", 10, 10, &[BodyKind::Heal], 0);
    let ally = f.creep("ally", "p1", 12, 10, &[BodyKind::Tough], 0);
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
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let ext = extension_at(&mut f, 11, 11, 1000, 0);
    // 近战射程 2 拒
    assert!(f.settle(heal_intent(medic, ally)).is_empty(), "射程 2 拒");
    // 非 creep 目标拒
    assert!(f.settle(heal_intent(medic, ext)).is_empty(), "结构不可治");
    // spawning 目标拒
    assert!(
        f.settle(heal_intent(medic, chick)).is_empty(),
        "spawning 拒"
    );
    // spawning 施疗者拒
    let mut b0 = IntentBatch::default();
    b0.add_object_intent("p1", chick, Action::Heal, IntentData::target(medic));
    assert!(f.settle(b0).is_empty(), "spawning 施疗者拒");
    // 他人 safeMode 拒（heal.js:25-27 与 attack 同形）
    let ctl = f.controller(2, Some("p2"));
    f.world
        .edit_object(ctl, |object| {
            if let Some(RoomObject::Controller(t)) = Some(object) {
                t.safe_mode = 1000;
            }
            Ok(())
        })
        .unwrap();
    let near = f.creep("near", "p1", 11, 10, &[BodyKind::Tough], 0);
    assert!(f.settle(heal_intent(medic, near)).is_empty(), "safeMode 拒");
    assert_eq!(creep_parts(&f, near).0, 100);
}
