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

use deno_core::v8;
use deno_core::{
    JsRuntime, ModuleId, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, ResolutionKind,
    RuntimeOptions, extension, op2, resolve_import,
};
use deno_error::JsErrorBox;
use serde_json::Value;

use crate::chain::{
    ActiveChain, ChainHost, ChainProgram, ChainState, op_mod_chain_fault, op_mod_chain_native,
    op_mod_chain_read, op_mod_chain_write,
};
use crate::convert::{Declaration, build_contribution};
use crate::error::{ModHostError, ModHostResult};
use crate::graph::dependency_order;
use crate::package::ModPackage;
use kk_mod_api::{
    Contribution, ContributionPlan, FreezeOptions, Key, Source, builtin, freeze_with_builtin,
};

/// SDK 虚拟模块的说明符与解析后的 URL。
const SDK_SPECIFIER: &str = "@kkscreeps/mod";
const SDK_URL: &str = "kkmods:sdk";

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

/// 注册期共享状态：当前正在注册的包与已收集声明。密封后拒绝一切声明。
struct Registry {
    current: Option<String>,
    declarations: Vec<Declaration>,
    sealed: bool,
}

#[op2]
fn op_mod_declare(
    state: &mut OpState,
    #[string] kind: String,
    #[serde] value: serde_json::Value,
) -> Result<(), JsErrorBox> {
    let (namespace, sealed) = {
        let registry = state.borrow::<Registry>();
        (registry.current.clone(), registry.sealed)
    };
    let Some(namespace) = namespace else {
        return Err(JsErrorBox::generic("注册不在进行中（宿主未开始或已收尾）"));
    };
    if sealed {
        return Err(JsErrorBox::generic("注册已密封"));
    }
    state
        .borrow_mut::<Registry>()
        .declarations
        .push(Declaration {
            namespace,
            kind,
            value,
        });
    Ok(())
}

extension!(
    kk_mod_host,
    ops = [
        op_mod_declare,
        op_mod_chain_read,
        op_mod_chain_write,
        op_mod_chain_native,
        op_mod_chain_fault
    ]
);

/// 包内模块加载器：字节来自装载期的内存快照，注册期不触碰磁盘。
/// 只允许包内相对导入与 `@kkscreeps/mod`；跨包导入与裸导入拒绝。
struct PackageModuleLoader {
    packages: Rc<RefCell<BTreeMap<PathBuf, Rc<ModPackage>>>>,
}

impl PackageModuleLoader {
    fn load_sync(
        &self,
        specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
    ) -> Result<ModuleSource, JsErrorBox> {
        if specifier.as_str() == SDK_URL {
            let sdk = include_str!("sdk.js").as_bytes().to_vec();
            return Ok(ModuleSource::new(
                ModuleType::JavaScript,
                ModuleSourceCode::Bytes(sdk.into_boxed_slice().into()),
                specifier,
                None,
            ));
        }
        if specifier.scheme() != "file" {
            return Err(JsErrorBox::generic(format!(
                "仅支持包内 file 模块：{specifier}"
            )));
        }
        let path = specifier
            .to_file_path()
            .map_err(|_| JsErrorBox::generic(format!("非法模块路径：{specifier}")))?;
        let package = self
            .packages
            .borrow()
            .values()
            .find(|package| path.starts_with(&package.root))
            .cloned();
        let Some(package) = package else {
            return Err(JsErrorBox::generic(format!(
                "模块在全部包根之外：{}",
                path.display()
            )));
        };
        if let Some(referrer) = maybe_referrer
            && referrer.specifier.scheme() == "file"
        {
            let referrer_path = referrer
                .specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic("非法引用者路径"))?;
            if !referrer_path.starts_with(&package.root) {
                return Err(JsErrorBox::generic(format!(
                    "跨包导入被拒绝：{} 引用 {}",
                    referrer_path.display(),
                    path.display()
                )));
            }
        }
        let relative = path
            .strip_prefix(&package.root)
            .map_err(|_| JsErrorBox::generic("模块不在包根内"))?;
        let bytes = package
            .file(relative)
            .ok_or_else(|| JsErrorBox::generic(format!("包内没有模块：{}", path.display())))?;
        let code = String::from_utf8(bytes.to_vec())
            .map_err(|_| JsErrorBox::generic(format!("模块须为 UTF-8：{}", path.display())))?;
        Ok(ModuleSource::new(
            ModuleType::JavaScript,
            ModuleSourceCode::Bytes(code.into_bytes().into_boxed_slice().into()),
            specifier,
            None,
        ))
    }
}

