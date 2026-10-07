//! 声明转换共用的基础读取与键校验。
use super::*;

pub(super) fn err<T>(namespace: &str, message: impl Into<String>) -> ModHostResult<T> {
    Err(ModHostError::new(namespace.to_string(), message))
}

pub(super) fn own_key(namespace: &str, value: &str) -> ModHostResult<Key> {
    let key =
        Key::new(value).map_err(|message| ModHostError::new(namespace.to_string(), message))?;
    if key.namespace() != namespace {
        return err(namespace, format!("键 {key} 不属于本包命名空间"));
    }
    Ok(key)
}

pub(super) fn any_key(namespace: &str, value: &str) -> ModHostResult<Key> {
    Key::new(value).map_err(|message| ModHostError::new(namespace.to_string(), message))
}

pub(super) fn as_str<'a>(namespace: &str, value: &'a Value, field: &str) -> ModHostResult<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ModHostError::new(namespace.to_string(), format!("缺少字符串字段 {field}")))
}

pub(super) fn optional_str<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

pub(super) fn as_u32(namespace: &str, value: &Value, field: &str) -> ModHostResult<u32> {
    let raw = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        ModHostError::new(namespace.to_string(), format!("字段 {field} 须为非负整数"))
    })?;
    if raw > u32::MAX as u64 {
        return err(namespace, format!("字段 {field} 超出 u32 范围"));
    }
    Ok(raw as u32)
}

pub(super) fn as_u64(namespace: &str, value: &Value, field: &str) -> ModHostResult<u64> {
    value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        ModHostError::new(namespace.to_string(), format!("字段 {field} 须为非负整数"))
    })
}

/// 存在但形状非法的数值选项一律报错：拼错形状静默落默认值等于改写
/// 校验语义，与装载器的 fail-loud 立场冲突。
pub(super) fn optional_u32(
    namespace: &str,
    value: &Value,
    field: &str,
) -> ModHostResult<Option<u32>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(_)) => {
            let raw = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
                ModHostError::new(namespace.to_string(), format!("字段 {field} 须为非负整数"))
            })?;
            if raw > u32::MAX as u64 {
                return err(namespace, format!("字段 {field} 超出 u32 范围"));
            }
            Ok(Some(raw as u32))
        }
        Some(_) => err(namespace, format!("字段 {field} 须为非负整数")),
    }
}

pub(super) fn optional_i64(
    namespace: &str,
    value: &Value,
    field: &str,
) -> ModHostResult<Option<i64>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(number) => number.as_i64().map(Some).ok_or_else(|| {
            ModHostError::new(namespace.to_string(), format!("字段 {field} 须为整数"))
        }),
    }
}
