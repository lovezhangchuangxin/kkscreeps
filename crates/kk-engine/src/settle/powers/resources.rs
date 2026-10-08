//! powerCreep 的资源四动作：transfer/withdraw/drop/pickup。
//!
//! 官方逐字委托 creeps 同名处理器（processor/intents/power-creeps/*.js），
//! 语义与 creep 完全一致；kk 的 creep 处理器按 CreepObj 取发起方库存，
//! 这里以 data.store/storeCapacity 的 Generic 读写镜像同一目标判定。
//! 目标匹配块与 settle/creep/store.rs 保持同形（官方同码），重构共享受
//! 益面另行评估。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::{pc_data, pc_owner};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::creep::{
    slots_add, slots_sub, store_capacity, store_slot, store_slot_add, store_slot_sub,
};
use crate::settle::resources::generic::{
    change_generic_store, change_mod_store, generic_capacity, generic_store,
};

/// 发起方快照：位置、主人、当前持有与总容量（storeCapacity 全资源共用）。
struct Actor {
    x: u8,
    y: u8,
    owner: String,
    held: i64,
    cap: i64,
}

fn actor(w: &Working, id: ObjId, resource: &str) -> Option<Actor> {
    let obj = w.room.objects.get(&id)?;
    let RoomObject::Generic(g) = obj else {
        return None;
    };
    let v = pc_data(obj)?;
    Some(Actor {
        x: g.x,
        y: g.y,
        owner: pc_owner(&v).map(str::to_string).unwrap_or_default(),
        held: v
            .pointer(&format!("/store/{resource}"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cap: v.get("storeCapacity").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 发起方库存增减（amount 可为负）；写回 data.store。
fn change_pc_store(w: &mut Working, id: ObjId, resource: &str, amount: i64) {
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id) {
        change_generic_store(&mut o.data, resource, amount, &w.plane.stores);
    }
}

/// transfer：请求超持有整条拒绝；目标容量不足只转可容纳量。
/// 来源：creeps/transfer.js（经 power-creeps/transfer.js 委托）。
pub(super) fn pc_transfer(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(amount_req) = data.amount else {
        return;
    };
    let Some(a) = actor(w, id, resource) else {
        return;
    };
    if a.held < amount_req || amount_req < 0 {
        return;
    }
    let Some(target_id) = data.id else { return };
    let extension_cap = || {
        w.controller()
            .map(|ctl| kk_world::types::extension_energy_capacity(ctl.level))
            .unwrap_or(0)
    };
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
            kk_world::types::storage_capacity(
                w.controller(),
                &t.owner,
                w.room.effects.get(&target_id),
                w.time,
            ),
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
        Some(RoomObject::Mod(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            let (capacity, stored) = generic_capacity(&value, resource);
            (capacity, (t.x, t.y), stored)
        }
        _ => return,
    };
    if (t_pos.0 as i32 - a.x as i32).abs() > 1 || (t_pos.1 as i32 - a.y as i32).abs() > 1 {
        return;
    }
    if t_stored >= target_cap {
        return;
    }
    let amount = amount_req.min(a.held).min(target_cap - t_stored);
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
    change_pc_store(w, id, resource, -amount);
    w.push_event(
        c::EVENT_TRANSFER,
        id,
        json!({ "targetId": target_id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// withdraw：他人安全模式、敌方私有 rampart、nuker/powerBank/受干扰
/// terminal 的拒绝与 creep 同门。来源：creeps/withdraw.js（委托同形）。
pub(super) fn pc_withdraw(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(target_id) = data.id else { return };
    let Some(a) = actor(w, id, resource) else {
        return;
    };
    if w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(a.owner.as_str()) && ctl.safe_mode > w.time)
    {
        return;
    }
    let total: i64 = w
        .room
        .objects
        .get(&id)
        .and_then(pc_data)
        .and_then(|v| v.get("store").cloned())
        .and_then(|s| s.as_object().cloned())
        .map(|s| s.values().filter_map(Value::as_i64).sum())
        .unwrap_or(0);
    let empty_space = a.cap - total;
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
        Some(RoomObject::Mod(t)) => {
            let Some(value) = generic_store(&t.data) else {
                return;
            };
            ((t.x, t.y), value["store"][resource].as_i64().unwrap_or(0))
        }
        _ => return,
    };
    let target = &w.room.objects[&target_id];
    if kk_world::domain::object_owner(target).as_deref() != Some(a.owner.as_str())
        && w.index
            .at(t_pos.0 as usize, t_pos.1 as usize)
            .iter()
            .any(|tid| {
                let Some(RoomObject::Generic(g)) = w.room.objects.get(tid) else {
                    return false;
                };
                let Ok(v) = serde_json::from_str::<Value>(&g.data) else {
                    return false;
                };
                v["structureType"].as_str() == Some("rampart")
                    && v["owner"].as_str() != Some(a.owner.as_str())
                    && !v["isPublic"].as_bool().unwrap_or(false)
            })
    {
        return;
    }
    if (t_pos.0 as i32 - a.x as i32).abs() > 1 || (t_pos.1 as i32 - a.y as i32).abs() > 1 {
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
    change_pc_store(w, id, resource, amount);
    w.push_event(
        c::EVENT_TRANSFER,
        target_id,
        json!({ "targetId": id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// drop：正数且足量才执行；掉落堆原地创建。来源：creeps/drop.js（委托）。
pub(super) fn pc_drop(w: &mut Working, id: ObjId, data: &IntentData) {
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !w.plane.resources.contains(resource) {
        return;
    }
    let Some(amount) = data.amount else { return };
    if amount <= 0 {
        return;
    }
    let Some(a) = actor(w, id, resource) else {
        return;
    };
    if a.held < amount {
        return;
    }
    change_pc_store(w, id, resource, -amount);
    w.create_resource(a.x, a.y, amount, resource);
}

/// pickup：相邻资源堆按总容量余量限量；拒绝 power 堆与目录外资源。
/// 来源：creeps/pickup.js（委托）。
pub(super) fn pc_pickup(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let Some(a) = actor(w, id, "energy") else {
        return;
    };
    let total: i64 = w
        .room
        .objects
        .get(&id)
        .and_then(pc_data)
        .and_then(|v| v.get("store").cloned())
        .and_then(|s| s.as_object().cloned())
        .map(|s| s.values().filter_map(Value::as_i64).sum())
        .unwrap_or(0);
    if total >= a.cap {
        return;
    }
    let Some(RoomObject::Energy(target)) = w.room.objects.get(&target_id) else {
        return;
    };
    let slot_resource = match crate::settle::energy_resource_name(target) {
        "power" => return,
        name => name.to_string(),
    };
    if !w.plane.resources.contains(&slot_resource) {
        return;
    }
    let (tx, ty, amount) = (target.x, target.y, target.amount as i64);
    if (tx as i32 - a.x as i32).abs() > 1 || (ty as i32 - a.y as i32).abs() > 1 {
        return;
    }
    let amount = amount.min(a.cap - total);
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
    change_pc_store(w, id, &slot_resource, amount);
}
