//! factory 结算：produce 单动作。
//!
//! 移植官方 engine/src/processor/intents/factories/produce.js：处理器对
//! 一切失败静默 no-op（错误码由玩家预检层产生），本模块只镜像官方检查
//! 顺序与记账。factory 用平面共享容量（storeCapacity，无资源槽），
//! 与 lab 的双槽记账不同。不保存上游渲染用 actionLog（与 structures.rs
//! 同约定）。

use kk_core::constants as c;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::Working;
use super::structures::{check_structure_against_controller, generic_view};
use crate::intent::IntentData;

/// COMMODITIES[resource] 配方。
struct Recipe {
    amount: i64,
    cooldown: i64,
    components: Vec<(String, i64)>,
    level: Option<i64>,
}

fn commodity(resource: &str) -> Option<Recipe> {
    use std::sync::OnceLock;
    static TABLE: OnceLock<Option<serde_json::Value>> = OnceLock::new();
    let table = TABLE.get_or_init(|| serde_json::from_str(c::COMMODITIES).ok());
    let recipe = table.as_ref()?.get(resource)?;
    let amount = recipe.get("amount").and_then(Value::as_i64).unwrap_or(1);
    let cooldown = recipe.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
    let level = recipe.get("level").and_then(Value::as_i64);
    let components = recipe
        .get("components")?
        .as_object()?
        .iter()
        .filter_map(|(res, need)| need.as_i64().map(|n| (res.clone(), n)))
        .collect();
    Some(Recipe {
        amount,
        cooldown,
        components,
        level,
    })
}

fn store_amount(data: &Value, resource: &str) -> i64 {
    data.pointer(&format!("/store/{resource}"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// store 全资源合计（官方 utils.calcResources）。
fn store_total(data: &Value) -> i64 {
    data.get("store")
        .and_then(Value::as_object)
        .map(|s| s.values().filter_map(Value::as_i64).sum())
        .unwrap_or(0)
}

fn on_cooldown(data: &Value, time: u64) -> bool {
    data.get("cooldownTime")
        .and_then(Value::as_u64)
        .is_some_and(|t| t > time)
}

/// 官方效果查询：data.effects 中存在同 power、同级且 endTime≥time 的项。
/// power 域未接线时 data 无 effects 键，恒为 false。
fn has_effect(data: &Value, power: i64, level: i64, time: u64) -> bool {
    data.get("effects")
        .and_then(Value::as_array)
        .is_some_and(|effects| {
            effects.iter().any(|e| {
                e.get("power").and_then(Value::as_i64) == Some(power)
                    && e.get("level").and_then(Value::as_i64) == Some(level)
                    && e.get("endTime")
                        .and_then(Value::as_u64)
                        .is_some_and(|t| t >= time)
            })
        })
}

/// factory 按配方消耗组件并产出商品，进入配方冷却。来源：
/// processor/intents/factories/produce.js。
pub fn produce(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "factory" {
        return;
    }
    let Some(resource) = d.resource_type.as_deref() else {
        return;
    };
    let Some(recipe) = commodity(resource) else {
        return;
    };
    let factory_level = view.data.get("level").and_then(Value::as_i64).unwrap_or(0);
    if recipe.level.is_some_and(|lvl| factory_level != lvl) {
        return;
    }
    if on_cooldown(&view.data, w.time) {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    if recipe.level.is_some()
        && factory_level > 0
        && !has_effect(
            &view.data,
            c::PWR_OPERATE_FACTORY,
            recipe.level.unwrap_or(0),
            w.time,
        )
    {
        return;
    }
    for (res, need) in &recipe.components {
        if store_amount(&view.data, res) < *need {
            return;
        }
    }
    let components_total: i64 = recipe.components.iter().map(|(_, n)| *n).sum();
    let capacity = view
        .data
        .get("storeCapacity")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if store_total(&view.data) - components_total + recipe.amount > capacity {
        return;
    }
    let cooldown_end = w.time + recipe.cooldown as u64;
    let resource = resource.to_string();
    if let Some(kk_schema::room::RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        for (res, need) in &recipe.components {
            v["store"][res.as_str()] = json!(store_amount(&v, res) - need);
        }
        v["store"][&resource] = json!(store_amount(&v, &resource) + recipe.amount);
        v["cooldownTime"] = json!(cooldown_end);
        g.data = v.to_string();
    }
}
