//! 自有对象 op；字段转换可执行 getter，转换期间不得持有候选/OpState 借用。
use super::*;
use kk_mod_api::objects::{ObjectPage, ObjectView};

#[op2]
#[serde]
pub(crate) fn op_mod_chain_object_get(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<Option<ObjectView>, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain.host.object_get(&id);
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[op2]
#[serde]
pub(crate) fn op_mod_chain_object_list(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] type_key: String,
    #[string] after: String,
    limit: f64,
) -> Result<ObjectPage, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = if limit.is_finite() && limit.fract() == 0.0 && (1.0..=256.0).contains(&limit) {
        chain.host.object_list(&type_key, &after, limit as u32)
    } else {
        Err("对象列表 limit 须为 1..256 的整数".into())
    };
    chain.outcome(ModFaultKind::Read, Some(&binding), result)
}

#[op2(reentrant)]
#[serde]
pub(crate) fn op_mod_chain_object_create<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    #[string] token: String,
    #[string] binding: String,
    #[string] type_key: String,
    fields: v8::Local<'s, v8::Value>,
) -> Result<ObjectView, JsErrorBox> {
    write_object(scope, state, token, binding, type_key, fields, true)
}

#[op2(reentrant)]
#[serde]
pub(crate) fn op_mod_chain_object_update<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
    fields: v8::Local<'s, v8::Value>,
) -> Result<ObjectView, JsErrorBox> {
    write_object(scope, state, token, binding, id, fields, false)
}

fn write_object<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    token: String,
    binding: String,
    target: String,
    fields: v8::Local<'s, v8::Value>,
    create: bool,
) -> Result<ObjectView, JsErrorBox> {
    {
        let mut state = state.borrow_mut();
        active(&mut state, &token)?.checked_binding(&binding)?;
    }
    let fields = {
        v8::tc_scope!(let tc, scope);
        crate::v8_convert::from_v8::<Value>(tc, fields)
            .map_err(|error| format!("对象字段转换失败: {error}"))
    };
    let mut state = state.borrow_mut();
    let chain = active(&mut state, &token)?;
    chain.checked_binding(&binding)?;
    let fields = chain.outcome(ModFaultKind::WriteConversion, Some(&binding), fields)?;
    let result = if create {
        chain.host.object_create(&binding, &target, &fields)
    } else {
        chain.host.object_update(&binding, &target, &fields)
    };
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_object_remove(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] id: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain.host.object_remove(&binding, &id);
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}
