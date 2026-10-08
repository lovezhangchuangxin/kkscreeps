//! 受击链只选择当前目标最终扣血量；世界写入在链完成后统一发生。
use std::cell::Cell;

use kk_core::constants as c;
use kk_mod_api::incoming::{
    DamageSource, IncomingInput, MAX_DAMAGE, MAX_SOURCES, ModIncomingInput,
};
use kk_world::ObjId;

use super::Working;

/// 同一受击调用内共享的数值候选；不包含世界写句柄，不进入检查点。
pub struct IncomingDamage {
    input: IncomingInput,
    amount: Cell<i64>,
}

impl IncomingDamage {
    pub fn input(&self) -> &IncomingInput {
        &self.input
    }

    pub fn damage(&self) -> i64 {
        self.amount.get()
    }

    /// 可多次调整以组合处理器；每次写入均须在 JS 安全整数表示域内。
    pub fn set_damage(&self, amount: i64) -> Result<(), String> {
        checked_amount(amount)?;
        self.amount.set(amount);
        Ok(())
    }

    pub(super) fn prepare(w: &Working, id: ObjId, default_damage: i64) -> Result<Self, String> {
        let raw_damage = w.damage_to_apply.get(&id).copied().unwrap_or(0);
        let pending_heal = w.heal_to_apply.get(&id).copied().unwrap_or(0);
        for amount in [raw_damage, default_damage, pending_heal] {
            checked_amount(amount)?;
        }
        let mut sources = Vec::new();
        let mut source_count = 0;
        let mut attributed = 0i64;
        let target_id = id.as_str();
        for event in &w.events {
            if event["event"] != c::EVENT_ATTACK || event["data"]["targetId"] != target_id {
                continue;
            }
            let Some(amount) = event["data"]["damage"].as_i64().filter(|n| *n > 0) else {
                continue;
            };
            attributed = attributed.checked_add(amount).ok_or("受击来源累计溢出")?;
            source_count += 1;
            if sources.len() < MAX_SOURCES {
                sources.push(DamageSource {
                    source_id: event["objectId"].as_str().ok_or("受击来源 ID 缺失")?.into(),
                    amount,
                    attack_type: event["data"]["attackType"].as_i64().ok_or("受击种类缺失")?,
                });
            }
        }
        let unattributed_damage = raw_damage
            .checked_sub(attributed)
            .filter(|n| *n >= 0)
            .ok_or("受击来源与待伤害队列不一致")?;
        Ok(Self {
            input: IncomingInput {
                raw_damage,
                default_damage,
                pending_heal,
                sources,
                source_count,
                sources_truncated: source_count > MAX_SOURCES,
                unattributed_damage,
            },
            amount: Cell::new(default_damage),
        })
    }
}

fn checked_amount(amount: i64) -> Result<(), String> {
    if !(0..=MAX_DAMAGE).contains(&amount) {
        return Err("受击伤害/治疗须为 JS 安全非负整数（0..=2^53-1）".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// G09.c 结构受击候选：立即伤害进入 Mod 目标前的一次数值选择；与 creep
/// 受击候选同构（set_damage 多次可调、JS 安全整数界），不含世界写句柄。
pub struct ModIncomingDamage {
    input: ModIncomingInput,
    amount: Cell<i64>,
}

impl ModIncomingDamage {
    pub fn input(&self) -> &ModIncomingInput {
        &self.input
    }

    pub fn damage(&self) -> i64 {
        self.amount.get()
    }

    /// 可多次调整以组合处理器；每次写入均须在 JS 安全整数表示域内。
    pub fn set_damage(&self, amount: i64) -> Result<(), String> {
        checked_amount(amount)?;
        self.amount.set(amount);
        Ok(())
    }

    /// 来源为当次攻击的立即值（攻击者与攻击种类），无累计队列语义。
    pub(super) fn prepare(damage: i64, attacker: ObjId, attack_type: i64) -> Result<Self, String> {
        checked_amount(damage)?;
        // 摘要描述攻击发生时的来源；自杀或后续结构摧毁不能抹掉已执行伤害。
        let source = Some(DamageSource {
            source_id: attacker.as_str(),
            amount: damage,
            attack_type,
        });
        Ok(Self {
            input: ModIncomingInput { damage, source },
            amount: Cell::new(damage),
        })
    }
}
