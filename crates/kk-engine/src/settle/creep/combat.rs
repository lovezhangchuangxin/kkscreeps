//! 结算 creep 的近远程战斗、治疗、修理和拆解。
//!
//! 结构受伤立即处理；creep 的伤害和治疗延迟到对象 tick。能耗和强化效果
//! 分开计算，rangedMassAttack 与单体攻击对 rampart 的处理不同。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_HEAL, ACTION_HEALED, ACTION_REPAIR, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::damage;
use crate::settle::movement;
use crate::settle::movement::type_name_of;

/// 近战攻击相邻、有耐久且非自身的目标，目标格有 rampart 时转打 rampart。
/// 功率计入 ATTACK boost；0 功率也会进入伤害入口并生成事件。
/// 来源：上游 engine/src/processor/intents/creeps/attack.js。
pub(super) fn intent_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            ),
        ),
        _ => return,
    };
    if target_id == id {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let (tx, ty) = target.pos();
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    if let RoomObject::Creep(t) = &target
        && t.spawning
    {
        return;
    }
    if damage::object_hits(&target) <= 0 {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    damage::apply_damage(w, id, target_id, power, c::EVENT_ATTACK_TYPE_MELEE);
}

/// 控制器不属于行动者且安全模式仍生效时，拒绝战斗动作；无主也视为非本人。
fn blocked_by_safe_mode(w: &Working, actor: &str) -> bool {
    w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(actor) && ctl.safe_mode > w.time)
}

/// 修复三格内的可建造结构。基础效果决定能耗，boost 后效果决定恢复耐久，
/// 最终不超过 hitsMax；效果为 0 不记事件。
/// 来源：上游 engine/src/processor/intents/creeps/repair.js。
pub(super) fn intent_repair(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, energy, repair_power, boost_incs) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::REPAIR_POWER,
            kk_world::types::boost_increments(
                &c.body,
                BodyKind::Work,
                "work",
                "repair",
                c::REPAIR_POWER,
            ),
        ),
        _ => return,
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let Some(hits_max) = damage::structure_hits_max(w, &target) else {
        return;
    };
    let hits = damage::object_hits(&target);
    if hits >= hits_max {
        return;
    }
    let (tx, ty) = target.pos();
    if (tx as i32 - pos.0 as i32).abs() > 3 || (ty as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    let energy_remaining = energy * 100; // energy / REPAIR_COST
    let missing = hits_max - hits;
    let effect = repair_power.min(energy_remaining).min(missing);
    let cost = energy.min((effect + 99) / 100); // ceil(effect × REPAIR_COST)
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, missing);
    if effect <= 0 {
        return;
    }
    damage::set_structure_hits(w, target_id, (hits + boosted).min(hits_max));
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy -= cost as u32;
        set_action_log(&mut creep.action_log, ACTION_REPAIR);
    }
    w.push_event(
        c::EVENT_REPAIR,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": boosted,
            "energySpent": cost,
        }),
    );
}

/// 拆解相邻结构，先按 rampart 重定向，再按目标耐久限量。
/// 先回收能量并处理超容，再施加伤害，避免击毁后丢失回收信息。
/// 来源：上游 engine/src/processor/intents/creeps/dismantle.js。
pub(super) fn intent_dismantle(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Work,
                "work",
                "dismantle",
                c::DISMANTLE_POWER,
            ),
        ),
        _ => return,
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let (tx, ty) = target.pos();
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    let hits = w
        .room
        .objects
        .get(&target_id)
        .map(damage::object_hits)
        .unwrap_or(0);
    let amount = power.min(hits);
    let energy_gain = amount / 200;
    if amount <= 0 {
        return;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy = creep
            .store
            .energy
            .saturating_add(u32::try_from(energy_gain).unwrap_or(u32::MAX));
    }
    damage::drop_energy_over_capacity(w, id);
    damage::apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_DISMANTLE);
}

