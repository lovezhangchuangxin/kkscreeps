//! 结算能量源和矿物的采集，按身体部件和 boost 计算产出。
//!
//! 采集先写入对应资源槽，再按全部资源计算超量掉落；矿物采集额外登记提取器冷却。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::intent::IntentData;
use crate::settle::Working;

use super::store::{intent_drop_amount, intent_drop_amount_res};
use super::{ACTION_HARVEST, set_action_log, slots_add, store_capacity};

/// 检查已出生和相邻条件，再分派 source、mineral 或 deposit。
/// 来源：上游 engine/src/processor/intents/creeps/harvest.js。
pub(super) fn intent_harvest(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let creep_pos = w.creep(id).map(|c| (c.x, c.y)).unwrap_or((0, 0));
    let (tx, ty) = target.pos();
    if (tx as i32 - creep_pos.0 as i32).abs() > 1 || (ty as i32 - creep_pos.1 as i32).abs() > 1 {
        return;
    }
    match target {
        RoomObject::Source(_) => intent_harvest_source(w, id, target_id),
        RoomObject::Mineral(_) => intent_harvest_mineral(w, id, target_id),
        RoomObject::Generic(g)
            if kk_world::domain::generic::type_name(&g.data).as_deref() == Some("deposit") =>
        {
            intent_harvest_deposit(w, id, target_id)
        }
        _ => {}
    }
}

/// 采集 deposit：冷却中静默拒绝；产出 WORK×HARVEST_DEPOSIT_POWER（含
/// harvest 类 boost）直接入仓并超容掉落；累计 harvested 决定下一轮冷却
/// ceil(0.001×harvested^1.2)（>1 才挂，经对象 tick 相落成 cooldownTime，
/// 同轮后续采集读旧值），每次采集刷新寿命 decayTime=time+50000。
/// 不发 EVENT_HARVEST（官方 deposit 分支无事件）。无库存上限。
/// 来源：harvest.js:114-139 与 deposits/tick.js。
fn intent_harvest_deposit(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (body, store_cap) = match w.creep(id) {
        Some(c) => (c.body.clone(), store_capacity(c)),
        None => return,
    };
    let (deposit_type, cooldown_time) = {
        let Some(RoomObject::Generic(g)) = w.room.objects.get(&target_id) else {
            return;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
            return;
        };
        let Some(dt) = v.get("depositType").and_then(serde_json::Value::as_str) else {
            return;
        };
        (
            dt.to_string(),
            v.get("cooldownTime").and_then(serde_json::Value::as_u64),
        )
    };
    if cooldown_time.is_some_and(|t| t > w.time) {
        return;
    }
    let work_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_DEPOSIT_POWER,
    );
    // 官方 deposit 分支无 amount 门（harvest.js:114-139，source/mineral 才有）：
    // WORK 全熄的 amount=0 仍走完整流程——落动作日志、harvested +0、按当前
    // 累计重算冷却并刷新寿命。
    let amount = work_power;
    let held_total = match w.creep(id) {
        Some(c) => kk_world::types::store_total(&c.store, &c.store_extra),
        None => return,
    };
    let sum = held_total + amount;
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, &deposit_type, amount);
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
        w.record_action(
            id,
            "harvest",
            super::super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
    if sum > store_cap {
        intent_drop_amount_res(w, id, &deposit_type, sum - store_cap);
    }
    let harvested = {
        let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&target_id) else {
            return;
        };
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
            return;
        };
        let harvested = v
            .get("harvested")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            + amount as u64;
        v["harvested"] = serde_json::json!(harvested);
        v["decayTime"] = serde_json::json!(w.time + c::DEPOSIT_DECAY_TIME as u64);
        g.data = v.to_string();
        harvested
    };
    let cooldown = (c::DEPOSIT_EXHAUST_MULTIPLY * (harvested as f64).powf(c::DEPOSIT_EXHAUST_POW))
        .ceil() as u64;
    if cooldown > 1 {
        w.deposit_cooldown_pending.insert(target_id, cooldown);
    }
}

