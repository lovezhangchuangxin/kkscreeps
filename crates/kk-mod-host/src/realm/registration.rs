//! 逐包注册、密封、冻结与玩家模块导出；失败统一毒化注册状态。
use super::*;

/// 注册选项：逐包配置（进入 `api.config` 与配置摘要）与冻结选项。
#[derive(Clone, Debug, Default)]
pub struct RegisterOptions {
    pub configs: BTreeMap<String, Value>,
    pub freeze: FreezeOptions,
    /// 新世界启动期的显式内容选择；默认为空（安装全部发现的包）。
    /// 属于分片私有启动快照：恢复重建沿同一选择重新解析并核对 PlanHash。
    pub selection: crate::ContentSelection,
    /// 必须与 realm 构造配置一致，恢复使用原配置。
    pub vm_limits: ModVmLimits,
    /// 诊断计时，默认关闭；不参与玩法计划身份，恢复沿用同一采集句柄。
    pub profiler: Option<ChainProfiler>,
    /// runtime 连续可归责结算故障的停用阈值；None 默认关闭自动停用。
    /// 不参与玩法 PlanHash，正整数由类型保证，恢复保留启动配置。
    pub fault_disable_threshold: Option<std::num::NonZeroU32>,
}

/// 一次成功注册的产物。处理器函数仍留在 realm 的 VM 内；
/// 丢弃 realm 即丢弃函数表，`executables` 只是键清单。
#[derive(Clone, Debug)]
pub struct Registered {
    /// 冻结组合及活动执行链的诊断；使用与链安装相同的选择逻辑。
    pub composition: crate::CompositionReport,
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

impl ModRealm {
    /// 依拓扑序注册全部包并冻结计划。注册是一次性的：成功后密封；
    /// **失败同样毒化 realm**（V8 模块缓存与 SDK 绑定表已部分消费，
    /// 重试不可能干净），再调用 `register` 一律拒绝。
    pub fn register(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
    ) -> ModHostResult<Registered> {
        self.register_with_disabled(packages, options, &BTreeSet::new())
    }

    /// 按完整包冻结原计划，仅屏蔽结算执行。调用方须先计算依赖停用闭包；
    /// 注册函数、对象/规则声明与玩家面仍保留，不能据此隔离有恶意的包。
    pub fn register_with_disabled(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
        disabled: &BTreeSet<String>,
    ) -> ModHostResult<Registered> {
        if let Some(id) = disabled.iter().find(|id| !packages.contains_key(*id)) {
            return Err(ModHostError::new("register", format!("未知停用 Mod {id}")));
        }
        let sealed = self
            .runtime
            .op_state()
            .borrow()
            .borrow::<Registry>()
            .is_sealed();
        if sealed {
            return Err(ModHostError::new("register", "realm 已密封，不能再次注册"));
        }
        let same_profiler = match (&self.profiler, &options.profiler) {
            (None, None) => true,
            (Some(a), Some(b)) => a.same_sink(b),
            _ => false,
        };
        let result = if options.vm_limits != self.limits {
            Err(ModHostError::new(
                "register",
                "VM 限额与构造配置不一致；使用 ModRealm::with_limits",
            ))
        } else if !same_profiler {
            Err(ModHostError::new(
                "register",
                "诊断句柄与构造配置不一致；使用 ModRealm::with_options",
            ))
        } else {
            self.with_budget(BudgetPhase::Registration, |realm| {
                realm.register_inner(packages, options, disabled)
            })
        };
        if result.is_err() {
            let cell = self.runtime.op_state();
            let mut state = cell.borrow_mut();
            let registry = state.borrow_mut::<Registry>();
            registry.poison();
        }
        result
    }

    fn register_inner(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
        disabled: &BTreeSet<String>,
    ) -> ModHostResult<Registered> {
        // 内容选择先于装载解析：未选包不进模块加载器、不注册、不导出
        // 玩家面；配置与停用输入指向未选包属于配置错误，在此整体拒绝。
        // 无显式选择时 resolve 返回 None，直接沿用发现的全部包，不复制包表。
        let (filtered, selection_report) =
            crate::package::resolve_selection(packages, &options.selection)?;
        let selected: &BTreeMap<String, ModPackage> = filtered.as_ref().unwrap_or(packages);
        if let Some(id) = disabled.iter().find(|id| !selected.contains_key(*id)) {
            return Err(ModHostError::new(
                "register",
                format!("停用的 Mod {id} 未被内容选择安装"),
            ));
        }
        if let Some(id) = options
            .configs
            .keys()
            .find(|id| !selected.contains_key(*id))
        {
            return Err(ModHostError::new(
                "register",
                format!("配置指定了未选择的包 {id}"),
            ));
        }
        let order = dependency_order(selected)?;
        {
            let mut loader = self.loader_packages.borrow_mut();
            loader.clear();
            for package in selected.values() {
                loader.insert(package.root.clone(), Rc::new(package.clone()));
            }
        }
        let mut collected: Vec<(String, Source, Vec<Declaration>)> = Vec::new();
        for id in &order {
            let package = &selected[id];
            let config = options.configs.get(id);
            {
                let cell = self.runtime.op_state();
                let mut state = cell.borrow_mut();
                let registry = state.borrow_mut::<Registry>();
                registry.begin_package(id)?;
            }
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
            self.check_registration_fault(id)?;
            self.call_register_function(mod_id, id, config)?;
            self.check_registration_fault(id)?;
            let declared = {
                let cell = self.runtime.op_state();
                let mut state = cell.borrow_mut();
                let registry = state.borrow_mut::<Registry>();
                registry.finish_package()?
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
            .seal();
        let mut contributions = Vec::with_capacity(collected.len());
        for (id, source, declarations) in collected {
            contributions.push(build_contribution(
                &id,
                source,
                &declarations,
                &executables,
            )?);
        }
        let plan = freeze_with_builtin(
            builtin::link(),
            contributions.to_vec(),
            options.freeze.clone(),
        )?;
        let composition = crate::CompositionReport::build(
            &plan,
            &contributions,
            selected,
            &options.freeze,
            disabled,
            &selection_report,
        )
        .map_err(|error| ModHostError::new("composition", error))?;
        let closed = composition
            .with_disabled(disabled)
            .map_err(|error| ModHostError::new("composition", error))?;
        if closed.disabled_mods != *disabled {
            return Err(ModHostError::new(
                "register",
                format!("停用集合未覆盖依赖闭包：{:?}", closed.disabled_mods),
            ));
        }
        self.install_chains(&plan, &contributions, disabled)?;
        let mut player_modules = Vec::new();
        for id in &order {
            let Some(entry) = selected[id].manifest.player.clone() else {
                continue;
            };
            let mut files = BTreeMap::new();
            for (path, bytes) in &selected[id].files {
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
            composition,
            plan,
            contributions,
            executables,
            player_modules,
        })
    }

    fn check_registration_fault(&self, id: &str) -> ModHostResult<()> {
        let state = self.runtime.op_state();
        let state = state.borrow();
        state.borrow::<Registry>().check_fault(id)
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
        let keys: Vec<String> = crate::v8_convert::from_v8(tc, result)
            .map_err(|error| ModHostError::new("seal", format!("绑定表序列化失败：{error}")))?;
        let mut executables = BTreeSet::new();
        for key in keys {
            let key = Key::new(&key)
                .map_err(|message| ModHostError::new("seal", format!("{message}（键 {key:?}）")))?;
            executables.insert(key);
        }
        Ok(executables)
    }
}
