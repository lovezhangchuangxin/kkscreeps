//! 移动请求预检链；正常拒绝丢弃 JS 决策并兑现登记时原生拒绝。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::movement::precheck::MovePrecheck;

pub(super) enum PrecheckSlot {
    Native,
    Hosted(HostedSlot),
}

impl PrecheckSlot {
    /// 是否为宿主槽形态（派发表 any_hosted 的组成判定）。
    pub(super) fn is_hosted(&self) -> bool {
        matches!(self, PrecheckSlot::Hosted(_))
    }
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_PRECHECK_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.movePrecheck.native" => {
                Ok(Self::Native)
            }
            _ => Err("移动请求预检贡献须经整槽宿主装配".into()),
        }
    }

    /// Native 或空输入不进入 JS；Hosted 成功/正常拒绝均返回请求过滤，
    /// 后者只兑现登记时的原生拒绝。竞争输入构造前应用全部决策。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<crate::settle::movement::precheck::PrecheckDecisions>, String> {
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
        let executor = super::hosted_executor(slot.executor)?;

        let outcome = executor.run(
            &slot.plan,
            ctx,
            SlotCall::Precheck {
                candidate: candidate.clone(),
            },
        )?;
        Ok(Some(
            candidate.decisions(matches!(outcome, SlotOutcome::Completed)),
        ))
    }

    pub(super) fn defers_static(&self) -> bool {
        matches!(self, Self::Hosted(_))
    }
}
