//! Generic 对象的共享 JSON 字段读取；不提供世界写能力或玩法默认值。
use serde_json::{Map, Value};

/// 解码对象字段；无效 JSON 或非对象返回 None，由投影方选择缺省行为。
/// 接管字段表，避免读取方解析后再次克隆整个 JSON 对象。
pub fn decode_fields(data: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str(data).ok()? {
        Value::Object(fields) => Some(fields),
        _ => None,
    }
}

/// 读取结构类型；缺失、类型错误和无效 JSON 均返回 None。
pub fn structure_type(data: &str) -> Option<String> {
    decode_fields(data)?
        .get("structureType")?
        .as_str()
        .map(str::to_string)
}

/// Generic 仓库通过 store/storeCapacity/storeCapacityResource 字段参与结算，
/// 不按结构类型列容量白名单；扩展结构可复用同一数据通道。
/// 来源：上游 utils.capacityForResource 和 creeps/transfer.js。
pub fn decode_store(data: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(data).ok()?;
    let store = value.get("store")?.as_object()?;
    if !store
        .values()
        .all(|slot| slot.as_i64().is_some_and(|n| n >= 0))
    {
        return None;
    }
    if let Some(capacities) = value.get("storeCapacityResource").filter(|v| !v.is_null()) {
        let slots = capacities.as_object()?;
        if !slots
            .values()
            .all(|slot| slot.is_null() || slot.as_i64().is_some_and(|n| n >= 0))
        {
            return None;
        }
    }
    Some(value)
}
