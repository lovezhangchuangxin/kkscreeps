//! 批量转换可执行 getter；转换期间不借用 OpState/候选，之后再验证能力。
use super::*;
use kk_mod_api::combat::{CombatPage, DamageRequest, MAX_COMBAT_BATCH, MAX_DAMAGE};

#[op2]
#[serde]
pub(crate) fn op_mod_chain_combat_list(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] after: String,
    limit: f64,
) -> Result<CombatPage, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = if limit.is_finite()
        && limit.fract() == 0.0
        && (1.0..=MAX_COMBAT_BATCH as f64).contains(&limit)
    {
        chain.host.combat_list(&after, limit as u32)
    } else {
        Err("战斗列表 limit 须为 1..256 的整数".into())
    };
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[derive(serde::Deserialize)]
struct RawDamageRequest {
    id: String,
    amount: f64,
}

#[op2(nofast, reentrant)]
pub(crate) fn op_mod_chain_combat_apply_damage_batch<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    #[string] token: String,
    #[string] binding: String,
    requests: v8::Local<'s, v8::Value>,
) -> Result<(), JsErrorBox> {
    {
        let mut state = state.borrow_mut();
        active(&mut state, &token)?.checked_binding(&binding)?;
    }
    let requests = {
        v8::tc_scope!(let tc, scope);
        (|| {
            let array = v8::Local::<v8::Array>::try_from(requests)
                .map_err(|_| "批量策略伤害须为数组".to_string())?;
            if array.length() > MAX_COMBAT_BATCH {
                return Err("批量策略伤害最多 256 项".into());
            }
            let raw = crate::v8_convert::from_v8::<Vec<RawDamageRequest>>(tc, requests)
                .map_err(|error| format!("批量策略伤害转换失败: {error}"))?;
            raw.into_iter()
                .map(|request| {
                    if !request.amount.is_finite()
                        || request.amount.fract() != 0.0
                        || !(0.0..=MAX_DAMAGE as f64).contains(&request.amount)
                    {
                        return Err("策略伤害须为 0..=1e9 的整数".into());
                    }
                    Ok(DamageRequest {
                        id: request.id,
                        amount: request.amount as i64,
                    })
                })
                .collect::<Result<Vec<_>, String>>()
        })()
    };
    let mut state = state.borrow_mut();
    let chain = active(&mut state, &token)?;
    chain.checked_binding(&binding)?;
    let requests = chain.outcome(ModFaultKind::WriteConversion, Some(&binding), requests)?;
    let _timer = chain
        .profiler
        .as_ref()
        .map(|p| p.measure("host_combat_apply_damage_batch"));
    let result = chain.host.combat_apply_damage_batch(&requests);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
