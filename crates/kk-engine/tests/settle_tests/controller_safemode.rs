//! B1 结构治理批自 settle_tests.rs 连续切块（不重排；原段落横幅注释随块
//! 保留；Fixture/helpers 经 `use super::*` 从 crate 根取用——142 个
//! #[test] 计数与原文序不变）。

use super::*;

#[test]
fn heal_target_validation_rejections() {
    // heal.js 各前置门：spawning 施疗者（:11-13）、非 creep 目标（:17-19）、
    // spawning 目标（:17-19）、射程外（:21-23）、他人 safeMode（:25-27）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let medic = f.creep("medic", "p1", 10, 10, &[BodyKind::Heal], 0);
    let ally = f.creep("ally", "p1", 12, 10, &[BodyKind::Tough], 0);
    let chick = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: true,
                action_log: 0,
                name: "chick".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let ext = extension_at(&mut f, 11, 11, 1000, 0);
    // 近战射程 2 拒
    assert!(f.settle(heal_intent(medic, ally)).is_empty(), "射程 2 拒");
    // 非 creep 目标拒
    assert!(f.settle(heal_intent(medic, ext)).is_empty(), "结构不可治");
    // spawning 目标拒
    assert!(
        f.settle(heal_intent(medic, chick)).is_empty(),
        "spawning 拒"
    );
    // spawning 施疗者拒
    let mut b0 = IntentBatch::default();
    b0.add_object_intent("p1", chick, Action::Heal, IntentData::target(medic));
    assert!(f.settle(b0).is_empty(), "spawning 施疗者拒");
    // 他人 safeMode 拒（heal.js:25-27 与 attack 同形）
    let ctl = f.controller(2, Some("p2"));
    if let Some(RoomObject::Controller(t)) = f.world.get_mut(ctl) {
        t.safe_mode = 1000;
    }
    let near = f.creep("near", "p1", 11, 10, &[BodyKind::Tough], 0);
    assert!(f.settle(heal_intent(medic, near)).is_empty(), "safeMode 拒");
    assert_eq!(creep_parts(&f, near).0, 100);
}

#[test]
fn claim_controller_success_and_state_reset() {
    // claimController.js:45-53：level=1、owner 换人、progress=0、
    // downgradeTime=null、reservation=null；GCL 门（:41）：rooms=0 时
    // calcNeededGcl(1) = 1e6 × 0^2.4 = 0——任何 gcl 放行
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.progress = 777;
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 123,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Move], 0);
    f.players.insert("p1".into(), PlayerState::default());
    let events = f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert!(events.is_empty(), "claim 无事件（官方处理器不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.level, 1, "claim → level 1");
    assert_eq!(t.owner.as_deref(), Some("p1"));
    assert_eq!(t.progress, 0, "progress 清零");
    // claim 落地 downgradeTime=null（:51），同 tick 的 controller tick 随即按
    // tick.js:33-36 初始化挂钟（time + DOWNGRADE[1] + 1 = 20001）
    assert_eq!(t.downgrade_time, Some(20_001), "清空后同 tick 重挂钟");
    assert_eq!(t.reservation, None, "自己的预定随 claim 清除");
}

#[test]
fn claim_controller_rejections() {
    // claimController.js 逐分支：level>0（:25-27）、无 CLAIM 部件（:28-30）、
    // 他人预定（:31-33）、非邻接（:19-21）、目标非 controller（:16-18）
    let mut f = Fixture::new(1, terrain_with(&[]));
    // level > 0 拒
    let ctl = f.controller(1, Some("p2"));
    let c1 = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c1, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner.as_deref(), Some("p2"), "已占房不可夺");
    // 无 CLAIM 部件拒（无主房）
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c2 = f.creep("w2", "p1", 5, 6, &[BodyKind::Move], 0);
    f.settle(controller_intent(c2, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "无 CLAIM 部件拒");
    // 他人预定拒
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 999,
        });
    });
    let c3 = f.creep("w3", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c3, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "他人预定拒");
    // 非邻接（Chebyshev 2）拒
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c4 = f.creep("w4", "p1", 5, 8, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c4, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "距离 2 拒");
    // 目标非 controller（source）拒
    let src = f.source(10, 10, 100);
    let c5 = f.creep("w5", "p1", 10, 11, &[BodyKind::Claim], 0);
    assert!(
        f.settle(controller_intent(c5, Action::ClaimController, src))
            .is_empty()
    );
}

