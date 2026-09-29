//! B1 结构治理批自 settle_tests.rs 连续切块（不重排；原段落横幅注释随块
//! 保留；Fixture/helpers 经 `use super::*` 从 crate 根取用——142 个
//! #[test] 计数与原文序不变）。

use super::*;

#[test]
fn source_capacity_3000_under_reservation() {
    // sources/tick.js:46-49：`user || reservation` → SOURCE_ENERGY_CAPACITY
    //(3000)；无主无预定 → 1500（B2 清偿项）。预约未过期 → 两种 tick 次序下
    // 都存活，断言与对象 id 序无关
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 5000,
        });
    });
    let src = f.source(10, 10, 1200);
    // fixture 的 source 挂 3000 容量——先压到 1500 模拟无预定期的归一
    if let RoomObject::Source(s) = f.world.get_mut(src).unwrap() {
        s.energy_capacity = 1500;
    }
    f.settle(IntentBatch::default());
    match f.obj(src) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy_capacity, 3000, "有效预定 → 3000");
            assert_eq!(s.energy, 1200, "抬升分支不钳 energy");
        }
        _ => panic!(),
    }
    // 无主无预定 → 1500 + 钳 energy（:41-45）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller(0, None);
    let src2 = f2.source(10, 10, 2000);
    if let RoomObject::Source(s) = f2.world.get_mut(src2).unwrap() {
        s.energy_capacity = 3000;
    }
    f2.settle(IntentBatch::default());
    match f2.obj(src2) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy_capacity, 1500, "无主无预定 → 1500");
            assert_eq!(s.energy, 1500, "降容量钳 energy");
        }
        _ => panic!(),
    }
}

#[test]
fn harvest_rejected_under_foreign_reservation() {
    // harvest.js:31-34：controller 被他人预定 → harvest 拒（B2 补的分支）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 5000,
        });
    });
    let src = f.source(10, 10, 3000);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let events = f.settle(controller_intent(c, Action::Harvest, src));
    assert!(events.is_empty(), "他人预定房 harvest 拒");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "能量未被动"),
        _ => panic!(),
    }
    // 自己的预定放行
    if let Some(t) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .values_mut()
        .find_map(|o| match o {
            RoomObject::Controller(t) => Some(t),
            _ => None,
        })
    {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 5000,
        });
    }
    let events2 = f.settle(controller_intent(c, Action::Harvest, src));
    assert!(
        events2
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_HARVEST),
        "自己的预定放行"
    );
}

#[test]
fn downgrade_sets_safe_mode_cooldown() {
    // controllers/tick.js:64-69：降一级也清 available 并挂冷却（M1 只做了
    // available 清零，B2 补冷却）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(2, Some("p1"), |t| {
        t.safe_mode_available = 2;
        t.downgrade_time = Some(0); // time=0：0 >= 0-1 触发降级
    });
    let _ = ctl;
    f.settle(IntentBatch::default());
    let t = ctl_of(&f);
    assert_eq!(t.level, 1, "2 → 1");
    assert_eq!(t.safe_mode_available, 0);
    assert_eq!(
        t.safe_mode_cooldown,
        Some(kk_core::constants::SAFE_MODE_COOLDOWN as u64),
        "降级挂冷却"
    );
    // 进度回退：progress += round(CONTROLLER_LEVELS[1] × 0.9) = 180
    assert_eq!(t.progress, 180, "LEVELS[1]=200 × 0.9 → 180");
    // 新挂钟 = 0 + DOWNGRADE[1]/2 + 1 = 10001
    assert_eq!(t.downgrade_time, Some(10_001));
}

