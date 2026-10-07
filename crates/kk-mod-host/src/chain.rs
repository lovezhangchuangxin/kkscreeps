//! 一次整槽 JS 调用的候选能力。候选方法不回入 JS；写入转换可执行 getter。

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::{ChainProfiler, ChainWrite, ModFault, ModFaultKind, ModHostError};
use deno_core::{OpState, op2, v8};
use deno_error::JsErrorBox;
use kk_mod_api::{Decline, Key};
use serde::{Serialize, Serializer};
use serde_json::Value;

pub(crate) use crate::execution_plan::ExecutionChain as ChainProgram;

mod resources;
pub(crate) use resources::{op_mod_chain_resource_read, op_mod_chain_resource_transfer};
mod movement;
pub(crate) use movement::{
    op_mod_chain_move_cancel, op_mod_chain_move_pending, op_mod_chain_move_request,
};
mod combat;
mod incoming;
pub(crate) use combat::{
    op_mod_chain_combat_apply_damage, op_mod_chain_combat_apply_heal, op_mod_chain_combat_cancel,
    op_mod_chain_combat_get, op_mod_chain_combat_request,
};
pub(crate) use incoming::{op_mod_chain_incoming_damage, op_mod_chain_incoming_set_damage};
mod contest;
pub(crate) use contest::{op_mod_chain_contest_priority, op_mod_chain_contest_set_priority};
mod admission;
pub(crate) use admission::{
    op_mod_chain_admission_admitted, op_mod_chain_admission_evaluate,
    op_mod_chain_admission_set_admitted,
};
mod obstacle;
pub(crate) use obstacle::{op_mod_chain_obstacle_block, op_mod_chain_obstacle_blocked};
mod move_cost;
mod precheck;
mod pull_registration;
pub(crate) use move_cost::{op_mod_chain_cost_fatigue_of, op_mod_chain_cost_set_fatigue};
pub(crate) use precheck::{
    op_mod_chain_precheck_allow_static, op_mod_chain_precheck_block, op_mod_chain_precheck_blocked,
};
pub(crate) use pull_registration::{op_mod_chain_pull_denied, op_mod_chain_pull_deny};
mod combat_batch;
pub(crate) use combat_batch::{
    op_mod_chain_combat_apply_damage_batch, op_mod_chain_combat_list,
    op_mod_chain_combat_set_retaliation,
};
mod build;
pub(crate) use build::{op_mod_chain_build_cancel, op_mod_chain_build_request};
mod objects;
pub(crate) use objects::{
    op_mod_chain_object_create, op_mod_chain_object_get, op_mod_chain_object_list,
    op_mod_chain_object_remove, op_mod_chain_object_update,
};

/// 固定的候选读取封套；开放领域的字段、参数与 namespace 仍为 JSON 树。
/// 字段顺序沿用原 JSON Map 的键序，避免改变 JS 可观察的枚举/刷新顺序。
#[derive(Serialize)]
pub struct ChainContext {
    pub args: Value,
    pub room: String,
    pub state: Value,
    pub target: ChainTarget,
    pub tick: u64,
}

#[derive(Serialize)]
pub struct ChainTarget {
    pub fields: Value,
    pub id: String,
}

/// 候选访问适配器：调用方绑定权威对象/参数，JS 无法改变 native 的接收者。
/// 方法执行期间不得回入 JS；每次访问自己取得并释放候选短借用。
pub trait ChainHost: ObjectsHost + ResourcesHost + CombatHost + BuildHost + MovementHost {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String>;
    fn write(&mut self, binding: &str, context: ChainWrite) -> Result<(), String>;
    fn native(&mut self) -> Result<(), String>;
}

mod host_objects;
pub use host_objects::ObjectsHost;
mod host_resources;
pub use host_resources::ResourcesHost;
mod host_combat;
pub use host_combat::CombatHost;
mod host_build;
pub use host_build::BuildHost;
mod host_movement;
pub use host_movement::MovementHost;

// op 的 serde 输出包装；计时围住真实转换，不从 op/host 的差额推测。
pub(crate) struct ChainRead {
    context: ChainContext,
    profiler: Option<ChainProfiler>,
}

impl Serialize for ChainRead {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let _timer = self.profiler.as_ref().map(|p| p.measure("read_to_v8"));
        self.context.serialize(serializer)
    }
}

