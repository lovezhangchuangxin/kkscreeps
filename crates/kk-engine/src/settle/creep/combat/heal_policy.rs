//! 策略治疗只校验表示域；与 native 共用延迟治疗、动作日志和事件入口。
use super::CombatAction;
use crate::settle::Working;
use crate::settle::creep::{ACTION_HEAL, ACTION_HEALED, set_action_log};
use kk_core::constants as c;
use kk_world::ObjId;

/// JS 决定治疗目标与数值，不重算部件/强化功率或检查距离、阵营、安全
/// 模式。仅允许同房存活且已出生的 creep；允许自愈，结构修理走独立能力。
pub fn slot_combat_apply_heal(
    w: &mut Working,
    id: ObjId,
    target_id: ObjId,
    amount: i64,
    action: CombatAction,
) -> Result<(), String> {
    if !(0..=kk_mod_api::combat::MAX_HEAL).contains(&amount) {
        return Err("策略治疗须为 0..=1e9 的整数".into());
    }
    let ranged = match action {
        CombatAction::Heal => false,
        CombatAction::RangedHeal => true,
        _ => return Err("策略治疗仅在 heal/rangedHeal 槽开放".into()),
    };
    let actor = w.creep(id).ok_or("策略执行者不是有效 creep")?;
    if actor.spawning {
        return Err("孵化中的 creep 不能施加策略治疗".into());
    }
    let target = w.creep(target_id).ok_or("策略治疗目标须为本房 creep")?;
    if target.spawning || target.hits == 0 {
        return Err("策略治疗目标须为存活且已出生的 creep".into());
    }
    apply_heal(w, id, target_id, amount, ranged)
}

/// 只登记治疗量，不在此按缺失生命截断：满血目标本轮仍可能受伤。
/// 身体耐久与最终上限由 damage::apply_pending_hits 在死亡判断前统一处理。
pub(super) fn apply_heal(
    w: &mut Working,
    id: ObjId,
    target_id: ObjId,
    amount: i64,
    ranged: bool,
) -> Result<(), String> {
    let accumulated = w
        .heal_to_apply
        .get(&target_id)
        .copied()
        .unwrap_or(0)
        .checked_add(amount)
        .ok_or("待治疗累计量超出表示域")?;
    w.heal_to_apply.insert(target_id, accumulated);
    // 官方 heal.js:30-31 / rangedHeal.js:30-31 双写：源侧 heal/rangedHeal
    // 记目标坐标，目标侧 healed 记治疗者坐标。kk 位图近/远治疗共位，
    // 导出面按 ranged 分键。
    let target_pos = w.room.objects.get(&target_id).map(|o| o.pos());
    let actor_pos = w.room.objects.get(&id).map(|o| o.pos());
    if let Some(actor) = w.creep_mut(id) {
        set_action_log(&mut actor.action_log, ACTION_HEAL);
    }
    if let Some((tx, ty)) = target_pos {
        w.record_action(
            id,
            if ranged { "rangedHeal" } else { "heal" },
            crate::settle::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
    if let Some(target) = w.creep_mut(target_id) {
        set_action_log(&mut target.action_log, ACTION_HEALED);
    }
    if let Some((hx, hy)) = actor_pos {
        if w.creep(target_id).is_some() {
            w.record_action(
                target_id,
                "healed",
                crate::settle::action_log::ActionLogPayload::Coords { x: hx, y: hy },
            );
        } else if w
            .room
            .objects
            .get(&target_id)
            .is_some_and(crate::settle::powers::is_power_creep)
        {
            // powerCreep 目标的动作记录在 data.actionLog（官方 heal.js:31
            // 对 creep/powerCreep 目标同写 healed；坐标=治疗者）。
            crate::settle::powers::set_pc_action_log(
                w,
                target_id,
                "healed",
                serde_json::json!({ "x": hx, "y": hy }),
            );
            w.record_action(
                target_id,
                "healed",
                crate::settle::action_log::ActionLogPayload::Coords { x: hx, y: hy },
            );
        }
    }
    w.push_event(
        c::EVENT_HEAL,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": amount,
            "healType": if ranged { c::EVENT_HEAL_TYPE_RANGED } else { c::EVENT_HEAL_TYPE_MELEE },
        }),
    );
    Ok(())
}
