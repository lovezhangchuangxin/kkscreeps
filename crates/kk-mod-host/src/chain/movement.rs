//! 移动请求能力：方向以原始数字接收，宿主在候选短借用内完成登记。
use super::*;

/// 读取执行者当前登记的请求方向；未登记为 null。
#[op2]
#[serde]
pub(crate) fn op_mod_chain_move_pending(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_move_pending"));
    let result = chain.host.movement_pending();
    let direction = chain.outcome(ModFaultKind::Read, Some(&binding), result)?;
    Ok(direction.map_or(serde_json::Value::Null, serde_json::Value::from))
}

/// 登记或替换执行者的移动请求。方向须为 1..=8 的整数；领域上
/// 越界、静态障碍或孵化中不登记并返回 false（与 native 预检同口径）。
#[op2(fast)]
pub(crate) fn op_mod_chain_move_request(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    direction: f64,
) -> Result<bool, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_move_request"));
    let result =
        if direction.is_finite() && direction.fract() == 0.0 && (1.0..=8.0).contains(&direction) {
            chain.host.movement_request(direction as u8)
        } else {
            Err("移动方向须为 1..=8 的整数".to_string())
        };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

/// 取消执行者的移动请求登记。
#[op2(fast)]
pub(crate) fn op_mod_chain_move_cancel(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_move_cancel"));
    let result = chain.host.movement_cancel();
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