#[test]
fn claim_controller_gcl_gate() {
    // claimController.js:34-43：claimedRooms+1 级 GCL 不足拒。
    // calcNeededGcl(2) = GCL_MULTIPLY × 1^GCL_POW = 1e6（utils.js:661-663）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into()); // 已有一房
    p.gcl = 999_999;
    f.players.insert("p1".into(), p);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "gcl 999999 < 1e6 拒");
    // 门槛值放行（官方 < 严格比较：恰好 1e6 不拒）
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into());
    p.gcl = 1_000_000;
    f.players.insert("p1".into(), p);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner.as_deref(), Some("p1"), "gcl == 1e6 放行");
}

#[test]
fn claim_controller_own_room_rejected() {
    // claimController.js:26-28：level > 0 拒——自己已拥有的房再 claim 同路
    //（分支评审 3 补测：既有测试只锁他人房形态）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.owner.as_deref(), Some("p1"), "owner 不变");
    assert_eq!(t.level, 1, "level>0 拒对自家房同效（不重置）");
}

#[test]
fn claim_controller_gcl_reads_last_tick_set() {
    // 官方 quirk：GCL 门读上一 tick 定集（bulkUsers.addToSet 不改内存对象，
    // bulk.js:93-101 亲验）——同 tick 双 claim 都按旧 rooms 判定（rooms=
    // {W2N2}、gcl 恰好 1e6 时两条都放行）。锁定"settle 期不更新
    // PlayerState.rooms"（重建在 Engine tick 末；若被挪进 settle_room，
    // 第二条 claim 会读到增长的 rooms 而拒，此测试转红）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl1 = f.controller(0, None);
    let ctl2 = f.controller(0, None);
    let c1 = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let c2 = f.creep("w2", "p1", 5, 4, &[BodyKind::Claim], 0);
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into());
    p.gcl = 1_000_000;
    f.players.insert("p1".into(), p);
    let mut batch = controller_intent(c1, Action::ClaimController, ctl1);
    batch.add_object_intent("p1", c2, Action::ClaimController, IntentData::target(ctl2));
    f.settle(batch);
    let t1 = ctl_of(&f);
    assert_eq!(t1.owner.as_deref(), Some("p1"), "第一条过门");
    // 第二条也过门：GCL 读数是 settle 前的定集（官方同 tick 不涨）
    let t2 = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .get(&ctl2)
        .and_then(|o| match o {
            kk_schema::room::RoomObject::Controller(c) => Some(c.clone()),
            _ => None,
        })
        .expect("第二个 controller 存在");
    assert_eq!(
        t2.owner.as_deref(),
        Some("p1"),
        "同 tick 双 claim 都按上一 tick 定集放行"
    );
}

#[test]
fn attack_controller_zero_claim_rejected() {
    // attackController.js:34/:42：effect 为 0（无存活 CLAIM）整票拒——
    // 无削减、无封锁、无事件（评审 3 补测：若删掉 effect==0 守卫，
    // upgrade_blocked 断言抓回）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = Some(20_000);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    let ev = f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).downgrade_time, Some(20_000), "零 CLAIM 不削减");
    assert_eq!(ctl_of(&f).upgrade_blocked, None, "零 CLAIM 不挂封锁");
    assert!(ev.is_empty());
}

#[test]
fn attack_controller_reservation_drain_below_zero_cleared_same_tick() {
    // 预约削减穿透 0：官方 endTime − effect 得负数入内存、同 tick 被
    // controllers/tick 预定清除分支消解（清除条件在 0/负数下同真）——
    // 我方 u64 饱和 0，终态等价（评审 3 补测：锁"削穿即清除"官方可观察值）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 1,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(
        ctl_of(&f).reservation,
        None,
        "1 − 2 削穿 → 同 tick 清除（官方负数同终态）"
    );
}

