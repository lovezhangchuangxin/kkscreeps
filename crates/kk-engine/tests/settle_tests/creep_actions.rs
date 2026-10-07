//! creep 动作与生命周期（say/suicide 等）域测试。

use super::*;

#[test]
fn say_intent_sets_action_log_bit() {
    // say.js:18-21：actionLog.say 落位（u16 位图 bit9）；spawning 拒；
    // message 缺省按官方 userString 折叠 "" 同样落位（isString 门恒过）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    let b1 = f.creep("b1", "p1", 11, 11, &[BodyKind::Move], 0);

    let mut batch = IntentBatch::default();
    let mut d = IntentData::bare();
    d.message = Some("hello".into());
    d.is_public = Some(true);
    batch.add_object_intent("p1", c, Action::Say, d);
    let d2 = IntentData::bare(); // 缺省 message（None ≙ 官方 ""）
    batch.add_object_intent("p1", b1, Action::Say, d2);
    assert!(f.world.get(c).is_some(), "settle 前对象在");
    assert!(f.world.get(b1).is_some(), "settle 前b1在");
    let events_dbg = f.settle(batch);
    println!("DBG events: {}", serde_json::Value::Array(events_dbg));
    assert!(f.world.get(c).is_some(), "settle 后 c 还在");

    let action_log = |fx: &Fixture, id: ObjId| match fx.obj(id) {
        RoomObject::Creep(cc) => cc.action_log,
        _ => panic!("非 creep"),
    };
    println!(
        "DBG after settle: c={} b1={}",
        f.world.get(c).is_some(),
        f.world.get(b1).is_some()
    );
    // 这里检查 say 动作位；消息载荷、公开性和重置由 combat_ranged 的测试覆盖。
    assert_eq!(action_log(&f, c), 1 << 9, "say 落 ACTION_SAY 位");
    assert_eq!(
        action_log(&f, b1),
        1 << 9,
        "缺省 message（None ≙ 官方 \"\"）同落位"
    );

    // Say 的孵化拒绝用例需要匹配的 spawn 任务，防止 creep 因孤儿清理而消失。
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let spawn_id = f2
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(kk_schema::room::SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 0,
                off: false,
                name: "S".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store::default(),
                spawning: Some(kk_schema::room::SpawningJob {
                    name: "sp1".into(),
                    need_time: 3,
                    spawn_time: 5,
                    directions: vec![],
                }),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let s = f2
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: true,
                action_log: 0,
                name: "sp1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: kk_schema::room::Store::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let _ = spawn_id;
    let mut batch2 = IntentBatch::default();
    batch2.add_object_intent(
        "p1",
        s,
        Action::Say,
        IntentData {
            message: Some("x".into()),
            ..IntentData::bare()
        },
    );
    f2.settle(batch2);
    assert_eq!(
        action_log(&f2, s),
        0,
        "spawning 中 say 不落位（say.js:11-13）"
    );
}

#[test]
fn suicide_intent_kills_creep_same_tick() {
    // 自杀意图立即移除 creep，生成墓碑和死亡事件。来源：上游 creeps/suicide.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 40);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    let events = f.settle(batch);
    assert!(f.world.get(c).is_none(), "suicide 当 tick 死亡");
    // 夹具 age_time 极大，回收量按每件上限 125 钳制；携带 40 加两件回收 250
    // 得到墓碑 energy=290，两件身体对应 10 tick 衰减期限。
    let tomb = f
        .world
        .rooms()
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("suicide 产墓碑");
    assert_eq!(tomb.store.energy, 290, "携带 40 + 造价回收 250");
    assert!(
        !f.world
            .rooms()
            .get("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "不直接掉地（墓碑承接）"
    );
    // 死亡事件类型为 EVENT_OBJECT_DESTROYED，来源：上游 creeps/_die.js。
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");

    // 本实现删除对象后跳过其余动作，因此 suicide 后的 upgradeController 不生效。
    // 上游处理器持有死亡前对象引用，仍可能执行后续升级；这是保留的行为差异。
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    let ctl = f2.controller(1, Some("p1"));
    let c2 = f2.creep("w2", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 100);
    let mut batch2 = IntentBatch::default();
    batch2.add_object_intent("p1", c2, Action::Suicide, IntentData::bare());
    batch2.add_object_intent("p1", c2, Action::UpgradeController, IntentData::target(ctl));
    f2.settle(batch2);
    assert!(f2.world.get(c2).is_none(), "对象消亡");
    match f2.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.progress, 0, "死后 upgradeController 静默跳过")
        }
        _ => panic!(),
    }
}

