//! 战斗目标改向与取消能力：目标以原始字符串接收，宿主在候选短借用内
//! 以新目标执行 native，或应用 JS 策略伤害/治疗；与取消共用恰一次门。
use super::*;

#[op2]
#[serde]
pub(crate) fn op_mod_chain_combat_get(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] target: String,
) -> Result<Option<kk_mod_api::combat::CombatView>, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain.host.combat_get(&target);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_combat_apply_damage(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] target: String,
    amount: f64,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_combat_apply_damage"));
    // op 也校验原始数字，不能靠 JS 预检后截断小数或把 NaN 转成零。
    let result = if amount.is_finite()
        && amount.fract() == 0.0
        && (0.0..=kk_mod_api::combat::MAX_DAMAGE as f64).contains(&amount)
    {
        chain.host.combat_apply_damage(&target, amount as i64)
    } else {
        Err("策略伤害须为 0..=1e9 的整数".into())
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_combat_apply_heal(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] target: String,
    amount: f64,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_combat_apply_heal"));
    // 原始数值校验在转换前完成，不能将小数截断或把 NaN 转成零。
    let result = if amount.is_finite()
        && amount.fract() == 0.0
        && (0.0..=kk_mod_api::combat::MAX_HEAL as f64).contains(&amount)
    {
        chain.host.combat_apply_heal(&target, amount as i64)
    } else {
        Err("策略治疗须为 0..=1e9 的整数".into())
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

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