#[test]
fn reserve_controller_success_and_cap() {
    // reserveController.js:35-52：新预定 endTime = gameTime+1 再 += effect；
    // 事件 amount=effect；上限 gameTime+CONTROLLER_RESERVE_MAX(5000)（:43-45）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Move], 0);
    let events = f.settle(controller_intent(c, Action::ReserveController, ctl));
    let t = ctl_of(&f);
    // time=0：base=1，effect=1×CONTROLLER_RESERVE(1) → 2
    assert_eq!(
        t.reservation,
        Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 2,
        })
    );
    assert_eq!(
        events,
        vec![serde_json::json!({
            "event": kk_core::constants::EVENT_RESERVE_CONTROLLER,
            "objectId": c.as_str(),
            "data": { "amount": 1 },
        })],
        "EVENT_RESERVE_CONTROLLER(8) amount=CLAIM×1"
    );
    // actionLog.reserveController 位（官方独立键，位图 bit8）
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 1 << 8),
        _ => panic!(),
    }
    // 续订：既有预约 end=100，5 部件 → 105（:49 累加）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 100,
        });
    });
    let c2 = f2.creep("w2", "p1", 5, 6, [BodyKind::Claim; 5].as_slice(), 0);
    f2.settle(controller_intent(c2, Action::ReserveController, ctl2));
    assert_eq!(
        ctl_of(&f2).reservation.as_ref().map(|r| r.end_time),
        Some(105),
        "续订 += 5"
    );
    // 上限窗：end=4998 + effect2 = 5000 恰好放行；end=4999 + effect2 拒
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 4998,
        });
    });
    let c3 = f3.creep("w3", "p1", 5, 6, [BodyKind::Claim; 2].as_slice(), 0);
    f3.settle(controller_intent(c3, Action::ReserveController, ctl3));
    assert_eq!(
        ctl_of(&f3).reservation.as_ref().map(|r| r.end_time),
        Some(5000),
        "恰好压线放行"
    );
    let mut f4 = Fixture::new(1, terrain_with(&[]));
    let ctl4 = f4.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 4999,
        });
    });
    let c4 = f4.creep("w4", "p1", 5, 6, [BodyKind::Claim; 2].as_slice(), 0);
    let ev = f4.settle(controller_intent(c4, Action::ReserveController, ctl4));
    assert_eq!(
        ctl_of(&f4).reservation.as_ref().map(|r| r.end_time),
        Some(4999),
        "超窗拒：预约不动"
    );
    assert!(ev.is_empty(), "拒分支无事件");
}

#[test]
fn reserve_controller_rejections() {
    // reserveController.js:26-33：已占房拒（房主也不行）、他人预定拒、
    // 无 CLAIM 部件拒（:31-33 无事件无 actionLog）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    let mine = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(mine, Action::ReserveController, ctl));
    assert_eq!(ctl_of(&f).reservation, None, "已占房不可预定");
    // 他人预定拒
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev = f2.settle(controller_intent(c2, Action::ReserveController, ctl2));
    assert_eq!(
        ctl_of(&f2).reservation.as_ref().map(|r| r.user.as_str()),
        Some("p2"),
        "他人预定不可覆盖"
    );
    assert!(ev.is_empty());
    // 无 CLAIM 部件：effect=0 拒，actionLog 不置位
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller(0, None);
    let c3 = f3.creep("w3", "p1", 5, 6, &[BodyKind::Move], 0);
    f3.settle(controller_intent(c3, Action::ReserveController, ctl3));
    assert_eq!(ctl_of(&f3).reservation, None);
    match f3.obj(c3) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 0, "零功率无 actionLog"),
        _ => panic!(),
    }
}

#[test]
fn reservation_expires_and_owned_room_clears_it() {
    // controllers/tick.js:10-12：gameTime >= endTime-1 到期清除（位于无主
    // 早退之前）；房被占（claim）同样清除
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 1, // time=0：0 >= 1-1=0 → 到期
        });
    });
    f.settle(IntentBatch::default());
    assert_eq!(ctl_of(&f).reservation, None, "到期清除");
    // 未到期保留：end=100 > time+1
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 100,
        });
    });
    f2.settle(IntentBatch::default());
    assert!(ctl_of(&f2).reservation.is_some(), "未到期保留");
}

