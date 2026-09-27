//! 资源经济 tick：source 重生（sources/tick.js）、掉落能量衰减（energy/tick.js）、
//! controller 降级计时（controllers/tick.js）。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::Working;

/// 官方 sources/tick.js：300 tick 重生钟 + 容量按控制器状态归一。
pub fn source_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Source(s)) = w.room.objects.get(&id) else {
        return;
    };
    let (energy, capacity, regen) = (s.energy, s.energy_capacity, s.next_regeneration_time);
    if energy < capacity {
        let regen = match regen {
            None => {
                // 官方：首见未满即挂钟 gameTime + ENERGY_REGEN_TIME
                let t = w.time + c::ENERGY_REGEN_TIME as u64;
                if let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id) {
                    s.next_regeneration_time = Some(t);
                }
                t
            }
            Some(t) => t,
        };
        if w.time >= regen.saturating_sub(1)
            && let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id)
        {
            s.next_regeneration_time = None;
            s.energy = s.energy_capacity;
        }
    }
    // 容量归一（sources/tick.js:38-46；M1 无 reservation）
    let owned = w.controller().is_some_and(|ctl| ctl.owner.is_some());
    let target_cap = match w.controller() {
        None => c::SOURCE_ENERGY_KEEPER_CAPACITY as u32,
        Some(_) if owned => c::SOURCE_ENERGY_CAPACITY as u32,
        Some(_) => c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u32,
    };
    if capacity != target_cap
        && let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id)
    {
        s.energy_capacity = target_cap;
        if !owned {
            s.energy = s.energy.min(target_cap);
        }
    }
}

/// 官方 energy/tick.js：每 tick 衰减 ceil(amount/ENERGY_DECAY)，≤0 移除。
pub fn energy_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Energy(e)) = w.room.objects.get(&id) else {
        return;
    };
    let decay = ((e.amount as i64) + c::ENERGY_DECAY - 1) / c::ENERGY_DECAY;
    let new_amount = e
        .amount
        .saturating_sub(decay.clamp(0, u32::MAX as i64) as u32);
    if new_amount == 0 {
        w.remove_obj(id);
    } else if let Some(RoomObject::Energy(e)) = w.room.objects.get_mut(&id) {
        e.amount = new_amount;
    }
}

/// 官方 controllers/tick.js 的 M1 子集：降级计时链（初始化/升级刷新/降级）。
pub fn controller_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
    let (level, progress, downgrade_time, owner) =
        (t.level, t.progress, t.downgrade_time, t.owner.clone());
    let Some(owner) = owner else { return };
    if owner == "2" {
        return; // invader（M1 不会出现，保持官方分支形态）
    }
    // 无钟初始化（controllers/tick.js:39-41）
    let Some(mut dt) = downgrade_time else {
        let t0 = w.time + controller_downgrade(level) + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(t0);
        }
        return;
    };
    // 本 tick 有升级 → 刷新（controllers/tick.js:50-55）
    if w.upgraded.get(&id).is_some_and(|u| *u > 0) {
        let refreshed = (dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 + 1)
            .min(w.time + controller_downgrade(level) + 1);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(refreshed);
        }
        return;
    }
    // 降级链（controllers/tick.js:69-105；while 语义处理深度过期）
    while w.time >= dt.saturating_sub(1) && level > 0 {
        // level 在循环中演化——从对象读当前值
        let (mut cur_level, mut cur_progress) = (level, progress);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) {
            cur_level = t.level;
            cur_progress = t.progress;
        }
        let new_level = cur_level - 1;
        if new_level == 0 {
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
                t.level = 0;
                t.progress = 0;
                t.owner = None;
                t.downgrade_time = None;
                t.safe_mode = 0;
                t.safe_mode_available = 0;
            }
            return;
        }
        let add_progress = (controller_levels(new_level) as f64 * 0.9).round() as u64;
        dt += controller_downgrade(new_level) / 2 + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.level = new_level;
            t.progress = cur_progress + add_progress;
            t.downgrade_time = Some(dt);
            t.safe_mode_available = 0;
        }
    }
}

fn controller_levels(level: u8) -> i64 {
    let key = level.to_string();
    kk_core::constants::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    kk_core::constants::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}
