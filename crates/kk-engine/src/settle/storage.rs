//! storage 对象 tick 的权威容量更新；查询和转入只读取保存值。
use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::{Working, controller_allowed, powers};

/// 在意图相之后按控制器归属/RCL 与严格有效的 power 更新容量。
/// 无控制器时保留原值；截止拍意图仍消费上轮保存的加成容量。
/// 来源：engine/src/processor/intents/storages/tick.js。
pub(super) fn tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Storage(storage)) = w.room.objects.get(&id) else {
        return;
    };
    let Some(controller) = w.controller() else {
        return;
    };
    let base = if controller.level > 0
        && controller.owner.as_deref() == Some(storage.owner.as_str())
        && controller_allowed("storage", i64::from(controller.level)) > 0
    {
        c::STORAGE_CAPACITY
    } else {
        0
    };
    let capacity = if base > 0 {
        base + kk_world::effects::active_power(
            w.room.effects.get(&id),
            c::PWR_OPERATE_STORAGE,
            w.time,
        )
        .and_then(|effect| powers::power_effect(c::PWR_OPERATE_STORAGE, effect.level))
        .unwrap_or(0)
    } else {
        0
    };
    if let Some(RoomObject::Storage(storage)) = w.room.objects.get_mut(&id) {
        storage.store_capacity = capacity;
    }
}
