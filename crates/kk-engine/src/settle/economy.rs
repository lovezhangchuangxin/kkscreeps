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
    // 容量归一（sources/tick.js:40-50）：有 controller 的房按
    // `user || reservation` 判 3000（SOURCE_ENERGY_CAPACITY），无主无预定
    // 1500（SOURCE_ENERGY_NEUTRAL_CAPACITY）——B2 补 reservation 分支；
    // 无 controller 4000（KEEPER）。过期未清的预约在官方同样使容量抬升
    //（tick.js 只判 truthiness，清除发生在 controllers/tick——两 tick 的
    // 相对次序官方按对象迭代序不定，我方按 id 序确定，同属 D4-1 灰区）
    let ctl = w.controller();
    let target_cap = match ctl {
        None => c::SOURCE_ENERGY_KEEPER_CAPACITY as u32,
        Some(ctl) if ctl.owner.is_some() || ctl.reservation.is_some() => {
            c::SOURCE_ENERGY_CAPACITY as u32
        }
        Some(_) => c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u32,
    };
    if capacity != target_cap
        && let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id)
    {
        s.energy_capacity = target_cap;
        // 官方只在降到 NEUTRAL 的分支钳 energy（:43-45），抬升分支不钳
        if target_cap == c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u32 {
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

/// 官方 controllers/tick.js（B2 补齐版）：预定过期清除、upgradeBlocked 持久化、
/// safeMode 激活消费、降级计时链（初始化/升级刷新/降级）。通知
/// （:45-47/:51 sendNotification）无通知体系——N/A；addRoomToUser（:18）由
/// Engine 每 tick 的 rooms 重建承载（settle/mod.rs PlayerState 文档）。
pub fn controller_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
    let (level, progress, downgrade_time, owner) =
        (t.level, t.progress, t.downgrade_time, t.owner.clone());
    // tick.js:10-12：预定到期（gameTime >= endTime-1）或房被占 → 清除。
    // 位于无主早退（:14）**之前**——无主房的预定同样在此过期
    if let Some(r) = &t.reservation
        && (w.time >= r.end_time.saturating_sub(1) || owner.is_some())
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.reservation = None;
    }
    // tick.js:14-16：无主（或 Source Keeper "2"）房到此为止
    let Some(owner) = owner else { return };
    if owner == "2" {
        return; // invader（M1 不会出现，保持官方分支形态）
    }
    // tick.js:20-23：attackController 的瞬态 _upgradeBlocked 持久化
    if let Some(blocked_until) = w.upgrade_block_pending.get(&id).copied()
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.upgrade_blocked = Some(blocked_until);
    }
    let blocked_now = w
        .room
        .objects
        .get(&id)
        .and_then(|o| match o {
            RoomObject::Controller(t) => t.upgrade_blocked,
            _ => None,
        })
        .unwrap_or(0);
    // tick.js:25-31：_safeModeActivated 且未被 upgradeBlocked 压制 → 消费一枚
    // available、挂 safeMode 时钟（SAFE_MODE_DURATION）与冷却
    //（SAFE_MODE_COOLDOWN；roomInfo.novice 新手区免冷却——我方无新手区，
    // 恒挂钟）。读取的 upgradeBlocked 含上一步刚持久化的同 tick 攻击——
    // 官方 :20-23 先写 :25 再读的同序语义
    if w.safe_mode_activated.contains(&id)
        && blocked_now <= w.time
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.safe_mode_available = t.safe_mode_available.saturating_sub(1);
        t.safe_mode = w.time + c::SAFE_MODE_DURATION as u64;
        t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
    }
    // 无钟初始化（controllers/tick.js:33-36）
    let Some(mut dt) = downgrade_time else {
        let t0 = w.time + controller_downgrade(level) + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(t0);
        }
        return;
    };
    // 本 tick 有升级 → 刷新（controllers/tick.js:38-43；upgradeBlocked 期不刷
    //——封锁下升级 intent 本就被拒（upgradeController.js:25-27），_upgraded
    // 不会累计，此处的官方防御性条件在此同形保留）
    if w.upgraded.get(&id).is_some_and(|u| *u > 0) && blocked_now <= w.time {
        let refreshed = (dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 + 1)
            .min(w.time + controller_downgrade(level) + 1);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(refreshed);
        }
        return;
    }
    // 降级链（controllers/tick.js:49-82；while 语义处理深度过期）
    while w.time >= dt.saturating_sub(1) && level > 0 {
        // level 在循环中演化——从对象读当前值
        let (mut cur_level, mut cur_progress) = (level, progress);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) {
            cur_level = t.level;
            cur_progress = t.progress;
        }
        let new_level = cur_level - 1;
        if new_level == 0 {
            // tick.js:52-63：归零——去主权、清 progress/downgradeTime/
            // upgradeBlocked/safeMode、available 归零、挂冷却、关 power；
            // removeRoomFromUser（:53）由 Engine rooms 重建承载
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
                t.level = 0;
                t.progress = 0;
                t.owner = None;
                t.downgrade_time = None;
                t.upgrade_blocked = None;
                t.safe_mode = 0;
                t.safe_mode_available = 0;
                t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
                t.is_power_enabled = false;
            }
            return;
        }
        // tick.js:64-69：降一级——挂钟 += DOWNGRADE[新级]/2+1、进度回退
        // round(LEVELS[新级]×0.9)、available 清零、挂冷却
        let add_progress = (controller_levels(new_level) as f64 * 0.9).round() as u64;
        dt += controller_downgrade(new_level) / 2 + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.level = new_level;
            t.progress = cur_progress + add_progress;
            t.downgrade_time = Some(dt);
            t.safe_mode_available = 0;
            t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
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

pub fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    kk_core::constants::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}
