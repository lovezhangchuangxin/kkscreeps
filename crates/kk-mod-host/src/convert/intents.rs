//! 按领域解析注册声明；只写本次包的贡献候选。
use super::*;

impl ContributionBuilder<'_> {
    pub(super) fn add_intent(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        let scope = match as_str(namespace, value, "scope")? {
            "object" => IntentScope::Object,
            "room" => IntentScope::Room,
            "player" => IntentScope::Player,
            "world" => IntentScope::World,
            other => return err(namespace, format!("未知动作作用域 {other:?}")),
        };
        let receiver = match value.get("target") {
            Some(target) => Some(TypeRef {
                key: any_key(namespace, as_str(namespace, target, "key")?)?,
                schema_version: as_u32(namespace, target, "version")?,
            }),
            None => None,
        };
        let merge = match optional_str(value, "mode").unwrap_or("last") {
            "last" => Merge::Last,
            "queue" => Merge::Queue {
                max_len: optional_u32(namespace, value, "queueMax")?
                    .unwrap_or(8)
                    .max(1),
            },
            other => return err(namespace, format!("未知合并模式 {other:?}")),
        };
        let process_binding = own_key(namespace, &format!("{}.process", key))?;
        let slot_key = match optional_str(value, "slot") {
            Some(slot) => any_key(namespace, slot)?,
            None => {
                let auto = own_key(namespace, &format!("{}.slot", key))?;
                self.contribution.slots.push(SlotDescriptor {
                    key: auto.clone(),
                    stage: stage_of_scope(&scope),
                    native_binding: process_binding.clone(),
                });
                auto
            }
        };
        self.required_executables.insert(process_binding.clone());
        self.contribution.bindings.insert(process_binding);
        // 压制者键数组（可跨命名空间，语义校验在冻结期：同 scope
        // 同接收者、未知压制动作拒）。
        let mut suppressed_by = BTreeSet::new();
        // null 与缺省同判（与 queueMax/mode 对 null 的容忍一致）；
        // 空数组才是显式空形态。
        if let Some(list) = value.get("suppressedBy").filter(|v| !v.is_null()) {
            let Some(list) = list.as_array() else {
                return err(
                    namespace,
                    format!("动作 {key} 的 suppressedBy 须为字符串数组"),
                );
            };
            for entry in list {
                let Some(entry) = entry.as_str() else {
                    return err(namespace, format!("动作 {key} 的压制动作键须为字符串"));
                };
                suppressed_by.insert(any_key(namespace, entry)?);
            }
        }
        self.contribution.intents.push(IntentDescriptor {
            key,
            scope,
            receiver,
            arguments: schema(
                namespace,
                value.get("args").ok_or_else(|| {
                    ModHostError::new(namespace.to_string(), "动作缺少参数 schema")
                })?,
            )?,
            conversion: ArgumentConversion::Strict,
            authority: IntentAuthority::Player,
            merge,
            slot: slot_key,
            suppressed_by,
        });
        Ok(())
    }
}

/// 动作自动入口的阶段映射。object/room 域动作与对象动作同段处理；
/// player/world 域动作现阶段统一归入全局段——更细的阶段划分待
/// 这些作用域出现真实包时再开放（登记为装载器残留）。
fn stage_of_scope(scope: &IntentScope) -> Stage {
    match scope {
        IntentScope::Object | IntentScope::Room => Stage::ObjectAction,
        IntentScope::Player | IntentScope::World => Stage::Market,
    }
}
