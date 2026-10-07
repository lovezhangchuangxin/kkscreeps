//! 按领域解析注册声明；只写本次包的贡献候选。
use super::*;

impl ContributionBuilder<'_> {
    pub(super) fn add_handler(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        let slot = any_key(namespace, as_str(namespace, value, "slot")?)?;
        let kind = match as_str(namespace, value, "kind")? {
            "before" => HandlerKind::Before,
            "wrap" => HandlerKind::Wrap,
            "replace" => HandlerKind::Replace,
            "after" => HandlerKind::After,
            other => return err(namespace, format!("未知处理器种类 {other:?}")),
        };
        let binding = key.clone();
        self.contribution.bindings.insert(binding.clone());
        self.required_executables.insert(binding.clone());
        self.contribution.handlers.push(HandlerDescriptor {
            key,
            slot,
            kind,
            binding,
            depends_on: handler_dependencies(namespace, value)?,
        });
        Ok(())
    }
}

/// 依赖输入必须显式为稳定键列表；不能把 null/字符串等形状静默降为空集。
fn handler_dependencies(namespace: &str, value: &Value) -> ModHostResult<BTreeSet<Key>> {
    let options = match value.get("options") {
        Some(options) => {
            let object = options
                .as_object()
                .ok_or_else(|| ModHostError::new(namespace, "处理器 options 须为对象"))?;
            if let Some(key) = object
                .keys()
                .find(|key| !matches!(key.as_str(), "id" | "dependsOn"))
            {
                return err(
                    namespace,
                    format!("未知处理器选项 {key}（依赖请使用 dependsOn）"),
                );
            }
            options
        }
        None => value,
    };
    let Some(dependencies) = options.get("dependsOn") else {
        return Ok(BTreeSet::new());
    };
    let dependencies = dependencies
        .as_array()
        .ok_or_else(|| ModHostError::new(namespace, "处理器 dependsOn 须为稳定键数组"))?;
    dependencies
        .iter()
        .map(|dependency| {
            let dependency = dependency.as_str().ok_or_else(|| {
                ModHostError::new(namespace, "处理器 dependsOn 成员须为稳定键字符串")
            })?;
            any_key(namespace, dependency)
        })
        .collect()
}
