//! controller 结算族：upgrade/claim/reserve/attack/sign/generateSafeMode
//! （官方 processor/intents/creeps/ 同名文件；等级表与降级钟）。
//! B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_ATTACK, ACTION_RESERVE, ACTION_UPGRADE, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;

/// 官方 upgradeController.js：3 格、归属、8 级上限、升级进位、gcl 计入。
pub(super) fn intent_upgrade_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // 官方两步（upgradeController.js:51-58，GPT 二轮复审 P1-1）：基础量
    // （无强化计数 × POWER）定耗能与效果点数；强化效果按增量数组
    // （UPGRADE_CONTROLLER_POWER=1 ⇒ 增量=(m-1)）
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
    // 官方 upgradeController.js:25-27：`target.upgradeBlocked &&
    // upgradeBlocked > gameTime` 拒——attackController 的 1000 tick 封锁期
    //（B2 起生效；null → 0 > t 恒假，放行）
    if upgrade_blocked.unwrap_or(0) > w.time {
        return;
    }
    let mut effect = base_power.min(energy);
    // 官方 _upgraded 累计（8 级每 tick 上限 15）
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
    // 官方两步尾段（:51-58）：强化效果（boosted）用于 progress/_upgraded/
    // gcl/事件量；能量扣基础量（:58 store.energy -= buildEffect）
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, effect);
    let new_progress;
    if level < 8 {
        let next_level_progress = controller_levels(level);
        let downgrade_time = match w.room.objects.get(&target_id) {
            Some(RoomObject::Controller(t)) => t.downgrade_time,
            _ => None,
        };
        // 官方升级条件（upgradeController.js:63-66）：进位需 downgrade 窗口健康
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
    // gcl 计入玩家（官方 bulkUsers.inc(target.user, 'gcl', boostedEffect)）
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

// ---- B2：controller 结算族（官方 processor/intents/creeps/ 同名文件） ----

/// controller 族公共预检：目标存在、是 controller、Chebyshev 邻接 ≤ 1
/// （claimController.js:15-21 / reserveController.js:19-24 /
/// attackController.js:15-20 / signController.js:14-18 /
/// generateSafeMode.js:11-16 五处同形）。返回目标 id。
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

/// 官方 claimController.js：夺主。0 级无主（或仅被自己预定）的 controller，
/// 带存活 CLAIM 部件的 creep 邻接claim → level=1、owner 换人、progress 清零、
/// downgradeTime/reservation 清空。
pub(super) fn intent_claim_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // claimController.js:8-13：非 creep 拒（分派层保证）、spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :15-21：目标 + 邻接（公共预检）
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (level, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.level, t.reservation.clone()),
        _ => return,
    };
    // :22-24：`target.bindUser && object.user != target.bindUser` 拒——bindUser
    // 在官方 OSS 引擎全库无写入点（仅 hosted 后端 backend-npm 读取），私有服
    // 世界不可达，不建模
    // :25-27：level > 0 拒（已占房不可夺，unclaim/降级才回 0 级）
    if level > 0 {
        return;
    }
    // :28-30：存活 CLAIM 部件数为零拒
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim))
        .unwrap_or(0);
    if claim_parts == 0 {
        return;
    }
    // :31-33：他人预定拒（自己的预定不拦，:52 落地时清掉）
    if reservation.is_some_and(|r| r.user != owner) {
        return;
    }
    // :34-43：GCL 门。shardAccess（:37-39）是 intershard 门槛，单分片恒有——
    // N/A。claimedRooms 读 user.rooms（官方 bulk addToSet 不改内存对象
    // （bulk.js:101-107）——读到上一 tick 定集，我方 PlayerState.rooms 同形）；
    // calcNeededGcl（utils.js:661-663）= GCL_MULTIPLY × (n-1)^GCL_POW
    let claimed_rooms = w.players.get(&owner).map(|p| p.rooms.len()).unwrap_or(0);
    let needed_gcl = c::GCL_MULTIPLY as f64 * (claimed_rooms as f64).powf(c::GCL_POW);
    let gcl = w.players.get(&owner).map(|p| p.gcl).unwrap_or(0);
    if (gcl as f64) < needed_gcl {
        return;
    }
    // :45-53：落地。addRoomToUser（:55）经 tick 末 rooms 重建承载
    // （settle/mod.rs PlayerState 文档）
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.level = 1;
        t.owner = Some(owner);
        // 重新占领清除"曾拥有"哨兵（claimController.js:45-53 只写 user）
        t.owner_cleared = false;
        t.progress = 0;
        t.downgrade_time = None;
        t.reservation = None;
    }
}

