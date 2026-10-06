//! 移动请求预检链；正常拒绝仅丢弃否决集，登记矩阵保持原状。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::precheck::MovePrecheck;

pub(super) enum PrecheckSlot {
    Native,
    Hosted(HostedSlot),
}

impl PrecheckSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_PRECHECK_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.movePrecheck.native" => {
                Ok(Self::Native)
            }
            _ => Err("移动请求预检贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行预检槽。Native、无登记请求或正常拒绝时返回 None（调用方
    /// 保留登记矩阵原状），成功返回 JS 追加的否决集；调用方在竞争
    /// 输入构造之前应用。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeSet<kk_world::ObjId>>, String> {
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        let input = {
            let w = ctx.borrow();
            w.movement_precheck_input()
        };
        let Some(candidate) = MovePrecheck::new(input)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::Precheck {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.into_blocks()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
