//! 移动准入终值链；正常拒绝仅丢弃覆盖，保留原生预填终值。
use super::assemble::dispatch_of;
use super::*;
use crate::settle::admission::MoveAdmission;

pub(super) enum AdmissionSlot {
    Native,
    Hosted(HostedSlot),
}

impl AdmissionSlot {
    pub(super) fn native_from_plan(plan: &ContributionPlan) -> Result<Self, String> {
        match dispatch_of(plan, kk_mod_api::builtin::MOVE_ADMISSION_SLOT)? {
            Dispatch::Native(binding)
                if binding.as_str() == "builtin:creep.moveAdmission.native" =>
            {
                Ok(Self::Native)
            }
            _ => Err("移动准入策略贡献须经整槽宿主装配".into()),
        }
    }

    /// 运行准入槽。先在引擎侧预填原生准入终值（取消链消费，与槽
    /// 是否 Hosted 无关）；Native、无胜者或正常拒绝时返回 None（调用
    /// 方保留原生预填），成功返回 JS 调整后的准入终值全表。输入构造
    /// 只在 Hosted 路径发生。
    pub(super) fn run(
        &self,
        ctx: &RoomCtx,
    ) -> Result<Option<std::collections::BTreeMap<kk_world::ObjId, bool>>, String> {
        ctx.borrow_mut().prefill_movement_admissions();
        let Self::Hosted(slot) = self else {
            return Ok(None);
        };
        let (input, native) = {
            let w = ctx.borrow();
            (w.movement_admission_input(), w.movement_admission_finals())
        };
        let Some(candidate) = MoveAdmission::new(input, native)? else {
            return Ok(None);
        };
        let candidate = Rc::new(candidate);
        let outcome = slot.executor.run(
            &slot.plan,
            ctx,
            SlotCall::Admission {
                candidate: candidate.clone(),
            },
        )?;
        Ok(match outcome {
            SlotOutcome::Completed => Some(candidate.into_finals()),
            SlotOutcome::Declined(_) => None,
        })
    }
}
