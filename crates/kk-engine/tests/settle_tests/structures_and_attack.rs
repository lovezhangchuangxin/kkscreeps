//! 测试结构阻挡与配额、资源转移、近战事件顺序，以及 Engine 的沙箱指令归属。

use super::*;

#[test]
fn transfer_clamps_to_target_capacity() {
    // transfer 钳制：spawn 容量 300
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = f
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 250,
                    ghodium: 0,
                },
                spawning: None,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 20, 21, &[BodyKind::Move, BodyKind::Carry], 100);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(spawn);
    d.amount = Some(100);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    // 250+100 > 300 → 只转 50
    assert_eq!(events[0]["data"]["amount"], 50);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 300),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn withdraw_only_from_stored_structures() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(src);
    d.amount = Some(10);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Withdraw, d);
    f.settle(b);
    // source 不可 withdraw（官方走 harvest）
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "withdraw 不动 source"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 0);
}

#[test]
fn controller_downgrade_chain() {
    // controllers/tick.js 降级链：无升级 → 到点 level-1、progress += 90%、
    // safeModeAvailable 清零
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p1"));
    // level 2 的 DOWNGRADE=10000：手造快钟绕过长等待——直接改对象
    if let Some(RoomObject::Controller(t)) = f.world.get_mut(ctl) {
        t.downgrade_time = Some(2);
    }
    f.settle(IntentBatch::default());
    f.settle(IntentBatch::default()); // time=1 >= 2-1 → 降级
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.level, 1, "2→1");
            // progress += round(LEVELS[1]×0.9) = 180
            assert_eq!(t.progress, 180);
            assert_eq!(t.safe_mode_available, 0);
        }
        _ => panic!(),
    }
}

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
fn own_construction_site_blocks_movement() {
    // 自家障碍工地在移动仲裁时挡路，而非请求预检时。
    // 来源：上游 intents/movement.js 的 checkObstacleAtXY。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "spawn", 0, 15000, "p1");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5)); // → (10,11) 自家 spawn 工地
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "自家 obstacle 工地挡路");
    assert!(f.world.get(site).is_some(), "工地未被动");
}

#[test]
fn moving_onto_enemy_site_stomps_it() {
    // movement.js:224-233：进格踩掉敌方工地（safeMode 门外），退一半工程为
    // 掉落能量；tick 期插入的 energy 不在阶段 8 键快照内 → 本 tick 不衰减
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "road", 10, 300, "p2");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "敌方工地可入");
    assert!(f.world.get(site).is_none(), "工地被踩掉");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(5), "退半工程 10/2=5，本 tick 不衰减");
}

#[test]
fn transfer_into_extension_clamps_by_rcl() {
    // Extension 的能量容量按 RCL 计算，本例 RCL 3 为 50。
    // 来源：上游 extensions/tick.js 和 creeps/transfer.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ext = f
        .world
        .insert(
            "W1N1",
            RoomObject::Extension(kk_schema::room::ExtensionObj {
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        20,
        21,
        &[BodyKind::Move, BodyKind::Carry, BodyKind::Carry],
        60,
    );
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(ext);
    d.amount = Some(30);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["amount"], 10, "钳到容量差 50-40");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.store.energy, 50),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn cascade_removal_clears_chain_on_head_failure() {
    // movement.js:154-186 级联清链：链尾无 MOVE（canMove 不过）→ 其移动出矩阵，
    // 沿"要进我原格"递归取消上游——全链原地
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let c = f.creep("c", "p1", 12, 10, &[BodyKind::Work], 0); // 无 MOVE
    let mut batch = move_intent(a, 3);
    let mut d = IntentData::bare();
    d.direction = Some(3);
    batch.add_object_intent("p1", b, Action::Move, d);
    let mut d2 = IntentData::bare();
    d2.direction = Some(3);
    batch.add_object_intent("p1", c, Action::Move, d2);
    f.settle(batch);
    assert_eq!(
        f.creep_state(a).0,
        10,
        "级联取消：链尾 c 无 MOVE → b、a 全链取消"
    );
    assert_eq!(f.creep_state(b).0, 11);
    assert_eq!(f.creep_state(c).0, 12);
}

#[test]
fn edge_entry_zero_fatigue() {
    // movement.js:242-245：从非边缘进边缘格 fatigue=0（跨房间预备）；常规平原
    // 应为体重 2×2 − 恢复 2 = 2，此处必须为 0
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        1,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    f.settle(move_intent(c, 1)); // TOP → (10,0) 边缘
    assert_eq!(f.creep_state(c), (10, 0, 0, 0));
}

#[test]
fn movement_wears_road() {
    // movement.js:213：路上移动磨损 nextDecayTime -= ROAD_WEAROUT(1) × body 部件数
    let mut f = Fixture::new(1, terrain_with(&[]));
    let road = f
        .world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                notify_when_attacked: None,
                id: 0,
                x: 10,
                y: 11,
                hits: 5000,
                next_decay_time: 5000,
            }),
        )
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        0,
    );
    f.settle(move_intent(c, 5));
    match f.obj(road) {
        RoomObject::Road(r) => assert_eq!(r.next_decay_time, 4997, "3 部件 × ROAD_WEAROUT(1)"),
        _ => panic!(),
    }
}

