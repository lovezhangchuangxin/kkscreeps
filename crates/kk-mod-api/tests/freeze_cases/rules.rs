use super::*;

#[test]
fn one_override_wins_but_multiple_require_explicit_selection() {
    let a = override_rule("alpha", Number::Signed(900));
    let b = override_rule("beta", Number::Signed(1000));
    let single = plan(vec![builtin::link(), a.clone()]).unwrap();
    let handle = single
        .rules()
        .resolve(&key("builtin:link.capacity"))
        .unwrap();
    assert_eq!(single.rules().get(handle), Some(&Number::Signed(900)));
    let error = plan(vec![builtin::link(), a.clone(), b.clone()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("alpha, beta"));
    let options = FreezeOptions {
        rule_sources: BTreeMap::from([(key("builtin:link.capacity"), "beta".into())]),
        ..Default::default()
    };
    let selected = freeze(vec![b, builtin::link(), a], options).unwrap();
    let handle = selected
        .rules()
        .resolve(&key("builtin:link.capacity"))
        .unwrap();
    assert_eq!(selected.rules().get(handle), Some(&Number::Signed(1000)));
}

#[test]
fn selected_override_does_not_hide_an_invalid_contribution() {
    let options = FreezeOptions {
        rule_sources: BTreeMap::from([(key("builtin:link.capacity"), "beta".into())]),
        ..Default::default()
    };
    let error = freeze(
        vec![
            builtin::link(),
            override_rule("alpha", Number::Signed(-1)),
            override_rule("beta", Number::Signed(900)),
        ],
        options,
    )
    .unwrap_err();
    assert!(error.to_string().contains("alpha / builtin:link.capacity"));
}

#[test]
fn unknown_duplicate_wrong_type_and_stale_override_selection_are_rejected() {
    let mut a = override_rule("alpha", Number::Signed(900));
    a.overrides[0].rule = key("builtin:missing");
    assert!(
        plan(vec![builtin::link(), a])
            .unwrap_err()
            .message
            .contains("未知规则")
    );
    let mut a = override_rule("alpha", Number::Signed(900));
    a.overrides.push(a.overrides[0].clone());
    assert!(
        plan(vec![builtin::link(), a])
            .unwrap_err()
            .message
            .contains("重复覆盖")
    );
    assert!(
        plan(vec![
            builtin::link(),
            override_rule("alpha", Number::Unsigned(900))
        ])
        .unwrap_err()
        .message
        .contains("类型不匹配")
    );
    let options = FreezeOptions {
        rule_sources: BTreeMap::from([(key("builtin:link.capacity"), "absent".into())]),
        ..Default::default()
    };
    assert!(
        freeze(vec![builtin::link()], options)
            .unwrap_err()
            .message
            .contains("未覆盖")
    );
}

#[test]
fn numeric_bounds_finite_values_and_cross_rule_constraints_are_checked() {
    for (cost, valid) in [
        (i64::from(u32::MAX), true),
        (i64::from(u32::MAX) + 1, false),
    ] {
        let mut c = builtin::link();
        c.rules
            .iter_mut()
            .find(|r| r.key == key("builtin:link.buildCost"))
            .unwrap()
            .default = Number::Signed(cost);
        assert_eq!(plan(vec![c]).is_ok(), valid, "工地进度总量须能表示建造成本");
    }
    for value in [
        Number::Signed(-1),
        Number::Signed(i64::MAX),
        Number::Float(f64::NAN),
    ] {
        assert!(plan(vec![builtin::link(), override_rule("alpha", value)]).is_err());
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1, 1.1] {
        let mut c = builtin::link();
        c.rules
            .iter_mut()
            .find(|r| r.key == key("builtin:link.lossRatio"))
            .unwrap()
            .default = Number::Float(value);
        error(c, "规则值");
    }
    let mut c = builtin::link();
    c.rules[0].domain = NumberDomain::Signed { min: 2, max: 1 };
    error(c, "数值范围");
    let mut c = builtin::link();
    c.rules
        .iter_mut()
        .find(|r| r.key == key("builtin:link.hitsMax"))
        .unwrap()
        .default = Number::Signed(999);
    error(c, "规则约束不满足");
    let mut c = builtin::link();
    c.constraints.insert(RuleConstraint::LessOrEqual {
        left: key("builtin:absent"),
        right: key("builtin:link.hitsMax"),
    });
    error(c, "约束引用未知规则");
}
