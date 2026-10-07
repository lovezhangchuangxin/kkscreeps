//! 按领域解析注册声明；只写本次包的贡献候选。
use super::*;

impl ContributionBuilder<'_> {
    pub(super) fn add_rule(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        let kind = as_str(namespace, value, "type")?;
        if !matches!(kind, "i32" | "i64" | "u32" | "u64" | "f64") {
            return err(namespace, format!("规则 {key} 的类型 {kind:?} 未知"));
        }
        let Some(consumers) = value.get("consumers").and_then(Value::as_array) else {
            return err(namespace, format!("规则 {key} 缺少消费者列表"));
        };
        let mut consumer_keys = BTreeSet::new();
        for consumer in consumers {
            let Some(consumer) = consumer.as_str() else {
                return err(namespace, "规则消费者须为绑定键字符串");
            };
            consumer_keys.insert(any_key(namespace, consumer)?);
        }
        self.contribution.rules.push(RuleDescriptor {
            key: key.clone(),
            domain: number_domain(namespace, kind, value)?,
            default: number_of(
                namespace,
                kind,
                value.get("default").ok_or_else(|| {
                    ModHostError::new(namespace.to_string(), format!("规则 {key} 缺少默认值"))
                })?,
            )?,
            consumers: consumer_keys,
        });
        Ok(())
    }

    pub(super) fn add_override(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        self.contribution.overrides.push(RuleOverride {
            rule: any_key(namespace, as_str(namespace, value, "rule")?)?,
            value: match value.get("value").and_then(Value::as_i64) {
                Some(integral) => Number::Signed(integral),
                None => {
                    Number::Float(value.get("value").and_then(Value::as_f64).ok_or_else(|| {
                        ModHostError::new(namespace.to_string(), "规则覆盖值须为数")
                    })?)
                }
            },
        });
        Ok(())
    }
}
