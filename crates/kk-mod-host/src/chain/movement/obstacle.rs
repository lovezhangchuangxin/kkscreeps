//! 移动障碍追加能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取胜者当前障碍判定：预填时点 native 初判或已追加的阻挡。
#[op2(fast)]
pub(crate) fn op_mod_chain_obstacle_blocked(
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
        .map(|p| p.measure("host_obstacle_blocked"));
    let result = chain.host.obstacle_blocked(&id);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

/// 追加阻挡；单向能力，不提供放行。id 须为本次输入中的胜者。
#[op2(fast)]
pub(crate) fn op_mod_chain_obstacle_block(
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
        .map(|p| p.measure("host_obstacle_block"));
    let result = chain.host.obstacle_block(&id);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
