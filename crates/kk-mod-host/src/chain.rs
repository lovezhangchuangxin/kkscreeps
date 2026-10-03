//! 一次整槽 JS 调用的候选能力。普通 op 只读写 Rust 候选，不回入 JS。

use std::collections::BTreeSet;
use std::rc::Rc;

use deno_core::{OpState, op2};
use deno_error::JsErrorBox;
use kk_mod_api::{ContributionPlan, Dispatch};
use serde::Serialize;
use serde_json::Value;

/// 候选访问适配器：调用方绑定权威对象/参数，JS 无法改变 native 的接收者。
/// 方法执行期间不得回入 JS；每次访问自己取得并释放候选短借用。
pub trait ChainHost {
    fn read(&mut self, binding: &str) -> Result<Value, String>;
    fn write(&mut self, binding: &str, context: Value) -> Result<(), String>;
    fn native(&mut self) -> Result<(), String>;
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
}

impl ChainProgram {
    pub fn from_plan(plan: &ContributionPlan) -> Result<Vec<Self>, String> {
        let mut programs = Vec::new();
        for slot in plan.slots() {
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
            let names = |keys: &[kk_mod_api::Key]| keys.iter().map(ToString::to_string).collect();
            programs.push(Self {
                key: slot.key.to_string(),
                before: names(before),
                wraps: names(wraps),
                center: center.to_string(),
                after: names(after),
                native_center: center.namespace() == "builtin",
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
    pub fault: Option<String>,
    pub native_called: bool,
    pub native_allowed: bool,
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
            Some(error) => Err(JsErrorBox::generic(error.clone())),
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
        self.fault.get_or_insert(error.clone());
        Err(JsErrorBox::generic(error))
    }

    fn outcome<T>(&mut self, result: Result<T, String>) -> Result<T, JsErrorBox> {
        result.map_err(|error| {
            self.fault.get_or_insert(error.clone());
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
) -> Result<serde_json::Value, JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain
        .host
        .read(&binding)
        .map_err(|error| format!("处理器 {binding}: {error}"));
    chain.outcome(result)
}

#[op2]
pub(crate) fn op_mod_chain_write(
    state: &mut OpState,
    #[string] token: String,
    #[string] binding: String,
    #[serde] context: serde_json::Value,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.checked_binding(&binding)?;
    let result = chain
        .host
        .write(&binding, context)
        .map_err(|error| format!("处理器 {binding}: {error}"));
    chain.outcome(result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_native(
    state: &mut OpState,
    #[string] token: String,
) -> Result<(), JsErrorBox> {
    let chain = active(state, &token)?;
    chain.check_fault()?;
    if !chain.native_allowed || chain.native_called {
        return chain.outcome(Err("本次槽调用没有可执行的 native 中心，或已执行过".into()));
    }
    chain.native_called = true;
    let result = chain.host.native();
    chain.outcome(result)
}

#[op2(fast)]
pub(crate) fn op_mod_chain_fault(
    state: &mut OpState,
    #[string] token: String,
    #[string] error: String,
) -> Result<(), JsErrorBox> {
    active(state, &token)?.fault.get_or_insert(error);
    Ok(())
}
