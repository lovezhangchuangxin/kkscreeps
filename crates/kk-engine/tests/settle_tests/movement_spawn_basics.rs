//! B1 结构治理批自 settle_tests.rs 连续切块（不重排；原段落横幅注释随块
//! 保留；Fixture/helpers 经 `use super::*` 从 crate 根取用——142 个
//! #[test] 计数与原文序不变）。

use super::*;

// ---- 移动 ----

#[test]
fn move_fatigue_by_terrain() {
    // 官方 movement.js:204-239：road=1 / plain=2 / swamp=10 × 体重（非 move/carry 部件）
    let mut f = Fixture::new(1, terrain_with(&[(10, 12)]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);

    // 官方 tick 序：movement.execute(+fatigue) 后即恢复(-2×MOVE)——
    // [move,work] 在 plain 上每 tick 净 0、可持续移动
    f.settle(move_intent(c, 5)); // BOTTOM → (10,11) plain
    assert_eq!(f.creep_state(c), (10, 11, 0, 0), "plain：+2 后恢复 -2");

    f.settle(move_intent(c, 5)); // → (10,12) swamp
    assert_eq!(f.creep_state(c).1, 12);
    assert_eq!(f.creep_state(c).2, 8, "swamp：+10 后恢复 -2");

    // 无 MOVE 部件：canMove 不过，不动
    let c2 = f.creep("w2", "p1", 20, 20, &[BodyKind::Work], 0);
    f.settle(move_intent(c2, 5));
    assert_eq!(f.creep_state(c2), (20, 20, 0, 0));
}

#[test]
fn fatigue_gates_movement() {
    // 2 work + 1 move：疲劳 4/移动、恢复 2/tick → 动一停一
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c), (10, 11, 2, 0), "+4 恢复 -2");
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "疲劳 2≠0：不动");
    assert_eq!(f.creep_state(c).2, 0, "未移动仍恢复 -2");
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 12, "疲劳清零后恢复移动");
}

#[test]
fn swap_move_both_succeed() {
    // 对穿：互向对方格 → 双方成功（官方矩阵 + rate1=100 语义）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 3); // RIGHT → 11,10（b 的原格）
    let mut d = IntentData::bare();
    d.direction = Some(7); // LEFT → 10,10（a 的原格）
    batch.add_object_intent("p1", b, Action::Move, d);
    f.settle(batch);
    assert_eq!(f.creep_state(a).0, 11, "对穿 a 成功");
    assert_eq!(f.creep_state(b).0, 10, "对穿 b 成功");
}

#[test]
fn contested_cell_single_winner() {
    // 抢格：两 creep 挤同一目标 → 恰一个胜者
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 12, 12, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 4); // BOTTOM_RIGHT → 11,11
    let mut d = IntentData::bare();
    d.direction = Some(8); // TOP_LEFT → 11,11
    batch.add_object_intent("p1", b, Action::Move, d);
    f.settle(batch);
    let hit_a = f.creep_state(a).0 == 11 && f.creep_state(a).1 == 11;
    let hit_b = f.creep_state(b).0 == 11 && f.creep_state(b).1 == 11;
    assert!(hit_a ^ hit_b, "恰一个胜者：a={} b={}", hit_a, hit_b);
}

#[test]
fn static_obstacle_blocks_move_intent() {
    // 墙格不可入（无路时）——move intent 静态预检
    let mut t = kk_core::Terrain::plains();
    t.as_mut_slice()[10 * 50 + 11] |= kk_core::TERRAIN_MASK_WALL;
    t.normalize();
    let mut f = Fixture::new(1, t);
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 3)); // RIGHT → 墙
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "墙格不可入");
}

// ---- 采集 ----

#[test]
fn harvest_amount_and_events() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    let c = f.creep(
        "w1",
        "p1",
        11,
        10,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    // 2×WORK × HARVEST_POWER(2) = 4
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_HARVEST);
    assert_eq!(events[0]["data"]["amount"], 4);
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 2996),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 4);

    // 非邻接（移远后）拒
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    // creep 在 (11,10) 与 source (10,10) 邻接——先采一次后挪走两格再验
    f.settle(move_intent(c, 3));
    f.settle(move_intent(c, 3));
    let far = f.creep_state(c).0 >= 13;
    if far {
        let mut b2 = IntentBatch::default();
        b2.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        let before = match f.obj(src) {
            RoomObject::Source(s) => s.energy,
            _ => 0,
        };
        f.settle(b2);
        match f.obj(src) {
            RoomObject::Source(s) => assert_eq!(s.energy, before, "非邻接不采"),
            _ => panic!(),
        }
    }
}

