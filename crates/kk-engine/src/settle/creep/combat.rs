//! 结算 creep 的近远程战斗、治疗、修理和拆解。
//!
//! 结构受伤立即处理；creep 的伤害和治疗延迟到对象 tick。能耗和强化效果
//! 分开计算，rangedMassAttack 与单体攻击对 rampart 的处理不同。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_REPAIR, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::damage;
use crate::settle::movement;
use crate::settle::movement::type_name_of;

mod damage_policy;
pub use damage_policy::{
    slot_combat_apply_damage, slot_combat_apply_damage_with_retaliation, slot_combat_get,
};
mod batch_policy;
pub use batch_policy::{
    slot_combat_apply_damage_batch, slot_combat_apply_damage_batch_with_retaliation,
    slot_combat_list,
};
mod retaliation_policy;
pub use retaliation_policy::{RetaliationPolicy, slot_combat_execute_with_retaliation};
mod heal_policy;
pub use heal_policy::slot_combat_apply_heal;

/// 战斗能力面认得的动作族成员；与 builtin:creep.* 五槽一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatAction {
    Attack,
    RangedAttack,
    RangedMassAttack,
    Heal,
    RangedHeal,
}

impl CombatAction {
    /// 槽键是否属于战斗族；宿主桥与引擎边界共用此判定。
    pub fn from_slot_key(slot_key: &str) -> Option<Self> {
        Some(match slot_key {
            "builtin:creep.attack" => Self::Attack,
            "builtin:creep.rangedAttack" => Self::RangedAttack,
            "builtin:creep.rangedMassAttack" => Self::RangedMassAttack,
            "builtin:creep.heal" => Self::Heal,
            "builtin:creep.rangedHeal" => Self::RangedHeal,
            _ => return None,
        })
    }

    /// 目标改向只对带显式目标的动作开放；rangedMassAttack 无目标可改。
    pub fn targeted(self) -> bool {
        !matches!(self, Self::RangedMassAttack)
    }

    fn run(self, w: &mut Working, id: ObjId, data: &IntentData) {
        match self {
            Self::Attack => intent_attack(w, id, data),
            Self::RangedAttack => intent_ranged_attack(w, id, data),
            Self::RangedMassAttack => intent_ranged_mass_attack(w, id),
            Self::Heal => intent_heal(w, id, data, false),
            Self::RangedHeal => intent_heal(w, id, data, true),
        }
    }
}

/// 槽 native 入口：与意图派发共用同一实现，hosted 链中心与默认直调
/// 行为一致。近远程治疗与无目标的群体攻击按槽包装成三参形态。
pub(in crate::settle) fn heal_melee_native(w: &mut Working, id: ObjId, data: &IntentData) {
    intent_heal(w, id, data, false);
}

pub(in crate::settle) fn heal_ranged_native(w: &mut Working, id: ObjId, data: &IntentData) {
    intent_heal(w, id, data, true);
}

pub(in crate::settle) fn ranged_mass_native(w: &mut Working, id: ObjId, _data: &IntentData) {
    intent_ranged_mass_attack(w, id);
}

/// 能力改向将触碰的对象：请求目标及其格上的 rampart（攻击重定向面）。
/// 仅返回候选中存在的对象；供调用方在执行前做首触快照。
pub fn combat_touch_targets(w: &Working, target_id: ObjId) -> Vec<ObjId> {
    let mut targets = vec![target_id];
    if let Some(target) = w.room.objects.get(&target_id)
        && let Some(rampart) = damage::rampart_at(w, target.pos())
        && rampart != target_id
    {
        targets.push(rampart);
    }
    targets.retain(|id| w.room.objects.contains_key(id));
    targets
}

