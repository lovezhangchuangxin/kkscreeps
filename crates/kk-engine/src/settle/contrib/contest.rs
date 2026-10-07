//! 移动竞争优先级链；正常拒绝仅丢弃优先级与暂存 state，回退默认评分。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::movement::contest::MoveContest;

pub(super) enum ContestSlot {
    Native,
    Hosted(HostedSlot),
}

impl ContestSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_CONTEST_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.moveContest.native" => {
                Ok(Self::Native)
            }
            _ => Err("移动竞争策略贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行竞争策略槽。无贡献、无竞争格或正常拒绝时返回 None，
    /// 调用方按原生默认仲裁；成功返回优先级终值候选。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeMap<kk_world::ObjId, i64>>, String> {
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        if !ctx.borrow().movement.has_contested_cells() {
            return Ok(None);
        }
        let input = {
            let w = ctx.borrow();
            w.movement.contest_input(&w.room)
        };
        let Some(candidate) = MoveContest::new(input)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::Contest {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.priorities()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