pub(crate) struct ActiveChain {
    pub token: String,
    pub bindings: Rc<BTreeSet<String>>,
    pub host: Box<dyn ChainHost>,
    pub slot: String,
    pub fault: Option<ModHostError>,
    /// 仅 SDK 首次认领的故障可补充一次文本；不允许传播阶段改写诊断。
    pub fault_detail_pending: bool,
    pub declined: Option<Decline>,
    pub decline_allowed: bool,
    pub native_called: bool,
    pub native_allowed: bool,
    pub profiler: Option<ChainProfiler>,
}

#[derive(Default)]
pub(crate) struct ChainState {
    pub active: Option<ActiveChain>,
}

fn active<'a>(state: &'a mut OpState, token: &str) -> Result<&'a mut ActiveChain, JsErrorBox> {
    let chain = state
        .borrow_mut::<ChainState>()
        .active
        .as_mut()
        .ok_or_else(|| JsErrorBox::generic("槽调用已经结束，候选能力不可用"))?;
    if chain.token != token {
        return Err(JsErrorBox::generic("槽调用能力已失效"));
    }
    Ok(chain)
}

impl ActiveChain {
    fn check_fault(&self) -> Result<(), JsErrorBox> {
        match &self.fault {
            Some(error) => Err(JsErrorBox::generic(error.message.clone())),
            None if self.declined.is_some() => {
                Err(JsErrorBox::generic("本次槽已正常拒绝，候选访问已关闭"))
            }
            None => Ok(()),
        }
    }

    fn checked_binding(&mut self, binding: &str) -> Result<(), JsErrorBox> {
        // 故障后的读写也要报错，使被处理器直接捕获的 op 故障在 SDK
        // 的下一次同步边界重新浮出，不能继续执行 after 或候选步骤。
        self.check_fault()?;
        if self.bindings.contains(binding) {
            return Ok(());
        }
        let error = format!("绑定 {binding} 不属于本次槽调用");
        self.record_fault(ModFaultKind::Capability, None, error.clone());
        Err(JsErrorBox::generic(error))
    }

    fn record_fault(&mut self, kind: ModFaultKind, binding: Option<&str>, message: String) {
        // 首个领域/处理器错误优先，外层 wrap 的传播不能改写来源。
        self.fault.get_or_insert_with(|| {
            ModHostError::execution(
                self.slot.clone(),
                message,
                ModFault {
                    slot: Some(self.slot.clone()),
                    binding: binding.and_then(|key| Key::new(key).ok()),
                    kind,
                },
            )
        });
    }

    fn outcome<T>(
        &mut self,
        kind: ModFaultKind,
        binding: Option<&str>,
        result: Result<T, String>,
    ) -> Result<T, JsErrorBox> {
        result.map_err(|error| {
            self.record_fault(kind, binding, error.clone());
            JsErrorBox::generic(error)
        })
    }
}

#[op2]
#[serde]
pub(crate) fn op_mod_chain_read(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
) -> Result<ChainRead, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = {
        let _timer = chain.profiler.as_ref().map(|p| p.measure("host_read"));
        chain
            .host
            .read(&binding)
            .map_err(|error| format!("处理器 {binding}: {error}"))
    };
    let context = chain.outcome(ModFaultKind::Read, Some(&binding), result)?;
    Ok(ChainRead {
        context,
        profiler: chain.profiler.clone(),
    })
}

