//! 资源能力仅接收原始字符串/数字，不序列化对象或在候选借用内执行 getter。
use super::*;
use kk_mod_api::resources::{ResourceTransfer, ResourceView};

#[op2]
#[serde]
pub(crate) fn op_mod_chain_resource_read(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
    #[string] resource: String,
) -> Result<ResourceView, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_resource_read"));
    let result = chain.host.resource_read(&id, &resource);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[op2]
#[serde]
pub(crate) fn op_mod_chain_resource_transfer(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] from: String,
    #[string] to: String,
    #[string] resource: String,
    amount: f64,
    loss: f64,
) -> Result<ResourceTransfer, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_resource_transfer"));
    let integer = |number: f64| {
        if number.is_finite()
            && number.fract() == 0.0
            && (0.0..=9_007_199_254_740_991.0).contains(&number)
        {
            Ok(number as i64)
        } else {
            Err("资源数量须为非负 JS 安全整数".to_string())
        }
    };
    let result = integer(amount).and_then(|amount| {
        integer(loss).and_then(|loss| {
            chain
                .host
                .resource_transfer(&from, &to, &resource, amount, loss)
        })
    });
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