#[test]
fn engine_recomputes_player_rooms_after_tick() {
    // user.rooms 不动点（claimController.js:34-35 的 GCL 读数 +
    // driver addRoomToUser/removeRoomFromUser 的等价承载）：Engine 每 tick 后
    // 按世界真值重建——同 tick 内读数保持上一 tick 定集（官方 bulk addToSet
    // 不改内存对象，bulk.js:101-107）
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone());
    world.register_room("W2N2", t);
    for name in ["W1N1", "W2N2"] {
        world
            .insert(
                name,
                RoomObject::Controller(kk_schema::room::ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level: if name == "W1N1" { 1 } else { 0 },
                    safe_mode: 0,
                    safe_mode_available: 0,
                    progress: 0,
                    downgrade_time: None,
                    owner_cleared: false,
                    owner: if name == "W1N1" {
                        Some("p1".into())
                    } else {
                        None
                    },
                    is_power_enabled: false,
                    reservation: None,
                    sign: None,
                    upgrade_blocked: None,
                    safe_mode_cooldown: None,
                }),
            )
            .unwrap();
    }
    let mut engine = Engine::new(world);
    engine.players.entry("p1".to_string()).or_default().gcl = 1_000_000;
    // 空 tick 后 rooms 重建 = {W1N1}
    engine.tick().unwrap();
    assert_eq!(
        engine.players["p1"].rooms,
        BTreeSet::from(["W1N1".to_string()])
    );
    // W2N2 claim：rooms=1 → 需要 GCL 1e6（恰好放行）
    let w2_ctl = *engine
        .world
        .room("W2N2")
        .unwrap()
        .objects
        .keys()
        .next()
        .unwrap();
    let claimer = engine
        .world
        .insert(
            "W2N2",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 5,
                y: 6,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Claim]),
                store: Default::default(),
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent(
        "p1",
        claimer,
        Action::ClaimController,
        IntentData::target(w2_ctl),
    );
    engine.tick_with_intents(b).unwrap();
    assert_eq!(
        engine.players["p1"].rooms,
        BTreeSet::from(["W1N1".to_string(), "W2N2".to_string()]),
        "claim 后纳入 rooms"
    );
    match engine
        .world
        .room("W2N2")
        .unwrap()
        .objects
        .values()
        .find(|o| matches!(o, RoomObject::Controller(_)))
    {
        Some(RoomObject::Controller(c)) => {
            assert_eq!(c.level, 1);
            assert_eq!(c.owner.as_deref(), Some("p1"));
        }
        _ => panic!(),
    }
}

#[test]
fn activate_safe_mode_owner_gate_at_dispatch() {
    // processor.js:248：`object.user && object.user != userId` → 整对象 intent
    // 跳过——敌人对他人 controller 发 activateSafeMode 在分派层丢弃
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001);
    });
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert_eq!(t.safe_mode, 0, "敌方 intent 分派层丢弃");
    assert_eq!(t.safe_mode_available, 1, "available 未被动");
}

#[test]
fn activate_safe_mode_null_cooldown_at_time_zero_rejects() {
    // activateSafeMode.js:14 的 JS null 数值比较角落：未挂冷却（null → 0）
    // 时 `0 >= gameTime` 在 gameTime 0 为真——拒绝。我方 None 统一按 null
    // 语义建模（undefined 初态的分歧仅放置态可达，见 controller.rs 注释）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(20_001);
    });
    assert_eq!(f.world.time, 0);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode, 0, "time=0 + 冷却 None（0>=0）拒");
}

// ---- A4-iii：unclaim（官方 processor/intents/controllers/unclaim.js）----

#[test]
fn unclaim_clears_ownership_and_sets_cooldown() {
    // unclaim.js:15-25：user=null（→ owner_cleared 哨兵）、level=0、progress=0、
    // downgradeTime=null、safeMode=null、safeModeAvailable=0、冷却
    // gameTime+SAFE_MODE_COOLDOWN（roomInfo.novice 免冷却路径——我方无新手区
    // 恒挂钟）、isPowerEnabled=false。**不清 reservation/sign/upgradeBlocked**
    // （官方 bulk update 键表无此三项——与降级归零路径 tick.js:52-63 的刻意
    // 差异：sign 跨 unclaim 存续是玩家可观察语义；reservation 由 controllers/
    // tick 的到期/已占条件另行清除，unclaim 后无主不满足）。removeRoomFromUser
    // （:15）由 Engine rooms 重建承载（engine_recomputes_player_rooms_after_tick）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 100;
    let ctl = f.controller_full(4, Some("p1"), |t| {
        t.progress = 12_345;
        t.safe_mode = 500;
        t.safe_mode_available = 3;
        t.downgrade_time = Some(9_000);
        t.is_power_enabled = true;
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 10_000,
        });
        t.sign = Some(kk_schema::room::Sign {
            user: "p1".into(),
            text: "hi".into(),
            time: 50,
            datetime: None,
        });
        t.upgrade_blocked = Some(700);
    });
    // 同 tick 追加 activateSafeMode：processor.js:307-309 unclaim 先序——归零
    // 后 activate 的 `!object.user || !object.level` 再拒（官方同序）
    let mut b = controller_action("p1", ctl, Action::Unclaim, IntentData::bare());
    b.add_object_intent("p1", ctl, Action::ActivateSafeMode, IntentData::bare());
    let events = f.settle(b);
    assert!(events.is_empty(), "unclaim 无事件（官方处理器不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.owner, None);
    assert!(
        t.owner_cleared,
        "官方 user=null 落库哨兵 → 玩家侧 my === false"
    );
    assert_eq!(t.level, 0);
    assert_eq!(t.progress, 0);
    assert_eq!(
        t.safe_mode, 0,
        "同 tick activateSafeMode 被 unclaim 先序压掉"
    );
    assert_eq!(t.safe_mode_available, 0);
    assert_eq!(t.downgrade_time, None);
    assert_eq!(
        t.safe_mode_cooldown,
        Some(100 + 50_000),
        "冷却 = time + SAFE_MODE_COOLDOWN（无新手区恒挂钟）"
    );
    assert!(!t.is_power_enabled);
    assert_eq!(
        t.sign.as_ref().map(|s| s.text.as_str()),
        Some("hi"),
        "sign 存续"
    );
    assert_eq!(
        t.upgrade_blocked,
        Some(700),
        "upgradeBlocked 不清（官方键表无）"
    );
    assert_eq!(
        t.reservation.as_ref().map(|r| r.end_time),
        Some(10_000),
        "reservation 存续（未到期且 unclaim 后无主——tick 清除条件不满足）"
    );
}

