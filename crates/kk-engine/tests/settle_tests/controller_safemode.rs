//! 安全模式激活、消耗与跨房限制域测试。

use super::*;

#[test]
fn generate_safe_mode_consumes_ghodium() {
    // generateSafeMode.js:19-24：store["G"] >= SAFE_MODE_COST(1000) →
    // safeModeAvailable += 1、扣 1000 ghodium；不足拒（无部分消耗）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(3, Some("p1"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    f.world
        .edit_object(c, |object| {
            if let RoomObject::Creep(cr) = object {
                cr.store.ghodium = 1500;
            }
            Ok(())
        })
        .unwrap();
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
    f2.world
        .edit_object(c2, |object| {
            if let RoomObject::Creep(cr) = object {
                cr.store.ghodium = 999;
            }
            Ok(())
        })
        .unwrap();
    f2.settle(controller_intent(c2, Action::GenerateSafeMode, ctl2));
    assert_eq!(ctl_of(&f2).safe_mode_available, 0, "999 < 1000 拒");
    // 非邻接拒
    let c3 = f2.creep("w3", "p1", 5, 8, &[BodyKind::Move], 0);
    f2.world
        .edit_object(c3, |object| {
            if let RoomObject::Creep(cr) = object {
                cr.store.ghodium = 5000;
            }
            Ok(())
        })
        .unwrap();
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
    f.world.set_time_for_test(10).unwrap();
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
        f.world.set_time_for_test(10).unwrap();
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
    f.world.set_time_for_test(10).unwrap();
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
    // 房主安全模式生效时，敌方 creep 移入工地格也不能踩掉工地。
    // 来源：上游 intents/movement.js 的敌方工地移除条件。
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

#[test]
fn downgrade_sets_safe_mode_cooldown() {
    // 控制器降级会清零安全模式可用数并设置冷却。
    // 来源：上游 controllers/tick.js 的降级处理。
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
fn activate_safe_mode_owner_gate_at_dispatch() {
    // processor.js:248：`object.user && object.user != userId` → 整对象 intent
    // 跳过——敌人对他人 controller 发 activateSafeMode 在分派层丢弃
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.set_time_for_test(10).unwrap();
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
    assert_eq!(f.world.time(), 0);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode, 0, "time=0 + 冷却 None（0>=0）拒");
}

#[test]
fn safe_mode_blocks_hostile_attack_and_dismantle() {
    // attack.js:30-32 / dismantle.js:24-26：他人 safeMode 生效期拒（引擎层二次门）。
    // attack 与 dismantle 各自独立 settle 验证——两者同 creep 同 tick 会互相压制
    // （dismantle 在 attack 的压制表里），单一空事件断言无法归因到 attack 的门
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Work], 0);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
                id: 0,
                x: 10,
                y: 11,
                hits: 1000,
            }),
        )
        .unwrap();
    // 直接设置控制器的安全模式，单独验证攻击和拆解的结算检查。
    f.world
        .edit_object(ctl, |object| {
            if let Some(RoomObject::Controller(t)) = Some(object) {
                t.safe_mode = 1000;
            }
            Ok(())
        })
        .unwrap();
    let ext = extension_at(&mut f, 11, 10, 1000, 0);
    // attack 的门：单独 settle
    let events = f.settle(attack_intent(atk, ext));
    assert!(events.is_empty(), "safeMode：攻击拒");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    // dismantle 的门：单独 settle
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", atk, Action::Dismantle, IntentData::target(wall));
    let events2 = f.settle(batch);
    assert!(events2.is_empty(), "safeMode：拆解拒");
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 1000),
        _ => panic!(),
    }
}
