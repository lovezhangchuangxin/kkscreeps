//! 能量源容量、预定与采集域测试。

use super::*;

#[test]
fn source_capacity_3000_under_reservation() {
    // Source 在有房主或有效预定时容量为 3000，无主且无预定时为 1500。
    // 本例预定远未到期，避免对象 tick 顺序影响预定是否存在。
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 5000,
        });
    });
    let src = f.source(10, 10, 1200);
    // fixture 的 source 挂 3000 容量——先压到 1500 模拟无预定期的归一
    f.world
        .edit_object(src, |object| {
            if let RoomObject::Source(s) = object {
                s.energy_capacity = 1500;
            }
            Ok(())
        })
        .unwrap();
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
    f2.world
        .edit_object(src2, |object| {
            if let RoomObject::Source(s) = object {
                s.energy_capacity = 3000;
            }
            Ok(())
        })
        .unwrap();
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
    // 他人预定房间中的采集被拒绝，自己的预定允许采集。来源：上游 creeps/harvest.js。
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
    f.world
        .edit_room("W1N1", |room| {
            for object in room.objects.values_mut() {
                if let RoomObject::Controller(t) = object {
                    t.reservation = Some(kk_schema::room::Reservation {
                        user: "p1".into(),
                        end_time: 5000,
                    });
                }
            }
            Ok(())
        })
        .unwrap();
    let events2 = f.settle(controller_intent(c, Action::Harvest, src));
    assert!(
        events2
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_HARVEST),
        "自己的预定放行"
    );
}

#[test]
fn carry_death_drops_overflow_energy() {
    // tick.js:141-143 → _drop-resources-without-space：CARRY 部件损毁缩容 → 超容掉落
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f.creep(
        "haul",
        "p2",
        11,
        10,
        &[BodyKind::Carry, BodyKind::Carry],
        80,
    );
    f.settle(attack_intent(a, b));
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 80, "200-120");
    assert_eq!(parts, vec![0, 80], "首个 carry 报废");
    assert_eq!(f.creep_state(b).3, 50, "容量缩至 50");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(30), "超容 80-50 掉地（tick 阶段不衰减）");
}

/// 命名矿物 Z 已占满容量，采得的 U 因超容掉落，energy 和 Z 不变。
/// 来源：上游 creeps/harvest.js 的 calcResources 与矿物 drop 分支。
#[test]
fn harvest_mixed_holding_overflow_drops_mined_mineral() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let m = f.mineral(10, 10, "U", 70_000, 3, None);
    f.extractor(10, 10, Some("p1"), 0);
    // 容量 50 已被 Z 占满（命名槽）
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 11,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Work, BodyKind::Work, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Harvest, IntentData::target(m));
    f.settle(batch);
    // 全槽 sum = 50(Z)+2(U 产出)=52 > 50 → 掉 2 U（intent 相掉落同 tick 衰减
    // ceil(2/1000)=1 → 堆 1）；Z 槽不动、energy 不动
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(cc.store.energy, 0, "energy 槽不动（只掉本 tick 产出资源）");
            assert_eq!(
                cc.store_extra,
                vec![
                    kk_schema::room::ResourceStack {
                        resource: "Z".into(),
                        amount: 50
                    },
                    kk_schema::room::ResourceStack {
                        resource: "U".into(),
                        amount: 0
                    },
                ],
                "Z 槽不动；U 产出全掉但键留 0（官方 store[U]=0 落库同构）"
            );
        }
        _ => panic!(),
    }
    let u_pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e)
                if e.x == 10 && e.y == 11 && e.resource.as_deref() == Some("U") =>
            {
                Some(e.amount)
            }
            _ => None,
        });
    assert_eq!(
        u_pile,
        Some(1),
        "掉 2 U 同 tick 衰减 1 → 堆 1（官方出生 tick 衰减一致）"
    );
}

/// Z 已占满容量，source 采集只增加 energy 槽；按全仓超容量掉落 energy。
/// 不能将全仓总量写入 energy 槽，否则会复制已持有的矿物。
#[test]
fn harvest_source_mixed_holding_overflow_drops_energy_only() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f
        .world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 10,
                y: 10,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    // 容量 50 已被 Z 占满
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 11,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Work, BodyKind::Work, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    f.settle(batch);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(
                cc.store.energy, 0,
                "采 4 → 全槽 54>50 → 掉 min(4, 4)=4，energy 归 0"
            );
            assert_eq!(
                cc.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 50
                }],
                "Z 槽不动（只掉本 tick 产出的 energy）"
            );
        }
        _ => panic!(),
    }
    // 掉 4 energy 同 tick 衰减 ceil(4/1000)=1 → 堆 3
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 11 => Some(e.amount),
            _ => None,
        });
    assert_eq!(pile, Some(3), "掉 4 同 tick 衰减 1");
}

