//! 有预算保护的诊断脚本，不绕过超限后的毒化状态。
use super::*;

impl ModRealm {
    /// 在 realm 内执行一段诊断脚本（测试与排障用；realm 本身是可信面，
    /// 这里的额外暴露只有宿主自己写下的代码）。
    pub fn evaluate_probe(&mut self, code: &str) -> ModHostResult<Value> {
        self.with_budget(BudgetPhase::Settlement, |realm| {
            realm.evaluate_probe_inner(code)
        })
    }

    fn evaluate_probe_inner(&mut self, code: &str) -> ModHostResult<Value> {
        let value = self
            .runtime
            .execute_script("mod-host-probe", code.to_string())
            .map_err(|error| ModHostError::new("probe", format!("{error}")))?;
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let local = v8::Local::new(tc, value);
        crate::v8_convert::from_v8(tc, local)
            .map_err(|error| ModHostError::new("probe", format!("结果序列化失败：{error}")))
    }
}
