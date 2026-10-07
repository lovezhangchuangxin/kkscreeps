//! 移动请求预检能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取静态初判与 JS 决策组合后的请求否决判定。
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

/// 追加不可撤销的否决；静态放行不能清除它。id 须在本次输入中。
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

/// 放行请求的静态障碍；动态占格与显式否决由宿主继续复核。
#[op2(fast)]
pub(crate) fn op_mod_chain_precheck_allow_static(
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
        .map(|p| p.measure("host_precheck_allow_static"));
    let result = chain.host.precheck_allow_static(&id);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
