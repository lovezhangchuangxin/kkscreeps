//! 移动竞争优先级能力：op 保留 JS 安全整数域校验与 token/binding 故障归属。
use super::*;

/// 读取竞争者当前优先级；未设置为 0（原生默认层）。
#[op2(fast)]
pub(crate) fn op_mod_chain_contest_priority(
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
        .map(|p| p.measure("host_contest_priority"));
    let result = chain.host.contest_priority(&id);
    chain
        .outcome(ModFaultKind::Read, Some(&binding), result)
        .map(|value| value as f64)
}

/// 写入竞争者优先级；id 须为本次输入中的竞争者，数值在 ±(2^53-1) 内。
#[op2(fast)]
pub(crate) fn op_mod_chain_contest_set_priority(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
    priority: f64,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_contest_set_priority"));
    let limit = kk_mod_api::movement::contest::MAX_PRIORITY as f64;
    let result = if priority.is_finite()
        && priority.fract() == 0.0
        && (-limit..=limit).contains(&priority)
    {
        chain.host.contest_set_priority(&id, priority as i64)
    } else {
        Err("竞争优先级须为 ±(2^53-1) 内的 JS 安全整数".into())
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
