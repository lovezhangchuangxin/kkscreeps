//! 已绑定房间候选中的资源读取与精确转账；不派发动作或执行 JS。
//!
//! 不分配 ID、不移动或删除对象；两对象副本预检成功后一起写入并追加事件。
//! 调用方负责能力范围与正常拒绝的局部恢复。默认动作不经过此 API。
pub(in crate::settle) mod generic;

use kk_core::constants as c;
use kk_mod_api::resources::{ResourceTransfer, ResourceView};
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::{Working, creep};
use generic::{change_generic_store, generic_store};

const MAX_SAFE: i64 = 9_007_199_254_740_991;

fn quantity(n: i64) -> Result<i64, String> {
    if (0..=MAX_SAFE).contains(&n) {
        Ok(n)
    } else {
        Err("资源数量须为非负 JS 安全整数".into())
    }
}

fn total(mut values: impl Iterator<Item = i64>) -> Result<i64, String> {
    values.try_fold(0_i64, |sum, value| {
        quantity(value).and_then(|value| quantity(sum + value))
    })
}

/// 读取房间候选中的库存；只支持现有资源名与已接线的仓库形态。
pub fn inspect(w: &Working, id: ObjId, resource: &str) -> Result<ResourceView, String> {
    if !c::RESOURCES_ALL.contains(&resource) {
        return Err("未接线的资源类型".into());
    }
    let object = w
        .room
        .objects
        .get(&id)
        .ok_or("资源对象不在本次房间候选中")?;
    let fixed = |amount: i64, capacity: i64| view(amount, Some(capacity), amount);
    let shared = |store: &kk_schema::room::Store, extra: &[kk_schema::room::ResourceStack], cap| {
        let used = total(
            std::iter::once(i64::from(store.energy))
                .chain(std::iter::once(i64::from(store.ghodium)))
                .chain(extra.iter().map(|slot| i64::from(slot.amount))),
        )?;
        view(
            kk_world::types::store_extra_slot(store, extra, resource),
            cap,
            used,
        )
    };
    let energy_cap = |cap| if resource == "energy" { cap } else { 0 };
    match object {
        RoomObject::Creep(o) if !o.spawning => {
            shared(&o.store, &o.store_extra, Some(creep::store_capacity(o)))
        }
        RoomObject::Spawn(o) => fixed(
            if resource == "energy" {
                i64::from(o.store.energy)
            } else {
                0
            },
            energy_cap(c::SPAWN_ENERGY_CAPACITY),
        ),
        RoomObject::Extension(o) => fixed(
            if resource == "energy" {
                i64::from(o.store.energy)
            } else {
                0
            },
            energy_cap(
                w.controller()
                    .map(|ctl| kk_world::types::extension_energy_capacity(ctl.level))
                    .unwrap_or(0),
            ),
        ),
        RoomObject::Storage(o) => shared(&o.store, &o.store_extra, Some(c::STORAGE_CAPACITY)),
        RoomObject::Tombstone(o) => shared(&o.store, &o.store_extra, None),
        RoomObject::Ruin(o) => shared(&o.store, &o.store_extra, None),
        RoomObject::Generic(o) => {
            let value = generic_store(&o.data).ok_or("Generic 库存或容量记录非法")?;
            let store = value["store"].as_object().expect("已校验库存");
            // 先验证全部库存，避免无关大数或负数被共享容量忽略。
            total(store.values().map(|v| v.as_i64().expect("已校验库存")))?;
            let slots = value["storeCapacityResource"].as_object();
            let reserved = total(
                slots
                    .into_iter()
                    .flat_map(|s| s.values())
                    .filter_map(Value::as_i64),
            )?;
            let amount = store.get(resource).and_then(Value::as_i64).unwrap_or(0);
            let shared_capacity = match value.get("storeCapacity") {
                None | Some(Value::Null) => 0,
                Some(v) => quantity(v.as_i64().ok_or("共享容量须为非负整数")?)?,
            };
            let explicit = slots.and_then(|s| s.get(resource)).and_then(Value::as_i64);
            let (capacity, used) = if let Some(cap) = explicit {
                (cap, amount)
            } else {
                let cap = shared_capacity.saturating_sub(reserved).max(0);
                // 未绑定的资源共用剩余容量；独立资源槽的库存不重复占用它。
                let used = total(
                    store
                        .iter()
                        .filter(|(name, _)| {
                            slots
                                .and_then(|s| s.get(*name))
                                .and_then(Value::as_i64)
                                .is_none()
                        })
                        .map(|(_, v)| v.as_i64().expect("已校验库存")),
                )?;
                (cap, used)
            };
            view(amount, Some(capacity), used)
        }
        _ => Err("对象没有已接线的可用资源仓库".into()),
    }
}

