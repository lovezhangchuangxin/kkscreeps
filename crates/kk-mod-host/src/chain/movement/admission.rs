//! 移动准入终值能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取执行者当前准入终值；未覆盖项即原生 can_move 结果。
#[op2(fast)]
pub(crate) fn op_mod_chain_admission_admitted(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<bool, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_admission_admitted"));
    let result = chain.host.admission_admitted(&id);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

/// 覆盖执行者准入终值；id 须为本次输入中的执行者。
#[op2(fast)]
pub(crate) fn op_mod_chain_admission_set_admitted(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
    admitted: bool,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_admission_set_admitted"));
    let result = chain.host.admission_set_admitted(&id, admitted);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

/// 按候选当前终值只读预演取消链，返回幸存胜者对象 ID（升序）。
#[op2]
#[serde]
pub(crate) fn op_mod_chain_admission_evaluate(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<Vec<String>, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_admission_evaluate"));
    let result = chain.host.admission_evaluate();
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}