/// 孵化 creep 的 age_time 初值为 0，出生后首次进入寿命分支时初始化。
/// 本例 spawn 先于 creep 执行 tick，所以出生同轮即初始化；其他对象顺序可能推迟一轮。
/// 任意 CLAIM 件使用 600 tick 寿命，否则为 1500；失去 spawn 的孵化对象被清理。
#[test]
fn chick_age_time_model() {
    // ① [move] 雏形：孵化中 0，出生 tick（time 2）赋 2+1500
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1：任务建（spawn_time 3）、雏形插入
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.name == "child" => Some(*id),
            _ => None,
        })
        .unwrap();
    match f.obj(chick) {
        RoomObject::Creep(c) => {
            assert!(c.spawning);
            assert_eq!(c.age_time, 0, "官方雏形不带 ageTime");
        }
        _ => panic!(),
    }
    f.settle(IntentBatch::default()); // time 1→2（1 < 3-1 未到点）
    f.settle(IntentBatch::default()); // time 2→3：出生（2 >= 3-1）
    match f.obj(chick) {
        RoomObject::Creep(c) => {
            assert!(!c.spawning, "已出生");
            assert_eq!(
                c.age_time, 1502,
                "出生 tick 首触赋值 time(2)+CREEP_LIFE_TIME"
            );
        }
        _ => panic!(),
    }

    // ② CLAIM 部件：CREEP_CLAIM_LIFE_TIME(600)（claim 成本 600——spawn 备足）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 700);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("claimer".into());
    d.body = vec![BodyKind::Claim];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1
    f.settle(IntentBatch::default()); // time 1→2
    f.settle(IntentBatch::default()); // time 2→3：出生（2 >= 3-1）
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.name == "claimer" => Some(*id),
            _ => None,
        })
        .unwrap();
    match f.obj(chick) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 602, "CLAIM 部件 600"),
        _ => panic!(),
    }

    // ③ 孵化中 spawn 消亡 → 雏形被 creep tick 移除（孤儿自愈）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("orphan".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1：雏形在（同格 spawn 在——正常孵化）
    let orphan = chick_of(&f, "orphan");
    assert!(f.world.get(orphan).is_some());
    f.world.remove(spawn).unwrap(); // spawn 消亡
    f.settle(IntentBatch::default()); // time 1→2：孤儿雏形移除
    assert!(
        f.world.get(orphan).is_none(),
        "同格无 spawn 的孵化中雏形移除"
    );
}

#[test]
fn creep_dies_at_age_time() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 直接造一个快死的（age_time = time+2 → 第二 tick 死）
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 9,
                y: 9,
                fatigue: 0,
                age_time: 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "old".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move, BodyKind::Work]),
                store: kk_schema::room::Store {
                    energy: 30,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // 官方 creeps/tick.js:93：gameTime >= ageTime-1 → time=1 的第二次结算死亡
    f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_some(), "time=0 < age_time-1=1：未死");
    let events = f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_none(), "time=1 >= 1：死亡");
    // TTL 到期时剩余寿命为 1，身体造价回收向下取整为 0，墓碑只保留携带能量 30。
    // 两件身体对应 10 tick 衰减期限。
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
        .expect("TTL 死亡产墓碑");
    assert_eq!(tomb.store.energy, 30, "携带进墓碑（回收 floor 0）");
    assert_eq!(tomb.death_time, 1);
    assert_eq!(tomb.decay_time, 1 + 10, "2 部件 × TOMBSTONE_DECAY_PER_PART");
    assert_eq!(tomb.creep_ticks_to_live, 1, "ageTime − gameTime");
    assert_eq!(
        tomb.creep_body,
        vec![BodyKind::Move.code(), BodyKind::Work.code()],
        "尸体 body 类型码"
    );
    // 死亡产生 EVENT_OBJECT_DESTROYED，data.type 为 creep。来源：上游 creeps/_die.js。
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");
}
