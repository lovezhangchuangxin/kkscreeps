//! 移动请求预检能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取请求者当前否决判定：原生初值恒为否，之外只有已追加的否决。
#[op2(fast)]
pub(crate) fn op_mod_chain_precheck_blocked(
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
        .map(|p| p.measure("host_precheck_blocked"));
    let result = chain.host.precheck_blocked(&id);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

/// 追加否决；单向能力，不提供放行。id 须为本次输入中的请求者。
#[op2(fast)]
pub(crate) fn op_mod_chain_precheck_block(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_precheck_block"));
    let result = chain.host.precheck_block(&id);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
