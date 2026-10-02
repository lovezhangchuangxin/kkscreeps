use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::*;

/// 在 Clone 和递归值校验前检查深度，避免先复制过深的调用方 schema。
pub(crate) fn schema_depth(
    schema: &ValueSchema,
    max_depth: u16,
    context: &str,
) -> Result<(), FreezeError> {
    let mut pending = vec![(schema, 1)];
    while let Some((schema, depth)) = pending.pop() {
        if depth > max_depth {
            return Err(FreezeError::new(context, "schema 嵌套超过限额"));
        }
        match &schema.kind {
            ValueKind::Array { element, .. } => pending.push((element, depth + 1)),
            ValueKind::Map { value, .. } => pending.push((value, depth + 1)),
            ValueKind::Record { fields, .. } => {
                pending.extend(fields.values().map(|v| (v, depth + 1)))
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn domain(domain: &NumberDomain, context: &str) -> Result<(), FreezeError> {
    let valid = match domain {
        NumberDomain::Signed { min, max } => min <= max,
        NumberDomain::Unsigned { min, max } => min <= max,
        NumberDomain::Float { min, max } => min.is_finite() && max.is_finite() && min <= max,
    };
    if !valid {
        return Err(FreezeError::new(context, "数值范围无效或不是有限值"));
    }
    Ok(())
}

pub(crate) fn number(
    value: &Number,
    domain: &NumberDomain,
    context: &str,
) -> Result<(), FreezeError> {
    let valid = match (value, domain) {
        (Number::Signed(n), NumberDomain::Signed { min, max }) => (min..=max).contains(&n),
        (Number::Unsigned(n), NumberDomain::Unsigned { min, max }) => (min..=max).contains(&n),
        (Number::Float(n), NumberDomain::Float { min, max }) => {
            n.is_finite() && (min..=max).contains(&n)
        }
        _ => false,
    };
    if !valid {
        return Err(FreezeError::new(
            context,
            "规则值类型不匹配、越界或不是有限值",
        ));
    }
    Ok(())
}

pub(crate) fn type_ref(
    reference: &TypeRef,
    types: &BTreeMap<Key, ObjectTypeDescriptor>,
    context: &str,
) -> Result<(), FreezeError> {
    let Some(ty) = types.get(&reference.key) else {
        return Err(FreezeError::new(
            context,
            format!("未知类型 {}", reference.key),
        ));
    };
    if reference.schema_version != ty.schema_version {
        return Err(FreezeError::new(
            context,
            format!("类型 {} 的 schema_version 不匹配", reference.key),
        ));
    }
    Ok(())
}

pub(crate) fn schema(
    value: &ValueSchema,
    types: &BTreeMap<Key, ObjectTypeDescriptor>,
    depth: u16,
    max_depth: u16,
    context: &str,
) -> Result<(), FreezeError> {
    if depth > max_depth {
        return Err(FreezeError::new(context, "schema 嵌套超过限额"));
    }
    match &value.kind {
        ValueKind::Number(d) => domain(d, context)?,
        ValueKind::ObjectRef(r) => type_ref(r, types, context)?,
        ValueKind::Enum(values)
            if values.is_empty() || values.iter().any(|v| v.is_empty() || v.len() > 128) =>
        {
            return Err(FreezeError::new(context, "枚举为空或成员长度无效"));
        }
        ValueKind::Array { element, .. } => schema(element, types, depth + 1, max_depth, context)?,
        ValueKind::Map { value, .. } => schema(value, types, depth + 1, max_depth, context)?,
        ValueKind::Record { fields, optional } => {
            if optional.iter().any(|name| !fields.contains_key(name)) {
                return Err(FreezeError::new(
                    context,
                    "Record 的 optional 引用了未知成员",
                ));
            }
            for (name, member) in fields {
                if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
                    return Err(FreezeError::new(context, "Record 成员名称无效"));
                }
                schema(member, types, depth + 1, max_depth, context)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 校验候选值满足字段 schema（类型、取值域、长度与元素限额、有限浮点）。
/// 供扩展字段的写入入口复用；context 用于错误定位。
pub fn check_value(value: &Value, schema: &ValueSchema, context: &str) -> Result<(), String> {
    default_value(value, schema, context).map_err(|e| format!("{e}: {}", e.message))
}

pub(crate) fn default_value(
    value: &Value,
    schema: &ValueSchema,
    context: &str,
) -> Result<(), FreezeError> {
    if value.is_null() && schema.nullable {
        return Ok(());
    }
    let valid = match (&schema.kind, value) {
        (ValueKind::Bool, Value::Bool(_)) => true,
        (ValueKind::Number(NumberDomain::Signed { min, max }), _) => {
            value.as_i64().is_some_and(|v| (*min..=*max).contains(&v))
        }
        (ValueKind::Number(NumberDomain::Unsigned { min, max }), _) => {
            value.as_u64().is_some_and(|v| (*min..=*max).contains(&v))
        }
        (ValueKind::Number(NumberDomain::Float { min, max }), _) => value
            .as_f64()
            .is_some_and(|v| v.is_finite() && (*min..=*max).contains(&v)),
        (ValueKind::String { max_bytes }, Value::String(s)) => s.len() <= *max_bytes as usize,
        (ValueKind::ObjectRef(_), Value::String(s)) => !s.is_empty() && s.len() <= 128,
        (ValueKind::Enum(values), Value::String(s)) => values.contains(s),
        (ValueKind::Array { element, max_len }, Value::Array(values)) => {
            if values.len() > *max_len as usize {
                false
            } else {
                for item in values {
                    default_value(item, element, context)?;
                }
                true
            }
        }
        (ValueKind::Record { fields, optional }, Value::Object(values)) => {
            if values.keys().any(|key| !fields.contains_key(key)) {
                false
            } else {
                for (key, member) in fields {
                    let Some(value) = values.get(key) else {
                        if optional.contains(key) {
                            continue;
                        }
                        return Err(FreezeError::new(
                            context,
                            format!("默认 Record 缺少成员 {key}"),
                        ));
                    };
                    default_value(value, member, context)?;
                }
                true
            }
        }
        (
            ValueKind::Map {
                value: member,
                max_len,
                max_key_bytes,
            },
            Value::Object(values),
        ) => {
            if values.len() > *max_len as usize
                || values.keys().any(|k| k.len() > *max_key_bytes as usize)
            {
                false
            } else {
                for value in values.values() {
                    default_value(value, member, context)?;
                }
                true
            }
        }
        _ => false,
    };
    if !valid {
        return Err(FreezeError::new(context, "字段默认值不满足 schema"));
    }
    Ok(())
}

pub(crate) fn binding(
    key: &Key,
    bindings: &BTreeSet<Key>,
    context: &str,
) -> Result<(), FreezeError> {
    if !bindings.contains(key) {
        return Err(FreezeError::new(context, format!("未知绑定 {key}")));
    }
    Ok(())
}

pub(crate) fn object(
    ty: &ObjectTypeDescriptor,
    types: &BTreeMap<Key, ObjectTypeDescriptor>,
    bindings: &BTreeSet<Key>,
    slots: &BTreeMap<Key, SlotDescriptor>,
    max_depth: u16,
) -> Result<(), FreezeError> {
    let context = ty.key.to_string();
    if ty.schema_version == 0 || ty.player_class.is_empty() || ty.player_class.len() > 128 {
        return Err(FreezeError::new(&context, "schema_version 或玩家类名无效"));
    }
    let mut seen = BTreeSet::new();
    for field in &ty.fields {
        let context = format!("{} / {}", ty.key, field.key);
        if !seen.insert(&field.key) {
            return Err(FreezeError::new(&context, "重复字段"));
        }
        schema(&field.schema, types, 1, max_depth, &context)?;
        binding(&field.binding, bindings, &context)?;
        if let Visibility::Conditional(key) = &field.visibility {
            binding(key, bindings, &context)?;
        }
        if field.visibility == Visibility::Owner && ty.owner_field.is_none() {
            return Err(FreezeError::new(
                &context,
                "owner 字段权限要求类型声明归属字段",
            ));
        }
        if let FieldDefault::Value(value) = &field.default {
            default_value(value, &field.schema, &context)?;
        }
    }
    if let Some(owner) = &ty.owner_field {
        let field = ty
            .fields
            .iter()
            .find(|f| &f.key == owner)
            .ok_or_else(|| FreezeError::new(&context, format!("未知归属字段 {owner}")))?;
        if !matches!(field.schema.kind, ValueKind::String { .. }) {
            return Err(FreezeError::new(&context, "归属字段须为字符串"));
        }
    }
    if let Some([x, y]) = &ty.position_fields {
        if x == y {
            return Err(FreezeError::new(&context, "位置字段不能相同"));
        }
        for key in [x, y] {
            let field = ty
                .fields
                .iter()
                .find(|f| &f.key == key)
                .ok_or_else(|| FreezeError::new(&context, format!("未知位置字段 {key}")))?;
            if !matches!(
                field.schema.kind,
                ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 })
            ) || field.schema.nullable
            {
                return Err(FreezeError::new(
                    &context,
                    "房间位置字段须为非 null 的 0..49 无符号整数",
                ));
            }
        }
    }
    if ty.visibility == ObjectVisibility::OwnerCollection && ty.owner_field.is_none() {
        return Err(FreezeError::new(&context, "owner 集合要求归属字段"));
    }
    if let ObjectVisibility::Conditional(key) = &ty.visibility {
        binding(key, bindings, &context)?;
    }
    let mut capabilities = BTreeSet::new();
    for capability in &ty.capabilities {
        if !capabilities.insert(&capability.key) || capability.consumers.is_empty() {
            return Err(FreezeError::new(
                &context,
                format!("能力 {} 重复或未声明消费者", capability.key),
            ));
        }
        for key in &capability.consumers {
            binding(key, bindings, &context)?;
        }
    }
    for key in &ty.lifecycle_slots {
        if !slots.contains_key(key) {
            return Err(FreezeError::new(
                &context,
                format!("未知生命周期入口 {key}"),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_transfer_schema_accepts_payload_without_unused_resource_type() {
        let link = builtin::link();
        for payload in [
            serde_json::json!({"id":"1", "amount":100}),
            serde_json::json!({"id":"1", "amount":100,"resourceType":"energy"}),
            serde_json::json!({"id":"1", "amount":100,"resourceType":null}),
        ] {
            default_value(&payload, &link.intents[0].arguments, "link transfer").unwrap();
        }
        assert!(
            default_value(
                &serde_json::json!({"id":"1"}),
                &link.intents[0].arguments,
                "link transfer"
            )
            .is_err()
        );
    }
}