#[test]
fn attack_controller_owned_downgrade_and_block() {
    // attackController.js:41-49：downgradeTime -= CLAIM×CONTROLLER_CLAIM_DOWNGRADE
    //(300)；_upgradeBlocked = gameTime+1000（tick 持久化）；:50 actionLog.attack；
    // :52 EVENT_ATTACK_CONTROLLER
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = Some(20_000);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    let events = f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.downgrade_time, Some(20_000 - 2 * 300), "2 CLAIM × 300");
    assert_eq!(t.upgrade_blocked, Some(1000), "time=0 + 1000");
    assert!(
        events.iter().any(
            |e| e["event"] == kk_core::constants::EVENT_ATTACK_CONTROLLER
                && e["objectId"] == c.as_str()
        ),
        "EVENT_ATTACK_CONTROLLER(3)（官方无 data 键——我方封套空对象）"
    );
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 1 << 3, "attack 位"),
        _ => panic!(),
    }
    // 升级封锁双门（upgradeController.js:25-27 + activateSafeMode.js:17）：
    // 下一 tick 升级被拒（progress 不动）
    let worker = f.creep("up", "p2", 5, 6, &[BodyKind::Work, BodyKind::Carry], 50);
    let mut b = IntentBatch::default();
    b.add_object_intent(
        "p2",
        worker,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    f.settle(b);
    assert_eq!(ctl_of(&f).progress, 0, "封锁期升级拒");
}

#[test]
fn attack_controller_reservation_drain() {
    // attackController.js:33-40：预约削减 endTime -= CLAIM×CONTROLLER_RESERVE；
    // 无主房不设 upgradeBlocked
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(
        t.reservation,
        Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 498,
        }),
        "500 − 2×1，user 保留（官方 merge 语义）"
    );
    assert_eq!(t.upgrade_blocked, None, "无主分支不设封锁");
}

#[test]
fn attack_controller_rejections() {
    // attackController.js:22-28：原始房拒；他人 safeMode 拒；
    // upgradeBlocked > gameTime 拒（对房主同样生效）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).downgrade_time, None, "无主无预定不可攻");
    // 他人 safeMode（:25-26）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 1000;
        t.downgrade_time = Some(20_000);
    });
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev = f2.settle(controller_intent(c2, Action::AttackController, ctl2));
    assert_eq!(ctl_of(&f2).downgrade_time, Some(20_000), "safeMode 拒");
    assert!(ev.is_empty());
    // 房主自己也被 upgradeBlocked 拒（:26-27）
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller_full(1, Some("p1"), |t| {
        t.downgrade_time = Some(20_000);
        t.upgrade_blocked = Some(500);
    });
    let c3 = f3.creep("w3", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev3 = f3.settle(controller_intent(c3, Action::AttackController, ctl3));
    assert_eq!(ctl_of(&f3).downgrade_time, Some(20_000), "封锁期拒");
    assert!(ev3.is_empty());
}

#[test]
fn attack_controller_null_downgrade_time_loses_room_next_tick() {
    // 官方 quirk：downgradeTime 未挂钟（新 claim 房的当 tick）时 attack 的
    // `null − effect` 得负数 → 降级链即刻触发（attackController.js:46 +
    // controllers/tick.js:49-63）。u64 饱和到 0——触发条件同真
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = None;
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.level, 0, "当 tick 即走降级链归零");
    assert_eq!(t.owner, None, "主权丢失");
    assert!(
        t.owner_cleared,
        "降级归零置 owner 哨兵（tick.js:56 user=null）"
    );
    assert_eq!(
        t.safe_mode_cooldown,
        Some(kk_core::constants::SAFE_MODE_COOLDOWN as u64),
        "降级到 0 挂冷却（tick.js:61）"
    );
}