#[test]
fn carried_resources_add_move_weight() {
    // movement.js:41-59 calcResourcesWeight：从 body 尾部数装下携带量所需的
    // CARRY 部件数，计入移动体重——[move,carry,carry,work] 带 60：weight = 1 + 2
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Carry,
            BodyKind::Work,
        ],
        60,
    );
    f.settle(move_intent(c, 5)); // plain：3×2 − 2 = 4
    assert_eq!(f.creep_state(c).2, 4, "满载两 carry 计入体重");
    // 对照：同 body 空载 → weight 1 → 2 − 2 = 0
    let c2 = f.creep(
        "w2",
        "p1",
        12,
        12,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Carry,
            BodyKind::Work,
        ],
        0,
    );
    f.settle(move_intent(c2, 5));
    assert_eq!(f.creep_state(c2).2, 0, "空载不计资源重量");
}

#[test]
fn build_blocked_by_creep_on_tile() {
    // build.js:50-59：障碍类型工地格上有 creep（无 safeMode ⇒ 任何 creep）→ 拒建
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "spawn", 0, 15000, "p1");
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let _blocker = f.creep("blocker", "p1", 12, 10, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    f.settle(b);
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => {
            assert_eq!(s.progress, 0, "格被 creep 占：拒建")
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 10, "能量未扣");
}

#[test]
fn create_creep_rejected_for_off_spawn() {
    // create-creep.js:17 checkStructureAgainstController（utils.js:456-506）：
    // RCL 3 只许 1 个 spawn，距 controller 更远者不活跃——即使能量充足也不得孵化
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let _near = f.spawn("Near", 5, 6, 300);
    let far = f.spawn("Far", 25, 25, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    b.add_object_intent("p1", far, Action::CreateCreep, d);
    f.settle(b);
    match f.obj(far) {
        RoomObject::Spawn(s) => {
            assert!(s.spawning.is_none(), "超配额 spawn 不孵化");
            assert_eq!(s.store.energy, 300, "未扣费");
        }
        _ => panic!(),
    }
    match f.obj(_near) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 300, "Near 同样未被动"),
        _ => panic!(),
    }
    assert!(
        f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .all(|o| !matches!(o, RoomObject::Creep(c) if c.name == "child")),
        "无雏形"
    );
}

#[test]
fn drop_rejects_amount_over_holdings() {
    // drop.js:13：持有不足整条拒绝（官方不部分掉落）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 20);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(50);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    assert_eq!(f.creep_state(c).3, 20, "不足额：整条拒绝");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "无掉落"
    );
}

#[test]
fn placed_tower_blocks_birth() {
    // Generic tower 必须按 structureType 识别为障碍；固定返回 structure 会漏掉塔。
    // 来源：上游 spawns/_born-creep.js 的 checkObstacleFn。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    f.world.insert("W1N1", generic_tower(20, 19)).unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    d.directions = vec![1];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    for _ in 0..12 {
        f.settle(IntentBatch::default());
    }
    // 方向 1 被 tower 永占：反复重试、永不出世（tower 不会让开）
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
    assert!(chick.2, "tower 格不可出生");
    assert_eq!((chick.0, chick.1), (20, 20), "雏形留在 spawn 格");
}

