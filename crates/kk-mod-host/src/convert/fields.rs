//! 声明转换共用的字段绑定及可见性。
use super::*;

pub(super) fn field_visibility(namespace: &str, value: &str) -> ModHostResult<Visibility> {
    match value {
        "public" => Ok(Visibility::Public),
        "owner" => Ok(Visibility::Owner),
        "server-only" => Ok(Visibility::ServerOnly),
        other => err(namespace, format!("未知字段可见性 {other:?}")),
    }
}

pub(super) fn object_visibility(
    namespace: &str,
    value: Option<&str>,
) -> ModHostResult<ObjectVisibility> {
    match value {
        None => Ok(ObjectVisibility::RoomVisible),
        Some("room-visible") => Ok(ObjectVisibility::RoomVisible),
        Some("owner-collection") => Ok(ObjectVisibility::OwnerCollection),
        Some("server-only") => Ok(ObjectVisibility::ServerOnly),
        Some(other) => err(namespace, format!("未知对象可见性 {other:?}")),
    }
}

pub(super) fn field_descriptor(
    namespace: &str,
    name: &str,
    value: &Value,
    binding_for: impl Fn(&str) -> String,
) -> ModHostResult<FieldDescriptor> {
    // 字段名禁点号：字段绑定 {ns}:{类型名}.field.{name} 依赖名字段单射，
    // 含点的名字可与"其他类型名+字段名"拼出同一绑定而在集合内静默合并。
    if name.is_empty() || name.bytes().any(|b| b == b'.' || b == b':') {
        return err(namespace, format!("字段名 {name:?} 不得为空、含点号或冒号"));
    }
    let field_key = own_key(namespace, &format!("{namespace}:{name}"))?;
    let binding = own_key(namespace, &format!("{namespace}:{}", binding_for(name)))?;
    Ok(FieldDescriptor {
        key: field_key,
        schema: schema(
            namespace,
            value.get("schema").ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("字段 {name} 缺少 schema"))
            })?,
        )?,
        default: match value.get("default") {
            None | Some(Value::Null) => FieldDefault::Missing,
            Some(other) => FieldDefault::Value(other.clone()),
        },
        visibility: field_visibility(
            namespace,
            optional_str(value, "visibility").unwrap_or("public"),
        )?,
        binding,
    })
}