#[test]
fn downgrade_then_claim_resets_owner_sentinel() {
    // owner 三态往返锁：降级归零（cleared=true）→ 重新 claim（cleared=false）。
    // 防未来 claim 路径漏复位导致落库态长期不一致（投影 Some 分支优先，
    // 漏复位属潜伏态——owner_cleared 断言是唯一可见面）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = None; // 当 tick即归零（同上 quirk）
    });
    let attacker = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(attacker, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).owner, None);
    assert!(ctl_of(&f).owner_cleared, "归零后哨兵置位");
    // p1 重新 claim（claimController.js:45-53 只写 user——哨兵同步复位）
    let claimer = f.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(claimer, Action::ClaimController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.owner, Some("p1".into()));
    assert!(!t.owner_cleared, "claim 复位哨兵");
    assert_eq!(t.level, 1);
}

#[test]
fn attack_controller_suppressed_by_heal() {
    // intents.js:5 压制图：attackController 被 heal 压制——同 tick heal 存在
    // 则 attackController 不执行（玩家 API 早已返回成功）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Heal], 0);
    let ally = f.creep("ally", "p1", 5, 5, &[BodyKind::Tough], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::AttackController, IntentData::target(ctl));
    b.add_object_intent("p1", c, Action::Heal, IntentData::target(ally));
    let events = f.settle(b);
    assert_eq!(
        ctl_of(&f).reservation.as_ref().map(|r| r.end_time),
        Some(500),
        "attackController 被 heal 压制：预约不动"
    );
    assert!(
        events
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_HEAL),
        "heal 照常执行"
    );
    assert!(
        !events
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_ATTACK_CONTROLLER),
        "无 attackController 事件"
    );
}

#[test]
fn sign_controller_sets_and_clears() {
    // signController.js:23-28：{user, text, time: gameTime}；空/缺省 sign 落
    // null（官方 `intent.sign ?` 判空 + userString 的 ||"" 折叠）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(ctl);
    d.sign = Some("reserved sector".into());
    b.add_object_intent("p1", c, Action::SignController, d);
    f.settle(b);
    // datetime 经 fixture 固定墙钟已可锁值（同 tick 多签共享同一 now_ms——
    // 官方逐签 Date.now() 的 ms 粒度下几乎恒同值，灰区登记）；此处比较业务
    // 三元组与 datetime 齐备性
    assert_eq!(
        ctl_of(&f)
            .sign
            .as_ref()
            .map(|s| (s.user.as_str(), s.text.as_str(), s.time)),
        Some(("p1", "reserved sector", 0)),
    );
    assert!(
        ctl_of(&f).sign.as_ref().unwrap().datetime.is_some(),
        "签名结算落 datetime"
    );
    // 清除：sign 缺省
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::SignController, IntentData::target(ctl));
    f.settle(b);
    assert_eq!(ctl_of(&f).sign, None, "缺省 sign = 清除");
    // 再签 + 空串清除
    let mut b2 = IntentBatch::default();
    let mut d2 = IntentData::target(ctl);
    d2.sign = Some("again".into());
    b2.add_object_intent("p1", c, Action::SignController, d2);
    f.settle(b2);
    assert!(ctl_of(&f).sign.is_some());
    let mut b3 = IntentBatch::default();
    let mut d3 = IntentData::target(ctl);
    d3.sign = Some(String::new());
    b3.add_object_intent("p1", c, Action::SignController, d3);
    f.settle(b3);
    assert_eq!(ctl_of(&f).sign, None, "空串 = 清除（官方 falsy）");
    // 距离 2 拒（signController.js:18-20）
    let far = f.creep("far", "p1", 5, 8, &[BodyKind::Move], 0);
    let mut b4 = IntentBatch::default();
    let mut d4 = IntentData::target(ctl);
    d4.sign = Some("x".into());
    b4.add_object_intent("p1", far, Action::SignController, d4);
    f.settle(b4);
    assert_eq!(ctl_of(&f).sign, None, "非邻接拒");
}

