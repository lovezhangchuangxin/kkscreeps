//! 测试结构阻挡与配额、资源转移、近战事件顺序，以及 Engine 的沙箱指令归属。

use super::*;

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

#[path = "structures_and_attack/combat.rs"]
mod combat;

#[path = "structures_and_attack/resources.rs"]
mod resources;

#[path = "structures_and_attack/ownership.rs"]
mod ownership;
