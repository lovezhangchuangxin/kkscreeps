//! 拉拽登记链；正常拒绝仅丢弃否决集，配对表保持原状。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::movement::pull_registration::PullRegistration;

pub(super) enum PullRegistrationSlot {
    Native,
    Hosted(HostedSlot),
}

impl PullRegistrationSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_PULL_REGISTRATION_SLOT)? {
            Dispatch::Native(binding)
                if binding.as_str() == "builtin:room.movePullRegistration.native" =>
            {
                Ok(Self::Native)
            }
            _ => Err("拉拽登记策略贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行拉拽登记槽。Native、无有效配对或正常拒绝时返回 None
    /// （调用方保留配对表原状），成功返回 JS 追加的否决集；调用方在
    /// 移动请求预检之前应用。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeSet<kk_world::ObjId>>, String> {
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        let input = {
            let w = ctx.borrow();
            w.movement_pull_registration_input()
        };
        let Some(candidate) = PullRegistration::new(input)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::PullRegistration {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.into_denies()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