impl ModuleLoader for PackageModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        kind: ResolutionKind,
    ) -> Result<ModuleSpecifier, JsErrorBox> {
        if specifier == SDK_SPECIFIER || specifier == SDK_URL {
            return ModuleSpecifier::parse(SDK_URL)
                .map_err(|error| JsErrorBox::generic(error.to_string()));
        }
        // 动态导入与 import.meta.resolve 一并拒绝：动态导入的根装载没有
        // referrer，跨包一致性检查结构性缺席，静态导入图是唯一的装载形态。
        if matches!(kind, ResolutionKind::DynamicImport) {
            return Err(JsErrorBox::generic(
                "不支持动态导入：注册面只接受静态导入（包内相对路径与 @kkscreeps/mod）",
            ));
        }
        if referrer.starts_with("kkmods:") {
            return Err(JsErrorBox::generic("SDK 模块不导入任何依赖"));
        }
        let absolute = specifier.contains("://");
        if absolute && !specifier.starts_with("file://") {
            return Err(JsErrorBox::generic(format!(
                "不支持的外部导入 {specifier:?}：仅包内相对路径与 @kkscreeps/mod"
            )));
        }
        if !absolute {
            let bare = !specifier.starts_with("./")
                && !specifier.starts_with("../")
                && !specifier.starts_with('/');
            if bare {
                return Err(JsErrorBox::generic(format!(
                    "不支持的外部导入 {specifier:?}：仅包内相对路径与 @kkscreeps/mod"
                )));
            }
        }
        // 绝对 file URL 是宿主装载入口的形态；模块自身的绝对 file 导入
        // 由 load 侧的包根包含约束拦截，二者共用此分支。
        resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        specifier: &ModuleSpecifier,
        maybe_referrer: Option<&ModuleLoadReferrer>,
        _options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        ModuleLoadResponse::Sync(self.load_sync(specifier, maybe_referrer))
    }
}

/// 注册选项：逐包配置（进入 `api.config` 与配置摘要）与冻结选项。
#[derive(Clone, Debug, Default)]
pub struct RegisterOptions {
    pub configs: BTreeMap<String, Value>,
    pub freeze: FreezeOptions,
}

/// 一次成功注册的产物。处理器函数仍留在 realm 的 VM 内；
/// 丢弃 realm 即丢弃函数表，`executables` 只是键清单。
#[derive(Clone, Debug)]
pub struct Registered {
    pub plan: ContributionPlan,
    /// Mod 贡献，依依赖拓扑序（被依赖者在前）；不含 builtin 贡献。
    pub contributions: Vec<Contribution>,
    /// 有注册函数的绑定键全集。
    pub executables: BTreeSet<Key>,
    /// 声明了玩家面的包：玩家模块（入口与全部可解码源文件）依依赖
    /// 拓扑序导出，供宿主装进每个玩家沙箱执行。
    pub player_modules: Vec<PlayerModulePackage>,
}

/// 一个包的玩家面模块集。`files` 是包快照内全部 UTF-8 文件（相对
/// 路径键）——玩家模块的包内相对导入在玩家 VM 内按同一映射解析；
/// 非包内与动态导入由玩家侧加载器拒绝（同注册 realm 的纪律）。
#[derive(Clone, Debug)]
pub struct PlayerModulePackage {
    pub namespace: String,
    /// 入口模块的包内相对路径（manifest 的 player 声明）。
    pub entry: String,
    pub files: BTreeMap<String, String>,
}

