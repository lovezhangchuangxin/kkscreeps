use std::collections::{BTreeMap, BTreeSet};

use crate::{validate, *};

pub(crate) fn effective_rules(
    rules: &BTreeMap<Key, RuleDescriptor>,
    overrides: &BTreeMap<Key, BTreeMap<String, Number>>,
    options: &FreezeOptions,
    bindings: &BTreeSet<Key>,
) -> Result<Vec<Number>, FreezeError> {
    for key in overrides.keys() {
        if !rules.contains_key(key) {
            return Err(FreezeError::new(key.to_string(), "覆盖了未知规则"));
        }
    }
    for (key, ns) in &options.rule_sources {
        if !overrides.get(key).is_some_and(|o| o.contains_key(ns)) {
            return Err(FreezeError::new(
                key.to_string(),
                format!("所选来源 {ns} 未覆盖该规则"),
            ));
        }
    }
    let mut values = Vec::new();
    for rule in rules.values() {
        let context = rule.key.as_str();
        validate::domain(&rule.domain, context)?;
        validate::number(&rule.default, &rule.domain, context)?;
        if rule.consumers.is_empty() {
            return Err(FreezeError::new(context, "规则未声明消费者"));
        }
        for key in &rule.consumers {
            validate::binding(key, bindings, context)?;
        }
        let mut value = rule.default.clone();
        if let Some(candidates) = overrides.get(&rule.key) {
            for (ns, candidate) in candidates {
                validate::number(candidate, &rule.domain, &format!("{ns} / {context}"))?;
            }
            let selected = match options.rule_sources.get(&rule.key) {
                Some(ns) => &candidates[ns],
                None if candidates.len() == 1 => candidates.values().next().expect("非空覆盖集合"),
                None => {
                    return Err(FreezeError::new(
                        context,
                        format!(
                            "规则覆盖冲突：{}",
                            candidates.keys().cloned().collect::<Vec<_>>().join(", ")
                        ),
                    ));
                }
            };
            value = selected.clone();
        }
        values.push(value);
    }
    Ok(values)
}