#[test]
fn unclaim_rejections() {
    // unclaim.js:9-12 `!object.user || !object.level` 早退（不挂冷却、不置
    // 哨兵）；分派层（processor.js:248 `object.user && object.user != userId`）
    // 他人 controller 的 intent 整对象跳过
    // ① 无主 + 0 级拒
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller(0, None);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert!(!t.owner_cleared, "无主：哨兵不置（从未拥有）");
    assert_eq!(t.safe_mode_cooldown, None, "无主：不挂冷却");
    // ② 有主但 0 级拒
    f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(0, Some("p1"), |_| {});
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode_cooldown, None, "0 级：不挂冷却");
    // ③ 他人 controller：分派层丢弃
    f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(3, Some("p2"), |t| {
        t.safe_mode_available = 1;
    });
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert_eq!(t.owner.as_deref(), Some("p2"), "敌方 intent 分派层丢弃");
    assert_eq!(t.level, 3);
    assert_eq!(t.safe_mode_available, 1);
}

#[test]
fn engine_safemode_gate4_backstop_drops_cross_room_activation() {
    // 门④可信侧兜底（A4-iii 评审 2 必修锁值）：官方 game 层在 isolated-vm
    // 可信 realm，门④（任一自家 controller safeMode 生效 → ERR_BUSY，
    // structures.js:223-225）处理器不复核；我方 prelude 与玩家共享 realm，
    // 迭代器污染可令门④扫描失明——Engine 层按世界真值兜底：玩家已有生效
    // 中的 safeMode → 其 ActivateSafeMode intent 全部丢弃（双房 safeMode
    // 并存是官方不可能态）
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone());
    world.register_room("W2N2", t);
    let mk_ctl = |safe_mode: u64, level: u8| {
        RoomObject::Controller(kk_schema::room::ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level,
            safe_mode,
            safe_mode_available: 1,
            progress: 0,
            downgrade_time: Some(20_000),
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        })
    };
    // W1N1：safeMode 已生效（time=3 起算挂 1000）；W2N2：五门真值可通过
    let a = world.insert("W1N1", mk_ctl(3 + 1000, 1)).unwrap();
    let b = world.insert("W2N2", mk_ctl(0, 1)).unwrap();
    let mut engine = Engine::new(world);
    // 恶意场景：绕过 prelude 门④直接构造 B 房的 ActivateSafeMode intent
    //（embed 侧收集器闭包私有本不可达——本测试模拟的就是门④失明的等价批）
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::ActivateSafeMode, IntentData::bare());
    engine.tick_with_intents(batch).unwrap();
    let ctl = engine.world.room("W2N2").unwrap().objects.get(&b).unwrap();
    match ctl {
        RoomObject::Controller(c) => {
            assert_eq!(c.safe_mode, 0, "A 房生效中 → B 房激活被 Engine 兜底丢弃");
            assert_eq!(c.safe_mode_available, 1, "可用数不消耗");
        }
        _ => panic!("controller 不见了"),
    }
    // 对照：A 房自身的 safeMode 不受影响
    let ctl_a = engine.world.room("W1N1").unwrap().objects.get(&a).unwrap();
    match ctl_a {
        RoomObject::Controller(c) => assert_eq!(c.safe_mode, 3 + 1000),
        _ => panic!(),
    }
}