pub struct ModRealm {
    runtime: JsRuntime,
    executor: tokio::runtime::Runtime,
    loader_packages: Rc<RefCell<BTreeMap<PathBuf, Rc<ModPackage>>>>,
    sdk_create_api: v8::Global<v8::Function>,
    sdk_seal: v8::Global<v8::Function>,
    sdk_dispatch: v8::Global<v8::Function>,
    sdk_install_chains: v8::Global<v8::Function>,
    sdk_run_chain: v8::Global<v8::Function>,
    chains: BTreeMap<String, (bool, Rc<BTreeSet<String>>)>,
    chain_sequence: u64,
}

impl ModRealm {
    pub fn new() -> ModHostResult<Self> {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| ModHostError::new("realm", format!("创建执行器失败：{error}")))?;
        let _entered = executor.enter();
        let loader_packages = Rc::new(RefCell::new(BTreeMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![kk_mod_host::init()],
            module_loader: Some(Rc::new(PackageModuleLoader {
                packages: loader_packages.clone(),
            })),
            ..Default::default()
        });
        {
            let op_state = runtime.op_state();
            let mut op_state = op_state.borrow_mut();
            op_state.put(Registry {
                current: None,
                declarations: Vec::new(),
                sealed: false,
            });
            op_state.put(ChainState::default());
        }
        let sdk_url = ModuleSpecifier::parse(SDK_URL)
            .map_err(|error| ModHostError::new("realm", error.to_string()))?;
        let executor_ref = &executor;
        let runtime_ref = &mut runtime;
        let mod_id: ModuleId = executor_ref
            .block_on(async {
                let id = runtime_ref.load_side_es_module(&sdk_url).await?;
                let evaluate = runtime_ref.mod_evaluate(id);
                runtime_ref.run_event_loop(Default::default()).await?;
                evaluate.await?;
                Ok::<_, deno_core::error::CoreError>(id)
            })
            .map_err(|error| ModHostError::new("realm", format!("SDK 模块装载失败：{error}")))?;
        let namespace = runtime
            .get_module_namespace(mod_id)
            .map_err(|error| ModHostError::new("realm", format!("SDK 命名空间不可用：{error}")))?;
        let (sdk_create_api, sdk_seal, sdk_dispatch, sdk_install_chains, sdk_run_chain) = {
            let runtime_ref = &mut runtime;
            deno_core::scope!(scope, runtime_ref);
            let namespace = v8::Local::new(scope, namespace.clone());
            // 与 kk-runner 的句柄捕获同法：经 Deref 链传 scope，
            // 不手写作用域泛型参数（标注会断开强制转换链）。
            macro_rules! grab {
                ($name:literal) => {{
                    let key = v8::String::new_from_utf8(
                        scope,
                        $name.as_bytes(),
                        v8::NewStringType::Normal,
                    )
                    .ok_or_else(|| ModHostError::new("realm", "键名构造失败"))?;
                    let value = namespace.get(scope, key.into()).ok_or_else(|| {
                        ModHostError::new("realm", format!("SDK 缺少导出 {}", $name))
                    })?;
                    let function = v8::Local::<v8::Function>::try_from(value).map_err(|_| {
                        ModHostError::new("realm", format!("SDK 导出 {} 非函数", $name))
                    })?;
                    v8::Global::new(scope, function)
                }};
            }
            (
                grab!("createApi"),
                grab!("seal"),
                grab!("dispatch"),
                grab!("installChains"),
                grab!("runChain"),
            )
        };
        Ok(Self {
            runtime,
            executor,
            loader_packages,
            sdk_create_api,
            sdk_seal,
            sdk_dispatch,
            sdk_install_chains,
            sdk_run_chain,
            chains: BTreeMap::new(),
            chain_sequence: 0,
        })
    }

    /// 结算期调用一个已注册的处理器函数：`ctx`（tick/room/state 等）以
    /// JSON 传入，处理器同步执行并返回其终值。处理器异常、返回 Promise
    /// 或结果不可序列化都以错误返回——由调用方据此使本轮候选失败。
    /// 调用方负责用返回的 state 写回候选域。
    pub fn invoke(&mut self, binding: &str, ctx: Value) -> ModHostResult<Value> {
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let dispatch = v8::Local::new(tc, self.sdk_dispatch.clone());
        let key = v8::String::new_from_utf8(tc, binding.as_bytes(), v8::NewStringType::Normal)
            .ok_or_else(|| ModHostError::new("invoke", "键名构造失败"))?;
        let ctx_local = deno_core::serde_v8::to_v8(tc, &ctx).map_err(|error| {
            ModHostError::new("invoke", format!("处理器上下文序列化失败：{error}"))
        })?;
        let receiver = v8::undefined(tc).into();
        let result = dispatch
            .call(tc, receiver, &[key.into(), ctx_local])
            .ok_or_else(|| scope_error!(tc, "invoke", "处理器调用失败"))?;
        deno_core::serde_v8::from_v8(tc, result)
            .map_err(|error| ModHostError::new("invoke", format!("处理器结果序列化失败：{error}")))
    }

    /// 一次进入 JS 完成整个槽。候选能力仅在本次调用期间放入 OpState，
    /// 无论返回或抛错都取出并销毁；已捕获的 next/候选访问故障仍使调用失败。
    pub fn invoke_chain(&mut self, slot: &str, host: Box<dyn ChainHost>) -> ModHostResult<()> {
        let program = self
            .chains
            .get(slot)
            .ok_or_else(|| ModHostError::new("chain", format!("槽 {slot} 未安装")))?;
        self.chain_sequence = self
            .chain_sequence
            .checked_add(1)
            .ok_or_else(|| ModHostError::new("chain", "槽调用序号耗尽"))?;
        let token = self.chain_sequence.to_string();
        let active = ActiveChain {
            token: token.clone(),
            bindings: program.1.clone(),
            host,
            fault: None,
            native_called: false,
            native_allowed: program.0,
        };
        let op_state = self.runtime.op_state();
        if op_state.borrow().borrow::<ChainState>().active.is_some() {
            return Err(ModHostError::new("chain", "不可重入整槽宿主"));
        }
        op_state.borrow_mut().borrow_mut::<ChainState>().active = Some(active);
        let result = self.call_chain(slot, &token);
        // 尾部清理必须先于向外传播错误，RoomCtx 不得残留到结算的取回阶段。
        let active = op_state
            .borrow_mut()
            .borrow_mut::<ChainState>()
            .active
            .take()
            .expect("本次调用已安装候选能力");
        if let Some(fault) = active.fault {
            return Err(ModHostError::new(slot.to_string(), fault));
        }
        result
    }

    fn call_chain(&mut self, slot: &str, token: &str) -> ModHostResult<()> {
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let run = v8::Local::new(tc, self.sdk_run_chain.clone());
        let slot =
            v8::String::new(tc, slot).ok_or_else(|| ModHostError::new("chain", "槽键构造失败"))?;
        let token = v8::String::new(tc, token)
            .ok_or_else(|| ModHostError::new("chain", "能力标识构造失败"))?;
        let receiver = v8::undefined(tc).into();
        run.call(tc, receiver, &[slot.into(), token.into()])
            .ok_or_else(|| scope_error!(tc, "chain", "槽调用失败"))?;
        Ok(())
    }

    fn install_chains(&mut self, plan: &ContributionPlan) -> ModHostResult<()> {
        let programs =
            ChainProgram::from_plan(plan).map_err(|error| ModHostError::new("chain", error))?;
        {
            let runtime_ref = &mut self.runtime;
            deno_core::scope!(scope, runtime_ref);
            v8::tc_scope!(let tc, scope);
            let install = v8::Local::new(tc, self.sdk_install_chains.clone());
            let value = deno_core::serde_v8::to_v8(tc, &programs)
                .map_err(|error| ModHostError::new("chain", error.to_string()))?;
            let receiver = v8::undefined(tc).into();
            install
                .call(tc, receiver, &[value])
                .ok_or_else(|| scope_error!(tc, "chain", "链安装失败"))?;
        }
        self.chains = programs
            .into_iter()
            .map(|program| {
                (
                    program.key.clone(),
                    (program.native_center, Rc::new(program.bindings())),
                )
            })
            .collect();
        Ok(())
    }

    /// 依拓扑序注册全部包并冻结计划。注册是一次性的：成功后密封；
    /// **失败同样毒化 realm**（V8 模块缓存与 SDK 绑定表已部分消费，
    /// 重试不可能干净），再调用 `register` 一律拒绝。
    pub fn register(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
    ) -> ModHostResult<Registered> {
        let sealed = self.runtime.op_state().borrow().borrow::<Registry>().sealed;
        if sealed {
            return Err(ModHostError::new("register", "realm 已密封，不能再次注册"));
        }
        let result = self.register_inner(packages, options);
        if result.is_err() {
            let cell = self.runtime.op_state();
            let mut state = cell.borrow_mut();
            let registry = state.borrow_mut::<Registry>();
            registry.current = None;
            registry.sealed = true;
        }
        result
    }

    fn register_inner(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
    ) -> ModHostResult<Registered> {
        let order = dependency_order(packages)?;
        {
            let mut loader = self.loader_packages.borrow_mut();
            loader.clear();
            for package in packages.values() {
                loader.insert(package.root.clone(), Rc::new(package.clone()));
            }
        }
        let mut collected: Vec<(String, Source, Vec<Declaration>)> = Vec::new();
        for id in &order {
            let package = &packages[id];
            let config = options.configs.get(id);
            let start = {
                let cell = self.runtime.op_state();
                let mut state = cell.borrow_mut();
                let registry = state.borrow_mut::<Registry>();
                registry.current = Some(id.clone());
                registry.declarations.len()
            };
            let entry_url = ModuleSpecifier::from_file_path(package.entry_path())
                .map_err(|_| ModHostError::new(id.clone(), "入口路径无法转为模块 URL"))?;
            let executor_ref = &self.executor;
            let runtime_ref = &mut self.runtime;
            let mod_id: ModuleId = executor_ref
                .block_on(async {
                    let id = runtime_ref.load_side_es_module(&entry_url).await?;
                    let evaluate = runtime_ref.mod_evaluate(id);
                    runtime_ref.run_event_loop(Default::default()).await?;
                    evaluate.await?;
                    Ok::<_, deno_core::error::CoreError>(id)
                })
                .map_err(|error| {
                    ModHostError::new(id.clone(), format!("入口模块装载或求值失败：{error}"))
                })?;
            self.call_register_function(mod_id, id, config)?;
            let declared = {
                let cell = self.runtime.op_state();
                let mut state = cell.borrow_mut();
                let registry = state.borrow_mut::<Registry>();
                registry.current = None;
                registry.declarations.drain(start..).collect::<Vec<_>>()
            };
            let source = Source {
                namespace: id.clone(),
                api_version: 1,
                revision: package.manifest.version.clone(),
                code_digest: package.code_digest(),
                config_digest: ModPackage::config_digest(config),
                player_api_digest: package.player_api_digest(),
            };
            collected.push((id.clone(), source, declared));
        }
        let executables = self.call_seal()?;
        self.runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<Registry>()
            .sealed = true;
        let mut contributions = Vec::with_capacity(collected.len());
        for (id, source, declarations) in collected {
            contributions.push(build_contribution(
                &id,
                source,
                &declarations,
                &executables,
            )?);
        }
        let plan = freeze_with_builtin(builtin::link(), contributions.to_vec(), options.freeze)?;
        self.install_chains(&plan)?;
        let mut player_modules = Vec::new();
        for id in &order {
            let Some(entry) = packages[id].manifest.player.clone() else {
                continue;
            };
            let mut files = BTreeMap::new();
            for (path, bytes) in &packages[id].files {
                let Some(path) = path.to_str() else {
                    continue;
                };
                let Ok(source) = std::str::from_utf8(bytes) else {
                    continue;
                };
                files.insert(path.to_string(), source.to_string());
            }
            // 入口必须在导出集内（UTF-8 且路径可作包内键）：非 UTF-8 或
            // URL 特殊字符的入口在这里显式失败，而不是推迟到每个玩家
            // 沙箱 spawn 才以误导文案失败。
            if !files.contains_key(&entry) {
                return Err(ModHostError::new(
                    id.clone(),
                    format!("玩家模块入口 {entry:?} 不可装载（须为包内 UTF-8 文件）"),
                ));
            }
            player_modules.push(PlayerModulePackage {
                namespace: id.clone(),
                entry,
                files,
            });
        }
        Ok(Registered {
            plan,
            contributions,
            executables,
            player_modules,
        })
    }

    /// 调用包入口的 default 导出执行注册。register 函数必须同步返回，
    /// 返回 Promise 视为注册错误（与结算回调的同一条纪律）。
    fn call_register_function(
        &mut self,
        mod_id: ModuleId,
        id: &str,
        config: Option<&Value>,
    ) -> ModHostResult<()> {
        let namespace = self.runtime.get_module_namespace(mod_id).map_err(|error| {
            ModHostError::new(id.to_string(), format!("模块命名空间不可用：{error}"))
        })?;
        let error_context = id.to_string();
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let namespace = v8::Local::new(tc, namespace.clone());
        let default_key = v8::String::new_from_utf8(tc, b"default", v8::NewStringType::Normal)
            .ok_or_else(|| ModHostError::new(error_context.clone(), "键名构造失败"))?;
        let default = namespace
            .get(tc, default_key.into())
            .ok_or_else(|| ModHostError::new(error_context.clone(), "入口模块缺少 default 导出"))?;
        let register = v8::Local::<v8::Function>::try_from(default).map_err(|_| {
            ModHostError::new(error_context.clone(), "入口模块 default 导出须为函数")
        })?;

        let create_api = v8::Local::new(tc, self.sdk_create_api.clone());
        let namespace_name =
            v8::String::new_from_utf8(tc, id.as_bytes(), v8::NewStringType::Normal)
                .ok_or_else(|| ModHostError::new(error_context.clone(), "键名构造失败"))?;
        let config_value = match config {
            Some(value) => deno_core::serde_v8::to_v8(tc, value).map_err(|error| {
                ModHostError::new(error_context.clone(), format!("配置序列化失败：{error}"))
            })?,
            None => v8::undefined(tc).into(),
        };
        let receiver = v8::undefined(tc).into();
        let api = create_api
            .call(tc, receiver, &[namespace_name.into(), config_value])
            .ok_or_else(|| scope_error!(tc, error_context.clone(), "createApi 调用失败"))?;
        let register_result = register
            .call(tc, receiver, &[api])
            .ok_or_else(|| scope_error!(tc, error_context.clone(), "register 执行失败"))?;
        if register_result.is_promise() {
            return Err(ModHostError::new(
                error_context,
                "register 不得返回 Promise（注册必须同步完成）",
            ));
        }
        Ok(())
    }

    /// 调用 SDK 密封并取回绑定函数键清单。
    fn call_seal(&mut self) -> ModHostResult<BTreeSet<Key>> {
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let seal = v8::Local::new(tc, self.sdk_seal.clone());
        let receiver = v8::undefined(tc).into();
        let result = seal
            .call(tc, receiver, &[])
            .ok_or_else(|| scope_error!(tc, "seal", "seal 调用失败"))?;
        let keys: Vec<String> = deno_core::serde_v8::from_v8(tc, result)
            .map_err(|error| ModHostError::new("seal", format!("绑定表序列化失败：{error}")))?;
        let mut executables = BTreeSet::new();
        for key in keys {
            let key = Key::new(&key)
                .map_err(|message| ModHostError::new("seal", format!("{message}（键 {key:?}）")))?;
            executables.insert(key);
        }
        Ok(executables)
    }

    /// 在 realm 内执行一段诊断脚本（测试与排障用；realm 本身是可信面，
    /// 这里的额外暴露只有宿主自己写下的代码）。
    pub fn evaluate_probe(&mut self, code: &str) -> ModHostResult<Value> {
        let value = self
            .runtime
            .execute_script("mod-host-probe", code.to_string())
            .map_err(|error| ModHostError::new("probe", format!("{error}")))?;
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        let local = v8::Local::new(scope, value);
        deno_core::serde_v8::from_v8(scope, local)
            .map_err(|error| ModHostError::new("probe", format!("结果序列化失败：{error}")))
    }
}
