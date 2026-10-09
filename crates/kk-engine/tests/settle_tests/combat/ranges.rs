//! 战斗射程规则（builtin:creep.attack/rangedAttack/heal/rangedHeal.range）
//! 贯穿结算距离判定：覆盖放宽后命中、默认与官方硬编码口径一致、
//! 零射程拒绝，以及 rangedMassAttack 不受单目标射程规则影响。

use super::*;

/// 冻结 builtin + 只带规则覆盖的贡献，返回解析后的战斗射程快照。
/// 覆盖键不带 builtin: 前缀（与 contrib_link 的 overrides_mod 同形）。
fn combat_of(rules: &[(&str, i64)]) -> kk_mod_api::builtin::CombatRanges {
    let mut c = kk_mod_api::Contribution {
        source: kk_mod_api::Source {
            namespace: "testmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: rules
            .iter()
            .map(|(rule, value)| kk_mod_api::RuleOverride {
                rule: kk_mod_api::Key::new(format!("builtin:{rule}")).unwrap(),
                value: kk_mod_api::Number::Signed(*value),
            })
            .collect(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    };
    c.overrides.sort_by_key(|o| o.rule.clone());
    let plan = kk_mod_api::freeze_with_builtin(
        kk_mod_api::builtin::link(),
        vec![c],
        kk_mod_api::FreezeOptions::default(),
    )
    .unwrap();
    kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap()
}

/// 以覆盖射程构造贡献平面；其余域保持内置默认（与 Fixture::settle_with 同基线）。
fn combat_plane(combat: kk_mod_api::builtin::CombatRanges) -> kk_engine::settle::Plane {
    kk_engine::settle::Plane {
        buildings: Default::default(),
        links: kk_world::place::builtin_link_rules(),
        stores: kk_world::place::builtin_store_rules(),
        combat,
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        static_passability: Default::default(),
        resources: Default::default(),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan: kk_engine::settle::builtin_plan(),
    }
}

#[test]
fn attack_range_override_extends_melee_reach() {
    // 默认射程 1：距离 2 不命中；规则放宽到 3 后同距离同意图命中。
    // 一致性锚点：默认行为与官方 attack.js 的硬编码 1 相同。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    let victim = f.creep("v", "p2", 12, 10, &[BodyKind::Move], 0);
    f.settle(attack_intent_by("p1", atk, victim));
    assert_eq!(creep_parts(&f, victim).0, 100, "默认射程 1：距离 2 拒");
    f.settle_with_plane(
        attack_intent_by("p1", atk, victim),
        combat_plane(combat_of(&[("creep.attack.range", 3)])),
    );
    assert_eq!(creep_parts(&f, victim).0, 70, "放宽到 3：距离 2 命中 30");
}

#[test]
fn zero_ranged_attack_range_rejects_adjacent_target() {
    // rangedAttack.range=0：两对象不同格，一切目标都在射程外。
    // 默认臂对照：同距离命中 10。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("ra", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let victim = f.creep("v", "p2", 10, 11, &[BodyKind::Move], 0);
    f.settle(super::ranged::ranged_attack_intent("p1", atk, victim));
    assert_eq!(creep_parts(&f, victim).0, 90, "默认射程 3：距离 1 命中");

    let mut f2 = Fixture::new(2, terrain_with(&[]));
    let atk2 = f2.creep("ra", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let victim2 = f2.creep("v", "p2", 10, 11, &[BodyKind::Move], 0);
    f2.settle_with_plane(
        super::ranged::ranged_attack_intent("p1", atk2, victim2),
        combat_plane(combat_of(&[("creep.rangedAttack.range", 0)])),
    );
    assert_eq!(creep_parts(&f2, victim2).0, 100, "零射程：邻格拒绝");
}

#[test]
fn heal_and_ranged_heal_ranges_override_independently() {
    // heal.range=3 使距离 2 的近战治疗生效；rangedHeal.range 独立放宽才
    // 覆盖距离 4——两条规则互不牵连。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    f.world
        .edit_object(victim, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.hits = 50;
            }
            Ok(())
        })
        .unwrap();
    let melee_medic = f.creep("m1", "p1", 12, 10, &[BodyKind::Heal], 0);
    // 近战治疗距离 2：默认射程 1 拒，放宽到 3 后 +12。
    f.settle_with_plane(
        heal_intent(melee_medic, victim),
        combat_plane(combat_of(&[("creep.heal.range", 3)])),
    );
    assert_eq!(
        creep_parts(&f, victim).0,
        62,
        "heal.range=3：距离 2 治疗 12"
    );

    let far_medic = f.creep("m2", "p1", 14, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    // 远程治疗距离 4：只放宽 heal.range 时仍按默认 3 拒。
    f.settle_with_plane(
        ranged_heal_intent(far_medic, victim),
        combat_plane(combat_of(&[("creep.heal.range", 3)])),
    );
    assert_eq!(
        creep_parts(&f, victim).0,
        62,
        "rangedHeal 未放宽：距离 4 拒"
    );
    // rangedHeal.range=5 后同距离 +8（2×HEAL×4）。
    f.settle_with_plane(
        ranged_heal_intent(far_medic, victim),
        combat_plane(combat_of(&[("creep.rangedHeal.range", 5)])),
    );
    assert_eq!(
        creep_parts(&f, victim).0,
        70,
        "rangedHeal.range=5：距离 4 治疗 8"
    );
}

#[test]
fn range_rules_use_chebyshev_metric_on_diagonal_offsets() {
    // (10,10)→(11,12)：切比雪夫 2、曼哈顿 3。heal.range=2 须放行——
    // 曼哈顿口径会误拒；覆盖射程 + 对角偏移钉死度量语义（本文件其余
    // 用例为轴向偏移，自身区分不了两种度量）。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 11, 12, &[BodyKind::Tough], 0);
    f.world
        .edit_object(victim, |object| {
            if let Some(RoomObject::Creep(c)) = Some(object) {
                c.hits = 50;
            }
            Ok(())
        })
        .unwrap();
    let medic = f.creep("m", "p1", 10, 10, &[BodyKind::Heal], 0);
    f.settle_with_plane(
        heal_intent(medic, victim),
        combat_plane(combat_of(&[("creep.heal.range", 2)])),
    );
    assert_eq!(
        creep_parts(&f, victim).0,
        62,
        "切比雪夫 2 ≤ 射程 2：对角治疗放行"
    );
}

#[test]
fn ranged_mass_attack_keeps_native_area_semantics() {
    // rangedMassAttack 是三格面积语义（距离倍率 1/0.4/0.1），首批不随
    // 单目标射程规则放宽：覆盖 rangedAttack.range=1 后距离 2 的面积目标
    // 仍按原语义命中 0.4 档。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::RangedAttack], 0);
    let victim = f.creep("v", "p2", 12, 10, &[BodyKind::Move], 0);
    f.settle_with_plane(
        super::ranged::ranged_mass_intent("p1", atk),
        combat_plane(combat_of(&[("creep.rangedAttack.range", 1)])),
    );
    // 10×0.4=4：面积档不受单目标射程覆盖影响。
    assert_eq!(creep_parts(&f, victim).0, 96);
}