/// 官方 reserveController.js：无主房预定。CONTROLLER_RESERVE(1)/CLAIM 部件/
/// tick，上限 CONTROLLER_RESERVE_MAX(5000)（相对 gameTime 的滑动窗）。
pub(super) fn intent_reserve_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // reserveController.js:8-13：spawning 拒；:15-17 shardAccess 单分片 N/A
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :19-24：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    let (ctl_owner, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.owner.clone(), t.reservation.clone()),
        _ => return,
    };
    // :26-28：`target.user || (target.reservation && reservation.user !=
    // object.user)` 拒——已占房不可预定（房主自己也不行）；他人预定不可覆盖
    if ctl_owner.is_some() || reservation.as_ref().is_some_and(|r| r.user != owner) {
        return;
    }
    // :30-33：effect = 存活 CLAIM × CONTROLLER_RESERVE；零功率拒（无事件）
    let effect = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64 * c::CONTROLLER_RESERVE)
        .unwrap_or(0);
    if effect == 0 {
        return;
    }
    // :35-40：无预约先建 {user, endTime: gameTime+1}（仅内存值，未持久化）
    let base_end = match &reservation {
        Some(r) => r.end_time,
        None => w.time + 1,
    };
    // :43-45：滑窗上限：endTime + effect > gameTime + CONTROLLER_RESERVE_MAX 拒。
    // 官方在内存对象上先建了预约再判上限——被拒的"幻影预约"仅本 tick 后续
    // 处理器可见、永不入库；我方工作副本即持久层（D4-2 灰区），按整票拒绝
    // （不落任何预约状态）
    if base_end + effect as u64 > w.time + c::CONTROLLER_RESERVE_MAX as u64 {
        return;
    }
    // :47：actionLog.reserveController（官方独立键，位图 bit8）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_RESERVE);
    }
    // :49-50：endTime += effect 并持久化
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.reservation = Some(kk_schema::room::Reservation {
            user: owner,
            end_time: base_end + effect as u64,
        });
    }
    // :52：EVENT_RESERVE_CONTROLLER {amount: effect}
    w.push_event(
        c::EVENT_RESERVE_CONTROLLER,
        id,
        serde_json::json!({ "amount": effect }),
    );
}