/// CARRY 损毁后按 RESOURCES_ALL 顺序掉落全部资源中的超量部分。
/// 本例先掉 energy，再掉 Z；对象 tick 中产生的掉落堆本轮不衰减。
#[test]
fn carry_death_drops_overflow_mixed_resources() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "haul".into(),
                owner: "p2".into(),
                body: body(&[BodyKind::Carry, BodyKind::Carry]),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "Z".into(),
                    amount: 60,
                }],
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    f.settle(attack_intent(a, b));
    // 容量 100→50（毁一 carry）；总持有 40e+60Z=100 > 50 → 掉 50：
    // energy 40 全掉 → Z 补 10 → creep 留 Z 50
    assert_eq!(f.creep_state(b).3, 0, "energy 全掉");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 40), ("Z".to_string(), 10)],
        "序贯掉落 energy 先 Z 补足（tick 相不衰减）"
    );
    match f.obj(b) {
        RoomObject::Creep(cc) => assert_eq!(
            cc.store_extra,
            vec![kk_schema::room::ResourceStack {
                resource: "Z".into(),
                amount: 50
            }],
            "Z 留 50"
        ),
        _ => panic!(),
    }
}

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

/// 零绝对钟按官方真值门重新挂钟；干扰先延后，再判断已逾期的普通再生。
#[test]
fn source_disrupt_initializes_absent_zero_clock_and_preserves_overdue_regeneration() {
    use kk_core::constants as c;
    for (clock, expected_clock, expected_energy) in [
        (None, Some(401), 0),
        (Some(0), Some(401), 0),
        (Some(101), Some(102), 0),
        (Some(99), None, 3000),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        f.controller(8, Some("p1"));
        let src = f.source(10, 10, 0);
        f.world.set_time_for_test(100).unwrap();
        f.world
            .edit_room("W1N1", |room| {
                if let Some(RoomObject::Source(s)) = room.objects.get_mut(&src) {
                    s.next_regeneration_time = clock;
                }
                room.effects.insert(
                    src,
                    vec![kk_world::EffectEntry {
                        effect: c::PWR_DISRUPT_SOURCE,
                        power: c::PWR_DISRUPT_SOURCE,
                        level: 1,
                        end_time: 200,
                    }],
                );
                Ok(())
            })
            .unwrap();
        f.settle(IntentBatch::default());
        let RoomObject::Source(s) = f.obj(src) else {
            panic!("非 source")
        };
        assert_eq!(s.next_regeneration_time, expected_clock, "{clock:?}");
        assert_eq!(s.energy, expected_energy, "{clock:?}");
    }
}

/// 容量切换同轮的周期补充先使用旧容量；所有权调整随后发生。
#[test]
fn source_regen_pulse_uses_old_capacity_before_controller_capacity_update() {
    use kk_core::constants as c;
    for (owner, old_cap, energy, expected_cap, expected_energy) in [
        (Some("p1"), 1500, 1490, 3000, 1500),
        (None, 3000, 1490, 1500, 1500),
    ] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        f.controller(8, owner);
        let src = f.source(10, 10, energy);
        f.world.set_time_for_test(100).unwrap();
        f.world
            .edit_room("W1N1", |room| {
                if let Some(RoomObject::Source(s)) = room.objects.get_mut(&src) {
                    s.energy_capacity = old_cap;
                    s.next_regeneration_time = Some(500);
                }
                room.effects.insert(
                    src,
                    vec![kk_world::EffectEntry {
                        effect: c::PWR_REGEN_SOURCE,
                        power: c::PWR_REGEN_SOURCE,
                        level: 5,
                        end_time: 101,
                    }],
                );
                Ok(())
            })
            .unwrap();
        f.settle(IntentBatch::default());
        let RoomObject::Source(s) = f.obj(src) else {
            panic!("非 source")
        };
        assert_eq!(s.energy_capacity, expected_cap);
        assert_eq!(s.energy, expected_energy);
        assert_eq!(s.next_regeneration_time, Some(500));
    }
}
