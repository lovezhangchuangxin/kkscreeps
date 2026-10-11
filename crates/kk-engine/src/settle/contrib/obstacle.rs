//! 移动障碍追加链；正常拒绝仅丢弃追加集，native 障碍判定不变。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::movement::obstacle::MoveObstacle;

pub(super) enum ObstacleSlot {
    Native,
    Hosted(HostedSlot),
}

impl ObstacleSlot {
    /// 是否为宿主槽形态（派发表 any_hosted 的组成判定）。
    pub(super) fn is_hosted(&self) -> bool {
        matches!(self, ObstacleSlot::Hosted(_))
    }
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_OBSTACLE_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.moveObstacle.native" => {
                Ok(Self::Native)
            }
            _ => Err("移动障碍策略贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行障碍槽。Native、无胜者或正常拒绝时返回 None（调用方保留
    /// 空追加集，取消链只按 native 判定），成功返回 JS 追加的阻挡集。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeSet<kk_world::ObjId>>, String> {
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        let input = {
            let w = ctx.borrow();
            w.movement_obstacle_input()
        };
        let Some(candidate) = MoveObstacle::new(input)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let executor = super::hosted_executor(slot.executor)?;

        let outcome = executor.run(
            &slot.plan,
            ctx,
            SlotCall::Obstacle {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.into_blocks()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
