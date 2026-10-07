//! 移动代价能力：op 保留 token/binding 故障归属。
use super::*;

/// 读取移动者当前代价候选：未覆盖项为原生代价。
#[op2(fast)]
pub(crate) fn op_mod_chain_cost_fatigue_of(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<f64, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_cost_fatigue_of"));
    let result = chain.host.cost_fatigue_of(&id);
    chain
        .outcome(ModFaultKind::Read, Some(&binding), result)
        .map(|value| value as f64)
}

/// 设置疲劳覆盖；值域 0..=u32::MAX，id 须为本次输入中的幸存者。
#[op2(fast)]
pub(crate) fn op_mod_chain_cost_set_fatigue(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
    value: f64,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_cost_set_fatigue"));
    let limit = u32::MAX as f64;
    let result = if value.is_finite() && value.fract() == 0.0 && (0.0..=limit).contains(&value) {
        chain.host.cost_set_fatigue(&id, value as i64)
    } else {
        Err("移动代价须为 0..=u32::MAX 内的整数".into())
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