/// 以指定目标执行本动作的完整 native 语义（含距离/孵化/耐久/安全模式/
/// rampart 全部原生预检）。目标侧预检不通过时静默返回，与原生意图一致；
/// 执行者侧（非 creep 或孵化中）为故障——玩家面已前置拦截，这里是纵深
/// 防御，阻断"改向攻击执行者自己的 spawn 连带移除自身"的自毁链。
pub fn slot_combat_request(
    w: &mut Working,
    id: ObjId,
    target_id: ObjId,
    action: CombatAction,
) -> Result<(), String> {
    let Some(creep) = w.creep(id) else {
        return Err(format!("执行者 {} 不是有效 creep", id.as_str()));
    };
    if creep.spawning {
        return Err("孵化中的 creep 不能发出战斗动作".into());
    }
    let data = IntentData::target(target_id);
    action.run(w, id, &data);
    Ok(())
}

/// 近战攻击相邻、有耐久且非自身的目标，目标格有 rampart 时转打 rampart。
/// 功率计入 ATTACK boost；0 功率也会进入伤害入口并生成事件。
/// 来源：上游 engine/src/processor/intents/creeps/attack.js。
pub(in crate::settle) fn intent_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    intent_attack_policy(w, id, data, None);
}

fn intent_attack_policy(
    w: &mut Working,
    id: ObjId,
    data: &IntentData,
    policy: Option<&RetaliationPolicy>,
) {
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
    // 射程来自 Plane 的战斗射程快照（builtin:creep.attack.range）。
    if (tx as i32 - pos.0 as i32).abs() as i64 > w.plane.combat.attack
        || (ty as i32 - pos.1 as i32).abs() as i64 > w.plane.combat.attack
    {
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
    retaliation_policy::apply_damage(
        w,
        id,
        target_id,
        power,
        c::EVENT_ATTACK_TYPE_MELEE,
        policy,
        true,
    );
}

/// 控制器不属于行动者且安全模式仍生效时，拒绝战斗动作；无主也视为非本人。
fn blocked_by_safe_mode(w: &Working, actor: &str) -> bool {
    w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(actor) && ctl.safe_mode > w.time)
}

