//! 群体伤害的完整批量预检与战斗候选分页；应用沿共用伤害/死亡路径。
use super::{CombatAction, damage_policy};
use crate::settle::{Working, damage};
use kk_core::constants as c;
use kk_mod_api::combat::{CombatPage, MAX_COMBAT_BATCH, MAX_DAMAGE};
use kk_world::ObjId;
use std::collections::BTreeSet;
use std::ops::Bound;

/// 同房全部可表示战斗视图，包含执行者、孵化与零耐久对象，供 JS 决定
/// 策略筛选。页是当次候选的脱离副本，不是跨查询或修改的稳定快照。
pub fn slot_combat_list(w: &Working, after: &str, limit: u32) -> Result<CombatPage, String> {
    if !(1..=MAX_COMBAT_BATCH).contains(&limit) {
        return Err("战斗列表 limit 须为 1..256 的整数".into());
    }
    let start = if after.is_empty() {
        Bound::Unbounded
    } else {
        Bound::Excluded(ObjId::parse(after).ok_or("战斗游标 ID 非法")?)
    };
    let mut matching = w
        .room
        .objects
        .range((start, Bound::Unbounded))
        .filter_map(|(id, _)| damage_policy::slot_combat_get(w, *id));
    let objects: Vec<_> = matching.by_ref().take(limit as usize).collect();
    let next = matching
        .next()
        .and_then(|_| objects.last().map(|object| object.id.clone()));
    Ok(CombatPage { objects, next })
}

/// 先验证所有目标、唯一性和延迟队列表示域，再按输入顺序施加。空批量
/// 也成功并记录群体动作日志；调用方统一消耗动作并保存全部首触目标。
pub fn slot_combat_apply_damage_batch(
    w: &mut Working,
    id: ObjId,
    requests: &[(ObjId, i64)],
    action: CombatAction,
) -> Result<(), String> {
    if action != CombatAction::RangedMassAttack {
        return Err("批量策略伤害仅在 rangedMassAttack 槽开放".into());
    }
    if requests.len() > MAX_COMBAT_BATCH as usize {
        return Err("批量策略伤害最多 256 项".into());
    }
    let actor = w.creep(id).ok_or("策略执行者不是有效 creep")?;
    if actor.spawning {
        return Err("孵化中的 creep 不能施加批量策略伤害".into());
    }
    let mut seen = BTreeSet::new();
    for &(target, amount) in requests {
        if !seen.insert(target) {
            return Err("批量策略伤害目标不能重复".into());
        }
        if !(0..=MAX_DAMAGE).contains(&amount) {
            return Err("策略伤害须为 0..=1e9 的整数".into());
        }
        damage_policy::validate_damage_target(w, target)?;
        w.damage_to_apply
            .get(&target)
            .copied()
            .unwrap_or(0)
            .checked_add(amount)
            .ok_or("待伤害累计量超出表示域")?;
    }
    for &(target, amount) in requests {
        damage::apply_damage_without_retaliation(
            w,
            id,
            target,
            amount,
            c::EVENT_ATTACK_TYPE_RANGED_MASS,
        );
    }
    if let Some(actor) = w.creep_mut(id) {
        super::super::set_action_log(&mut actor.action_log, super::super::ACTION_RANGED_MASS);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