/// ADR-013 决策 5 的重放一致性回归：同初始世界 + 同 intents + 同 TickInput
/// 两次结算产物逐字节一致（含 signController 的 datetime 落盘——原
/// `SystemTime::now` 内联读取是结算链的唯一非确定性源，两次执行必然不同）；
/// 不同 TickInput 只改 datetime（玩家可见的墙钟语义不变）。
#[test]
fn tick_input_replay_consistency() {
    fn run_once(now_ms: u64) -> (String, (String, String, u64, Option<u64>)) {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let ctl = f.controller(2, Some("p2"));
        let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
        let mut b = IntentBatch::default();
        let mut d = IntentData::target(ctl);
        d.sign = Some("replayable".into());
        b.add_object_intent("p1", c, Action::SignController, d);
        f.settle_with_input(b, kk_engine::settle::TickInput { now_ms });
        let s = ctl_of(&f).sign.as_ref().unwrap();
        let sign = (s.user.clone(), s.text.clone(), s.time, s.datetime);
        let world = serde_json::to_string(&f.world.world_json_for_viewer("p1", 0)).unwrap();
        (world, sign)
    }
    let (world_a, sign_a) = run_once(1_750_000_000_000);
    let (world_b, sign_b) = run_once(1_750_000_000_000);
    assert_eq!(world_a, world_b, "同输入两次结算的世界投影逐字节一致");
    assert_eq!(sign_a, sign_b, "sign 落盘逐字节一致");
    assert_eq!(sign_a.3, Some(1_750_000_000_000), "datetime = 注入墙钟");
    let (world_c, sign_c) = run_once(1_750_000_123_456);
    assert_eq!(
        sign_c.3,
        Some(1_750_000_123_456),
        "墙钟输入可变（首跑实际值形态）"
    );
    assert_ne!(world_a, world_c, "datetime 落盘进世界投影");
}

/// [`kk_engine::settle::Engine::tick_with_intents_with_input`] 的引擎级重放
/// （B0-2 对抗评审 P2-2 清偿）：上面 Fixture 版直调 settle_room 绕过了
/// Engine 层的房间循环/共享 next_id/intent 按房过滤——本测试经引擎入口
/// 双房各签一个 controller，同 TickInput 双跑的世界投影逐字节一致。
#[test]
fn tick_input_replay_consistency_engine_level() {
    use kk_engine::settle::TickInput;
    fn run_once(now_ms: u64) -> String {
        let mut world = World::new(7);
        world.register_room("W1N1", kk_core::Terrain::plains());
        world.register_room("E2N2", kk_core::Terrain::plains());
        let mut batch = IntentBatch::default();
        for name in ["W1N1", "E2N2"] {
            let ctl = world
                .insert(
                    name,
                    RoomObject::Controller(kk_schema::room::ControllerObj {
                        id: 0,
                        x: 5,
                        y: 5,
                        level: 2,
                        safe_mode: 0,
                        safe_mode_available: 0,
                        progress: 0,
                        downgrade_time: None,
                        owner: Some("p2".into()),
                        owner_cleared: false,
                        is_power_enabled: false,
                        reservation: None,
                        sign: None,
                        upgrade_blocked: None,
                        safe_mode_cooldown: None,
                    }),
                )
                .unwrap();
            let creep_id = world
                .insert(
                    name,
                    RoomObject::Creep(CreepObj {
                        id: 0,
                        x: 5,
                        y: 6,
                        fatigue: 0,
                        age_time: u64::MAX / 2,
                        hits: 100,
                        spawning: false,
                        action_log: 0,
                        name: format!("w-{name}"),
                        owner: "p1".into(),
                        body: body(&[BodyKind::Move]),
                        store: kk_schema::room::Store {
                            energy: 0,
                            ghodium: 0,
                        },
                        store_extra: Vec::new(),
                        say_message: None,
                        say_public: None,
                        notify_when_attacked: None,
                    }),
                )
                .unwrap();
            let mut d = IntentData::target(ctl);
            d.sign = Some(format!("replay {name}"));
            batch.add_object_intent("p1", creep_id, Action::SignController, d);
        }
        let mut engine = kk_engine::settle::Engine::new(world);
        engine.players.insert(
            "p1".to_string(),
            PlayerState {
                gcl: 1,
                rooms: BTreeSet::new(),
                raw_memory: String::new(),
            },
        );
        engine
            .tick_with_intents_with_input(batch, TickInput { now_ms })
            .unwrap();
        serde_json::to_string(&engine.world.world_json_for_viewer("p1", 0)).unwrap()
    }
    let a = run_once(1_750_000_000_000);
    let b = run_once(1_750_000_000_000);
    assert_eq!(a, b, "引擎级同输入双跑世界投影逐字节一致（双房+签名）");
    let c = run_once(1_750_000_999_999);
    assert_ne!(a, c, "不同墙钟输入产生不同投影");
}

