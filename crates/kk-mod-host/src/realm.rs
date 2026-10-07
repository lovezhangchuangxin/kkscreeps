//! 可信注册 realm：一个装载全部 Mod 包的 deno_core 隔离区。
//!
//! 信任模型与 [设计](../../../docs/designs/mod-system.md) 一致：共同可信宿主，
//! Mod 之间不构成安全隔离边界；注册期 SDK 只暴露声明面，没有世界读写 op。
//! 处理器函数保存在本 VM 的绑定表内（sdk.js），Rust 侧只记录键——
//! 结算期一次调用完成整槽；候选访问由调用方通过 ChainHost 适配。
//!
//! 线程亲和：V8 隔离区须在创建线程上使用；`ModRealm` 创建后不得跨线程移动使用，
//! 也不得在 tokio runtime 上下文（async fn 内）调用——内部 `block_on` 会恐慌。
//! 字段顺序保证先销毁 `runtime` 后销毁 tokio 执行器。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use deno_core::v8;
use deno_core::{JsRuntime, ModuleId, ModuleSpecifier, OpState, RuntimeOptions, extension, op2};
use deno_error::JsErrorBox;
use serde::Serialize;
use serde_json::Value;

use crate::chain::{ActiveChain, ChainHost, ChainProgram, ChainState};
use crate::convert::{Declaration, build_contribution};
use crate::error::{ModFault, ModFaultKind, ModHostError, ModHostResult};
use crate::graph::dependency_order;
use crate::loader::{PackageModuleLoader, SDK_URL};
use crate::package::ModPackage;
use crate::{ChainProfiler, ModVmLimits};
use kk_mod_api::{
    Contribution, ContributionPlan, FreezeOptions, Key, Number, SlotOutcome, Source, builtin,
    freeze_with_builtin,
};
use kk_vm::budget::{ExecutionLimits, ResourceState, Usage, VmBudget};

/// 从 TryCatch 作用域提取异常文本。宏形态绕开 v8 作用域类型的
/// 泛型参数书写——这些类型按 Deref 链参与强制转换，标注即断链。
macro_rules! scope_error {
    ($tc:expr, $context:expr, $fallback:expr) => {{
        let detail = $tc
            .exception()
            .map(|exception| exception.to_rust_string_lossy($tc))
            .unwrap_or_else(|| $fallback.to_string());
        ModHostError::new($context, detail)
    }};
}

mod registry;
pub(crate) use registry::Registry;
mod ops;
use ops::kk_mod_host;
mod bootstrap;
mod registration;
pub use registration::{PlayerModulePackage, RegisterOptions, Registered};
mod execution;
mod installation;
use execution::InstalledChain;
mod budget;
use budget::BudgetPhase;
mod diagnostics;
mod lifecycle;

pub struct ModRealm {
    // 看门狗必须先于 isolate 回收；排队回调的数据最后回收。
    budget: VmBudget,
    runtime: JsRuntime,
    executor: tokio::runtime::Runtime,
    loader_packages: Rc<RefCell<BTreeMap<PathBuf, Rc<ModPackage>>>>,
    sdk_create_api: v8::Global<v8::Function>,
    sdk_seal: v8::Global<v8::Function>,
    sdk_dispatch: v8::Global<v8::Function>,
    sdk_install_chains: v8::Global<v8::Function>,
    sdk_install_rules: v8::Global<v8::Function>,
    sdk_run_chain: v8::Global<v8::Function>,
    chains: BTreeMap<String, InstalledChain>,
    inactive_slots: BTreeSet<String>,
    chain_sequence: u64,
    limits: ModVmLimits,
    settlement_usage: Option<Usage>,
    profiler: Option<ChainProfiler>,
    _budget_keepalive: Arc<ResourceState>,
    isolate_order: kk_vm::isolate_order::IsolateOrder,
}

#[cfg(test)]
mod sdk_wiring_tests;
