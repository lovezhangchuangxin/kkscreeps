//! 一次整槽 JS 调用的候选能力。候选方法不回入 JS；写入转换可执行 getter。

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::{ChainProfiler, ChainWrite, ModFault, ModFaultKind, ModHostError};
use deno_core::{OpState, op2, v8};
use deno_error::JsErrorBox;
use kk_mod_api::{ContributionPlan, Decline, Dispatch, Key, Stage};
use serde::{Serialize, Serializer};
use serde_json::Value;

mod resources;
pub(crate) use resources::{op_mod_chain_resource_read, op_mod_chain_resource_transfer};
mod movement;
pub(crate) use movement::{
    op_mod_chain_move_cancel, op_mod_chain_move_pending, op_mod_chain_move_request,
};
mod combat;
pub(crate) use combat::{op_mod_chain_combat_cancel, op_mod_chain_combat_request};

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
pub trait ChainHost {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String>;
    fn write(&mut self, binding: &str, context: ChainWrite) -> Result<(), String>;
    fn native(&mut self) -> Result<(), String>;
    /// 默认拒绝，宿主须为有局部恢复边界的槽显式开放资源能力。
    fn resource_read(
        &mut self,
        _id: &str,
        _resource: &str,
    ) -> Result<kk_mod_api::resources::ResourceView, String> {
        Err("本次槽没有资源读取能力".into())
    }
    fn resource_transfer(
        &mut self,
        _from: &str,
        _to: &str,
        _resource: &str,
        _amount: i64,
        _loss: i64,
    ) -> Result<kk_mod_api::resources::ResourceTransfer, String> {
        Err("本次槽没有资源转账能力".into())
    }
    /// 默认拒绝，宿主须为移动槽显式开放移动请求能力；仅作用于
    /// 本次调用的执行者，不提供任意对象的请求读写。
    fn movement_pending(&mut self) -> Result<Option<u8>, String> {
        Err("本次槽没有移动请求能力".into())
    }
    fn movement_request(&mut self, _direction: u8) -> Result<bool, String> {
        Err("本次槽没有移动请求能力".into())
    }
    fn movement_cancel(&mut self) -> Result<(), String> {
        Err("本次槽没有移动请求能力".into())
    }
    /// 默认拒绝，宿主须为战斗槽显式开放目标改向/取消能力；仅作用于
    /// 本次调用的执行者与本动作，不提供任意对象的伤害/治疗写入。
    fn combat_request(&mut self, _target: &str) -> Result<(), String> {
        Err("本次槽没有战斗目标改向能力".into())
    }
    fn combat_cancel(&mut self) -> Result<(), String> {
        Err("本次槽没有战斗能力".into())
    }
}

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

/// 启动时交给 JS 的固定链描述，后续调用只传槽键与本次能力标识。
#[derive(Serialize)]
pub(crate) struct ChainProgram {
    pub key: String,
    pub before: Vec<String>,
    pub wraps: Vec<String>,
    pub center: String,
    pub after: Vec<String>,
    pub native_center: bool,
    pub decline_allowed: bool,
}

impl ChainProgram {
    pub fn from_plan(
        plan: &ContributionPlan,
        contributions: &[kk_mod_api::Contribution],
        disabled: &BTreeSet<String>,
    ) -> Result<Vec<Self>, String> {
        // 原始中心来自声明，不从 replace 后的冻结 dispatch 反推。
        let builtin = kk_mod_api::builtin::link();
        let centers: std::collections::BTreeMap<_, _> = std::iter::once(&builtin)
            .chain(contributions)
            .flat_map(|contribution| &contribution.slots)
            .map(|slot| (&slot.key, &slot.native_binding))
            .collect();
        let enabled = |key: &Key| !disabled.contains(key.namespace());
        let mut programs = Vec::new();
        for slot in plan.slots() {
            if !enabled(&slot.key) {
                continue;
            }
            let (before, wraps, center, after) = match &slot.dispatch {
                Dispatch::Native(center) => (&[][..], &[][..], center, &[][..]),
                Dispatch::Chained {
                    before,
                    wraps,
                    center,
                    after,
                } => (
                    before.as_slice(),
                    wraps.as_slice(),
                    center,
                    after.as_slice(),
                ),
            };
            // 每层保留 JS 栈帧，显式限制防止依赖包数变成运行期栈溢出。
            if wraps.len() > 64 {
                return Err(format!("槽 {} 的 wrap 层数超过 64", slot.key));
            }
            let center = if enabled(center) {
                center
            } else {
                centers
                    .get(&slot.key)
                    .copied()
                    .ok_or_else(|| format!("槽 {} 缺少原始中心", slot.key))?
            };
            if !enabled(center) {
                return Err(format!("槽 {} 的原始中心已经停用", slot.key));
            }
            let names = |keys: &[Key]| {
                keys.iter()
                    .filter(|key| enabled(key))
                    .map(ToString::to_string)
                    .collect()
            };
            programs.push(Self {
                key: slot.key.to_string(),
                before: names(before),
                wraps: names(wraps),
                center: center.to_string(),
                after: names(after),
                native_center: center.namespace() == "builtin",
                decline_allowed: slot.stage == Stage::ObjectAction
                    && (slot.key.namespace() != "builtin"
                        || kk_mod_api::builtin::RESOURCE_ACTION_SLOTS.contains(&slot.key.as_str())
                        || kk_mod_api::builtin::MOVE_ACTION_SLOTS.contains(&slot.key.as_str())
                        || kk_mod_api::builtin::COMBAT_ACTION_SLOTS.contains(&slot.key.as_str())),
            });
        }
        Ok(programs)
    }

    pub fn bindings(&self) -> BTreeSet<String> {
        self.before
            .iter()
            .chain(&self.wraps)
            .chain(&self.after)
            .chain((!self.native_center).then_some(&self.center))
            .cloned()
            .collect()
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
            Err("该槽尚不支持正常拒绝；仅 Mod 自有对象动作与已接局部恢复边界的内建槽（资源/移动/战斗）可回滚".into()),
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
