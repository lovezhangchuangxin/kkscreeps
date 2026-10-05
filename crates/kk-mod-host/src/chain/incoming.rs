//! 受击数值候选不改世界；op 保留原始浮点校验与 token/binding 故障归属。
use super::*;

#[op2(fast)]
pub(crate) fn op_mod_chain_incoming_damage(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<f64, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain.host.incoming_damage().map(|n| n as f64);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_incoming_set_damage(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    amount: f64,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = if amount.is_finite()
        && amount.fract() == 0.0
        && (0.0..=kk_mod_api::incoming::MAX_DAMAGE as f64).contains(&amount)
    {
        chain.host.incoming_set_damage(amount as i64)
    } else {
        Err("受击伤害须为 JS 安全非负整数（0..=2^53-1）".into())
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