#[test]
fn source_regen_clock() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(2, Some("p1"));
    let src = f.source(10, 10, 100);
    f.settle(IntentBatch::default());
    match f.obj(src) {
        // 首见未满挂钟 time+300（sources/tick.js:16-19）
        RoomObject::Source(s) => assert_eq!(s.next_regeneration_time, Some(300)),
        _ => panic!(),
    }
    for _ in 0..298 {
        f.settle(IntentBatch::default());
    }
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 100, "time=298 未到钟（time>=299 才触发）"),
        _ => panic!(),
    }
    f.settle(IntentBatch::default());
    match f.obj(src) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy, 3000, "到钟回满");
            assert_eq!(s.next_regeneration_time, None);
        }
        _ => panic!(),
    }
}

#[test]
fn harvest_overflow_drops() {
    // 满 carry 后继续采 → 超容部分掉地（harvest.js:55-60）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    // 1 carry（50）+ 8 work（16/tick）：4 tick 采 64 → 超 14 掉地
    let c = f.creep(
        "w1",
        "p1",
        11,
        10,
        &[
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
        0,
    );
    for _ in 0..4 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        f.settle(b);
    }
    assert_eq!(f.creep_state(c).3, 50, "钳到容量");
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
    // 官方：本 tick 掉落的资源在 tick 阶段即参与衰减（forEach 键快照含 intent 期
    // 插入者）→ 14-1=13
    assert_eq!(dropped, Some(13), "超容 64-50=14 掉地且同 tick 衰减 1");
}

// ---- 建造 ----

#[test]
fn build_progress_and_completion() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "constructedWall", 0, 1, "p1");
    // 1 work → 5/tick（BUILD_POWER=5）；墙成本 1 → 一次完成
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    let events = f.settle(b);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_BUILD);
    assert_eq!(events[0]["data"]["incomplete"], false);
    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&site),
        "工地移除"
    );
    let wall = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructedWall(w) => Some(w),
            _ => None,
        })
        .unwrap();
    assert_eq!(wall.hits, 1, "官方落成墙 hits=WALL_HITS(1)");
    assert_eq!(f.creep_state(c).3, 9, "耗 1 能量");
}

// ---- 升级 ----

#[test]
fn upgrade_progress_and_gcl() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.settle(IntentBatch::default()); // 挂降级钟
    let c = f.creep("w1", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 200);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
    f.settle(b);
    match f.obj(ctl) {
        RoomObject::Controller(t) => assert_eq!(t.progress, 1, "1×WORK×1"),
        _ => panic!(),
    }
    assert_eq!(f.players["p1"].gcl, 1, "gcl 计入");
    assert_eq!(f.creep_state(c).3, 199);
}

#[test]
fn controller_levelup_cycle() {
    // 1 级 200 进位（CONTROLLER_LEVELS[1]=200）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.settle(IntentBatch::default());
    let c = f.creep("w1", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 500);
    for _ in 0..200 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
        f.settle(b);
    }
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.level, 2, "200 进位升 2 级");
            assert_eq!(t.safe_mode_available, 1, "升级 +1");
        }
        _ => panic!(),
    }
}

// ---- 孵化 ----

#[test]
fn spawn_creep_cycle() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
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
                    energy: 300,
                    ghodium: 0,
                },
                spawning: None,
            }),
        )
        .unwrap();
    // createCreep [move, work]：成本 150、needTime=3×2=6
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    let (spawning, spawn_energy) = match f.obj(spawn) {
        RoomObject::Spawn(s) => (s.spawning.clone(), s.store.energy),
        _ => panic!(),
    };
    let job = spawning.expect("孵化任务已建");
    assert_eq!((job.need_time, job.spawn_time), (6, 6));
    // 官方涓流：energyAvailable(150)<300 且 store<300 → spawn 每 tick +1
    assert_eq!(spawn_energy, 151, "扣 150 后免费涓流 +1");
    // 雏形 creep 占位于 spawn 格
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning, c.body.len())),
            _ => None,
        })
        .unwrap();
    assert!(chick.2);
    assert_eq!((chick.0, chick.1), (20, 20));
    assert_eq!(chick.3, 2);

    // 推到第 6 tick：出生（spawn 周围 8 格全空 → 出生在方向 1 (20,19)）
    for _ in 0..5 {
        f.settle(IntentBatch::default());
    }
    let chick_after = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .unwrap();
    assert!(!chick_after.2, "已出生");
    assert_ne!(
        (chick_after.0, chick_after.1),
        (20, 20),
        "出生移出 spawn 格"
    );
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_none(), "任务清空"),
        _ => panic!(),
    }
}

