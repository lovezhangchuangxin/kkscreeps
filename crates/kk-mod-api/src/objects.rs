//! 同房自有对象能力的数据封套和创建字段准备；不读取世界或执行 JS。
use crate::{FieldDefault, ObjectTypeDescriptor, validate};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectView {
    pub fields: Value,
    pub id: String,
    pub room: String,
    pub type_key: String,
}

#[derive(Serialize)]
pub struct ObjectPage {
    pub objects: Vec<ObjectView>,
    pub next: Option<String>,
}

/// 管理初始化与候选创建共用默认值、字段集合与数值 backing 校验。
pub fn prepare_fields(provided: &Value, ty: &ObjectTypeDescriptor) -> Result<Value, String> {
    let provided = provided.as_object().ok_or("Mod 对象字段须为 JSON 对象")?;
    let mut fields = provided.clone();
    for field in &ty.fields {
        let name = validate::field_name(&field.key);
        if !fields.contains_key(name) {
            let FieldDefault::Value(value) = &field.default else {
                return Err(format!("类型 {} 的字段 {name} 未提供且无默认值", ty.key));
            };
            fields.insert(name.to_string(), value.clone());
        }
    }
    let mut fields = Value::Object(fields);
    validate::check_object_payload(&fields, &ty.fields, ty.key.as_str())?;
    validate::normalize_object_payload(&mut fields, &ty.fields);
    Ok(fields)
}

/// 无位置声明的自有对象沿现有管理放置口径使用 (0,0)。
pub fn position(fields: &Value, ty: &ObjectTypeDescriptor) -> Result<(u8, u8), String> {
    let Some([x, y]) = &ty.position_fields else {
        return Ok((0, 0));
    };
    let read = |key: &crate::Key| {
        fields
            .get(validate::field_name(key))
            .and_then(Value::as_u64)
            .filter(|n| *n < 50)
            .map(|n| n as u8)
            .ok_or_else(|| format!("位置字段 {key} 须为 0..49 的整数"))
    };
    Ok((read(x)?, read(y)?))
}