fn view(amount: i64, capacity: Option<i64>, used: i64) -> Result<ResourceView, String> {
    let amount = quantity(amount)?;
    let capacity = capacity.map(quantity).transpose()?;
    Ok(ResourceView {
        amount,
        capacity,
        free: capacity.map(|cap| cap.saturating_sub(used).max(0)),
    })
}

/// 精确扣除 amount，到账 amount-loss；条件不足返回零结果，避免部分损耗。
/// Mod 决定距离、归属及业务策略；这里只维护同房库存、容量和数值不变量。
pub fn transfer(
    w: &mut Working,
    from: ObjId,
    to: ObjId,
    resource: &str,
    amount: i64,
    loss: i64,
) -> Result<ResourceTransfer, String> {
    quantity(amount)?;
    quantity(loss)?;
    if from == to || loss > amount {
        return Err("转账两端须不同，损耗不得超过扣除量".into());
    }
    let source = inspect(w, from, resource)?;
    let target = inspect(w, to, resource)?;
    let free = target.free.ok_or("墓碑与废墟只能作为资源来源")?;
    let credited = amount - loss;
    if source.amount < amount || credited > free || amount == 0 {
        return Ok(ResourceTransfer::default());
    }
    let mut source = w.room.objects[&from].clone();
    let mut target = w.room.objects[&to].clone();
    change(&mut source, resource, -amount, &w.plane.stores)?;
    if credited > 0 {
        change(&mut target, resource, credited, &w.plane.stores)?;
    }
    // 所有可失败的检查已在副本上完成，以下没有半条转账的返回路径。
    w.room.objects.insert(from, source);
    w.room.objects.insert(to, target);
    let mut data = json!({"targetId":to.as_str(),"resourceType":resource,"amount":credited});
    if loss > 0 {
        data["debited"] = amount.into();
        data["loss"] = loss.into();
    }
    w.push_event(c::EVENT_TRANSFER, from, data);
    Ok(ResourceTransfer {
        debited: amount,
        credited,
        lost: loss,
    })
}

fn change(
    object: &mut RoomObject,
    resource: &str,
    delta: i64,
    stores: &kk_mod_api::builtin::StoreRules,
) -> Result<(), String> {
    let update = |store: &mut kk_schema::room::Store,
                  extra: &mut Vec<kk_schema::room::ResourceStack>| {
        let held = kk_world::types::store_extra_slot(store, extra, resource);
        if !(0..=i64::from(u32::MAX)).contains(&(held + delta)) {
            return Err("类型化资源槽超出 u32 范围".into());
        }
        if delta > 0 {
            creep::slots_add(store, extra, resource, delta);
        } else {
            creep::slots_sub(store, extra, resource, -delta);
        }
        Ok(())
    };
    match object {
        RoomObject::Creep(o) => update(&mut o.store, &mut o.store_extra),
        RoomObject::Storage(o) => update(&mut o.store, &mut o.store_extra),
        RoomObject::Tombstone(o) => update(&mut o.store, &mut o.store_extra),
        RoomObject::Ruin(o) => update(&mut o.store, &mut o.store_extra),
        RoomObject::Spawn(o) => update(&mut o.store, &mut Vec::new()),
        RoomObject::Extension(o) => update(&mut o.store, &mut Vec::new()),
        RoomObject::Generic(o) => {
            change_generic_store(&mut o.data, resource, delta, stores);
            // 已超容的其他资源也可能占满安全整数范围；独立槽有余量
            // 不代表写后合计仍可表示。副本上的检查失败不能扣除来源。
            let value = generic_store(&o.data).ok_or("写后 Generic 库存非法")?;
            total(
                value["store"]
                    .as_object()
                    .expect("已校验库存")
                    .values()
                    .map(|slot| slot.as_i64().expect("已校验库存")),
            )?;
            Ok(())
        }
        _ => Err("资源对象类型未接线".into()),
    }
}

#[cfg(test)]
mod tests;