#[test]
fn tower_counts_toward_controller_quota() {
    // Generic tower 须计入当前结构数量，RCL 3 已有一座时不再允许建塔。
    // 来源：上游 utils.js::checkControllerAvailability。
    let mk_intent = |b: &mut IntentBatch| {
        b.add_room_intent(
            "p1",
            kk_engine::intent::RoomIntent::CreateConstructionSite {
                room: "W1N1".into(),
                x: 15,
                y: 15,
                structure_type: "tower".into(),
                name: None,
            },
        );
    };
    let has_tower_site =
        |f: &Fixture| {
            f.world.room("W1N1").unwrap().objects.values().any(
                |o| matches!(o, RoomObject::ConstructionSite(s) if s.structure_type == "tower"),
            )
        };
    // 无 tower：rcl 3 允许 1 座 → 工地建成
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let mut b = IntentBatch::default();
    mk_intent(&mut b);
    f.settle(b);
    assert!(has_tower_site(&f), "配额空位：允许建塔");
    // 已有 1 座 tower：配额满 → 拒建
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller(3, Some("p1"));
    f2.world.insert("W1N1", generic_tower(25, 25)).unwrap();
    let mut b2 = IntentBatch::default();
    mk_intent(&mut b2);
    f2.settle(b2);
    assert!(!has_tower_site(&f2), "现存 tower 计入配额：拒建");
}

#[test]
fn build_blocked_by_tower_on_site_tile() {
    // build.js:51-53 障碍对象拒建——Generic tower 在工地格上必须挡建造
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "spawn", 0, 15000, "p1");
    f.world.insert("W1N1", generic_tower(12, 10)).unwrap();
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    f.settle(b);
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => {
            assert_eq!(s.progress, 0, "tower 占格：拒建")
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 10);
}

#[test]
fn energy_merge_saturates_at_u32_max() {
    // 掉落合并的受检运算：u32::MAX 堆 + 1 不 panic（debug）/不回绕丢数据（release），
    // 随后照常衰减 ceil(MAX/1000)
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                amount: u32::MAX,
                resource: None,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Move, BodyKind::Carry], 1);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(1);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    let decay = (u32::MAX as i64 + 999) / 1000; // ceil(4294967295/1000) = 4294968
    let expected = u32::MAX - decay as u32;
    let heap = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        })
        .unwrap();
    assert_eq!(heap, expected, "饱和合并后照常衰减（无 panic/回绕）");
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

#[test]
fn melee_attack_damage_and_body_distribution() {
    // attack.js:38 伤害 = 2×ATTACK×30=60；_recalc-body.js:10-18 从尾部灌满——
    // 前列部件先掉 hit：[tough,work,work,move] 400→340，tough 100→40
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[BodyKind::Attack, BodyKind::Attack],
        0,
    );
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "无反击目标（无 ATTACK 部件）→ 单事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(events[0]["objectId"], a.as_str());
    assert_eq!(events[0]["data"]["targetId"], b.as_str());
    assert_eq!(events[0]["data"]["damage"], 60);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_MELEE
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 340, "400 - 60");
    assert_eq!(parts, vec![40, 100, 100, 100], "受伤自头部推进");
    // actionLog（_damage.js:59-75）：攻击方 attack 位、受击方 attacked 位
    let (log_a, log_b) = match (f.obj(a), f.obj(b)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED as u16);
}

#[test]
fn melee_hit_back_and_event_order() {
    // _damage.js:17-19：近战 creep 目标反击（同样延迟）；:89-93 反击事件先于主事件
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
        ],
        0,
    );
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
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 2, "反击事件 + 主事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["objectId"],
        b.as_str(),
        "反击事件以受击者为 objectId"
    );
    assert_eq!(events[0]["data"]["targetId"], a.as_str());
    assert_eq!(events[0]["data"]["damage"], 60, "2×ATTACK×30 反击");
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[1]["data"]["damage"], 120, "4×ATTACK×30 主伤害");
    // 双方伤害同 tick 各自结算：a 400-60=340、b 400-120=280
    assert_eq!(creep_parts(&f, a).0, 340);
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280);
    assert_eq!(parts, vec![0, 80, 100, 100], "b 首个 ATTACK 部件被击毁");
    // 攻击者同时记 attack 与 attacked（_damage.js:74 + :88）
    let log_a = match f.obj(a) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log_a,
        kk_engine::settle::creep::ACTION_ATTACK as u16
            | kk_engine::settle::creep::ACTION_ATTACKED as u16
    );
}