// ---- A4-iv：移动杂项族（say/suicide/pull/notifyWhenAttacked） ----

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
    // 官方 say 键带 message(截 10)/isPublic——位图只落事件位（灰区登记）
    assert_eq!(action_log(&f, c), 1 << 9, "say 落 ACTION_SAY 位");
    assert_eq!(
        action_log(&f, b1),
        1 << 9,
        "缺省 message（None ≙ 官方 \"\"）同落位"
    );

    // spawning creep：处理器拒（无 actionLog）。A4-i 的孵化模型并入后，孤立
    // 的 spawning 雏形会在 tick 相被孤儿移除——本例给它配同格带匹配任务
    // （name "sp1"）的 spawn：intent 相 say 拒先于 tick 相的孵化处理，断言
    // 窗口内 s 恒在
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
    // suicide.js：intent 相即 _die——对象当 tick 消亡（B3：墓碑承接 + 死亡事件）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 40);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    let events = f.settle(batch);
    assert!(f.world.get(c).is_none(), "suicide 当 tick 死亡");
    // 墓碑承接（B3）：dropRate 0.2、满寿（fixture age_time 极大 ⇒ lifeRate 巨大
    // ⇒ 每件钳 125）⇒ 40 携带 + 125×2 回收 = 290；2 部件 ⇒ 10 tick
    let tomb = f
        .world
        .rooms
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
            .rooms
            .get("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "不直接掉地（墓碑承接）"
    );
    // _die.js:97：EVENT_OBJECT_DESTROYED（B3 补齐——M1 灰区销账）
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");

    // 同 tick 后续 intents 不再作用（对象消亡）——**有意分歧**：官方执行序
    // creepActions 里 upgradeController 排 suicide 之后（intents.js:15-17），
    // 但官方处理器持死引用快照继续执行、对其他对象的效果落地（processor.js:241/
    // _die.js:12-13 不清引用、upgradeController.js 无存活复检——同 tick 官方
    // 升级照常生效）；我方对象消亡即跳过（A4-iv 评审 1/3 双确认，compatibility.md
    // 登记为有意分歧：处理器持 pre-death 快照侵入面大、真实 bot 罕见）——本断言
    // 锁的是我方登记后的分歧行为，非官方语义
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

#[test]
fn pull_pair_moves_fatigue_free_creep() {
    // 拉拽官方语义（pull.js + movement.js）：拉者 A 记 pull，被拉 B 记
    // move(A)（id 形态）——A 移动让位，B 挤入 A 原格；B 无 MOVE 部件也带疲劳
    // 照样移动（canMove 的 _pulled 分支），B 的移动疲劳沿链记在 A 头上
    //（_add-fatigue 归链）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0); // 拉者
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0); // 被拉（无 MOVE）
    if let RoomObject::Creep(cc) = f
        .world
        .rooms
        .get_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&b)
        .unwrap()
    {
        cc.fatigue = 2; // 被拉者带疲劳
    }

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m = IntentData::bare();
    m.id = Some(a); // 被拉 creep 的 move(拉者)——id 形态
    batch.add_object_intent("p1", b, Action::Move, m);
    let mut m2 = IntentData::bare();
    m2.direction = Some(7); // LEFT：A 从 (10,10) → (9,10)
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    // A 移至 (9,10)；B 挤入 A 原格 (10,10)
    assert_eq!(f.creep_state(a).0, 9, "拉者左移");
    assert_eq!(f.creep_state(b).0, 10, "被拉者补位（无 MOVE + 疲劳照移）");
    // 疲劳（官方 movement.js:235-245 无 weight||1——纯 MOVE 移动 0 疲劳）：
    // A 自身移动 1 部件×2 + B 的移动 1×2 归链到 A = 4，A 的 1 MOVE 恢复 -2 → 2
    //（无归链则 A 仅 2-2=0——2 锁的就是归链发生）；B 自身疲劳不动（2）且无
    // 恢复（无 MOVE）
    assert_eq!(f.creep_state(a).2, 2, "拉者累计双方移动疲劳后恢复 -2");
    assert_eq!(
        f.creep_state(b).2,
        2,
        "被拉者疲劳不动（归链 + 无 MOVE 无恢复）"
    );
}