#[test]
fn sign_text_truncated_to_100_chars() {
    // system.js:74 `sign: 'userString'` → 截 100（UTF-16 码元 vs Unicode
    // chars 的差异沿用既有灰区登记——BMP 文本两者等值）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    // 截断发生在清洗层（system.js:74 `sign: 'userString'`）——经生产清洗入口
    // from_json 构造（引擎层不截、官方同形）
    let raw = serde_json::json!({
        c.as_str(): { "signController": { "id": ctl.as_str(), "sign": "x".repeat(150) } }
    });
    let b = IntentBatch::from_json("p1", &raw);
    f.settle(b);
    assert_eq!(
        ctl_of(&f).sign.as_ref().map(|s| s.text.chars().count()),
        Some(100),
        "签名文本截 100"
    );
}

#[test]
fn generate_safe_mode_consumes_ghodium() {
    // generateSafeMode.js:19-24：store["G"] >= SAFE_MODE_COST(1000) →
    // safeModeAvailable += 1、扣 1000 ghodium；不足拒（无部分消耗）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(3, Some("p1"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f.world.get_mut(c).unwrap() {
        cr.store.ghodium = 1500;
    }
    f.settle(controller_intent(c, Action::GenerateSafeMode, ctl));
    assert_eq!(ctl_of(&f).safe_mode_available, 1, "available 0→1");
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.store.ghodium, 500, "1500 − 1000"),
        _ => panic!(),
    }
    // 不足拒
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller(3, Some("p1"));
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f2.world.get_mut(c2).unwrap() {
        cr.store.ghodium = 999;
    }
    f2.settle(controller_intent(c2, Action::GenerateSafeMode, ctl2));
    assert_eq!(ctl_of(&f2).safe_mode_available, 0, "999 < 1000 拒");
    // 非邻接拒
    let c3 = f2.creep("w3", "p1", 5, 8, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f2.world.get_mut(c3).unwrap() {
        cr.store.ghodium = 5000;
    }
    f2.settle(controller_intent(c3, Action::GenerateSafeMode, ctl2));
    assert_eq!(ctl_of(&f2).safe_mode_available, 0, "距离 2 拒");
}

#[test]
fn activate_safe_mode_full_chain() {
    // activateSafeMode.js:6-24（意图半步）+ controllers/tick.js:25-31（消费
    // 半步）：available −1、safeMode = time+SAFE_MODE_DURATION(20000)、
    // safeModeCooldown = time+SAFE_MODE_COOLDOWN(50000)。
    // time 取 10：gameTime 0 时未挂冷却的 null 在官方 `cooldown >= gameTime`
    // 数值比较中是 0 >= 0 → 拒（JS null 强制转换角落，行为已由
    // activate_safe_mode_rejections 的 cooldown 用例锁住）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.set_time_for_test(10);
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001); // 健康窗：>= time + 20000/2 − 5000
    });
    let events = f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert!(events.is_empty(), "激活链无事件（官方两半步都不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.safe_mode_available, 0, "消耗一枚");
    assert_eq!(t.safe_mode, 10 + 20_000, "time + SAFE_MODE_DURATION");
    assert_eq!(
        t.safe_mode_cooldown,
        Some(10 + 50_000),
        "time + SAFE_MODE_COOLDOWN"
    );
}

