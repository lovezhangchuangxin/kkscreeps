//! 结算资源转移、提取、主动掉落与拾取。
//!
//! 共享仓容量按全部资源之和计算，固定能量仓按 energy 槽计算。内部超容掉落
//! 与主动 drop 的数量不足处理不同，分别保留钳制和整条拒绝。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::{
    creep_slot, slots_add, slots_sub, store_capacity, store_slot, store_slot_add, store_slot_sub,
};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::resources::generic::{
    change_generic_store, change_mod_store, generic_capacity, generic_store,
};

/// 向相邻 creep 或有容量字段的结构转移资源。
/// 请求超过自身持有量时整条拒绝；目标容量不足时只转可容纳的数量。
/// 来源：上游 engine/src/processor/intents/creeps/transfer.js。
pub(in crate::settle) fn intent_transfer(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(amount_req) = data.amount else {
        return;
    };
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), creep_slot(c, resource)),
        _ => return,
    };
    if held < amount_req || amount_req < 0 {
        return;
    }
    let Some(target_id) = data.id else { return };
    // Extension 的能量容量按房间控制器等级计算，没有控制器时容量为 0。
    let extension_cap = || {
        w.controller()
            .map(|ctl| kk_world::types::extension_energy_capacity(ctl.level))
            .unwrap_or(0)
    };
    // Spawn/extension 只接收 energy；creep/storage 的容量由全部资源共用。
    let per_resource_cap = |base: i64| if resource == "energy" { base } else { 0 };
    let (target_cap, t_pos, t_stored) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Creep(t)) if !t.spawning => (
            store_capacity(t),
            (t.x, t.y),
            kk_world::types::store_total(&t.store, &t.store_extra),
        ),
        Some(RoomObject::Spawn(t)) => (
            per_resource_cap(c::SPAWN_ENERGY_CAPACITY),
            (t.x, t.y),
            store_slot(&t.store, resource),
        ),
        Some(RoomObject::Extension(t)) => (
            per_resource_cap(extension_cap()),
            (t.x, t.y),
            store_slot(&t.store, resource),
        ),
        Some(RoomObject::Storage(t)) => (
            t.store_capacity,
            (t.x, t.y),
            kk_world::types::store_total(&t.store, &t.store_extra),
        ),
        Some(RoomObject::Generic(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            let (capacity, stored) = generic_capacity(&value, resource);
            (capacity, (t.x, t.y), stored)
        }
        // Mod 对象库存（G09.a）：store 声明类型经创建写入信封；无信封
        // （未声明类型）按不可转移处理，与 Generic 缺键同形。
        Some(RoomObject::Mod(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            let (capacity, stored) = generic_capacity(&value, resource);
            (capacity, (t.x, t.y), stored)
        }
        _ => return,
    };
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    if t_stored >= target_cap {
        return;
    }
    let amount = amount_req.min(held).min(target_cap - t_stored);
    if amount == 0 {
        return;
    }
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Creep(t)) => slots_add(&mut t.store, &mut t.store_extra, resource, amount),
        Some(RoomObject::Spawn(t)) => store_slot_add(&mut t.store, resource, amount),
        Some(RoomObject::Extension(t)) => store_slot_add(&mut t.store, resource, amount),
        Some(RoomObject::Storage(t)) => {
            slots_add(&mut t.store, &mut t.store_extra, resource, amount)
        }
        Some(RoomObject::Generic(t)) => {
            change_generic_store(&mut t.data, resource, amount, &w.plane.stores)
        }
        Some(RoomObject::Mod(t)) => change_mod_store(&mut t.data, resource, amount),
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        slots_sub(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.push_event(
        c::EVENT_TRANSFER,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 从相邻有库存的结构、墓碑或废墟提取资源。
/// 他人安全模式会拒绝提取；目标归属不同且被敌方私有 rampart 覆盖时也拒绝。
/// 数量按请求、携带余量和目标库存取最小值。事件由资源提供方指向提取者。
/// nuker、powerBank 和受干扰的 terminal 拒绝提取。
/// 来源：上游 engine/src/processor/intents/creeps/withdraw.js。
pub(in crate::settle) fn intent_withdraw(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(target_id) = data.id else { return };
    let (pos, owner, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning && store_capacity(c) > 0 => (
            (c.x, c.y),
            c.owner.clone(),
            store_capacity(c),
            kk_world::types::store_total(&c.store, &c.store_extra),
        ),
        _ => return,
    };
    if w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(owner.as_str()) && ctl.safe_mode > w.time)
    {
        return;
    }
    let empty_space = cap - held;
    let Some(amount_req) = data.amount else {
        return;
    };
    let mut amount = amount_req.min(empty_space);
    if amount < 0 {
        return;
    }
    let (t_pos, t_amount) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Spawn(t)) => ((t.x, t.y), store_slot(&t.store, resource)),
        Some(RoomObject::Extension(t)) => ((t.x, t.y), store_slot(&t.store, resource)),
        Some(RoomObject::Storage(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        Some(RoomObject::Tombstone(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        Some(RoomObject::Ruin(t)) => (
            (t.x, t.y),
            kk_world::types::store_extra_slot(&t.store, &t.store_extra, resource),
        ),
        Some(RoomObject::Generic(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            match value["structureType"].as_str() {
                Some("nuker" | "powerBank") => return,
                Some("terminal")
                    if kk_world::effects::active_power(
                        w.room.effects.get(&target_id),
                        c::PWR_DISRUPT_TERMINAL,
                        w.time,
                    )
                    .is_some() =>
                {
                    return;
                }
                _ => {}
            }
            ((t.x, t.y), value["store"][resource].as_i64().unwrap_or(0))
        }
        // Mod 对象库存（G09.a）：信封由创建写入，结构专属拒绝
        // （nuker/powerBank/terminal）不适用。object_owner 对 Mod 返回
        // None——声明 owner 字段不参与下方 rampart 门（与 Generic 结构
        // 不同，首片口径：敌方私 rampart 下任何人不可提取）。
        Some(RoomObject::Mod(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            ((t.x, t.y), value["store"][resource].as_i64().unwrap_or(0))
        }
        _ => return,
    };
    let target = &w.room.objects[&target_id];
    if kk_world::domain::object_owner(target).as_deref() != Some(owner.as_str())
        && w.index
            .at(t_pos.0 as usize, t_pos.1 as usize)
            .iter()
            .any(|tid| {
                let Some(RoomObject::Generic(g)) = w.room.objects.get(tid) else {
                    return false;
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
                    return false;
                };
                v["structureType"].as_str() == Some("rampart")
                    && v["owner"].as_str() != Some(owner.as_str())
                    && !v["isPublic"].as_bool().unwrap_or(false)
            })
    {
        return;
    }
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    amount = amount.min(t_amount);
    if amount <= 0 {
        return;
    }
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Spawn(t)) => store_slot_sub(&mut t.store, resource, amount),
        Some(RoomObject::Extension(t)) => store_slot_sub(&mut t.store, resource, amount),
        Some(RoomObject::Storage(t)) => {
            slots_sub(&mut t.store, &mut t.store_extra, resource, amount)
        }
        Some(RoomObject::Tombstone(t)) => {
            slots_sub(&mut t.store, &mut t.store_extra, resource, amount)
        }
        Some(RoomObject::Ruin(t)) => slots_sub(&mut t.store, &mut t.store_extra, resource, amount),
        Some(RoomObject::Generic(t)) => {
            change_generic_store(&mut t.data, resource, -amount, &w.plane.stores)
        }
        Some(RoomObject::Mod(t)) => change_mod_store(&mut t.data, resource, -amount),
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.push_event(
        c::EVENT_TRANSFER,
        target_id,
        serde_json::json!({ "targetId": id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 主动掉落请求必须为正数且持有足量，不按现有持有量部分执行。
pub(super) fn intent_drop(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(amount) = data.amount else { return };
    if amount <= 0 {
        return;
    }
    if w.creep(id)
        .is_none_or(|c| c.spawning || creep_slot(c, resource) < amount)
    {
        return;
    }
    intent_drop_amount_res(w, id, resource, amount);
}

/// 内部能量掉落入口，用于采集超容和拆解回收。
pub(in crate::settle) fn intent_drop_amount(w: &mut Working, id: ObjId, amount: i64) {
    intent_drop_amount_res(w, id, "energy", amount);
}

/// 掉落任意资源，最多扣除实际持有量；同格容器先接收，余量合并到掉落堆。
/// 内部超容处理使用此入口，主动 drop 的整条拒绝检查在 intent_drop 中。
pub(in crate::settle) fn intent_drop_amount_res(
    w: &mut Working,
    id: ObjId,
    resource: &str,
    amount: i64,
) {
    let (pos, held) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), creep_slot(c, resource)),
        _ => return,
    };
    let amount = amount.min(held);
    if amount <= 0 {
        return;
    }
    if let Some(c) = w.creep_mut(id) {
        slots_sub(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.create_resource(pos.0, pos.1, amount, resource);
}

/// 拾取相邻资源堆，按全部资源占用后的余量限量，清空的堆立即移除。
/// 当前拒绝 power 堆及目录外资源，其他资源按资源名入槽。
pub(super) fn intent_pickup(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, cap, held) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            store_capacity(c),
            kk_world::types::store_total(&c.store, &c.store_extra),
        ),
        _ => return,
    };
    if held >= cap {
        return;
    }
    let Some(RoomObject::Energy(target)) = w.room.objects.get(&target_id) else {
        return;
    };
    let slot_resource = match super::energy_resource_name(target) {
        "power" => return,
        name => name.to_string(),
    };
    if !w.plane.resources.contains(&slot_resource) {
        return;
    }
    let (tx, ty, amount) = (target.x, target.y, target.amount as i64);
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    let amount = amount.min(cap - held);
    if amount <= 0 {
        return;
    }
    if let Some(RoomObject::Energy(t)) = w.room.objects.get_mut(&target_id) {
        t.amount -= amount as u32;
    }
    let drained = w
        .room
        .objects
        .get(&target_id)
        .is_some_and(|o| matches!(o, RoomObject::Energy(e) if e.amount == 0));
    if drained {
        w.remove_obj(target_id);
    }
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, &slot_resource, amount);
    }
}
