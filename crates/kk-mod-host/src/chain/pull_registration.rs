//! 拉拽登记能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取拉者当前否决判定：原生初值恒为未否决，之外只有已追加的否决。
#[op2(fast)]
pub(crate) fn op_mod_chain_pull_denied(
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
        .map(|p| p.measure("host_pull_denied"));
    let result = chain.host.pull_denied(&id);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

/// 追加拉拽否决；单向能力，不提供放行或新增。id 须为本次输入中的拉者。
#[op2(fast)]
pub(crate) fn op_mod_chain_pull_deny(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain.profiler.as_ref().map(|p| p.measure("host_pull_deny"));
    let result = chain.host.pull_deny(&id);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
