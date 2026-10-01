//! 结算 creep 对控制器的升级、占领、预定、攻击、签名和安全模式补充。
//!
//! 升级使用三格范围，其他动作使用相邻范围。玩家房间集合到全世界提交时
//! 才更新，签名墙钟使用本轮外部输入。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_ATTACK, ACTION_RESERVE, ACTION_UPGRADE, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;

/// 在三格内升级自家控制器，基础效果决定能耗，强化效果增加进度和 GCL。
/// RCL 8 按本轮累计升级量限额；升级封锁读取对象上的时钟。
/// 来源：上游 engine/src/processor/intents/creeps/upgradeController.js。
pub(super) fn intent_upgrade_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, energy, base_power, boost_incs) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.owner.clone(),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::UPGRADE_CONTROLLER_POWER,
            kk_world::types::boost_increments(
                &c.body,
                BodyKind::Work,
                "work",
                "upgradeController",
                c::UPGRADE_CONTROLLER_POWER,
            ),
        ),
        _ => return,
    };
    let Some(RoomObject::Controller(_)) = w.room.objects.get(&target_id) else {
        return;
    };
    let (ctl_x, ctl_y, ctl_owner, level, progress, upgrade_blocked) =
        match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => (
                t.x,
                t.y,
                t.owner.clone(),
                t.level,
                t.progress,
                t.upgrade_blocked,
            ),
            _ => return,
        };
    if (ctl_x as i32 - pos.0 as i32).abs() > 3 || (ctl_y as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    if level == 0 || ctl_owner.as_deref() != Some(owner.as_str()) {
        return;
    }
    if upgrade_blocked.unwrap_or(0) > w.time {
        return;
    }
    let mut effect = base_power.min(energy);
    let upgraded = *w.upgraded.get(&target_id).unwrap_or(&0);
    if level == 8 {
        let limit = c::CONTROLLER_MAX_UPGRADE_PER_TICK;
        if upgraded >= limit {
            return;
        }
        effect = effect.min(limit - upgraded);
    }
    if effect <= 0 {
        return;
    }
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, effect);
    let new_progress;
    if level < 8 {
        let next_level_progress = controller_levels(level);
        let downgrade_time = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => t.downgrade_time,
            _ => None,
        };
        // 进度足够仍须满足降级时钟的健康窗口，才提升等级。
        // 升级后保留溢出进度，升到 RCL 8 则清零，并获得一次安全模式。
        if progress + boosted as u64 >= next_level_progress
            && downgrade_time.is_some_and(|dt| {
                dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 >= w.time + controller_downgrade(level)
            })
        {
            new_progress = progress + boosted as u64 - next_level_progress;
            *w.upgraded.entry(target_id).or_insert(0) += boosted;
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
                t.level = level + 1;
                t.progress = new_progress;
                if level + 1 == 8 {
                    t.progress = 0;
                }
                t.safe_mode_available += 1;
                t.downgrade_time = Some(w.time + controller_downgrade(level + 1) / 2);
            }
        } else {
            new_progress = progress + boosted as u64;
            *w.upgraded.entry(target_id).or_insert(0) += boosted;
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
                t.progress = new_progress;
            }
        }
    } else {
        *w.upgraded.entry(target_id).or_insert(0) += boosted;
    }
    let player = w.players.entry(owner.clone()).or_default();
    player.gcl += boosted;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy -= effect as u32;
        set_action_log(&mut c.action_log, ACTION_UPGRADE);
    }
    w.push_event(
        c::EVENT_UPGRADE_CONTROLLER,
        id,
        serde_json::json!({ "amount": boosted, "energySpent": effect }),
    );
}

/// 确认目标为相邻控制器；不检查身体部件、出生状态或所有权。
fn controller_target(w: &Working, id: ObjId, data: &IntentData) -> Option<ObjId> {
    let target_id = data.id?;
    let (x, y) = w.creep(id).map(|c| (c.x as i32, c.y as i32))?;
    let target = w.room.objects.get(&target_id)?;
    if !matches!(target, RoomObject::Controller(_)) {
        return None;
    }
    let (tx, ty) = target.pos();
    if (tx as i32 - x).abs() > 1 || (ty as i32 - y).abs() > 1 {
        return None;
    }
    Some(target_id)
}

/// 使用存活 CLAIM 占领 RCL 0 的控制器，拒绝他人的预定，清除原预定和降级时钟。
/// 不模拟 bindUser 或 shardAccess；GCL 限额由已提交的玩家房间集合计算。
pub(super) fn intent_claim_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (level, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.level, t.reservation.clone()),
        _ => return,
    };
    if level > 0 {
        return;
    }
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim))
        .unwrap_or(0);
    if claim_parts == 0 {
        return;
    }
    if reservation.is_some_and(|r| r.user != owner) {
        return;
    }
    // 本轮的先后 claim 都读取上一 tick 的房间数；新集合到发布时才重建。
    let claimed_rooms = w.players.get(&owner).map(|p| p.rooms.len()).unwrap_or(0);
    let needed_gcl = c::GCL_MULTIPLY as f64 * (claimed_rooms as f64).powf(c::GCL_POW);
    let gcl = w.players.get(&owner).map(|p| p.gcl).unwrap_or(0);
    if (gcl as f64) < needed_gcl {
        return;
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.level = 1;
        t.owner = Some(owner);
        t.owner_cleared = false;
        t.progress = 0;
        t.downgrade_time = None;
        t.reservation = None;
    }
}

