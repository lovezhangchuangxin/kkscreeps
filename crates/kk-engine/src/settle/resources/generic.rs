//! Generic 仓库的解析、容量与实验室槽切换；native 与领域能力共用。
use serde_json::Value;

pub(in crate::settle) use kk_world::domain::generic::decode_store as generic_store;

pub(in crate::settle) fn generic_capacity(value: &Value, resource: &str) -> (i64, i64) {
    let capacities = value
        .get("storeCapacityResource")
        .and_then(Value::as_object);
    let reserved: i64 = capacities
        .into_iter()
        .flat_map(|slots| slots.values())
        .filter_map(Value::as_i64)
        .fold(0_i64, i64::saturating_add);
    let capacity = capacities
        .and_then(|slots| slots.get(resource))
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            value["storeCapacity"]
                .as_i64()
                .unwrap_or(0)
                .saturating_sub(reserved)
                .max(0)
        });
    let stored = if capacities.is_some() {
        value["store"][resource].as_i64().unwrap_or(0)
    } else {
        value["store"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(Value::as_i64)
            .fold(0_i64, i64::saturating_add)
    };
    (capacity, stored)
}

/// 实验室装入首种矿物时锁定该槽，取空后恢复空闲矿物容量。
/// 其他字段原样保留，来源：上游 creeps/{transfer,withdraw}.js。
pub(in crate::settle) fn change_generic_store(
    data: &mut String,
    resource: &str,
    amount: i64,
    stores: &kk_mod_api::builtin::StoreRules,
) {
    let mut value = generic_store(data).expect("读阶段已确认 Generic 仓库");
    let next = value["store"][resource].as_i64().unwrap_or(0) + amount;
    value["store"][resource] = next.into();
    if value["structureType"].as_str() == Some("lab") && resource != "energy" {
        if amount > 0
            && value["storeCapacityResource"][resource]
                .as_i64()
                .unwrap_or(0)
                == 0
        {
            value["storeCapacityResource"][resource] = stores.lab_mineral.into();
            value["storeCapacity"] = Value::Null;
        } else if amount < 0 && next == 0 {
            value["storeCapacityResource"][resource] = Value::Null;
            value["storeCapacity"] = (stores.lab_energy + stores.lab_mineral).into();
        }
    }
    *data = value.to_string();
}

/// Mod 对象库存（G09.a）的纯变更：只动 store[resource]，不套用 Generic
/// 结构的实验室槽切换与 structureType 语义（Mod 声明面无逐资源槽）。
pub(in crate::settle) fn change_mod_store(data: &mut String, resource: &str, amount: i64) {
    let mut value = generic_store(data).expect("读阶段已确认库存信封");
    let next = value["store"][resource].as_i64().unwrap_or(0) + amount;
    value["store"][resource] = next.into();
    *data = value.to_string();
}
