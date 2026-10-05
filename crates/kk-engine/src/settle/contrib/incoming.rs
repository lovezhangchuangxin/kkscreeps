//! 受击数值链；正常拒绝仅丢弃数值与暂存 state，随后仍执行默认受伤/治疗。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::incoming::IncomingDamage;

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
