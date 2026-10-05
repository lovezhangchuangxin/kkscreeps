//! 反击表属于本次攻击链；只对实际受击目标登记一次反击，不触发递归。
use super::{CombatAction, damage_policy};
use crate::intent::IntentData;
use crate::settle::{Working, damage};
use kk_mod_api::combat::{MAX_COMBAT_BATCH, MAX_DAMAGE};
use kk_world::ObjId;
use std::collections::BTreeMap;

/// 不可直接构造的已校验反击表。存在空表表示禁用本动作所有反击；
/// 未声明表保持原生攻击/JS 伤害入口各自的默认行为。
#[derive(Clone)]
pub struct RetaliationPolicy(BTreeMap<ObjId, i64>);

impl RetaliationPolicy {
    pub fn new(w: &Working, requests: &[(ObjId, i64)]) -> Result<Self, String> {
        if requests.len() > MAX_COMBAT_BATCH as usize {
            return Err("反击策略最多 256 项".into());
        }
        let mut table = BTreeMap::new();
        for &(target, amount) in requests {
            if !(0..=MAX_DAMAGE).contains(&amount) {
                return Err("策略反击须为 0..=1e9 的整数".into());
            }
            damage_policy::validate_damage_target(w, target)?;
            if table.insert(target, amount).is_some() {
                return Err("反击策略目标不能重复".into());
            }
        }
        Ok(Self(table))
    }
}

/// 为执行者预留整表反击量和可能的自伤量，执行前失败则完全不写候选。
/// 未命中目标的预留也计入表示检查；不根据原生准入预测实际命中集。
pub(super) fn preflight(
    w: &Working,
    actor: ObjId,
    policy: Option<&RetaliationPolicy>,
    self_damage: i64,
) -> Result<(), String> {
    let Some(policy) = policy else { return Ok(()) };
    let total: i64 = policy.0.values().sum(); // 256 × 1e9 在 i64 表示域内。
    w.damage_to_apply
        .get(&actor)
        .copied()
        .unwrap_or(0)
        .checked_add(total)
        .and_then(|n| n.checked_add(self_damage))
        .ok_or("反击待伤害累计量超出表示域")?;
    Ok(())
}

/// 保留原生功率、距离、安全模式及 rampart 目标选择，只替换反击。
pub fn slot_combat_execute_with_retaliation(
    w: &mut Working,
    actor: ObjId,
    data: &IntentData,
    action: CombatAction,
    policy: &RetaliationPolicy,
) -> Result<(), String> {
    if matches!(action, CombatAction::Heal | CombatAction::RangedHeal) {
        return Err("治疗槽没有反击策略能力".into());
    }
    let creep = w.creep(actor).ok_or("反击策略执行者不是有效 creep")?;
    if creep.spawning {
        return Err("孵化中的 creep 不能执行攻击策略".into());
    }
    preflight(w, actor, Some(policy), 0)?;
    match action {
        CombatAction::Attack => super::intent_attack_policy(w, actor, data, Some(policy)),
        CombatAction::RangedAttack => {
            super::intent_ranged_attack_policy(w, actor, data, Some(policy))
        }
        CombatAction::RangedMassAttack => {
            super::intent_ranged_mass_attack_policy(w, actor, Some(policy))
        }
        _ => unreachable!("上面已拒绝治疗"),
    }
    Ok(())
}

pub(super) fn apply_damage(
    w: &mut Working,
    actor: ObjId,
    target: ObjId,
    amount: i64,
    attack_type: i64,
    policy: Option<&RetaliationPolicy>,
    native: bool,
) {
    if let Some(policy) = policy {
        damage::apply_damage_with_retaliation(
            w,
            actor,
            target,
            amount,
            attack_type,
            policy.0.get(&target).copied().unwrap_or(0),
        );
    } else if native {
        damage::apply_damage(w, actor, target, amount, attack_type);
    } else {
        damage::apply_damage_without_retaliation(w, actor, target, amount, attack_type);
    }
}