#[test]
fn pull_pairing_breaks_on_destination_mismatch() {
    // movement.js:176-180：被拉对象的 move 目标格 ≠ 拉者当前格 → 解除配对
    // → 常规 canMove（无 MOVE 部件拒）——B 不动；A 独自移动
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m = IntentData::bare();
    m.direction = Some(3); // RIGHT：B 朝 (12,10)——不是拉者的当前格
    batch.add_object_intent("p1", b, Action::Move, m);
    let mut m2 = IntentData::bare();
    m2.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    assert_eq!(f.creep_state(a).0, 9, "拉者照常移动");
    assert_eq!(f.creep_state(b).0, 11, "配对解除后 B 无 MOVE 不动");
    assert_eq!(f.creep_state(b).2, 0, "B 无疲劳发生");
}

#[test]
fn pulled_creep_without_move_intent_stays() {
    // 只有 pull 没有被拉方的 move：位移不发生（官方矩阵只收 move intent；
    // _pull/_pulled 只影响仲裁 tie-break 与 canMove）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m2 = IntentData::bare();
    m2.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    assert_eq!(f.creep_state(a).0, 9);
    assert_eq!(f.creep_state(b).0, 11, "无 move intent 的被拉者不动");
}

#[test]
fn pull_cycle_chain_rejected() {
    // movement.js:70-75 checkRecursiveTarget：A→B→C 后 C 拉 A 成环——配对
    // 拒绝（C 的 pull 无效；A/B 链不受影响照常工作）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Move], 0);
    let c = f.creep("C", "p1", 10, 11, &[BodyKind::Move], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    batch.add_object_intent("p1", b, Action::Pull, IntentData::target(c));
    batch.add_object_intent("p1", c, Action::Pull, IntentData::target(a)); // 成环
    // 三方 move：B move(A)、C move(B)、A move(7)
    let mut mb = IntentData::bare();
    mb.id = Some(a);
    batch.add_object_intent("p1", b, Action::Move, mb);
    let mut mc = IntentData::bare();
    mc.id = Some(b);
    batch.add_object_intent("p1", c, Action::Move, mc);
    let mut ma = IntentData::bare();
    ma.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, ma);
    f.settle(batch);

    // A (10,10)→(9,10)；B (11,10)→(10,10)（A 的让位格）；C (10,11) 的目标
    // 是 B 的原格 (11,10)——C→B 配对成立（环只在 C 拉 A 处断），B 让位可入。
    // 全员纯 MOVE：每步疲劳 0（movement.js:235-245 无 weight||1）
    assert_eq!(f.creep_state(a), (9, 10, 0, 0), "A 移动");
    assert_eq!(f.creep_state(b).0, 10, "B 补 A 的位");
    assert_eq!(f.creep_state(c).0, 11, "C 补 B 的位（链式拉拽传递）");
}

#[test]
fn notify_when_attacked_intent_sets_flag() {
    // processor.js:313-314：CONSTRUCTION_COST 类型 + 所有权门 → 落库
    let mut f = Fixture::new(1, terrain_with(&[]));
    let sp = f.spawn("S1", 20, 20, 300);
    let rd = f.road_at(21, 20);
    let ctl = f.controller(3, Some("p1"));

    let mut batch = IntentBatch::default();
    let mut d = IntentData::bare();
    d.enabled = Some(false);
    batch.add_object_intent("p1", sp, Action::NotifyWhenAttacked, d);
    let mut d2 = IntentData::bare();
    d2.enabled = Some(true);
    batch.add_object_intent("p1", rd, Action::NotifyWhenAttacked, d2.clone());
    // controller 不在 CONSTRUCTION_COST——通用段类型门拒（API 面不暴露，
    // 伪造 intent 的兜底）
    batch.add_object_intent("p1", ctl, Action::NotifyWhenAttacked, d2);
    f.settle(batch);

    assert_eq!(
        match f.obj(sp) {
            RoomObject::Spawn(s) => s.notify_when_attacked,
            _ => panic!(),
        },
        Some(false),
        "spawn 落 enabled=false"
    );
    assert_eq!(
        match f.obj(rd) {
            RoomObject::Road(r) => r.notify_when_attacked,
            _ => panic!(),
        },
        Some(true),
        "中性路同样可落（无主不拦——官方 :248）"
    );

    // 他人对象：分派层通用段跳过（p2 对 p1 的 spawn）
    let mut batch2 = IntentBatch::default();
    let mut d3 = IntentData::bare();
    d3.enabled = Some(true);
    batch2.add_object_intent("p2", sp, Action::NotifyWhenAttacked, d3);
    f.settle(batch2);
    assert_eq!(
        match f.obj(sp) {
            RoomObject::Spawn(s) => s.notify_when_attacked,
            _ => panic!(),
        },
        Some(false),
        "他人 intent 跳过"
    );
}

