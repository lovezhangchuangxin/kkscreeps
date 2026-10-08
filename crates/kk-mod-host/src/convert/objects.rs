//! 按领域解析注册声明；只写本次包的贡献候选。
use super::*;

impl ContributionBuilder<'_> {
    pub(super) fn add_object(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let key = own_key(namespace, as_str(namespace, value, "key")?)?;
        let type_key_label = key.to_string();
        let type_name = key
            .as_str()
            .split_once(':')
            .expect("Key 含分隔符")
            .1
            .to_string();
        let mut fields = Vec::new();
        let Some(field_values) = value.get("fields").and_then(Value::as_array) else {
            return err(namespace, format!("类型 {key} 缺少字段列表"));
        };
        for field in field_values {
            let name = as_str(namespace, field, "name")?;
            let descriptor = field_descriptor(namespace, name, field, |name| {
                format!("{type_name}.field.{name}")
            })?;
            self.contribution
                .bindings
                .insert(descriptor.binding.clone());
            fields.push(descriptor);
        }
        let named_key = |name: &str| own_key(namespace, &format!("{namespace}:{name}"));
        let owner_field = match optional_str(value, "ownerField") {
            Some(name) => Some(named_key(name)?),
            None => None,
        };
        let position_fields = match value.get("positionFields").and_then(Value::as_array) {
            Some(pair) if pair.len() == 2 => {
                let mut names = Vec::new();
                for name in pair {
                    let Some(name) = name.as_str() else {
                        return err(namespace, format!("类型 {key} 的位置字段名须为字符串"));
                    };
                    names.push(named_key(name)?);
                }
                Some([names[0].clone(), names[1].clone()])
            }
            Some(_) => {
                return err(
                    namespace,
                    format!("类型 {key} 的 positionFields 须为两个名字"),
                );
            }
            None => None,
        };
        let mut lifecycle_slots = BTreeSet::new();
        if let Some(tick) = value.get("tick").filter(|tick| !tick.is_null()) {
            let slot = own_key(namespace, as_str(namespace, tick, "slot")?)?;
            let process = own_key(namespace, as_str(namespace, tick, "process")?)?;
            if slot.as_str() != format!("{key}.tick")
                || process.as_str() != format!("{key}.tick.process")
            {
                return err(namespace, "对象 tick 的槽与绑定须从类型键派生");
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
        let store_capacity = match value.get("capacity") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                ModHostError::new(
                    namespace.to_string(),
                    format!("类型 {key} 的 capacity 须为非负整数"),
                )
            })?),
        };
        // 耐久声明键 hitsMax 与建筑声明同名（G09.b）；界值在冻结层
        // 统一把关（1..=u32::MAX）。
        let hits_max = match value.get("hitsMax") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64().ok_or_else(|| {
                ModHostError::new(
                    namespace.to_string(),
                    format!("类型 {key} 的 hitsMax 须为非负整数"),
                )
            })?),
        };
        self.contribution.object_types.push(ObjectTypeDescriptor {
            key,
            schema_version: as_u32(namespace, value, "version")?,
            owner_field,
            position_fields,
            fields,
            capabilities: Vec::new(),
            lifecycle_slots,
            visibility: object_visibility(namespace, optional_str(value, "visibility"))?,
            store_capacity,
            hits_max,
            player_class: {
                match optional_str(value, "playerClass") {
                    Some(name) if !name.is_empty() => name.to_string(),
                    _ => {
                        return err(
                            namespace,
                            format!(
                                "类型 {type_key_label} 缺少 playerClass（冻结校验要求非空玩家类名；\
                                 纯服务端类型也须显式命名）"
                            ),
                        );
                    }
                }
            },
        });
        Ok(())
    }

    pub(super) fn add_mount(&mut self, value: &Value) -> ModHostResult<()> {
        let namespace = self.namespace;
        let target = value
            .get("target")
            .ok_or_else(|| ModHostError::new(namespace.to_string(), "挂载缺少目标类型"))?;
        let name = as_str(
            namespace,
            value
                .get("field")
                .ok_or_else(|| ModHostError::new(namespace.to_string(), "挂载缺少字段"))?,
            "name",
        )?;
        let field = field_descriptor(
            namespace,
            name,
            value.get("field").expect("上面已判存在"),
            |name| format!("mount.{name}"),
        )?;
        self.contribution.bindings.insert(field.binding.clone());
        self.contribution.field_mounts.push(FieldMountDescriptor {
            target: TypeRef {
                key: any_key(namespace, as_str(namespace, target, "key")?)?,
                schema_version: as_u32(namespace, target, "version")?,
            },
            field,
        });
        Ok(())
    }
}