/// 采集能量源，拒绝他人拥有或预定的房间。采集量受 WORK、boost 和剩余能量限制。
fn intent_harvest_source(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, work_power, held_energy, held_total, store_cap) = match w.creep(id) {
        Some(c) => (
            c.owner.clone(),
            harvest_power(c),
            c.store.energy as i64,
            kk_world::types::store_total(&c.store, &c.store_extra),
            store_capacity(c),
        ),
        None => return,
    };
    if let Some(ctl) = w.controller()
        && (ctl.owner.as_deref().is_some_and(|u| u != owner)
            || ctl.reservation.as_ref().is_some_and(|r| r.user != owner))
    {
        return;
    }
    if work_power <= 0 {
        return;
    }
    let energy = match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Source(s)) if s.energy > 0 => s.energy.min(work_power as u32),
        _ => return,
    };
    let amount = energy as i64;
    if let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&target_id) {
        s.energy -= amount as u32;
    }
    // 采集量（与是否超容掉落无关）累计入房 raid 预算，供下次生成器阈值
    // 判定。来源：harvest.js:45-48 对 source 写 invaderHarvested（任意
    // 采集者都累计）；kk 聚合在房间 raid 状态——生成器只读总和。
    w.room.raid.harvested_energy = w.room.raid.harvested_energy.saturating_add(amount);
    let sum = held_total + amount;
    // 先把产出加到 energy 槽，再用全部资源之和检查容量。
    // 超量只从 energy 槽掉落，可能包括原有能量；不能把全槽总量写入 energy。
    // 来源：上游 engine/src/processor/intents/creeps/harvest.js 的 source 分支。
    let energy_after = held_energy + amount;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy = energy_after.min(u32::MAX as i64) as u32;
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
        w.record_action(
            id,
            "harvest",
            super::super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
    if sum > store_cap {
        let drop_amt = energy_after.min(sum - store_cap);
        if drop_amt > 0 {
            intent_drop_amount(w, id, drop_amt);
        }
    }
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": amount }),
    );
}

/// 采集矿物，要求同格提取器可用、属于自己或无主且不在冷却中。
/// 与 source 分支不同，这里不直接检查控制器归属或预定；有主提取器的
/// 控制器和配额条件由 extractor_active 检查。
fn intent_harvest_mineral(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, body, store_cap) = match w.creep(id) {
        Some(c) => (c.owner.clone(), c.body.clone(), store_capacity(c)),
        None => return,
    };
    let Some(RoomObject::Mineral(m)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if m.mineral_amount == 0 {
        return;
    }
    let extractor_id = w
        .index
        .at(m.x as usize, m.y as usize)
        .iter()
        .copied()
        .find(|eid| matches!(w.room.objects.get(eid), Some(RoomObject::Extractor(_))));
    let Some(extractor_id) = extractor_id else {
        return;
    };
    let (ex_owner, ex_cooldown) = match w.room.objects.get(&extractor_id) {
        Some(RoomObject::Extractor(e)) => (e.owner.clone(), e.cooldown),
        _ => return,
    };
    if ex_owner.as_deref().is_some_and(|u| u != owner) {
        return;
    }
    if !extractor_active(w, &ex_owner) {
        return;
    }
    if ex_cooldown != 0 {
        return;
    }
    let work_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_MINERAL_POWER,
    );
    if work_power <= 0 {
        return;
    }
    let amount = (m.mineral_amount as i64).min(work_power);
    if let Some(RoomObject::Mineral(t)) = w.room.objects.get_mut(&target_id) {
        t.mineral_amount -= amount as u32;
    }
    let held_total = match w.creep(id) {
        Some(c) => kk_world::types::store_total(&c.store, &c.store_extra),
        None => return,
    };
    let sum = held_total + amount;
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, &m.mineral_type, amount);
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
        w.record_action(
            id,
            "harvest",
            super::super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
    if sum > store_cap {
        intent_drop_amount_res(w, id, &m.mineral_type, sum - store_cap);
    }
    // 这里只登记冷却，extractor_tick 在对象 tick 阶段写回。
    // 本轮后续采集仍读取旧 cooldown，不读取这个待写回值。
    w.extractor_cooldown_pending
        .insert(extractor_id, c::EXTRACTOR_COOLDOWN as u64);
    // 矿物事件保存未按剩余矿量限制的功率，source 事件则保存实际采集量。
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": work_power }),
    );
}

/// 无主提取器总是可用；有主提取器须有同主控制器并且 RCL 配额非零。
/// 提取器的最高等级配额为 1，不进行其他结构的距离排序。
/// 来源：上游 engine/src/utils.js 的 checkStructureAgainstController。
fn extractor_active(w: &Working, ex_owner: &Option<String>) -> bool {
    let Some(owner) = ex_owner.as_deref() else {
        return true;
    };
    let Some(ctl) = crate::settle::structure_rcl_prologue(w, owner) else {
        return false;
    };
    crate::settle::structures::allowed_for(w, "extractor", i64::from(ctl.level)) > 0
}

/// 计算存活 WORK 的能量采集功率，包含 harvest boost。
pub fn harvest_power(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_POWER,
    )
}
