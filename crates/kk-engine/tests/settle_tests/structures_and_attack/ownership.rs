//! 显式指令的多轮结算与对象所有权校验。
use super::*;

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