/// renewCreep 结算锁值（renew-creep.js）：[work,carry,move] 成本 200 →
/// effect=floor(1.2·1500/3/3)=200、扣费 ceil(1.2·200/3/3)=27；ageTime +200、
/// actionLog.healed 置位；免费涓流（energyAvailable 273<300）+1 → spawn 274。
#[test]
fn renew_creep_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let creep = f.creep(
        "old",
        "p1",
        20,
        21,
        &[BodyKind::Work, BodyKind::Carry, BodyKind::Move],
        0,
    );
    // Fixture::creep 的 age_time 是 u64::MAX/2（寿命尽）——改到 time+100
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(creep) {
        RoomObject::Creep(c) => {
            assert_eq!(c.age_time, 300, "ageTime(100) + effect(600/3=200)");
            assert_eq!(
                c.action_log & 0b1000_0000,
                0b1000_0000,
                "actionLog.healed 置位"
            );
        }
        _ => panic!(),
    }
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 274, "扣 27 + 涓流 1"),
        _ => panic!(),
    }
}

/// renewCreep 结算守卫（renew-creep.js 的逐门拒）：孵化中（含同 tick 先
/// createCreep）、CLAIM 部件、非邻接、能量不足、满寿——ageTime 全部不动。
#[test]
fn renew_creep_guards() {
    // ① spawn 孵化中拒（:12-14，先于 target 校验）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(
        &mut f,
        20,
        20,
        300,
        Some(kk_schema::room::SpawningJob {
            name: "busy".into(),
            need_time: 3,
            spawn_time: 100,
            directions: vec![],
        }),
    );
    let creep = f.creep("old", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(creep) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "孵化中的 spawn 不续命"),
        _ => panic!(),
    }

    // ② CLAIM 部件拒（:21-23）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let claimer = f.creep("claimer", "p1", 20, 21, &[BodyKind::Claim], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&claimer)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(claimer));
    f.settle(b);
    match f.obj(claimer) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "CLAIM 部件不可续命"),
        _ => panic!(),
    }

    // ③ 非邻接拒（:18-20）+ ④ 能量不足拒（:30-35）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 0, None);
    let far = f.creep("far", "p1", 30, 30, &[BodyKind::Move], 0);
    let near = f.creep("near", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&far)
        .unwrap()
    {
        c.age_time = 100;
    }
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&near)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(far));
    // near 也拒：energyAvailable 面为 0（该 fixture 无 controller——spawn 同时
    // 被 calc_spawns 标 off，可观察结果与官方 energyAvailable 0 < cost=20 一致；
    // 评审 3 O2：归因注释按实际机制写）
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(near));
    f.settle(b);
    for id in [far, near] {
        match f.obj(id) {
            RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "非邻接/能量不足不续命"),
            _ => panic!(),
        }
    }

    // ⑤ 满寿拒（:25-28：ageTime + effect > gameTime + CREEP_LIFE_TIME）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let full = f.creep("full", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&full)
        .unwrap()
    {
        c.age_time = 1500; // time=0：1500 + 600 > 0 + 1500
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(full));
    f.settle(b);
    match f.obj(full) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 1500, "满寿拒"),
        _ => panic!(),
    }

    // ⑥ 同 tick createCreep 先行置 spawning → renew 被 object.spawning 门拒
    //   （spawns/intents.js 分派序：createCreep → renewCreep）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let creep = f.creep("old", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_some(), "createCreep 先行生效"),
        _ => panic!(),
    }
    match f.obj(creep) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "renew 被孵化门拒"),
        _ => panic!(),
    }
}

/// recycleCreep 结算（recycle-creep.js:23 → _die(target, 1.0, false)，B3）：
/// creep 移除、dropRate=1.0 的全额造价回收进墓碑；非邻接拒。
#[test]
fn recycle_creep_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let trash = f.creep("trash", "p1", 20, 21, &[BodyKind::Move], 50);
    let far = f.creep("far", "p1", 30, 30, &[BodyKind::Move], 7);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RecycleCreep, IntentData::target(trash));
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", spawn, Action::RecycleCreep, IntentData::target(far));
    f.settle(b);
    assert!(f.world.get(trash).is_none(), "回收目标移除");
    // 墓碑承接（B3）：dropRate 1.0 + 满寿（fixture 的 age_time 极大 ⇒ lifeRate
    // 巨大 ⇒ 每件钳 CREEP_PART_MAX_ENERGY=125）⇒ 50 携带 + 125 回收 = 175；
    // 1 部件 ⇒ ticksToDecay = 5
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.x == 20 && t.y == 21 => Some(t.clone()),
            _ => None,
        })
        .expect("回收死亡产墓碑");
    assert_eq!(tomb.store.energy, 175, "携带 50 + 造价回收 125");
    assert_eq!(tomb.creep_id, Some(trash.0), "尸体信息（creepId）");
    assert_eq!(tomb.creep_name.as_deref(), Some("trash"));
    assert_eq!(
        tomb.decay_time, 5,
        "死亡 tick 0 + TOMBSTONE_DECAY_PER_PART × 1 件"
    );
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 20 && e.y == 21)),
        "不直接掉地（墓碑承接）"
    );
    f.settle(b2);
    assert!(f.world.get(far).is_some(), "非邻接不回收");
    match f.obj(far) {
        RoomObject::Creep(c) => assert_eq!(c.store.energy, 7),
        _ => panic!(),
    }
}

