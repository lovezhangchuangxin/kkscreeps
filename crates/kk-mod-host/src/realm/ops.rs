//! 注册声明与结算能力的 op 装配；转换期间不持有状态借用。
use super::*;
use crate::chain::*;

// 与候选写入同样可执行 getter；进入转换前检查注册能力并释放全部借用。
#[op2(nofast, reentrant)]
fn op_mod_declare<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    #[string] kind: String,
    value: v8::Local<'s, v8::Value>,
) -> Result<(), JsErrorBox> {
    let namespace = state.borrow().borrow::<Registry>().active_namespace()?;
    let value = {
        v8::tc_scope!(let tc, scope);
        crate::v8_convert::from_v8::<Value>(tc, value)
            .map_err(|error| format!("声明 {kind} 转换失败：{error}"))
    };
    let mut state = state.borrow_mut();
    let registry = state.borrow_mut::<Registry>();
    // 内层转换错误即使被 getter 捕获，也必须阻止外层声明及计划发布。
    registry.record(namespace, kind, value)
}

extension!(
    kk_mod_host,
    ops = [
        op_mod_declare,
        op_mod_chain_read,
        op_mod_chain_write,
        op_mod_chain_native,
        op_mod_chain_fault,
        op_mod_chain_fault_detail,
        op_mod_chain_decline,
        op_mod_chain_resource_read,
        op_mod_chain_resource_transfer,
        op_mod_chain_move_pending,
        op_mod_chain_move_request,
        op_mod_chain_move_cancel,
        op_mod_chain_combat_request,
        op_mod_chain_combat_cancel,
        op_mod_chain_combat_apply_damage,
        op_mod_chain_combat_apply_damage_batch,
        op_mod_chain_combat_set_retaliation,
        op_mod_chain_combat_list,
        op_mod_chain_combat_apply_heal,
        op_mod_chain_combat_get,
        op_mod_chain_incoming_damage,
        op_mod_chain_incoming_set_damage,
        op_mod_chain_contest_priority,
        op_mod_chain_contest_set_priority,
        op_mod_chain_admission_admitted,
        op_mod_chain_admission_set_admitted,
        op_mod_chain_admission_evaluate,
        op_mod_chain_obstacle_blocked,
        op_mod_chain_obstacle_block,
        op_mod_chain_obstacle_evaluate,
        op_mod_chain_precheck_blocked,
        op_mod_chain_precheck_block,
        op_mod_chain_precheck_allow_static,
        op_mod_chain_pull_denied,
        op_mod_chain_pull_deny,
        op_mod_chain_cost_fatigue_of,
        op_mod_chain_cost_set_fatigue,
        op_mod_chain_build_request,
        op_mod_chain_build_cancel,
        op_mod_chain_object_get,
        op_mod_chain_object_list,
        op_mod_chain_object_create,
        op_mod_chain_object_update,
        op_mod_chain_object_remove,
        crate::rng::op_mod_random,
        crate::rng::op_mod_random_init
    ]
);
