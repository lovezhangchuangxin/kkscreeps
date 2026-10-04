//! 真实沙箱指令绑定到注册玩家的闭环校验。
use super::*;

#[test]
fn engine_scripted_sandbox_closed_loop() {
    // 通过 ScriptedSandbox 与 Engine 推进多 tick，验证指令采集和房间结算的衔接。
    let mut world = World::new(9);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t);
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

    // 每 tick：采 + 升（脚本是按当前 tick 注入的静态批）
    // 脚本按当前 world.time() 匹配：Engine 首 tick 的 time=0，覆盖 0..10
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", creep, Action::Harvest, IntentData::target(src));
    batch.add_object_intent(
        "p1",
        creep,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    let mut engine = kk_engine::Engine::new(world);
    for tick in 0..10u64 {
        sandbox.at(tick, batch.clone());
    }
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    for _ in 0..10 {
        engine.tick().unwrap();
    }
    let ctl_obj = engine.world.get(ctl).unwrap().1;
    match ctl_obj {
        RoomObject::Controller(t) => assert_eq!(t.progress, 20, "10 tick × 2"),
        _ => panic!(),
    }
    assert_eq!(engine.players["p1"].gcl, 20);
    assert!(!engine.last_events.is_empty(), "事件日志留存");
    // id 单一真相源：结算新建对象后 world.next_obj_id 已同步
    let max_id = engine
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .max()
        .unwrap()
        .0;
    assert!(engine.world.next_obj_id() > max_id, "分配器越过最大 id");
}

#[test]
fn sandbox_intents_bound_to_registered_player() {
    // 身份边界：沙箱返回的 intent 只归其注册玩家——冒名键（操作他人对象）整批丢弃
    let mut world = World::new(1);
    world.register_room("W1N1", kk_core::Terrain::plains());
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
    // 恶意沙箱注册为 p1，却发 p2 键的 move intent
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    sandbox.at(0, move_intent(victim, 3));
    let mut engine = kk_engine::Engine::new(world);
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    engine.tick().unwrap();
    let (x, y) = match engine.world.get(victim).unwrap().1 {
        RoomObject::Creep(c) => (c.x, c.y),
        _ => panic!(),
    };
    assert_eq!((x, y), (10, 10), "冒名 intent 被丢弃：p2 的 creep 未被移动");
}
