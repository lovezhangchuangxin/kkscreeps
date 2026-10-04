use super::*;

#[test]
fn store_defaults_match_player_constants_and_overrides_are_frozen() {
    let original = plan(vec![builtin::link()]).unwrap();
    let values = builtin::StoreRules::from_plan(original.rules())
        .unwrap()
        .constant_overrides();
    let dump: serde_json::Value =
        serde_json::from_str(include_str!("../../../kk-runner/assets/constants.json")).unwrap();
    assert_eq!(values.len(), 10);
    for (name, value) in values {
        assert_eq!(value, dump[&name], "{name}");
    }

    for value in [Number::Signed(0), Number::Signed(i64::from(u32::MAX))] {
        let mut c = module("alpha");
        c.overrides.push(RuleOverride {
            rule: key("builtin:store.lab.mineralCapacity"),
            value: value.clone(),
        });
        let frozen = plan(vec![builtin::link(), c]).unwrap();
        assert_ne!(original.hash(), frozen.hash());
        let rules = builtin::StoreRules::from_plan(frozen.rules()).unwrap();
        assert_eq!(Number::Signed(rules.lab_mineral), value);
    }
    let empty = plan(vec![module("none")]).unwrap();
    assert!(builtin::StoreRules::from_plan(empty.rules()).is_err());
}

#[test]
fn invalid_store_overrides_and_conflicting_sources_are_rejected() {
    for value in [
        Number::Signed(-1),
        Number::Signed(i64::from(u32::MAX) + 1),
        Number::Float(1.5),
    ] {
        let mut c = module("alpha");
        c.overrides.push(RuleOverride {
            rule: key("builtin:store.container.capacity"),
            value,
        });
        assert!(plan(vec![builtin::link(), c]).is_err());
    }
    let make = |name, value| {
        let mut c = module(name);
        c.overrides.push(RuleOverride {
            rule: key("builtin:store.container.capacity"),
            value: Number::Signed(value),
        });
        c
    };
    assert!(plan(vec![builtin::link(), make("alpha", 30), make("beta", 40)]).is_err());
}
