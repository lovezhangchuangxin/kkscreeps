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

/// 库存信封键（G09.a）：store 声明类型的 payload 中由引擎维护的键。
/// 字段校验只针对作者声明字段——校验前拆出信封、校验后按原值并回。
pub const STORE_ENVELOPE_KEYS: [&str; 3] = ["store", "storeCapacity", "storeCapacityResource"];

/// 创建时写入库存信封（与建筑完工同形：store 从 {"energy":0} 起，
/// 容量为冻结声明值）。仅 store 声明类型调用。
pub fn inject_store_envelope(fields: &mut Value, capacity: u64) {
    let map = fields
        .as_object_mut()
        .expect("prepare_fields 已确认字段为对象");
    map.insert("store".into(), serde_json::json!({"energy": 0}));
    map.insert("storeCapacity".into(), serde_json::json!(capacity));
}

/// 耐久信封键（G09.b）：hits 声明类型的 payload 中由引擎维护的键，
/// 与库存信封键同构、键集不相交（两类声明可并存于同一类型）。
pub const HITS_ENVELOPE_KEYS: [&str; 2] = ["hits", "hitsMax"];

/// 创建时写入耐久信封（满血创建，与建筑完工同形）。
/// 仅 hits 声明类型调用。
pub fn inject_hits_envelope(fields: &mut Value, hits_max: u64) {
    let map = fields
        .as_object_mut()
        .expect("prepare_fields 已确认字段为对象");
    map.insert("hits".into(), serde_json::json!(hits_max));
    map.insert("hitsMax".into(), serde_json::json!(hits_max));
}

/// 拆出库存信封键：返回（无信封的字段值, 原信封键值对）。无信封类型
/// 得到 None；信封值原样保留（数值不变量由资源动作维护，这里不动）。
pub fn split_store_envelope(payload: &Value) -> (Value, Option<serde_json::Map<String, Value>>) {
    split_envelope_keys(payload, &STORE_ENVELOPE_KEYS)
}

/// 拆出耐久信封键，语义同库存信封拆分。
pub fn split_hits_envelope(payload: &Value) -> (Value, Option<serde_json::Map<String, Value>>) {
    split_envelope_keys(payload, &HITS_ENVELOPE_KEYS)
}

fn split_envelope_keys(
    payload: &Value,
    keys: &[&str],
) -> (Value, Option<serde_json::Map<String, Value>>) {
    let Some(map) = payload.as_object() else {
        return (payload.clone(), None);
    };
    let mut envelope = serde_json::Map::new();
    let mut fields = serde_json::Map::new();
    for (key, value) in map {
        if keys.contains(&key.as_str()) {
            envelope.insert(key.clone(), value.clone());
        } else {
            fields.insert(key.clone(), value.clone());
        }
    }
    let envelope = if envelope.is_empty() {
        None
    } else {
        Some(envelope)
    };
    (Value::Object(fields), envelope)
}

/// 把拆出的库存信封并回字段值（保留原值，不重算）。
pub fn merge_store_envelope(fields: &mut Value, envelope: Option<serde_json::Map<String, Value>>) {
    merge_envelope_keys(fields, envelope)
}

/// 把拆出的耐久信封并回字段值（保留原值，不重算）。
pub fn merge_hits_envelope(fields: &mut Value, envelope: Option<serde_json::Map<String, Value>>) {
    merge_envelope_keys(fields, envelope)
}

fn merge_envelope_keys(fields: &mut Value, envelope: Option<serde_json::Map<String, Value>>) {
    let Some(envelope) = envelope else {
        return;
    };
    let map = fields.as_object_mut().expect("校验路径已确认字段为对象");
    for (key, value) in envelope {
        map.insert(key, value);
    }
}
