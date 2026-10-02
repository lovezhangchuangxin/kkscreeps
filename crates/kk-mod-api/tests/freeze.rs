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