// getter 可能执行 JS，显式保留慢入口；能力检查不因 reentrant 放宽。
#[op2(nofast, reentrant)]
pub(crate) fn op_mod_chain_write<'s, 'i>(
    scope: &mut v8::PinScope<'s, 'i>,
    state: Rc<RefCell<OpState>>,
    #[string] token: String,
    #[string] binding: String,
    context: v8::Local<'s, v8::Value>,
) -> Result<(), JsErrorBox> {
    let profiler = {
        let mut state = state.borrow_mut();
        let chain = active(&mut state, &token)?;
        chain.checked_binding(&binding)?;
        chain.profiler.clone()
    };
    // from_v8 可执行 getter 并嵌套 op；转换期间不得持有 OpState/候选借用。
    // 捕获 V8 异常，只格式化 serde 错误，不执行异常对象的属性或堆栈钩子。
    let context = {
        v8::tc_scope!(let tc, scope);
        let result = {
            let _timer = profiler.as_ref().map(|p| p.measure("write_from_v8"));
            crate::v8_convert::from_v8::<ChainWrite>(tc, context)
        };
        result.map_err(|error| format!("处理器 {binding}: 上下文转换失败: {error}"))
    };
    let mut state = state.borrow_mut();
    let chain = active(&mut state, &token)?;
    // getter 内被捕获的候选故障仍阻止外层写回；转换失败同样留存。
    chain.checked_binding(&binding)?;
    let context = chain.outcome(ModFaultKind::WriteConversion, Some(&binding), context)?;
    let _timer = chain.profiler.as_ref().map(|p| p.measure("host_write"));
    let result = chain
        .host
        .write(&binding, context)
        .map_err(|error| format!("处理器 {binding}: {error}"));
    chain.outcome(ModFaultKind::Write, Some(&binding), result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_native(
    state: &mut OpState,
    #[string] token: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.check_fault()?;
    if !chain.native_allowed || chain.native_called {
        return chain.outcome(
            ModFaultKind::Capability,
            None,
            Err("本次槽调用没有可执行的 native 中心，或已执行过".into()),
        );
    }
    chain.native_called = true;
    let _timer = chain.profiler.as_ref().map(|p| p.measure("host_native"));
    let result = chain.host.native();
    chain.outcome(ModFaultKind::Native, None, result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_fault(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] kind: String,
    #[string] error: String,
) -> Result<bool, JsErrorBox> {
    let chain = active(state, &token)?;
    // 故障上报允许拒绝之后的新异常，但不得覆盖更早的故障。
    let binding = if binding.is_empty() {
        None
    } else {
        if !chain.bindings.contains(&binding) {
            return chain.outcome(
                ModFaultKind::Capability,
                None,
                Err("故障来源绑定不属于本次槽调用".into()),
            );
        }
        Some(binding.as_str())
    };
    let kind = match kind.as_str() {
        "handler" => ModFaultKind::Handler,
        "control" => ModFaultKind::Control,
        "context" => ModFaultKind::Context,
        "chain" if binding.is_none() => ModFaultKind::Chain,
        _ => {
            return chain.outcome(
                ModFaultKind::Capability,
                None,
                Err("无效的故障来源阶段".into()),
            );
        }
    };
    if kind != ModFaultKind::Chain && binding.is_none() {
        return chain.outcome(
            ModFaultKind::Capability,
            None,
            Err("处理器故障缺少来源绑定".into()),
        );
    }
    if chain.fault.is_some() {
        return Ok(false);
    }
    chain.record_fault(kind, binding, error);
    chain.fault_detail_pending = true;
    Ok(true)
}

/// 来源先留存并关闭候选访问，JS 格式化完成后只补充诊断文本。
#[op2(fast)]
pub(crate) fn op_mod_chain_fault_detail(
    state: &mut OpState,
    #[string] token: String,
    #[string] error: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    if std::mem::take(&mut chain.fault_detail_pending) {
        chain.fault.as_mut().expect("认领来源时已留存故障").message = error;
    }
    Ok(())
}

/// 拒绝是已校验绑定的正常结局；不能覆盖已留存的异常。
#[op2(fast)]
pub(crate) fn op_mod_chain_decline(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[string] code: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    if !chain.decline_allowed {
        return chain.outcome(
            ModFaultKind::Control,
            Some(&binding),
            Err("该槽尚不支持正常拒绝；仅 Mod 自有对象动作与已接局部恢复边界的内建槽（资源/移动/战斗/建造族/受击/移动竞争/移动准入/移动障碍/移动预检/拉拽登记/移动代价）可回滚".into()),
        );
    }
    if code.is_empty()
        || code.len() > 128
        || !code
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c))
    {
        return chain.outcome(
            ModFaultKind::Control,
            Some(&binding),
            Err("拒绝代码须为 1..128 字节的 ASCII 字母、数字或 ._:-".into()),
        );
    }
    let binding = Key::new(binding).map_err(JsErrorBox::generic)?;
    chain.declined = Some(Decline { binding, code });
    Ok(())
}