/// setSpawnDirections 结算（set-spawn-directions.js）：合法组替换、
/// 越界组整单忽略、空列表静默忽略。
#[test]
fn set_spawn_directions_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let job = kk_schema::room::SpawningJob {
        name: "child".into(),
        need_time: 3,
        spawn_time: 100,
        directions: vec![1, 2],
    };
    let spawn = spawn_with_job(&mut f, 20, 20, 300, Some(job));

    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.directions = vec![3, 5];
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, d);
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "合法组替换"
        ),
        _ => panic!(),
    }

    // 越界（0）→ 整单忽略（官方 _.any 反向判定——非法元素不静默过滤）
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.directions = vec![0, 1];
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, d);
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "越界组忽略"
        ),
        _ => panic!(),
    }

    // 空列表 → 静默忽略（官方 directions.length > 0 门）
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, IntentData::bare());
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "空列表忽略"
        ),
        _ => panic!(),
    }
}

/// cancelSpawning 结算（cancel-spawning.js）：雏形 creep 裸移除（无掉落无
/// 事件——官方 bulk.remove 非 _die）+ 孵化任务清空；无任务时 no-op。
#[test]
fn cancel_spawning_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let job = kk_schema::room::SpawningJob {
        name: "child".into(),
        need_time: 3,
        spawn_time: 100,
        directions: vec![1],
    };
    let spawn = spawn_with_job(&mut f, 20, 20, 300, Some(job));
    let chick = f.creep("child", "p1", 20, 20, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&chick)
        .unwrap()
    {
        c.spawning = true;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::CancelSpawning, IntentData::bare());
    f.settle(b);
    assert!(f.world.get(chick).is_none(), "雏形移除");
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_none(), "任务清空"),
        _ => panic!(),
    }
    // 无任务再 cancel：no-op（无对象可动）
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::CancelSpawning, IntentData::bare());
    f.settle(b);
    assert!(f.world.get(spawn).is_some());
}

/// 雏形寿命模型（官方 create-creep.js:80-98 **不带 ageTime** + creeps/tick.js
/// :88-91 出生后首触赋值——CREEP-SPAWNING-002 的 ticksToLive undefined 根基）：
/// 孵化中 age_time=0；出生 tick 赋 time+1500（CLAIM 部件 600）；spawn 消亡的
/// 孵化中雏形被 creep tick 移除（tick.js:16-19）。
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
    f.world.room_mut("W1N1").unwrap().objects.remove(&spawn); // spawn 消亡
    f.settle(IntentBatch::default()); // time 1→2：孤儿雏形移除
    assert!(
        f.world.get(orphan).is_none(),
        "同格无 spawn 的孵化中雏形移除"
    );
}

// ---- 掉落能量衰减 ----

#[test]
fn dropped_energy_decays() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let e = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 3,
                y: 3,
                resource_type: 0,
                amount: 1000,
                resource: None,
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    match f.obj(e) {
        RoomObject::Energy(x) => assert_eq!(x.amount, 999, "ceil(1000/1000)=1 衰减"),
        _ => panic!(),
    }
    // 小量快速消亡
    let e2 = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 4,
                y: 4,
                resource_type: 0,
                amount: 2,
                resource: None,
            }),
        )
        .unwrap();
    // 官方衰减 ceil(amount/1000)：2→1→0 两个 tick 清零
    f.settle(IntentBatch::default());
    f.settle(IntentBatch::default());
    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&e2),
        "≤0 移除"
    );
}

// ---- TTL 死亡 ----

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
            }),
        )
        .unwrap();
    // 官方 creeps/tick.js:93：gameTime >= ageTime-1 → time=1 的第二次结算死亡
    f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_some(), "time=0 < age_time-1=1：未死");
    let events = f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_none(), "time=1 >= 1：死亡");
    // B3（官方 _die.js）：墓碑承接——ticksToLive=1 的 lifeRate=0.2×1/1500，
    // 造价回收 (50+100)×1.33e-4 → floor 0，store 只含携带 30；2 部件 ⇒ 10 tick
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
    // _die.js:97：EVENT_OBJECT_DESTROYED {type: 'creep'}（B3 补齐）
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");
}
