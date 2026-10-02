use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::*;
use serde_json::json;

fn key(value: &str) -> Key {
    Key::new(value).unwrap()
}
fn plan(contributions: Vec<Contribution>) -> Result<ContributionPlan, FreezeError> {
    freeze(contributions, FreezeOptions::default())
}
fn module(namespace: &str) -> Contribution {
    Contribution {
        source: Source {
            namespace: namespace.into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [1; 32],
            config_digest: [2; 32],
            player_api_digest: [3; 32],
        },
        bindings: BTreeSet::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
    }
}
fn override_rule(namespace: &str, value: Number) -> Contribution {
    let mut c = module(namespace);
    c.overrides.push(RuleOverride {
        rule: key("builtin:link.capacity"),
        value,
    });
    c
}
fn handler(c: &mut Contribution, name: &str, kind: HandlerKind, dependencies: &[&str]) {
    let key = key(&format!("{}:{name}", c.source.namespace));
    c.bindings.insert(key.clone());
    c.handlers.push(HandlerDescriptor {
        key: key.clone(),
        slot: self::key("builtin:link.transfer"),
        kind,
        binding: key,
        depends_on: dependencies.iter().map(|v| self::key(v)).collect(),
    });
}
fn error(c: Contribution, expected: &str) {
    let error = plan(vec![c]).unwrap_err().to_string();
    assert!(error.contains(expected), "{error}");
}

#[path = "freeze_cases/catalog.rs"]
mod catalog;
#[path = "freeze_cases/handlers.rs"]
mod handlers;
#[path = "freeze_cases/projection.rs"]
mod projection;
#[path = "freeze_cases/rules.rs"]
mod rules;

#[test]
fn link_rules_extract_builtin_defaults() {
    let frozen = plan(vec![builtin::link()]).unwrap();
    let rules = builtin::LinkRules::from_plan(frozen.rules()).unwrap();
    assert_eq!(rules.hits, kk_core::constants::LINK_HITS);
    assert_eq!(rules.hits_max, kk_core::constants::LINK_HITS_MAX);
    assert_eq!(rules.capacity, kk_core::constants::LINK_CAPACITY);
    assert_eq!(rules.cooldown, kk_core::constants::LINK_COOLDOWN);
    assert_eq!(rules.loss_ratio, kk_core::constants::LINK_LOSS_RATIO);
    assert_eq!(rules.build_cost, 5000);
    let quotas: serde_json::Value =
        serde_json::from_str(kk_core::constants::CONTROLLER_STRUCTURES).unwrap();
    for level in 0..=8 {
        assert_eq!(
            rules.rcl[level],
            quotas["link"][level.to_string()].as_i64().unwrap_or(0),
            "RCL {level} 配额应与官方表一致"
        );
    }
}

#[test]
fn link_rules_follow_override_and_reject_missing_plan() {
    let mut contrib = override_rule("mods", Number::Signed(400));
    contrib.overrides.push(RuleOverride {
        rule: key("builtin:link.lossRatio"),
        value: Number::Float(0.5),
    });
    let frozen = plan(vec![builtin::link(), contrib]).unwrap();
    let rules = builtin::LinkRules::from_plan(frozen.rules()).unwrap();
    assert_eq!(rules.capacity, 400);
    assert_eq!(rules.loss_ratio, 0.5);
    // 两个计划的指纹不同，规则快照互不可换。
    let plain = plan(vec![builtin::link()]).unwrap();
    assert_ne!(plain.hash(), frozen.hash());

    let empty = plan(vec![module("none")]).unwrap();
    let error = builtin::LinkRules::from_plan(empty.rules()).unwrap_err();
    assert!(error.contains("builtin:link.lossRatio"), "{error}");
}
