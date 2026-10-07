//! Engine 世界级行为：tick 后玩家房间重算、跨房安全模式门与显式指令闭环/所有权校验。

use super::*;

#[test]
fn engine_recomputes_player_rooms_after_tick() {
    // Engine 在发布本轮世界后重建玩家房间集合；同轮 claim 仍读取旧集合。
    // 来源：上游 creeps/claimController.js 与 driver/lib/bulk.js 的 addToSet。
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone()).unwrap();
    world.register_room("W2N2", t).unwrap();
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
    engine
        .edit_players(|players| {
            players.entry("p1".to_string()).or_default().gcl = 1_000_000;
            Ok(())
        })
        .unwrap();
    // 空 tick 后 rooms 重建 = {W1N1}
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        engine.players()["p1"].rooms,
        BTreeSet::from(["W1N1".to_string()])
    );
    // W2N2 claim：rooms=1 → 需要 GCL 1e6（恰好放行）
    let w2_ctl = *engine
        .world()
        .room("W2N2")
        .unwrap()
        .objects
        .keys()
        .next()
        .unwrap();
    let claimer = engine
        .insert_object(
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
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
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
        engine.players()["p1"].rooms,
        BTreeSet::from(["W1N1".to_string(), "W2N2".to_string()]),
        "claim 后纳入 rooms"
    );
    match engine
        .world()
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
fn engine_safemode_gate4_backstop_drops_cross_room_activation() {
    // 玩家已有生效的安全模式时，Engine 根据世界对象拒绝其其他房间的激活指令。
    // 直接构造指令绕过 prelude 检查，验证 Rust 结算仍执行这项跨房限制。
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone()).unwrap();
    world.register_room("W2N2", t).unwrap();
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
    // W1N1 已有安全模式，W2N2 的本地激活条件满足，只测试跨房限制。
    let a = world.insert("W1N1", mk_ctl(3 + 1000, 1)).unwrap();
    let b = world.insert("W2N2", mk_ctl(0, 1)).unwrap();
    let mut engine = Engine::new(world);
    // 直接构造 W2N2 的激活指令，不依赖 prelude 的预检。
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::ActivateSafeMode, IntentData::bare());
    engine.tick_with_intents(batch).unwrap();
    let ctl = engine
        .world()
        .room("W2N2")
        .unwrap()
        .objects
        .get(&b)
        .unwrap();
    match ctl {
        RoomObject::Controller(c) => {
            assert_eq!(c.safe_mode, 0, "A 房生效中 → B 房激活被 Engine 兜底丢弃");
            assert_eq!(c.safe_mode_available, 1, "可用数不消耗");
        }
        _ => panic!("controller 不见了"),
    }
    // 对照：A 房自身的 safeMode 不受影响
    let ctl_a = engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .get(&a)
        .unwrap();
    match ctl_a {
        RoomObject::Controller(c) => assert_eq!(c.safe_mode, 3 + 1000),
        _ => panic!(),
    }
}

#[test]
fn engine_explicit_intents_closed_loop() {
    // 显式提交同一组指令，验证多轮采集和升级结算的衔接。
    let mut world = World::new(9);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t).unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level: 1,
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
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 6,
                y: 6,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    let ctl = world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let src = world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Source(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let creep = world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 7,
                y: 7,
                fatigue: 0,
                age_time: 999_999,
                hits: 500,
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
                        kind: BodyKind::Carry.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();

    // 每轮同时采集和升级，候选指令由测试夹具显式提供。
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", creep, Action::Harvest, IntentData::target(src));
    batch.add_object_intent(
        "p1",
        creep,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    let mut engine = kk_engine::Engine::new(world);
    for _ in 0..10 {
        engine.tick_with_intents(batch.clone()).unwrap();
    }
    let ctl_obj = engine.world().get(ctl).unwrap().1;
    match ctl_obj {
        RoomObject::Controller(t) => assert_eq!(t.progress, 20, "10 tick × 2"),
        _ => panic!(),
    }
    assert_eq!(engine.players()["p1"].gcl, 20);
    assert!(!engine.last_events().is_empty(), "事件日志留存");
    // id 单一真相源：结算新建对象后 world.next_obj_id 已同步
    let max_id = engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .max()
        .unwrap()
        .0;
    assert!(engine.world().next_obj_id() > max_id, "分配器越过最大 id");
}

#[test]
fn engine_rejects_intents_for_foreign_owned_object() {
    // 即使上游提供了指令，结算器仍校验玩家是否拥有目标对象。
    let mut world = World::new(1);
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    let victim = world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "victim".into(),
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
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        victim,
        Action::Move,
        IntentData {
            direction: Some(3),
            ..IntentData::bare()
        },
    );
    let mut engine = kk_engine::Engine::new(world);
    engine.tick_with_intents(batch).unwrap();
    let (x, y) = match engine.world().get(victim).unwrap().1 {
        RoomObject::Creep(c) => (c.x, c.y),
        _ => panic!(),
    };
    assert_eq!((x, y), (10, 10), "p1 的指令不能移动 p2 的 creep");
}
