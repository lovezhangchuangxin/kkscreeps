//! 注册与结算预算入口；资源故障优先于普通处理器错误。
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BudgetPhase {
    Registration,
    Settlement,
}

impl ModRealm {
    /// 槽是否属停用命名空间（invoke_chain 对这类槽直接丢弃返回完成）；
    /// 诊断面据此区分"执行完成"与"因停用被丢弃"。
    pub fn is_inactive_slot(&self, slot: &str) -> bool {
        self.inactive_slots.contains(slot)
    }

    /// 每次实际结算前开始累计；不同槽、对象与房间共享同一轮 Mod 额度。
    /// 同一轮也重置随机流域序号：失败重试或丢弃后同 tick 重开，都会从
    /// 各域 index 0 重抽（G22.b 轮作用域重置）。
    pub fn begin_settlement(&mut self) -> ModHostResult<()> {
        if self.settlement_usage.is_some() {
            return Err(ModHostError::new("结算预算", "上一轮预算尚未结束"));
        }
        self.settlement_usage = Some(Usage::default());
        self.runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<crate::rng::RngState>()
            .begin_round();
        Ok(())
    }

    pub fn end_settlement(&mut self) {
        self.settlement_usage = None;
    }

    pub(super) fn with_budget<T>(
        &mut self,
        phase: BudgetPhase,
        work: impl FnOnce(&mut Self) -> ModHostResult<T>,
    ) -> ModHostResult<T> {
        let registration = phase == BudgetPhase::Registration;
        let label = if registration {
            "Mod 注册"
        } else {
            "Mod 结算"
        };
        let limit = if registration {
            self.limits.registration
        } else {
            self.limits.settlement
        };
        let used = if registration {
            Usage::default()
        } else {
            self.settlement_usage.unwrap_or_default()
        };
        let start_timer = self.profiler.as_ref().map(|p| p.measure("budget_start"));
        self.budget
            .start(ExecutionLimits {
                cpu: limit.cpu.saturating_sub(used.cpu),
                wall: limit.wall.saturating_sub(used.wall),
            })
            .map_err(|e| {
                ModHostError::execution(
                    label,
                    e.to_string(),
                    ModFault {
                        slot: None,
                        binding: None,
                        kind: ModFaultKind::Resource,
                    },
                )
            })?;
        drop(start_timer);
        let result = work(self);
        let finish_timer = self.profiler.as_ref().map(|p| p.measure("budget_finish"));
        let usage = self.budget.finish(self.runtime.v8_isolate()).map_err(|e| {
            ModHostError::execution(
                label,
                e.to_string(),
                ModFault {
                    slot: None,
                    binding: None,
                    kind: ModFaultKind::Resource,
                },
            )
        })?;
        drop(finish_timer);
        if !registration && let Some(total) = &mut self.settlement_usage {
            total.cpu += usage.cpu;
            total.wall += usage.wall;
        }
        result
    }
}