/// 修复三格内的可建造结构。基础效果决定能耗，boost 后效果决定恢复耐久，
/// 最终不超过 hitsMax；效果为 0 不记事件。
/// 来源：上游 engine/src/processor/intents/creeps/repair.js。
pub(in crate::settle) fn intent_repair(w: &mut Working, id: ObjId, data: &IntentData) {
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
    if !crate::settle::buildings::cost_bearing(w, &movement::type_name_of(&target)) {
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
    if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
        w.record_action(
            id,
            "repair",
            super::super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
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
pub(in crate::settle) fn intent_dismantle(w: &mut Working, id: ObjId, data: &IntentData) {
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
    if !crate::settle::buildings::cost_bearing(w, &movement::type_name_of(&target)) {
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
    if amount <= 0 {
        return;
    }
    // 官方 dismantle.js:37-40：目标带 SHIELD/FORTIFY/INVULNERABILITY
    //（>= 边界，与 _damage 的严格 > 分叉）时能量回收为 0，伤害照常走门。
    let invul_gain_block = w.room.effects.get(&target_id).is_some_and(|entries| {
        entries.iter().any(|e| {
            (e.power == c::PWR_SHIELD
                || e.power == c::PWR_FORTIFY
                || e.effect == Some(c::EFFECT_INVULNERABILITY))
                && e.end_time >= w.time
        })
    });
    // Mod 建筑按成本构成比例回收各资源；内建结构维持能量 amount/200。
    let redirected_type = w
        .room
        .objects
        .get(&target_id)
        .map(|o| movement::type_name_of(o).to_string())
        .unwrap_or_default();
    if invul_gain_block {
        // 无回收；伤害仍经 apply_damage（其 wall/rampart FORTIFY 门另行生效）。
    } else if let Some(building) = w.plane.buildings.get(&redirected_type).cloned() {
        crate::settle::buildings::dismantle_refund(w, id, &building, amount);
    } else if let Some(creep) = w.creep_mut(id) {
        creep.store.energy = creep
            .store
            .energy
            .saturating_add(u32::try_from(amount / 200).unwrap_or(u32::MAX));
    }
    damage::drop_energy_over_capacity(w, id);
    damage::apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_DISMANTLE);
}

/// 远程攻击三格内目标，rampart 存在时转打它，功率含 rangedAttack boost。
/// 不拒绝零功率，与 rangedMassAttack 的处理不同。
pub(in crate::settle) fn intent_ranged_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    intent_ranged_attack_policy(w, id, data, None);
}

fn intent_ranged_attack_policy(
    w: &mut Working,
    id: ObjId,
    data: &IntentData,
    policy: Option<&RetaliationPolicy>,
) {
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
    // 射程来自 Plane 的战斗射程快照（builtin:creep.rangedAttack.range）。
    if (tx as i32 - pos.0 as i32).abs() as i64 > w.plane.combat.ranged_attack
        || (ty as i32 - pos.1 as i32).abs() as i64 > w.plane.combat.ranged_attack
    {
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
    retaliation_policy::apply_damage(
        w,
        id,
        target_id,
        attack_power,
        c::EVENT_ATTACK_TYPE_RANGED,
        policy,
        true,
    );
}

/// 攻击三格内有主的敌方对象和 powerBank。非 rampart 目标若被 rampart 覆盖
/// 则跳过，不重定向伤害。距离 1/2/3 的倍率为 1/0.4/0.1，结果四舍五入。
/// 正功率即记录动作，即使没有目标；0 功率不记录。
/// 来源：上游 engine/src/processor/intents/creeps/rangedMassAttack.js。
pub(super) fn intent_ranged_mass_attack(w: &mut Working, id: ObjId) {
    intent_ranged_mass_attack_policy(w, id, None);
}

fn intent_ranged_mass_attack_policy(
    w: &mut Working,
    id: ObjId,
    policy: Option<&RetaliationPolicy>,
) {
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
        // 官方 rangedMassAttack.js:47：FORTIFY/INVULNERABILITY 目标
        //（>= 边界）整体跳过（含护盾下的 rampart 本体）。
        if w.room.effects.get(&tid).is_some_and(|entries| {
            entries.iter().any(|e| {
                (e.power == c::PWR_FORTIFY || e.effect == Some(c::EFFECT_INVULNERABILITY))
                    && e.end_time >= w.time
            })
        }) {
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
        retaliation_policy::apply_damage(
            w,
            id,
            tid,
            dmg,
            c::EVENT_ATTACK_TYPE_RANGED_MASS,
            policy,
            true,
        );
    }
    if let Some(a) = w.creep_mut(id) {
        super::set_action_log(&mut a.action_log, super::ACTION_RANGED_MASS);
    }
    // 官方 rangedMassAttack.js:58：群体攻击无坐标，写空对象载荷。
    w.record_action(
        id,
        "rangedMassAttack",
        super::super::action_log::ActionLogPayload::Mass(Default::default()),
    );
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
    // 目标门接受 creep（非孵化）与 powerCreep（官方 heal.js:16、
    // rangedHeal.js:16 的双类型门；pc 无孵化态）。
    let (tx, ty) = match &target {
        RoomObject::Creep(t) if !t.spawning => (t.x, t.y),
        RoomObject::Generic(g) if super::super::powers::is_power_creep(&target) => (g.x, g.y),
        _ => return,
    };
    // 射程来自 Plane 的战斗射程快照（builtin:creep.heal/rangedHeal.range）。
    let range = if ranged {
        w.plane.combat.ranged_heal
    } else {
        w.plane.combat.heal
    };
    if (tx as i32 - pos.0 as i32).abs() as i64 > range
        || (ty as i32 - pos.1 as i32).abs() as i64 > range
    {
        return;
    }
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // 原生部件功率有界；共用登记器在累计量溢出时不写入候选。
    let _ = heal_policy::apply_heal(w, id, target_id, power, ranged);
}