#[test]
fn no_hit_back_when_attacker_on_rampart() {
    // _damage.js:17：攻击者站 rampart（不分敌我/公开性）→ 无反击
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Attack, BodyKind::Tough],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "仅主事件");
    assert_eq!(creep_parts(&f, a).0, 100, "攻击者无伤");
    assert_eq!(creep_parts(&f, b).0, 170, "200 - 30");
}

#[test]
fn multiple_attackers_accumulate_damage() {
    // _damage.js:20：creep 目标伤害累加进 _damageToApply，目标自己的 tick 一次结算
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a1 = f.creep("a1", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let a2 = f.creep("a2", "p1", 12, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    let mut batch = attack_intent(a1, b);
    batch.add_object_intent("p1", a2, Action::Attack, IntentData::target(b));
    let events = f.settle(batch);
    assert_eq!(events.len(), 2, "两次主事件");
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280, "400 - 60×2");
    assert_eq!(parts, vec![0, 80, 100, 100]);
}

#[test]
fn attack_kills_creep_into_tombstone() {
    // 伤害结算后 hits 归零时死亡，资源进入墓碑；与 TTL 死亡使用同一回收入口。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut bodylist = vec![BodyKind::Attack; 40];
    bodylist.push(BodyKind::Move);
    let a = f.creep("atk", "p1", 10, 10, &bodylist, 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Tough, BodyKind::Tough],
        40,
    );
    let events = f.settle(attack_intent(a, b));
    assert!(f.world.get(b).is_none(), "1200 ≥ 200：击杀");
    // 夹具寿命极长，每件回收钳到 125；携带 40 加两件回收得到墓碑 energy=290。
    // 两件身体对应 10 tick 衰减期限。
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.x == 11 && t.y == 10 => Some(t.clone()),
            _ => None,
        })
        .expect("暴亡产墓碑");
    assert_eq!(tomb.store.energy, 290, "携带 40 + 造价回收 250");
    assert_eq!(tomb.owner.as_deref(), Some("p2"), "墓碑归死者");
    assert_eq!(tomb.creep_name.as_deref(), Some("victim"));
    // 事件序（官方）：intent 相的 EVENT_ATTACK 先（_damage.js:93）、tick 相的
    // OBJECT_DESTROYED 后（_die.js:97）
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[1]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[1]["objectId"], b.as_str());
}

#[test]
fn attack_kills_structure_and_spawn_chick() {
    // structures/_destroy.js:9-15：孵化中的 spawn 被毁 → 雏形一并移除；
    // Spawn 的库存进入废墟。来源：上游 structures/_destroy.js。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    // 造孵化任务与雏形（走真 createCreep intent）
    let mut b0 = IntentBatch::default();
    let mut d0 = IntentData::bare();
    d0.name = Some("child".into());
    d0.body = vec![BodyKind::Move];
    b0.add_object_intent("p1", spawn, Action::CreateCreep, d0);
    f.settle(b0);
    // 削弱 spawn 至一击必杀（120 伤害）
    if let Some(RoomObject::Spawn(s)) = f.world.get_mut(spawn) {
        s.hits = 100;
    }
    let atk = f.creep("atk", "p2", 20, 21, &[BodyKind::Attack; 4], 0);
    f.settle(attack_intent_by("p2", atk, spawn));
    assert!(f.world.get(spawn).is_none(), "spawn 被击毁");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Creep(c) if c.name == "child")),
        "雏形随 spawn 一并移除"
    );
    // 击毁 spawn 产生废墟，并保存结构信息与库存。
    // 300 初始能量减 50 孵化费，再加前一轮免费能量 1，库存为 251；衰减期限为 500 tick。
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 20 && r.y == 20 => Some(r.clone()),
            _ => None,
        })
        .expect("击毁产废墟");
    assert_eq!(ruin.structure_type, "spawn");
    assert_eq!(ruin.structure_id, spawn.0, "存照 id = 原结构 id");
    assert_eq!(
        ruin.structure_hits_max,
        Some(kk_core::constants::SPAWN_HITS as u32)
    );
    assert_eq!(ruin.owner.as_deref(), Some("p1"));
    assert_eq!(ruin.destroy_time, 1, "第二个结算 tick");
    assert_eq!(ruin.decay_time, 1 + 500, "RUIN_DECAY");
    assert_eq!(ruin.store.energy, 251, "store 全量承接（不再掉地）");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 20 && e.y == 20)),
        "store 不再落地（废墟承接）"
    );
}