/// 远程攻击三格内目标，rampart 存在时转打它，功率含 rangedAttack boost。
/// 不拒绝零功率，与 rangedMassAttack 的处理不同。
pub(super) fn intent_ranged_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    if target_id == id {
        return;
    }
    let Some((pos, owner, body)) = w
        .creep(id)
        .filter(|c| !c.spawning)
        .map(|c| ((c.x, c.y), c.owner.clone(), c.body.clone()))
    else {
        return;
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let (tx, ty) = (target.pos().0, target.pos().1);
    if (tx as i32 - pos.0 as i32).abs() > 3 || (ty as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    if let RoomObject::Creep(t) = &target
        && t.spawning
    {
        return;
    }
    if damage::object_hits(&target) <= 0 {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    let attack_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::RangedAttack,
        "ranged_attack",
        "rangedAttack",
        c::RANGED_ATTACK_POWER,
    );
    damage::apply_damage(w, id, target_id, attack_power, c::EVENT_ATTACK_TYPE_RANGED);
}

/// 攻击三格内有主的敌方对象和 powerBank。非 rampart 目标若被 rampart 覆盖
/// 则跳过，不重定向伤害。距离 1/2/3 的倍率为 1/0.4/0.1，结果四舍五入。
/// 正功率即记录动作，即使没有目标；0 功率不记录。
/// 来源：上游 engine/src/processor/intents/creeps/rangedMassAttack.js。
pub(super) fn intent_ranged_mass_attack(w: &mut Working, id: ObjId) {
    let Some((pos, owner, body)) = w
        .creep(id)
        .filter(|c| !c.spawning)
        .map(|c| ((c.x, c.y), c.owner.clone(), c.body.clone()))
    else {
        return;
    };
    let attack_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::RangedAttack,
        "ranged_attack",
        "rangedMassAttack",
        c::RANGED_ATTACK_POWER,
    );
    if attack_power == 0 {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    let mut targets: Vec<(ObjId, u8, u8)> = Vec::new();
    for (tid, obj) in w.room.objects.iter() {
        let owned = kk_world::domain::object_owner(obj);
        let is_target = owned.as_deref().is_some_and(|u| u != owner.as_str())
            || type_name_of(obj) == "powerBank";
        if !is_target {
            continue;
        }
        let (ox, oy) = obj.pos();
        if (ox as i32 - pos.0 as i32).abs() <= 3 && (oy as i32 - pos.1 as i32).abs() <= 3 {
            targets.push((*tid, ox, oy));
        }
    }
    let distance_rate = |d: i32| -> f64 {
        match d {
            0 | 1 => 1.0,
            2 => 0.4,
            _ => 0.1,
        }
    };
    for (tid, ox, oy) in targets {
        let obj = w.room.objects.get(&tid).cloned();
        let Some(obj) = obj else { continue };
        let is_rampart = type_name_of(&obj) == "rampart";
        if !is_rampart && damage::rampart_at(w, (ox, oy)).is_some() {
            continue;
        }
        if damage::object_hits(&obj) <= 0 {
            continue;
        }
        if let RoomObject::Creep(t) = &obj
            && t.spawning
        {
            continue;
        }
        let distance = (ox as i32 - pos.0 as i32)
            .abs()
            .max((oy as i32 - pos.1 as i32).abs());
        let dmg = (attack_power as f64 * distance_rate(distance)).round() as i64;
        damage::apply_damage(w, id, tid, dmg, c::EVENT_ATTACK_TYPE_RANGED_MASS);
    }
    if let Some(a) = w.creep_mut(id) {
        super::set_action_log(&mut a.action_log, super::ACTION_RANGED_MASS);
    }
}

/// 治疗已出生 creep，允许自愈；近程/远程范围为 1/3，基础功率为 12/4。
/// 功率含 boost，累加到待治疗量，在 creep tick 与伤害一起处理，0 功率也记事件。
pub(super) fn intent_heal(w: &mut Working, id: ObjId, data: &IntentData, ranged: bool) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            if ranged {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "rangedHeal",
                    c::RANGED_HEAL_POWER,
                )
            } else {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "heal",
                    c::HEAL_POWER,
                )
            },
        ),
        _ => return,
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let RoomObject::Creep(t) = &target else {
        return;
    };
    if t.spawning {
        return;
    }
    let (tx, ty) = (t.x, t.y);
    let range = if ranged { 3 } else { 1 };
    if (tx as i32 - pos.0 as i32).abs() > range || (ty as i32 - pos.1 as i32).abs() > range {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    *w.heal_to_apply.entry(target_id).or_insert(0) += power;
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_HEAL);
    }
    if let Some(t) = w.creep_mut(target_id) {
        set_action_log(&mut t.action_log, ACTION_HEALED);
    }
    w.push_event(
        c::EVENT_HEAL,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": power,
            "healType": if ranged {
                c::EVENT_HEAL_TYPE_RANGED
            } else {
                c::EVENT_HEAL_TYPE_MELEE
            },
        }),
    );
}
