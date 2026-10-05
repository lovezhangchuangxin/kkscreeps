//! 战斗目标改向与取消能力：目标以原始字符串接收，宿主在候选短借用内
//! 以新目标执行本动作的完整 native 语义。伤害/治疗量、功率与预检
//! 不经此面修改，执行恰一次（consumed 单向标记）。
use super::*;

/// 以新目标执行本动作的 native 语义。目标须为对象 ID 的原始字符串；
/// 距离、目标孵化、耐久、安全模式与 rampart 重定向仍按原生预检处理，
/// 不满足时静默无效果——与原生意图一致。执行者非 creep 或孵化中是
/// 故障（能力契约错误），不在静默预检之列。
#[op2(fast)]
pub(crate) fn op_mod_chain_combat_request(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] target: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_combat_request"));
    let result = chain.host.combat_request(&target);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

/// 取消本动作：native 中心不再执行。已通过能力执行或取消过的
/// 动作不能再取消（consumed 单向），由宿主判定并报错。
#[op2(fast)]
pub(crate) fn op_mod_chain_combat_cancel(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_combat_cancel"));
    let result = chain.host.combat_cancel();
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
