//! 按领域解析注册声明；只写本次包的贡献候选。
use super::*;

impl ContributionBuilder<'_> {
    pub(super) fn add_resource(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        // SDK 面已拦截未知选项；非对象载荷在 as_str 处先行失败
        //（缺少 key 字段），这里拒绝绕过 SDK 直接 declare 的额外成员，
        // 防止静默丢弃作者意图（与装载器的 fail-loud 立场一致）。
        if let Some(unknown) = value
            .as_object()
            .and_then(|object| object.keys().find(|member| member.as_str() != "key"))
        {
            return err(namespace, format!("资源 {key} 声明了未知选项 {unknown}"));
        }
        self.contribution
            .resources
            .push(kk_mod_api::resources::ResourceDescriptor { key });
        Ok(())
    }

    pub(super) fn add_building(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        // 拒绝未知成员：拼错的选项静默落空等于改写建筑语义
        //（与资源声明的 fail-loud 立场一致）。
        for member in value.as_object().into_iter().flat_map(|o| o.keys()) {
            if !matches!(
                member.as_str(),
                "key"
                    | "playerClass"
                    | "cost"
                    | "hitsMax"
                    | "pathing"
                    | "capacity"
                    | "production"
                    | "fields"
                    | "tick"
            ) {
                return err(namespace, format!("建筑 {key} 声明了未知选项 {member}"));
            }
        }
        let player_class = match optional_str(value, "playerClass") {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => {
                return err(
                    namespace,
                    format!("建筑 {key} 缺少 playerClass（玩家类名须显式命名）"),
                );
            }
        };
        let cost = resource_amounts(
            namespace,
            &key,
            value.get("cost").ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("建筑 {key} 缺少成本表"))
            })?,
            "成本",
        )?;
        let hits_max = as_u64(namespace, value, "hitsMax").map_err(|_| {
            ModHostError::new(
                namespace.to_string(),
                format!("建筑 {key} 的 hitsMax 须为非负整数"),
            )
        })?;
        let pathing = match as_str(namespace, value, "pathing")? {
            "obstacle" => kk_mod_api::buildings::BuildingPathing::Obstacle,
            "plain" => kk_mod_api::buildings::BuildingPathing::Plain,
            "road" => kk_mod_api::buildings::BuildingPathing::Road,
            other => {
                return err(
                    namespace,
                    format!("建筑 {key} 的寻路语义 {other:?} 未知（obstacle/plain/road）"),
                );
            }
        };
        let store_capacity = match value.get("capacity") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                ModHostError::new(
                    namespace.to_string(),
                    format!("建筑 {key} 的 capacity 须为非负整数"),
                )
            })?),
        };
        let production = match value.get("production") {
            None | Some(Value::Null) => None,
            Some(spec) => {
                for member in spec.as_object().into_iter().flat_map(|o| o.keys()) {
                    if !matches!(member.as_str(), "interval" | "inputs" | "outputs") {
                        return err(
                            namespace,
                            format!("建筑 {key} 的生产配方声明了未知选项 {member}"),
                        );
                    }
                }
                let interval = as_u32(namespace, spec, "interval")?;
                let inputs = spec
                    .get("inputs")
                    .map(|v| resource_amounts(namespace, &key, v, "生产输入"))
                    .transpose()?
                    .unwrap_or_default();
                let outputs = spec
                    .get("outputs")
                    .map(|v| resource_amounts(namespace, &key, v, "生产输出"))
                    .transpose()?
                    .unwrap_or_default();
                Some(kk_mod_api::buildings::ProductionSpec {
                    interval,
                    inputs,
                    outputs,
                })
            }
        };
        let type_name = key
            .as_str()
            .split_once(':')
            .expect("Key 含分隔符")
            .1
            .to_string();
        let mut building_fields = Vec::new();
        if let Some(list) = value.get("fields").and_then(Value::as_array) {
            for field in list {
                let name = as_str(namespace, field, "name")?;
                if field
                    .get("visibility")
                    .and_then(Value::as_str)
                    .is_some_and(|visibility| visibility != "public")
                {
                    return err(
                        namespace,
                        format!(
                            "建筑 {key} 的字段 {name} 可见性首片只支持 public（记录直供；owner/server-only 裁切未开放）"
                        ),
                    );
                }
                let descriptor = field_descriptor(namespace, name, field, |name| {
                    format!("{type_name}.field.{name}")
                })?;
                self.contribution
                    .bindings
                    .insert(descriptor.binding.clone());
                building_fields.push(descriptor);
            }
        }
        let mut lifecycle_slots = std::collections::BTreeSet::new();
        if let Some(tick) = value.get("tick").filter(|tick| !tick.is_null()) {
            let slot = own_key(namespace, as_str(namespace, tick, "slot")?)?;
            let process = own_key(namespace, as_str(namespace, tick, "process")?)?;
            if slot.as_str() != format!("{key}.tick")
                || process.as_str() != format!("{key}.tick.process")
            {
                return err(namespace, "建筑 tick 的槽与绑定须从建筑键派生");
            }
            self.contribution.bindings.insert(process.clone());
            self.required_executables.insert(process.clone());
            lifecycle_slots.insert(slot.clone());
            self.contribution.slots.push(SlotDescriptor {
                key: slot,
                stage: Stage::ObjectTick,
                native_binding: process,
            });
        }
        self.contribution
            .buildings
            .push(kk_mod_api::buildings::BuildingDescriptor {
                key,
                player_class,
                cost,
                hits_max,
                pathing,
                store_capacity,
                production,
                fields: building_fields,
                lifecycle_slots,
            });
        Ok(())
    }
}

/// 资源名 -> 数量的声明表：非空字符串键、非负整数、键内不得重复；
/// 数值域（1..=1e9、正容量等）在冻结期定论，这里拦明显畸形。
fn resource_amounts(
    namespace: &str,
    key: &Key,
    value: &Value,
    what: &str,
) -> ModHostResult<Vec<(String, u64)>> {
    let object = value.as_object().ok_or_else(|| {
        ModHostError::new(
            namespace.to_string(),
            format!("建筑 {key} 的{what}须为资源名到数量的对象"),
        )
    })?;
    let mut entries = Vec::with_capacity(object.len());
    for (resource, amount) in object {
        if resource.is_empty() || resource.contains(':') && Key::new(resource).is_err() {
            return err(
                namespace,
                format!("建筑 {key} 的{what}资源名 {resource:?} 非法"),
            );
        }
        let amount = amount.as_u64().ok_or_else(|| {
            ModHostError::new(
                namespace.to_string(),
                format!("建筑 {key} 的{what}资源 {resource:?} 数量须为非负整数"),
            )
        })?;
        entries.push((resource.clone(), amount));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let duplicates = entries.windows(2).any(|pair| pair[0].0 == pair[1].0);
    if duplicates {
        return err(namespace, format!("建筑 {key} 的{what}存在重复资源名"));
    }
    Ok(entries)
}