/// 用存活 CLAIM 延长无主控制器的预定，拒绝他人预定或超出滑动上限的请求。
/// 首次预定以 time+1 为起点。超限时不留下临时预定，区别于上游的内存对象更新。
/// 来源：上游 engine/src/processor/intents/creeps/reserveController.js。
pub(super) fn intent_reserve_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (ctl_owner, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.owner.clone(), t.reservation.clone()),
        _ => return,
    };
    if ctl_owner.is_some() || reservation.as_ref().is_some_and(|r| r.user != owner) {
        return;
    }
    let effect = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64 * c::CONTROLLER_RESERVE)
        .unwrap_or(0);
    if effect == 0 {
        return;
    }
    let base_end = match &reservation {
        Some(r) => r.end_time,
        None => w.time + 1,
    };
    if base_end + effect as u64 > w.time + c::CONTROLLER_RESERVE_MAX as u64 {
        return;
    }
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_RESERVE);
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.reservation = Some(kk_schema::room::Reservation {
            user: owner,
            end_time: base_end + effect as u64,
        });
    }
    w.push_event(
        c::EVENT_RESERVE_CONTROLLER,
        id,
        serde_json::json!({ "amount": effect }),
    );
}

/// 减少预定期限或已占房的降级期限，后者同时登记升级封锁。
/// 他人安全模式和现有封锁会拒绝攻击，现有封锁也限制房主本人。
/// 来源：上游 engine/src/processor/intents/creeps/attackController.js。
pub(super) fn intent_attack_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (ctl_owner, reservation, safe_mode, upgrade_blocked) = match w.room.objects.get(&target_id)
    {
        Some(RoomObject::Controller(t)) => (
            t.owner.clone(),
            t.reservation.clone(),
            t.safe_mode,
            t.upgrade_blocked,
        ),
        _ => return,
    };
    if ctl_owner.is_none() && reservation.is_none() {
        return;
    }
    if (ctl_owner.as_deref() != Some(owner.as_str()) && safe_mode > w.time)
        || upgrade_blocked.unwrap_or(0) > w.time
    {
        return;
    }
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64)
        .unwrap_or(0);
    if let Some(r) = reservation {
        let effect = claim_parts * c::CONTROLLER_RESERVE;
        if effect == 0 {
            return;
        }
        let new_end = r.end_time.saturating_sub(effect as u64);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
            t.reservation = Some(kk_schema::room::Reservation {
                user: r.user,
                end_time: new_end,
            });
        }
    }
    if ctl_owner.is_some() {
        let effect = claim_parts * c::CONTROLLER_CLAIM_DOWNGRADE;
        if effect == 0 {
            return;
        }
        // 未初始化的时钟按 0 处理，减法饱和到 0；不保存上游可能产生的负时钟。
        let new_dt = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => {
                t.downgrade_time.unwrap_or(0).saturating_sub(effect as u64)
            }
            _ => return,
        };
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
            t.downgrade_time = Some(new_dt);
        }
        w.upgrade_block_pending.insert(
            target_id,
            w.time + c::CONTROLLER_ATTACK_BLOCKED_UPGRADE as u64,
        );
    }
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_ATTACK);
    }
    // 此事件保留空 data 对象；上游对应事件没有 data 键。
    w.push_event(c::EVENT_ATTACK_CONTROLLER, id, serde_json::json!({}));
}

/// 保存控制器签名；缺失或空串清除。tick 时间与 Unix 毫秒墙钟分别保存，
/// 墙钟来自 TickInput，重试时由调用方保持。
pub(super) fn intent_sign_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let sign = data
        .sign
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|text| kk_schema::room::Sign {
            user: owner,
            text: text.clone(),
            time: w.time,
            datetime: Some(w.now_ms),
        });
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.sign = sign;
    }
}

/// 消耗足额 G 增加一次安全模式可用次数，不立即激活，也不检查目标所有权。
/// 来源：上游 engine/src/processor/intents/creeps/generateSafeMode.js。
pub(super) fn intent_generate_safe_mode(w: &mut Working, id: ObjId, data: &IntentData) {
    let spawning = w.creep(id).is_some_and(|c| c.spawning);
    if spawning {
        return;
    }
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let ghodium = w.creep(id).map(|c| c.store.ghodium as i64).unwrap_or(0);
    if ghodium < c::SAFE_MODE_COST {
        return;
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.safe_mode_available = t.safe_mode_available.saturating_add(1);
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.ghodium -= c::SAFE_MODE_COST as u32;
    }
}

fn controller_levels(level: u8) -> u64 {
    kk_world::domain::controller_progress_total_opt(level).unwrap_or(0) as u64
}

fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    c::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}
