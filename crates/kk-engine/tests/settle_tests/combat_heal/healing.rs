//! 从父测试模块拆出的领域验收，复用原夹具。
use super::*;

#[test]
fn heal_restores_hits_and_redistributes_parts() {
    // heal.js:29 + tick.js:126-139：+HEAL×HEAL_POWER(12) 延迟施加，治疗同样
    // 重排部件分布（从尾部灌满——前列部件先回血）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Tough], 0);
    let enemy = f.creep("atk", "p2", 11, 10, &[BodyKind::Attack], 0);
    let healer = f.creep("medic", "p1", 10, 11, &[BodyKind::Heal], 0);
    // t1：受击 30 → 170，parts [70,100]
    f.settle(attack_intent_by("p2", enemy, victim));
    assert_eq!(creep_parts(&f, victim), (170, vec![70, 100]));
    // t2：近战治疗 +12 → 182，parts [82,100]（前列回血）
    let events = f.settle(heal_intent(healer, victim));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_HEAL);
    assert_eq!(events[0]["objectId"], healer.as_str());
    assert_eq!(events[0]["data"]["targetId"], victim.as_str());
    assert_eq!(events[0]["data"]["amount"], 12, "1×HEAL×12");
    assert_eq!(
        events[0]["data"]["healType"],
        kk_core::constants::EVENT_HEAL_TYPE_MELEE
    );
    assert_eq!(creep_parts(&f, victim), (182, vec![82, 100]));
    // actionLog：heal 位（施疗者）+ healed 位（受疗者）
    let (log_healer, log_victim) = match (f.obj(healer), f.obj(victim)) {
        (RoomObject::Creep(a), RoomObject::Creep(b)) => (a.action_log, b.action_log),
        _ => panic!(),
    };
    assert_eq!(log_healer, kk_engine::settle::creep::ACTION_HEAL as u16);
    assert_eq!(log_victim, kk_engine::settle::creep::ACTION_HEALED as u16);
}

#[test]
fn ranged_heal_range_and_power() {
    // rangedHeal.js:21-23 射程 3；:26 功率 = HEAL×RANGED_HEAL_POWER(4)
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    f.world
        .edit_object(victim, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.hits = 50; // 直接造伤
            }
            Ok(())
        })
        .unwrap();
    let medic = f.creep("medic", "p1", 13, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    let events = f.settle(ranged_heal_intent(medic, victim));
    assert_eq!(events[0]["data"]["amount"], 8, "2×HEAL×4");
    assert_eq!(
        events[0]["data"]["healType"],
        kk_core::constants::EVENT_HEAL_TYPE_RANGED
    );
    assert_eq!(creep_parts(&f, victim).0, 58);
    // 射程 4 拒（Chebyshev）
    let far = f.creep("far", "p1", 14, 10, &[BodyKind::Heal], 0);
    assert!(f.settle(ranged_heal_intent(far, victim)).is_empty());
    assert_eq!(creep_parts(&f, victim).0, 58, "射程外不治");
}

#[test]
fn equal_damage_and_heal_same_tick_survives() {
    // COMBAT-SIMULT-002 引擎侧：tick.js:120-136 伤害先施加（可瞬时为负）、
    // 治疗后回补、死亡判定在两者之后——同 tick 治疗量 ≥ 伤害超出量即存活
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    let enemy = f.creep("atk", "p2", 10, 11, &[BodyKind::Attack; 4], 0);
    let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    let mut batch = attack_intent_by("p2", enemy, victim);
    batch.add_object_intent("p1", medic, Action::Heal, IntentData::target(victim));
    let events = f.settle(batch);
    // 100 - 120 + 24 = 4：存活
    assert!(f.world.get(victim).is_some(), "等量抵消：存活");
    assert_eq!(creep_parts(&f, victim).0, 4);
    assert_eq!(creep_parts(&f, victim).1, vec![4]);
    assert_eq!(events.len(), 2, "攻击与治疗事件各一");
    // 对照：去掉治疗即死（同伤害通道）
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    let v2 = f2.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    let e2 = f2.creep("atk", "p2", 10, 11, &[BodyKind::Attack; 4], 0);
    f2.settle(attack_intent_by("p2", e2, v2));
    assert!(f2.world.get(v2).is_none(), "无治疗：死亡");
}

#[test]
fn heal_clamps_at_hits_max() {
    // tick.js:130-132：治疗后钳 hitsMax（=body 部件数×100）；满血再治无变化
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Tough], 0);
    f.world
        .edit_object(victim, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.hits = 190; // parts 隐含 [90,100]
                c.body[0].hits = 90;
            }
            Ok(())
        })
        .unwrap();
    let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    f.settle(heal_intent(medic, victim));
    assert_eq!(
        creep_parts(&f, victim),
        (200, vec![100, 100]),
        "钳到 hitsMax"
    );
    // 满血再治：无变化（tick.js:137 不重排），事件照发
    let events = f.settle(heal_intent(medic, victim));
    assert_eq!(events[0]["data"]["amount"], 24);
    assert_eq!(creep_parts(&f, victim), (200, vec![100, 100]));
}

#[test]
fn self_heal_allowed() {
    // heal.js 无自指拒（对照 attack.js:18）：施疗者=受疗者合法，heal/healed
    // 两个 actionLog 位落在同一 creep
    let mut f = Fixture::new(1, terrain_with(&[]));
    let solo = f.creep(
        "solo",
        "p1",
        10,
        10,
        &[BodyKind::Heal, BodyKind::Heal, BodyKind::Tough],
        0,
    );
    let enemy = f.creep("atk", "p2", 11, 10, &[BodyKind::Attack], 0);
    f.settle(attack_intent_by("p2", enemy, solo));
    assert_eq!(creep_parts(&f, solo).0, 270, "300-30");
    f.settle(heal_intent(solo, solo));
    assert_eq!(creep_parts(&f, solo).0, 294, "自愈 +24");
    let log = match f.obj(solo) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log,
        kk_engine::settle::creep::ACTION_HEAL as u16
            | kk_engine::settle::creep::ACTION_HEALED as u16
    );
}

#[test]
fn heal_suppresses_harvest() {
    // intents.js 优先级图：heal 位于链顶——压制 harvest
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let ally = f.creep("ally", "p1", 11, 11, &[BodyKind::Tough], 0);
    let hybrid = f.creep(
        "hy",
        "p1",
        10,
        10,
        &[BodyKind::Heal, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", hybrid, Action::Heal, IntentData::target(ally));
    b.add_object_intent("p1", hybrid, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert!(
        events
            .iter()
            .all(|e| e["event"] != kk_core::constants::EVENT_HARVEST),
        "harvest 被压制"
    );
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_HEAL,
        "heal 照常执行"
    );
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "source 未被动"),
        _ => panic!(),
    }
}

#[test]
fn zero_power_heal_still_emits_event() {
    // heal.js:34 无条件发事件（引擎层无部件预检——API 层的事）：无 HEAL 部件
    // 的 heal intent 仍产 actionLog 与 0 治疗量事件
    let mut f = Fixture::new(1, terrain_with(&[]));
    let quack = f.creep("quack", "p1", 10, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let ally = f.creep("ally", "p1", 11, 10, &[BodyKind::Tough], 0);
    let events = f.settle(heal_intent(quack, ally));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["amount"], 0);
    assert_eq!(creep_parts(&f, ally).0, 100, "0 治疗不进结算路径");
    let (log_a, log_b) = match (f.obj(quack), f.obj(ally)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_HEAL as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_HEALED as u16);
}
