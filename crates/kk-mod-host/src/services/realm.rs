//! 线程亲和后台 V8；注册函数同步，生命周期回调在有限预算内驱动异步循环。
use super::ops::{ProbeState, kk_services};
use super::*;
use crate::package::{PackageModuleLoader, dependency_order};
use deno_core::{JsRuntime, ModuleSpecifier, RuntimeOptions, v8};
use kk_vm::budget::{ExecutionLimits, ResourceState, VmBudget};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub(super) struct ServiceRealm {
    // 先停看门狗，随后回收 isolate，最后销毁回调数据。
    budget: VmBudget,
    runtime: JsRuntime,
    executor: Option<tokio::runtime::Runtime>,
    create_api: v8::Global<v8::Function>,
    seal: v8::Global<v8::Function>,
    dispatch: v8::Global<v8::Function>,
    packages: Rc<RefCell<BTreeMap<PathBuf, Rc<ModPackage>>>>,
    options: ServiceOptions,
    sequence: u32,
    _keepalive: Arc<ResourceState>,
}

impl ServiceRealm {
    pub fn new(options: ServiceOptions) -> ModHostResult<Self> {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| error(e.to_string()))?;
        let _entered = executor.enter();
        let packages = Rc::new(RefCell::new(BTreeMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![kk_services::init()],
            module_loader: Some(Rc::new(PackageModuleLoader {
                services: true,
                packages: packages.clone(),
            })),
            create_params: Some(
                v8::CreateParams::default().heap_limits(0, options.vm_limits.memory_bytes),
            ),
            ..Default::default()
        });
        // SAFETY: 本线程使用预算，字段顺序使回调数据存活到 isolate 回收后。
        let budget = unsafe {
            VmBudget::new(
                runtime.v8_isolate().thread_safe_handle(),
                options.vm_limits.memory_bytes,
            )
        };
        let keepalive = budget.keepalive();
        runtime.add_near_heap_limit_callback(budget.near_heap_callback());
        runtime.op_state().borrow_mut().put(ProbeState {
            options: options.clone(),
            active: None,
            registering: None,
            registered: vec![],
            calls: 0,
            pending: 0,
            fault: None,
        });
        let url = ModuleSpecifier::parse("kkservices:sdk").map_err(|e| error(e.to_string()))?;
        let module = executor
            .block_on(async {
                let module = runtime.load_side_es_module(&url).await?;
                let evaluation = runtime.mod_evaluate(module);
                runtime.run_event_loop(Default::default()).await?;
                evaluation.await?;
                Ok::<_, deno_core::error::CoreError>(module)
            })
            .map_err(|e| error(e.to_string()))?;
        let namespace = runtime
            .get_module_namespace(module)
            .map_err(|e| error(e.to_string()))?;
        let (create_api, seal, dispatch) = {
            deno_core::scope!(scope, &mut runtime);
            let namespace = v8::Local::new(scope, namespace);
            let grab = |name: &str| -> ModHostResult<v8::Global<v8::Function>> {
                let key = v8::String::new(scope, name).ok_or_else(|| error("SDK 键构造失败"))?;
                let value = namespace
                    .get(scope, key.into())
                    .ok_or_else(|| error("后台 SDK 导出缺失"))?;
                let function = v8::Local::<v8::Function>::try_from(value)
                    .map_err(|_| error("后台 SDK 导出须为函数"))?;
                Ok(v8::Global::new(scope, function))
            };
            (grab("createApi")?, grab("seal")?, grab("dispatch")?)
        };
        drop(_entered);
        Ok(Self {
            budget,
            runtime,
            executor: Some(executor),
            create_api,
            seal,
            dispatch,
            packages,
            options,
            sequence: 0,
            _keepalive: keepalive,
        })
    }

    fn budgeted<T>(
        &mut self,
        limits: ExecutionLimits,
        operation: impl FnOnce(&mut Self) -> ModHostResult<T>,
    ) -> ModHostResult<T> {
        let executor_handle = self.executor.as_ref().expect("执行器存活").handle().clone();
        let _entered = executor_handle.enter();
        self.budget
            .start(limits)
            .map_err(|e| error(e.to_string()))?;
        let result = operation(self);
        self.budget
            .finish(self.runtime.v8_isolate())
            .map_err(|e| error(e.to_string()))?;
        let cell = self.runtime.op_state();
        let state = cell.borrow();
        if let Some(fault) = &state.borrow::<ProbeState>().fault {
            return Err(error(fault));
        }
        result
    }

    fn call(
        &mut self,
        function: v8::Global<v8::Function>,
        args: &[Value],
    ) -> ModHostResult<v8::Global<v8::Value>> {
        deno_core::scope!(scope, &mut self.runtime);
        v8::tc_scope!(let tc,scope);
        let function = v8::Local::new(tc, function);
        let values = args
            .iter()
            .map(|arg| deno_core::serde_v8::to_v8(tc, arg).map_err(|e| error(e.to_string())))
            .collect::<ModHostResult<Vec<_>>>()?;
        let receiver = v8::undefined(tc).into();
        let value = function.call(tc, receiver, &values).ok_or_else(|| {
            error(
                tc.exception()
                    .map(|e| e.to_rust_string_lossy(tc))
                    .unwrap_or_else(|| "后台 JS 调用失败".into()),
            )
        })?;
        Ok(v8::Global::new(tc, value))
    }

    fn value(&mut self, value: v8::Global<v8::Value>) -> ModHostResult<Value> {
        deno_core::scope!(scope, &mut self.runtime);
        let local = v8::Local::new(scope, value);
        let value: Value =
            deno_core::serde_v8::from_v8(scope, local).map_err(|e| error(e.to_string()))?;
        check_value(&value, self.options.max_value_bytes)?;
        Ok(value)
    }

    pub fn register(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
    ) -> ModHostResult<Vec<String>> {
        self.budgeted(self.options.vm_limits.registration, |realm| {
            realm.register_inner(packages)
        })
    }

    fn register_inner(
        &mut self,
        packages: &BTreeMap<String, ModPackage>,
    ) -> ModHostResult<Vec<String>> {
        let order = dependency_order(packages)?;
        for package in packages.values() {
            self.packages
                .borrow_mut()
                .insert(package.root.clone(), Rc::new(package.clone()));
        }
        for id in order {
            let package = &packages[&id];
            let Some(entry) = &package.manifest.services else {
                continue;
            };
            self.runtime
                .op_state()
                .borrow_mut()
                .borrow_mut::<ProbeState>()
                .registering = Some(id.clone());
            let url = ModuleSpecifier::from_file_path(package.root.join(entry))
                .map_err(|_| error("后台入口 URL 无效"))?;
            let wall = self.options.vm_limits.registration.wall;
            let module = self
                .executor
                .as_ref()
                .expect("执行器存活")
                .block_on(async {
                    tokio::time::timeout(wall, async {
                        let module = self.runtime.load_side_es_module(&url).await?;
                        let evaluation = self.runtime.mod_evaluate(module);
                        self.runtime.run_event_loop(Default::default()).await?;
                        evaluation.await?;
                        Ok::<_, deno_core::error::CoreError>(module)
                    })
                    .await
                    .map_err(|_| error("后台模块装载墙钟超限"))?
                    .map_err(|e| error(e.to_string()))
                })?;
            let namespace = self
                .runtime
                .get_module_namespace(module)
                .map_err(|e| error(e.to_string()))?;
            let register = {
                deno_core::scope!(scope, &mut self.runtime);
                let namespace = v8::Local::new(scope, namespace);
                let key =
                    v8::String::new(scope, "default").ok_or_else(|| error("入口键构造失败"))?;
                let value = namespace
                    .get(scope, key.into())
                    .ok_or_else(|| error("后台入口缺 default"))?;
                let function = v8::Local::<v8::Function>::try_from(value)
                    .map_err(|_| error("后台入口 default 须为同步注册函数"))?;
                v8::Global::new(scope, function)
            };
            let config = self
                .options
                .configs
                .get(&id)
                .cloned()
                .unwrap_or(Value::Null);
            let api = self.call(self.create_api.clone(), &[Value::String(id), config])?;
            {
                deno_core::scope!(scope, &mut self.runtime);
                v8::tc_scope!(let tc,scope);
                let register = v8::Local::new(tc, register);
                let api = v8::Local::new(tc, api);
                let receiver = v8::undefined(tc).into();
                let value = register.call(tc, receiver, &[api]).ok_or_else(|| {
                    error(
                        tc.exception()
                            .map(|e| e.to_rust_string_lossy(tc))
                            .unwrap_or_else(|| "后台注册失败".into()),
                    )
                })?;
                if value.is_promise() {
                    return Err(error("后台注册不得返回 Promise"));
                }
            }
        }
        self.runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<ProbeState>()
            .registering = None;
        let sealed = self.call(self.seal.clone(), &[])?;
        let keys: Vec<String> =
            serde_json::from_value(self.value(sealed)?).map_err(|e| error(e.to_string()))?;
        if self
            .runtime
            .op_state()
            .borrow()
            .borrow::<ProbeState>()
            .registered
            != keys
        {
            return Err(error("后台绑定表与原生登记不一致"));
        }
        if keys.len() > 1024 {
            return Err(error("后台服务总数超限"));
        }
        for key in &keys {
            kk_mod_api::Key::new(key).map_err(error)?;
        }
        Ok(keys)
    }

    pub fn invoke(
        &mut self,
        key: &str,
        phase: &str,
        args: Value,
        view: Value,
    ) -> ModHostResult<Value> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| error("后台能力序号耗尽"))?;
        let namespace = key
            .split_once(':')
            .ok_or_else(|| error("服务键非法"))?
            .0
            .to_owned();
        {
            let cell = self.runtime.op_state();
            let mut state = cell.borrow_mut();
            let probes = state.borrow_mut::<ProbeState>();
            probes.active = Some((self.sequence, namespace));
            probes.calls = 0;
        }
        let result = self.budgeted(self.options.vm_limits.settlement, |realm| {
            let value = realm.call(
                realm.dispatch.clone(),
                &[
                    Value::String(key.to_owned()),
                    Value::String(phase.to_owned()),
                    args,
                    view,
                    Value::from(realm.sequence),
                ],
            )?;
            let future = realm.runtime.resolve(value);
            let wall = realm.options.vm_limits.settlement.wall;
            let resolved = realm
                .executor
                .as_ref()
                .expect("执行器存活")
                .block_on(async {
                    tokio::time::timeout(
                        wall,
                        realm
                            .runtime
                            .with_event_loop_promise(future, Default::default()),
                    )
                    .await
                    .map_err(|_| error("后台回调墙钟超限"))?
                    .map_err(|e| error(e.to_string()))
                })?;
            let value = realm.value(resolved)?;
            if realm
                .runtime
                .op_state()
                .borrow()
                .borrow::<ProbeState>()
                .pending
                != 0
            {
                return Err(error("后台回调遗留未完成探针"));
            }
            Ok(value)
        });
        self.runtime
            .op_state()
            .borrow_mut()
            .borrow_mut::<ProbeState>()
            .active = None;
        result.map_err(|mut failure| {
            failure.context = format!("background {key} {phase}");
            failure
        })
    }
}

impl Drop for ServiceRealm {
    fn drop(&mut self) {
        // 墙钟故障可以取消等待，但无法中止已经进入 OS 的只读文件任务。
        // 不让阻塞池拖住后台域回收；它不持有 V8 或权威世界，最多留下有限读缓冲。
        if let Some(executor) = self.executor.take() {
            executor.shutdown_background();
        }
    }
}
