//! 构造 VM、装载 SDK 并捕获宿主句柄；保持字段销毁顺序。
use super::*;

impl ModRealm {
    pub fn new() -> ModHostResult<Self> {
        Self::with_limits(ModVmLimits::default())
    }

    pub fn with_limits(limits: ModVmLimits) -> ModHostResult<Self> {
        Self::with_options(&RegisterOptions {
            vm_limits: limits,
            ..Default::default()
        })
    }

    pub fn with_options(options: &RegisterOptions) -> ModHostResult<Self> {
        // 种子 0 是测试/默认口径；生产分片经 with_world_seed 注入世界种子，
        // 玩法随机流与 Date 映射都以它派生（G22.b 协议）。
        Self::with_world_seed(0, options)
    }

    /// 以世界种子构造 realm：随机流域与确定性时间全部由种子派生，种子
    /// 随分片生命周期不变（恢复/切换重建时由调用方传入同一世界种子）。
    pub fn with_world_seed(seed: u64, options: &RegisterOptions) -> ModHostResult<Self> {
        let limits = options.vm_limits.clone();
        let profiler = options.profiler.clone();
        limits.validate()?;
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| ModHostError::new("realm", format!("创建执行器失败：{error}")))?;
        let _entered = executor.enter();
        let loader_packages = Rc::new(RefCell::new(BTreeMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![kk_mod_host::init()],
            module_loader: Some(Rc::new(PackageModuleLoader {
                services: false,
                packages: loader_packages.clone(),
            })),
            create_params: Some(v8::CreateParams::default().heap_limits(0, limits.memory_bytes)),
            op_metrics_factory_fn: profiler.as_ref().map(ChainProfiler::op_metrics),
            ..Default::default()
        });
        // SAFETY: 字段顺序先 join 看门狗、再销毁 runtime，最后 drop keepalive。
        // realm 是线程亲和的，所有预算入口在所属 isolate 的执行线程调用。
        let budget = unsafe {
            VmBudget::new(
                runtime.v8_isolate().thread_safe_handle(),
                limits.memory_bytes,
            )
        };
        let budget_keepalive = budget.keepalive();
        runtime.add_near_heap_limit_callback(budget.near_heap_callback());
        {
            let op_state = runtime.op_state();
            let mut op_state = op_state.borrow_mut();
            op_state.put(Registry::default());
            op_state.put(ChainState::default());
            op_state.put(crate::rng::RngState::new(seed));
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
        let (
            sdk_create_api,
            sdk_seal,
            sdk_dispatch,
            sdk_install_chains,
            sdk_install_rules,
            sdk_run_chain,
        ) = {
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
                grab!("installRules"),
                grab!("runChain"),
            )
        };
        let isolate_order = kk_vm::isolate_order::IsolateOrder::register();
        Ok(Self {
            budget,
            runtime,
            executor,
            loader_packages,
            sdk_create_api,
            sdk_seal,
            sdk_dispatch,
            sdk_install_chains,
            sdk_install_rules,
            sdk_run_chain,
            chains: BTreeMap::new(),
            inactive_slots: BTreeSet::new(),
            chain_sequence: 0,
            limits,
            settlement_usage: None,
            profiler,
            _budget_keepalive: budget_keepalive,
            isolate_order,
        })
    }
}