#[test]
fn activate_safe_mode_rejections() {
    // activateSafeMode.js 逐门：无 available（:11）、冷却中（:14）、
    // upgradeBlocked（:17）、降级危险窗（:20）、无主/0 级（:8）
    fn base(f: &mut Fixture, tweak: &dyn Fn(&mut kk_schema::room::ControllerObj)) -> ObjId {
        f.world.set_time_for_test(10);
        f.controller_full(1, Some("p1"), |t| {
            t.safe_mode_available = 1;
            t.downgrade_time = Some(10 + 20_001);
            tweak(t);
        })
    }
    // 无 available
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = base(&mut f, &|t| t.safe_mode_available = 0);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode, 0, "无 available 拒");
    // 冷却中（cooldown >= gameTime）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = base(&mut f2, &|t| t.safe_mode_cooldown = Some(100)); // >= 10
    f2.settle(controller_action(
        "p1",
        ctl2,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f2).safe_mode, 0, "冷却中拒");
    // upgradeBlocked
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = base(&mut f3, &|t| t.upgrade_blocked = Some(500));
    f3.settle(controller_action(
        "p1",
        ctl3,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f3).safe_mode, 0, "封锁期拒");
    // 降级危险窗：level1 阈值 time+5000；downgradeTime=time+4999 拒 / +5000 放行
    let mut f4 = Fixture::new(1, terrain_with(&[]));
    let ctl4 = base(&mut f4, &|t| t.downgrade_time = Some(10 + 4_999));
    f4.settle(controller_action(
        "p1",
        ctl4,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f4).safe_mode, 0, "危险窗拒（5009 < 5010）");
    let mut f5 = Fixture::new(1, terrain_with(&[]));
    let ctl5 = base(&mut f5, &|t| t.downgrade_time = Some(10 + 5_000));
    f5.settle(controller_action(
        "p1",
        ctl5,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(
        ctl_of(&f5).safe_mode,
        10 + 20_000,
        "恰好压线放行（< 严格比较）"
    );
    // 无主 / 0 级
    let mut f6 = Fixture::new(1, terrain_with(&[]));
    let ctl6 = f6.controller_full(0, None, |t| t.safe_mode_available = 1);
    f6.settle(controller_action(
        "p1",
        ctl6,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f6).safe_mode, 0, "无主 0 级拒");
    // downgradeTime 未挂钟（null → 0 < time+5000）拒——新 claim 房当 tick不可激活
    let mut f7 = Fixture::new(1, terrain_with(&[]));
    let ctl7 = base(&mut f7, &|t| t.downgrade_time = None);
    f7.settle(controller_action(
        "p1",
        ctl7,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f7).safe_mode, 0, "未挂钟拒（官方 null 数值比较）");
}

#[test]
fn same_tick_attack_cancels_safe_mode_activation() {
    // 官方同序语义：attackController 的 _upgradeBlocked 被 tick.js:20-23 先
    // 持久化、:25 激活门读到它 → 激活取消（available 不减、safeMode 不挂）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.set_time_for_test(10);
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut b = controller_intent(c, Action::AttackController, ctl);
    b.add_object_intent("p1", ctl, Action::ActivateSafeMode, IntentData::bare());
    f.settle(b);
    let t = ctl_of(&f);
    assert_eq!(t.upgrade_blocked, Some(10 + 1000), "攻击封锁已持久化");
    assert_eq!(t.safe_mode_available, 1, "激活被同 tick 攻击取消：不消耗");
    assert_eq!(t.safe_mode, 0, "safeMode 未挂");
}

#[test]
fn enemy_stomp_blocked_under_safe_mode() {
    // movement.js:224：`!roomController || controller.user === mover ||
    // !(safeMode > gameTime)` 才允许踩敌方工地——B2 补齐的门（灰区清偿项）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 1000;
    });
    let _ = ctl;
    let site = f.site(10, 11, "road", 10, 300, "p2");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "进格本身不受门影响");
    assert!(f.world.get(site).is_some(), "敌方 safeMode 下工地受保护");
    // 门前对照：safeMode 过期 → 工地被踩
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 0;
    });
    let _ = ctl2;
    let site2 = f2.site(10, 11, "road", 10, 300, "p2");
    let c2 = f2.creep("w2", "p1", 10, 10, &[BodyKind::Move], 0);
    f2.settle(move_intent(c2, 5));
    assert!(f2.world.get(site2).is_none(), "无 safeMode 照常踩");
}
