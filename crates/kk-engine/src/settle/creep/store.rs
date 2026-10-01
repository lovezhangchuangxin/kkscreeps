//! 资源族：transfer/withdraw/drop/pickup 与掉落原语（官方同名 intent
//! 文件；drop 原语被 harvest 超容与 dismantle 回流共用）。
//! B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::{
    creep_slot, slots_add, slots_sub, store_capacity, store_slot, store_slot_add, store_slot_sub,
};
use crate::intent::IntentData;
use crate::settle::Working;

/// 官方 transfer.js：RESOURCES_ALL 白名单（:8-11）、邻接、容量双形态
/// （B4 起命名矿物槽经 creep/storage 的 store_extra 承载）。
pub(super) fn intent_transfer(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 transfer.js:8-11：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    // + 持有不足/负数拒（官方白名单含 power——power 无进 creep 的建模路径，
    // 放行面与官方逐字一致、不可达性由 pickup 门保证，见 intent_pickup）
    let resource = data.resource_type.as_deref().unwrap_or("");
    if !c::RESOURCES_ALL.contains(&resource) {
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
    // extension 容量按 RCL（官方 extensions/tick.js:10-17 归一
    // storeCapacityResource.energy，transfer.js:24-39 经 capacityForResource 钳制；
    // 无 controller ⇒ 官方 capacityForResource 得 undefined ⇒ `!targetCapacity` 拒）
    let extension_cap = || {
        w.controller()
            .map(|ctl| kk_world::types::extension_energy_capacity(ctl.level))
            .unwrap_or(0)
    };
    // 容量按资源分岔（官方 utils.capacityForResource，utils.js:671-675）：
    // 通用仓（creep/storage）数值容量全槽共用；per-resource 商店（spawn/
    // extension 的 scr.energy）对非 energy 得 0 ⇒ `!targetCapacity` 拒
    let per_resource_cap = |base: i64| if resource == "energy" { base } else { 0 };
    let (target_cap, t_pos, t_stored) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Creep(t)) if !t.spawning => (
            store_capacity(t),
            (t.x, t.y),
            // 官方 transfer.js:32：通用容量目标按 calcResources 全槽求和
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
            c::STORAGE_CAPACITY,
            (t.x, t.y),
            // 同 creep：全槽求和（GPT 二轮复审 P1-2/评审交叉确认）
            kk_world::types::store_total(&t.store, &t.store_extra),
        ),
        _ => return,
    };
    if (t_pos.0 as i32 - pos.0 as i32).abs() > 1 || (t_pos.1 as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    if t_stored >= target_cap {
        return;
    }
    let mut amount = amount_req.min(held);
    if t_stored + amount > target_cap {
        amount = target_cap - t_stored;
    }
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

/// 官方 withdraw.js：RESOURCES_ALL 白名单、空容量、safeMode/rampart 保护
/// （rampart 变体随对应批；目标白名单 spawn/extension/storage——B3 登记，
/// tombstone/ruin 目标随轨 B 后续）。
pub(super) fn intent_withdraw(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 withdraw.js:8-11：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !c::RESOURCES_ALL.contains(&resource) {
        return;
    }
    let Some(target_id) = data.id else { return };
    // actor 的空容量按 store 全槽求和（官方 calcResources）——含命名矿物槽
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
    // 可提取对象：有 store 的结构（spawn/extension/storage）+ Tombstone/Ruin
    //（官方预检 creeps.js:507-515 显式接受两类目标，withdraw.js:21 按目标
    // store 扣账——GPT 复审 P1-2 清偿：原仅三结构，tombstone/ruin 提取
    // API 返回 OK 结算静默丢弃）；source 只能 harvest、掉落资源只能 pickup
    //（官方引擎层无 source/energy 的 withdraw 路径）。命名矿物槽一并可提
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
        _ => return,
    }
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, resource, amount);
    }
    // 官方 withdraw 的 EVENT_TRANSFER 反向（objectId=target, data.targetId=object）
    w.push_event(
        c::EVENT_TRANSFER,
        target_id,
        serde_json::json!({ "targetId": id.as_str(), "resourceType": resource, "amount": amount }),
    );
}

/// 官方 drop.js：amount>0 落地为同格资源堆；持有不足整条拒绝（:13
/// `!(store[resourceType] >= intent.amount)` ⇒ return，不钳制部分掉落）。
pub(super) fn intent_drop(w: &mut Working, id: ObjId, data: &IntentData) {
    // 官方 drop.js:10：`_.contains(C.RESOURCES_ALL, resourceType)` 白名单
    //（B4 起命名矿物可掉——creep 的 store_extra 槽；掉落对象经命名槽承载）
    let resource = data.resource_type.as_deref().unwrap_or("energy");
    if !c::RESOURCES_ALL.contains(&resource) {
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

/// 掉落能量（drop.js 落地路径；dismantle 回流超容与 harvest 超容共用）。
pub(in crate::settle) fn intent_drop_amount(w: &mut Working, id: ObjId, amount: i64) {
    // 内部调用方（dismantle 回流/harvest 超容）恒为 energy 槽
    intent_drop_amount_res(w, id, "energy", amount);
}

/// 官方 drop.js 落地路径（:24 `_create-energy(amount, resourceType)`——
/// 同格 container 先承接 + 同格同资源并堆，B3 的 create_resource 全量同式；
/// 衰减对全资源同式（官方 energy/tick.js:12 按 resourceType 键通用）。
/// B4 起资源参数化到任意 RESOURCES_ALL 名（命名矿物掉落为命名堆）。
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
    // 钳制仅服务内部调用方（harvest 超容路径已保证 held ≥ amount）
    let amount = amount.min(held);
    if amount <= 0 {
        return;
    }
    if let Some(c) = w.creep_mut(id) {
        slots_sub(&mut c.store, &mut c.store_extra, resource, amount);
    }
    w.create_resource(pos.0, pos.1, amount, resource);
}

/// 官方 pickup.js：邻接 energy、余量钳制、目标清零移除。
pub(super) fn intent_pickup(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // 官方 pickup.js:9：carry = calcResources(object)（全槽求和）——含命名
    // 矿物槽（GPT 二轮复审 P1-3 的全资源口径，B4 落地）
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
    // 官方 pickup.js:23-30：按堆的 resourceType 入开放 store 对应键——B4 起
    // 命名矿物堆放行入槽（B3 的"矿物堆拒拾"登记随本批清偿）；power 堆维持
    // 拒绝（power 无建模槽位，随 power 批放开——B3 登记口径不变）。判定走
    // energy_resource_name 单源——code 与命名槽双态都折算后再放行
    let slot_resource = match super::energy_resource_name(target) {
        "power" => return,
        name => name.to_string(),
    };
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