#[test]
fn rampart_redirects_attack_until_broken() {
    // attack.js:33-36：目标格 rampart 存在 → 伤害转打 rampart；打破后直击本体
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 11, 10, &[BodyKind::Attack; 10], 0);
    let ext = extension_at(&mut f, 12, 10, 1000, 0);
    let rampart = generic_structure(&mut f, 12, 10, "rampart", "p2", 1000);
    // t1：extension 无伤、rampart -300
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(
        events[0]["data"]["targetId"],
        rampart.as_str(),
        "事件指向 rampart"
    );
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "本体受 rampart 保护"),
        _ => panic!(),
    }
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 700);
        }
        _ => panic!(),
    }
    // t2-t3：rampart 700→400→100
    f.settle(attack_intent(a, ext));
    f.settle(attack_intent(a, ext));
    // t4：100-300 ≤ 0 → rampart 击毁
    f.settle(attack_intent(a, ext));
    assert!(f.world.get(rampart).is_none(), "rampart 击毁");
    assert!(f.world.get(ext).is_some(), "本体仍在");
    // t5：无 rampart → 直击 extension
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(events[0]["data"]["targetId"], ext.as_str());
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 700),
        _ => panic!(),
    }
}

#[test]
fn build_rejects_existing_same_type_even_when_walkable() {
    for existing_type in ["road", "container"] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        if existing_type == "road" {
            f.road_at(10, 10);
        } else {
            generic_structure(&mut f, 10, 10, "container", "p1", 1000);
        }
        let site = f.site(10, 10, "road", 0, 5, "p1");
        let actor = f.creep(
            "builder",
            "p1",
            11,
            10,
            &[BodyKind::Work, BodyKind::Carry],
            5,
        );
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", actor, Action::Build, IntentData::target(site));
        let events = f.settle(batch);
        if existing_type == "road" {
            assert!(f.world.get(site).is_some(), "同类型结构存在时保留工地");
            assert_eq!(f.creep_state(actor).3, 5);
            assert!(
                !events
                    .iter()
                    .any(|e| e["event"] == kk_core::constants::EVENT_BUILD)
            );
        } else {
            assert!(f.world.get(site).is_none(), "道路可与容器同格");
            assert_eq!(f.creep_state(actor).3, 0);
            assert!(
                events
                    .iter()
                    .any(|e| e["event"] == kk_core::constants::EVENT_BUILD)
            );
        }
    }
}

#[test]
fn withdraw_respects_private_rampart_and_safe_mode() {
    for (target_owner, rampart_public, room_owner, safe_mode, allowed) in [
        ("p2", false, "p1", 0, false),
        ("p2", true, "p1", 0, true),
        ("p1", false, "p1", 0, true),
        ("p1", true, "p2", 10, false),
        ("p1", true, "p2", 0, true),
        ("p1", true, "p1", 10, true),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        f.controller_full(8, Some(room_owner), |ctl| ctl.safe_mode = safe_mode);
        let target = f.spawn("target", 10, 10, 100);
        if let Some(RoomObject::Spawn(s)) = f.world.get_mut(target) {
            s.owner = target_owner.into();
        }
        let rampart = generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
        if let Some(RoomObject::Generic(g)) = f.world.get_mut(rampart) {
            let mut v: Value = serde_json::from_str(&g.data).unwrap();
            v["isPublic"] = serde_json::json!(rampart_public);
            g.data = v.to_string();
        }
        let actor = f.creep("hauler", "p1", 11, 10, &[BodyKind::Carry], 0);
        let mut data = IntentData::target(target);
        data.amount = Some(10);
        data.resource_type = Some("energy".into());
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", actor, Action::Withdraw, data);
        let events = f.settle(batch);
        assert_eq!(
            f.creep_state(actor).3,
            if allowed { 10 } else { 0 },
            "target_owner={target_owner}, public={rampart_public}, room_owner={room_owner}, safe_mode={safe_mode}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e["event"] == kk_core::constants::EVENT_TRANSFER)
                .count(),
            usize::from(allowed)
        );
    }
}
