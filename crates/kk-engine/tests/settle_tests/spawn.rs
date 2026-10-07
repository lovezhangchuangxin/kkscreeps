//! 孵化与出生域测试：spawnCreep 全周期、出生阻挡与结构配额。

use super::*;

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
    // Fixture 默认寿命截止值为 u64::MAX/2，改为 time+100 以允许 renew。
    f.world
        .edit_object(creep, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
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
    f.world
        .edit_object(creep, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
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
    f.world
        .edit_object(claimer, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
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
    f.world
        .edit_object(far, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
    f.world
        .edit_object(near, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(far));
    // 本夹具没有控制器，spawn 被标记 off，可用能量为 0，renew 无法扣款。
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
    f.world
        .edit_object(full, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 1500; // time=0：1500 + 600 > 0 + 1500
            }
            Ok(())
        })
        .unwrap();
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
    f.world
        .edit_object(creep, |object| {
            if let RoomObject::Creep(c) = object {
                c.age_time = 100;
            }
            Ok(())
        })
        .unwrap();
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

/// 回收使用 dropRate=1.0，资源进入墓碑；非邻接目标被拒绝。
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
    // 夹具寿命极长，每件回收量钳到 125；携带 50 加单件回收得到墓碑 energy=175。
    // 单件身体对应 5 tick 衰减期限。
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
    f.world
        .edit_object(chick, |object| {
            if let RoomObject::Creep(c) = object {
                c.spawning = true;
            }
            Ok(())
        })
        .unwrap();
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
