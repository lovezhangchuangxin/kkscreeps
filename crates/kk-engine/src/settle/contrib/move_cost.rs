//! 移动代价链；正常拒绝仅丢弃覆盖，执行阶段回退原生代价。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::move_cost::MoveCost;

pub(super) enum MoveCostSlot {
    Native,
    Hosted(HostedSlot),
}

impl MoveCostSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_COST_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.moveCost.native" => {
                Ok(Self::Native)
            }
            _ => Err("移动代价策略贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行代价槽。Native、无幸存者或正常拒绝时返回 None（执行阶段
    /// 按原生代价收费），成功返回 JS 设置的覆盖表。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeMap<kk_world::ObjId, u32>>, String> {
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        let input = {
            let w = ctx.borrow();
            w.movement_cost_input()
        };
        let Some(candidate) = MoveCost::new(input)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::MoveCost {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.into_costs()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