/// 官方 attackController.js：预定削减 + 占房 downgrade 挂钟后移 +
/// 升级封锁（CONTROLLER_ATTACK_BLOCKED_UPGRADE=1000 tick）。
pub(super) fn intent_attack_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // attackController.js:8-13：spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :15-20：目标 + 邻接
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
    // :22-24：原始房（无主无预定）不可攻
    if ctl_owner.is_none() && reservation.is_none() {
        return;
    }
    // :25-28：`(roomController.user != object.user && safeMode > gameTime) ||
    // upgradeBlocked > gameTime` 拒——他人 safeMode 生效期不可攻；
    // upgradeBlocked 对任何攻击者（含房主自己）生效。roomController 即目标
    // （同房唯一 controller）
    if (ctl_owner.as_deref() != Some(owner.as_str()) && safe_mode > w.time)
        || upgrade_blocked.unwrap_or(0) > w.time
    {
        return;
    }
    // :29-31：EFFECT_INVULNERABILITY 免疫属 power 体系（M4）——N/A
    // 存活 CLAIM 部件（:34/:42 两分支共用）
    let claim_parts = w
        .creep(id)
        .map(|c| BodyKind::count_active(&c.body, BodyKind::Claim) as i64)
        .unwrap_or(0);
    // :33-40：预定削减分支——effect = floor(CLAIM × CONTROLLER_RESERVE)
    //（整数常量下 floor 恒等）；endTime -= effect（官方 bulk update 为
    // merge，user 保留）
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
    // :41-49：占房分支——effect = floor(CLAIM × CONTROLLER_CLAIM_DOWNGRADE)；
    // downgradeTime -= effect；_upgradeBlocked = gameTime + 1000（瞬态，
    // controller tick 持久化）。downgradeTime 未挂钟（新 claim 的房）时官方
    // null − effect 得负数 → 下 tick 降级链即刻触发；u64 饱和到 0 同 observable
    //（两者都 ≤ gameTime−1，触发条件同真）
    if ctl_owner.is_some() {
        let effect = claim_parts * c::CONTROLLER_CLAIM_DOWNGRADE;
        if effect == 0 {
            return;
        }
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
    // :50：actionLog.attack（与近战/dismantle 共用官方键）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_ATTACK);
    }
    // :52：EVENT_ATTACK_CONTROLLER——官方事件无 data 键；我方事件封套恒带
    // data（push_event 形态），空对象差异属投影层可见的既有形态（登记报告）
    w.push_event(c::EVENT_ATTACK_CONTROLLER, id, serde_json::json!({}));
}

/// 官方 signController.js：签名落库 / 空签名清除。
pub(super) fn intent_sign_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    // signController.js:8-13：spawning 拒
    let owner = match w.creep(id) {
        Some(c) if !c.spawning => c.owner.clone(),
        _ => return,
    };
    // :14-18：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    // :23-28：`intent.sign ? {user, text: intent.sign, time: gameTime,
    // datetime: Date.now()} : null`——userString 清洗下缺省与空串同为 ""
    //（falsy），都落清除
    let sign = data
        .sign
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|text| kk_schema::room::Sign {
            user: owner,
            text: text.clone(),
            time: w.time,
            // 官方 signController.js:23：datetime 落一次 Date.now() 毫秒墙钟
            //（玩家可见的墙钟语义不变）。确定性重放经 TickInput 注入：首跑
            // 取实际值、重放用记录值（ADR-013 决策 5）——官方自身无此通道，
            // replay 不确定性是官方既有行为
            datetime: Some(w.now_ms),
        });
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.sign = sign;
    }
}

/// 官方 generateSafeMode.js：消耗 1000 ghodium 为 controller 充一枚
/// safeModeAvailable。激活（消耗 available 设 safeMode 时钟）是另一半——
/// controller 对象的 activateSafeMode intent（settle/controller.rs）。
pub(super) fn intent_generate_safe_mode(w: &mut Working, id: ObjId, data: &IntentData) {
    // generateSafeMode.js:8-10：spawning 拒（本文件无 type 检查——分派层保证）
    let spawning = w.creep(id).is_some_and(|c| c.spawning);
    if spawning {
        return;
    }
    // :11-16：目标 + 邻接
    let Some(target_id) = controller_target(w, id, data) else {
        return;
    };
    // :19-21：`!object.store || !(store[RESOURCE_GHODIUM] >= SAFE_MODE_COST)`
    // 拒——store 缺失/ghodium 不足皆拒（无部分消耗）
    let ghodium = w.creep(id).map(|c| c.store.ghodium as i64).unwrap_or(0);
    if ghodium < c::SAFE_MODE_COST {
        return;
    }
    // :23-24：available += 1（官方 `(target.safeModeAvailable || 0) + 1`）；
    // store.G -= SAFE_MODE_COST
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.safe_mode_available = t.safe_mode_available.saturating_add(1);
    }
    if let Some(c) = w.creep_mut(id) {
        c.store.ghodium -= c::SAFE_MODE_COST as u32;
    }
}

fn controller_levels(level: u8) -> u64 {
    let key = level.to_string();
    c::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}

fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    c::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}
