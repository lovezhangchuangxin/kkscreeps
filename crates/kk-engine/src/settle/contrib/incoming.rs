//! 受击数值链；正常拒绝仅丢弃数值与暂存 state，随后仍执行默认受伤/治疗。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::incoming::{IncomingDamage, ModIncomingDamage};

pub(super) enum IncomingSlot {
    Native,
    Hosted(HostedSlot),
}

impl IncomingSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::INCOMING_DAMAGE_SLOT)? {
            Dispatch::Native(binding)
                if binding.as_str() == "builtin:creep.receiveDamage.native" =>
            {
                Ok(Self::Native)
            }
            _ => Err("受击策略贡献须经整槽宿主装配".into()),
        }
    }

    pub(super) fn run(&self, ctx: &RoomCtx, id: ObjId) -> Result<Option<i64>, String> {
        let Some(default_damage) = super::super::damage::default_pending_damage(&ctx.borrow(), id)
        else {
            return Ok(None);
        };
        let Self::Hosted(slot) = self else {
            return Ok(Some(default_damage));
        };
        let candidate = Rc::new(IncomingDamage::prepare(&ctx.borrow(), id, default_damage)?);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::Incoming {
                id,
                candidate: candidate.clone(),
            },
        )?;
        Ok(Some(match outcome {
            SlotOutcome::Completed => candidate.damage(),
            SlotOutcome::Declined(_) => default_damage,
        }))
    }
}

/// G09.c 结构受击槽（builtin:mod.receiveDamage）：输入为当次伤害与
/// 来源摘要，输出为修改后的伤害量；目标形状经链上下文 ctx.target
/// （Mod payload 含耐久/库存信封）承载。Native 直通伤害，Hosted 经
/// 槽候选取修改值，正常拒绝回退默认伤害。
pub(super) enum ModIncomingSlot {
    Native,
    Hosted(HostedSlot),
}

impl ModIncomingSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOD_INCOMING_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:mod.receiveDamage.native" => {
                Ok(Self::Native)
            }
            _ => Err("结构受击贡献须经整槽宿主装配".into()),
        }
    }

    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
        target: ObjId,
        damage: i64,
        attacker: ObjId,
        attack_type: i64,
    ) -> Result<i64, String> {
        let Self::Hosted(slot) = self else {
            return Ok(damage);
        };
        let candidate = Rc::new(ModIncomingDamage::prepare(damage, attacker, attack_type)?);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::ModIncoming {
                id: target,
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => candidate.damage(),
            SlotOutcome::Declined(_) => damage,
        })
    }
}