#[test]
fn built_structure_born_with_notify_attacked_true() {
    // build.js:116：玩家建造落成的结构出生即 notifyWhenAttacked: true
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let builder = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 200);
    // 进度 45/50：1 WORK×BUILD_POWER(5) 恰好补完剩余 5 → 落成
    let site = f.site(11, 10, "extension", 45, 50, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", builder, Action::Build, {
        let mut d = IntentData::target(site);
        d.x = Some(11);
        d.y = Some(10);
        d
    });
    f.settle(batch);
    let ext = f
        .world
        .rooms
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Extension(e) => Some(e.notify_when_attacked),
            _ => None,
        });
    assert_eq!(ext, Some(Some(true)), "落成 extension 出生即 true");
}

// ---- GPT 外部复审清偿（m2/a4-review）锁值 ----

#[test]
fn ghodium_resource_family_settles() {
    // P1-1：G 资源的 transfer/withdraw/drop 全链结算（官方 transfer.js:8/
    // withdraw.js:8/drop.js:10 按资源类型结算——此前执行器静默丢弃）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let st = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store::default(),
                store_extra: Vec::new(),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // G transfer：creep 40G → storage 20G
    let c = f.creep("w1", "p1", 21, 20, &[BodyKind::Move, BodyKind::Carry], 0);
    if let Some(RoomObject::Creep(cc)) = f.world.get_mut(c) {
        cc.store.ghodium = 40;
    }
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(st);
    d.amount = Some(20);
    d.resource_type = Some("G".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    f.settle(b);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 20, "creep G 40→20");
            assert_eq!(s.store.ghodium, 20, "storage G 0→20");
        }
        _ => panic!(),
    }
    // G withdraw：storage 20 → creep 全量
    let mut b2 = IntentBatch::default();
    let mut d2 = IntentData::bare();
    d2.id = Some(st);
    d2.amount = Some(20);
    d2.resource_type = Some("G".into());
    b2.add_object_intent("p1", c, Action::Withdraw, d2);
    f.settle(b2);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 40, "withdraw 回满");
            assert_eq!(s.store.ghodium, 0);
        }
        _ => panic!(),
    }
    // G drop：掉落为 resource_type=2 的资源堆（同格同资源并堆）
    let mut b3 = IntentBatch::default();
    let mut d3 = IntentData::bare();
    d3.amount = Some(5);
    d3.resource_type = Some("G".into());
    b3.add_object_intent("p1", c, Action::Drop, d3);
    f.settle(b3);
    let pile = f
        .world
        .rooms
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.resource_type == 2 => Some(e.amount),
            _ => None,
        });
    assert_eq!(
        pile,
        Some(4),
        "G 5 掉落 → 当 tick 衰减 ceil(5/1000)=1 → 4（官方全资源同式）"
    );
    assert_eq!(
        match f.obj(c) {
            RoomObject::Creep(cc) => cc.store.ghodium,
            _ => 0,
        },
        35
    );
}

#[test]
fn boost_effectiveness_unified() {
    // P1-2：强化部件统一效能——官方 movement.js:41-59（重量按强化容量扣）与
    // tick.js:105（恢复按 fatigue 效能）。KH 容量×2 / ZO 疲劳×2
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 1 carry(KH) + 2 move(ZO)：携带 80（无强化需 2 carry=weight 2；KH ×2
    // 容量 100 ⇒ weight 1）+ 1 move 够驱 ⇒ move 后疲劳 = (1+1) - 2×2 = 0
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 9_000,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "b1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Carry.code(),
                        hits: 100,
                        boost: Some("KH".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 80,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.direction = Some(3);
    b.add_object_intent("p1", c, Action::Move, d);
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!((cc.x, cc.y), (11, 10), "direction 3=RIGHT 落地");
            assert_eq!(
                cc.fatigue, 0,
                "weight=1（KH 容量×2）− 恢复 2×2（ZO 效能×2）= 1−4 → 0"
            );
        }
        _ => panic!(),
    }
}
