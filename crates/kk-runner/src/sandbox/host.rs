//! 捕获宿主 V8 句柄，安装玩家 API，并通过捕获句柄调用宿主。
use super::*;

const LODASH_SRC: &str = include_str!("../../assets/lodash-3.10.1.js");
const CONSTANTS_JSON: &str = include_str!("../../assets/constants.json");
/// 按依赖顺序装配共享工具、模块加载、玩家 API 与宿主函数。
/// 各段通过临时全局 __kkp 共享闭包状态；Rust 捕获宿主函数句柄后删除 __kkp。
/// 因此 90_host 只能注册句柄，不能提前删除共享容器。
pub(super) const PRELUDE_SEGMENTS: [(&str, &str); 24] = [
    (
        "kk-prelude-00-shared",
        include_str!("../prelude/00_shared.js"),
    ),
    (
        "kk-prelude-10-buffer",
        include_str!("../prelude/10_buffer.js"),
    ),
    ("kk-prelude-20-cjs", include_str!("../prelude/20_cjs.js")),
    (
        "kk-prelude-30-game-registry",
        include_str!("../prelude/30_game/_registry.js"),
    ),
    (
        "kk-prelude-30-game-kkpv",
        include_str!("../prelude/30_game/kkpv.js"),
    ),
    (
        "kk-prelude-30-game-room-position",
        include_str!("../prelude/30_game/room-position.js"),
    ),
    (
        "kk-prelude-30-game-room-terrain",
        include_str!("../prelude/30_game/room/terrain.js"),
    ),
    (
        "kk-prelude-30-game-room-visual",
        include_str!("../prelude/30_game/room/visual.js"),
    ),
    (
        "kk-prelude-30-game-room-queries",
        include_str!("../prelude/30_game/room/queries.js"),
    ),
    (
        "kk-prelude-30-game-room-creation",
        include_str!("../prelude/30_game/room/creation.js"),
    ),
    (
        "kk-prelude-30-game-room-pathfinding",
        include_str!("../prelude/30_game/room/pathfinding.js"),
    ),
    (
        "kk-prelude-30-game-room",
        include_str!("../prelude/30_game/room.js"),
    ),
    (
        "kk-prelude-30-game-creep",
        include_str!("../prelude/30_game/creep.js"),
    ),
    (
        "kk-prelude-30-game-creep-resources",
        include_str!("../prelude/30_game/creep_resources.js"),
    ),
    (
        "kk-prelude-30-game-structures",
        include_str!("../prelude/30_game/structures.js"),
    ),
    (
        "kk-prelude-30-game-structures-spawn",
        include_str!("../prelude/30_game/structures_spawn.js"),
    ),
    (
        "kk-prelude-30-game-tombstones",
        include_str!("../prelude/30_game/tombstones.js"),
    ),
    (
        "kk-prelude-30-game-sites-flags",
        include_str!("../prelude/30_game/sites-flags.js"),
    ),
    (
        "kk-prelude-30-game-resources",
        include_str!("../prelude/30_game/resources.js"),
    ),
    (
        "kk-prelude-30-game-path-finder",
        include_str!("../prelude/30_game/path-finder.js"),
    ),
    (
        "kk-prelude-30-game-memory",
        include_str!("../prelude/30_game/memory.js"),
    ),
    (
        "kk-prelude-30-game-market",
        include_str!("../prelude/30_game/market.js"),
    ),
    (
        "kk-prelude-30-game-game",
        include_str!("../prelude/30_game/game.js"),
    ),
    ("kk-prelude-90-host", include_str!("../prelude/90_host.js")),
];

// ---------------------------------------------------------------- 宿主通道私有句柄

/// 持有装配时捕获的宿主函数句柄。
/// Rust 在全部 prelude 执行后捕获 __kkp.host，再删除临时全局 __kkp，
/// 后续生命周期调用通过句柄完成，不查找玩家可替换的全局属性。
pub(super) struct HostFns {
    pub(super) begin_tick: v8::Global<v8::Function>,
    pub(super) begin_tick_kkpv: v8::Global<v8::Function>,
    pub(super) run_loop: v8::Global<v8::Function>,
    pub(super) run_expr: v8::Global<v8::Function>,
    pub(super) end_tick: v8::Global<v8::Function>,
    pub(super) reset_modules: v8::Global<v8::Function>,
    pub(super) set_raw_memory: v8::Global<v8::Function>,
    pub(super) set_memory_state: v8::Global<v8::Function>,
    pub(super) drain_missing: v8::Global<v8::Function>,
    pub(super) drain_stub_calls: v8::Global<v8::Function>,
}

impl HostFns {
    /// 从 `__kkp.host` 捕获全部句柄（prelude 装配后、`__kkp` 删除前）。
    pub(super) fn capture(runtime: &mut JsRuntime) -> anyhow::Result<Self> {
        // 局部捕获宏：host 对象按键取函数 → 校验为函数 → 提升为全局句柄
        //（失败直接从 capture 早退——宏在函数体内展开）
        macro_rules! grab {
            ($scope:expr, $host:expr, $key:literal) => {{
                let key = match v8::String::new_from_utf8(
                    $scope,
                    $key.as_bytes(),
                    v8::NewStringType::Normal,
                ) {
                    Some(k) => k,
                    None => return Err(anyhow::anyhow!("键名构造失败")),
                };
                let f = match $host.get($scope, key.into()) {
                    Some(f) => f,
                    None => return Err(anyhow::anyhow!("prelude 宿主容器缺 {}", $key)),
                };
                let f = match v8::Local::<v8::Function>::try_from(f) {
                    Ok(f) => f,
                    Err(_) => return Err(anyhow::anyhow!("prelude 宿主容器 {} 非函数", $key)),
                };
                v8::Global::new($scope, f)
            }};
        }
        deno_core::scope!(scope, runtime);
        let global = scope.get_current_context().global(scope);
        let name = v8::String::new_from_utf8(scope, b"__kkp", v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
        let kkp = global
            .get(scope, name.into())
            .ok_or_else(|| anyhow::anyhow!("prelude 未注册 __kkp"))?;
        let kkp =
            v8::Local::<v8::Object>::try_from(kkp).map_err(|_| anyhow::anyhow!("__kkp 非对象"))?;
        let host_key = v8::String::new_from_utf8(scope, b"host", v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
        let host = kkp
            .get(scope, host_key.into())
            .ok_or_else(|| anyhow::anyhow!("prelude 未注册 __kkp.host"))?;
        let host = v8::Local::<v8::Object>::try_from(host)
            .map_err(|_| anyhow::anyhow!("__kkp.host 非对象"))?;
        Ok(Self {
            begin_tick: grab!(scope, host, "beginTick"),
            begin_tick_kkpv: grab!(scope, host, "beginTickKkpv"),
            run_loop: grab!(scope, host, "runLoop"),
            run_expr: grab!(scope, host, "runExpr"),
            end_tick: grab!(scope, host, "endTick"),
            reset_modules: grab!(scope, host, "resetModules"),
            set_raw_memory: grab!(scope, host, "setRawMemory"),
            set_memory_state: grab!(scope, host, "setMemoryState"),
            drain_missing: grab!(scope, host, "drainMissing"),
            drain_stub_calls: grab!(scope, host, "drainStubCalls"),
        })
    }
}

/// 调用捕获的函数，将字符串参数作为 V8 值传入，再解析返回的 JSON。
/// JavaScript 异常或 V8 中断返回 Err；参数不作为源码拼接。
pub(super) fn call_host_json(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<serde_json::Value> {
    let text = call_host_string(tokio, runtime, f, args)?;
    Ok(serde_json::from_str(&text)?)
}

/// [`call_host_json`] 的无返回值形态（只关心成功/失败）。
pub(super) fn call_host_unit(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<()> {
    call_host_string(tokio, runtime, f, args).map(|_| ())
}

/// 调用捕获的宿主函数，参数为（世界信封字符串、私有束 ArrayBuffer、
/// 公开束 ArrayBuffer、CPU 字符串）。两个束都经 `with_backing_store`
/// 挂载——只克隆 backing store 引用计数，不复制字节；公开束因此可以是
/// 跨玩家共享的同一份内存。参数不作源码拼接。异常与中断的处理与
/// [`call_host_string`] 一致。
pub(super) fn call_host_kkpv_begin(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    world_json: &str,
    public: &crate::shared_bundle::SharedKkpvBundle,
    private: &crate::shared_bundle::SharedKkpvBundle,
    cpu_json: &str,
) -> anyhow::Result<()> {
    let _guard = tokio.enter();
    deno_core::scope!(scope, runtime);
    v8::tc_scope!(let scope, scope);
    let func = f.open(scope);
    let recv: v8::Local<v8::Value> = v8::undefined(scope).into();
    let world = v8::String::new_from_utf8(scope, world_json.as_bytes(), v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
    let cpu = v8::String::new_from_utf8(scope, cpu_json.as_bytes(), v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
    let public_buffer = public
        .array_buffer(scope)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（公开束挂载）"))?;
    let private_buffer = private
        .array_buffer(scope)
        .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（私有束挂载）"))?;
    let argv: [v8::Local<v8::Value>; 4] = [
        world.into(),
        private_buffer.into(),
        public_buffer.into(),
        cpu.into(),
    ];
    match func.call(scope, recv, &argv) {
        Some(_) => Ok(()),
        None => {
            let msg = if scope.is_execution_terminating() {
                "硬中断（terminate）".to_string()
            } else {
                scope
                    .exception()
                    .map(|e| e.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "无异常对象".into())
            };
            Err(anyhow::anyhow!("宿主通道调用失败: {msg}"))
        }
    }
}

pub(super) fn call_host_string(
    tokio: &tokio::runtime::Runtime,
    runtime: &mut JsRuntime,
    f: &v8::Global<v8::Function>,
    args: &[String],
) -> anyhow::Result<String> {
    let _guard = tokio.enter();
    deno_core::scope!(scope, runtime);
    // TryCatch 域：调用抛异常/被 terminate 时在此取异常文本（deno_core
    // scoped_call_with_args 同款模式）
    v8::tc_scope!(let scope, scope);
    let func = f.open(scope);
    let recv: v8::Local<v8::Value> = v8::undefined(scope).into();
    let mut argv = Vec::with_capacity(args.len());
    for a in args {
        let s = v8::String::new_from_utf8(scope, a.as_bytes(), v8::NewStringType::Normal)
            .ok_or_else(|| anyhow::anyhow!("宿主通道参数构造失败（超长）"))?;
        argv.push(s.into());
    }
    let result = match func.call(scope, recv, &argv) {
        Some(v) => v,
        None => {
            let msg = if scope.is_execution_terminating() {
                "硬中断（terminate）".to_string()
            } else {
                scope
                    .exception()
                    .map(|e| e.to_rust_string_lossy(scope))
                    .unwrap_or_else(|| "无异常对象".into())
            };
            return Err(anyhow::anyhow!("宿主通道调用失败: {msg}"));
        }
    };
    Ok(result.to_rust_string_lossy(scope))
}

pub(super) fn lodash_script() -> String {
    // lodash 3.10.1（npm 主入口，官方引擎 webpack 用的同一构建）。
    // 在 IIFE 里提供 module/exports（CJS 检测）并压掉 AMD；取 CJS 导出，回退 UMD 根挂载。
    let mut s = String::with_capacity(LODASH_SRC.len() + 200);
    s.push_str("(function(){\nvar module = { exports: {} }; var exports = module.exports; var define = undefined;\n");
    s.push_str(LODASH_SRC);
    s.push_str(
        "\nvar out = (module.exports && Object.keys(module.exports).length > 0) ? module.exports : globalThis._;\n\
         globalThis.__kkLodash = out;\n})();",
    );
    s
}

/// 近堆回调注册为独立函数：Mod 安装期也要挂好（安装先于 Self 构造）。
/// 接近 V8 堆上限时请求中断并临时放大上限；执行结束后由
/// restore_heap_limit 恢复。回调不覆盖 TypedArray 等全部堆外分配。
pub(super) fn install_near_heap_callback(runtime: &mut JsRuntime, shared: SharedRef) {
    let handle = runtime.v8_isolate().thread_safe_handle();
    runtime.add_near_heap_limit_callback(move |current_limit, _initial| {
        shared.near_heap_fired.store(true, Ordering::Relaxed);
        handle.terminate_execution();
        current_limit * 2
    });
}

pub(super) fn constants_script(
    overrides: &std::collections::BTreeMap<String, serde_json::Value>,
) -> String {
    // 将嵌入的常量 JSON 构造成对象，叠加宿主按有效规则提供的覆盖，
    // 再平铺至玩家全局；API 各段按需捕获常量。
    let extra = serde_json::to_string(overrides).expect("常量覆盖可序列化");
    format!(
        "(function(){{ var C = {CONSTANTS_JSON}; var O = {extra}; for (var k in O) {{ if (!Object.prototype.hasOwnProperty.call(C, k)) throw new Error('常量覆盖键不在常量表: ' + k); C[k] = O[k]; }} for (var k in C) globalThis[k] = C[k]; }})();"
    )
}

/// 装配 Mod 玩家面：清单注入 → 安装运行时（41 段）→ 逐包装载玩家模块
/// 并以安装 API 调用 default 导出 → finalize 校验必需绑定。任何一步
/// 失败都使沙箱创建失败；玩家模块只允许静态导入图与同步 install。
pub(super) fn install_mod_player_api(
    runtime: &mut JsRuntime,
    tokio: &tokio::runtime::Runtime,
    api: &crate::mod_api::ModPlayerApi,
) -> anyhow::Result<()> {
    // findBase 是 Rust 侧唯一权威（清单类型表的最小 FIND 常量；无类型
    // 时取 i64::MAX——沙箱侧的 mod 查询分支恒不命中）。
    let find_base = api
        .types
        .iter()
        .map(|(_, find, _)| *find)
        .min()
        .unwrap_or(i64::MAX);
    let manifest = serde_json::json!({
        "types": api
            .types
            .iter()
            .map(|(key, find, look)| serde_json::json!({ "key": key, "find": find, "look": look }))
            .collect::<Vec<_>>(),
        "intents": api.intents,
        "buildings": api
            .buildings
            .iter()
            .map(|(key, pathing, cost, fields)| serde_json::json!({
                "key": key, "pathing": pathing, "cost": cost, "fields": fields,
            }))
            .collect::<Vec<_>>(),
        "findBase": find_base,
    });
    runtime.execute_script(
        "kk-mod-api-manifest",
        format!(
            "globalThis.__kkModApiManifest = {};",
            serde_json::to_string(&manifest)?
        ),
    )?;
    runtime.execute_script(
        "kk-prelude-41-mod-player",
        include_str!("../prelude/41_mod_player.js").to_string(),
    )?;
    // 安装运行时的两个入口捕获为 Rust 持有的 v8 句柄：此后不再经
    // __kkp 全局查找，且可以在模块执行前删除 __kkp——Mod 代码与玩家
    // 同权限面（读不到 gw/modApiRuntime 等宿主内部容器）。
    let create_install_api = runtime.execute_script(
        "kk-mod-api-grab-factory",
        "globalThis.__kkp.modApiRuntime.createInstallApi".to_string(),
    )?;
    let finalize = runtime.execute_script(
        "kk-mod-api-grab-finalize",
        "globalThis.__kkp.modApiRuntime.finalize".to_string(),
    )?;
    runtime.execute_script("kk-mod-api-unlink", "delete globalThis.__kkp".to_string())?;
    for module in api.modules.iter() {
        let install_api = call_install_factory(runtime, &create_install_api, &module.namespace)?;
        let url = crate::mod_api::entry_specifier(module).map_err(anyhow::Error::msg)?;
        let runtime_ref: &mut JsRuntime = runtime;
        let mod_id = tokio
            .block_on(async {
                let id = runtime_ref.load_side_es_module(&url).await?;
                let evaluate = runtime_ref.mod_evaluate(id);
                runtime_ref.run_event_loop(Default::default()).await?;
                evaluate.await?;
                Ok::<_, deno_core::error::CoreError>(id)
            })
            .map_err(|error| {
                anyhow::anyhow!("玩家模块 {} 装载或求值失败：{error}", module.namespace)
            })?;
        call_player_install(runtime, mod_id, &install_api, &module.namespace)?;
    }
    call_captured_finalize(runtime, &finalize)?;
    Ok(())
}

/// 经捕获句柄为命名空间创建安装 API（不经 __kkp 全局查找）。
fn call_install_factory(
    runtime: &mut JsRuntime,
    factory: &v8::Global<v8::Value>,
    namespace: &str,
) -> anyhow::Result<v8::Global<v8::Value>> {
    deno_core::scope!(scope, runtime);
    v8::tc_scope!(let tc, scope);
    let factory = v8::Local::<v8::Function>::try_from(v8::Local::new(tc, factory.clone()))
        .map_err(|_| anyhow::anyhow!("安装 API 工厂非函数（41 段装配缺陷）"))?;
    let namespace = v8::String::new_from_utf8(tc, namespace.as_bytes(), v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
    let receiver = v8::undefined(tc).into();
    let result = factory
        .call(tc, receiver, &[namespace.into()])
        .ok_or_else(|| {
            let detail = tc
                .exception()
                .map(|exception| exception.to_rust_string_lossy(tc))
                .unwrap_or_else(|| "无异常细节".to_string());
            anyhow::anyhow!("createInstallApi 调用失败：{detail}")
        })?;
    Ok(v8::Global::new(tc, result))
}

/// 经捕获句柄调用 finalize：缺必需绑定在 JS 侧抛错，异常文本随错误返回。
fn call_captured_finalize(
    runtime: &mut JsRuntime,
    finalize: &v8::Global<v8::Value>,
) -> anyhow::Result<()> {
    deno_core::scope!(scope, runtime);
    v8::tc_scope!(let tc, scope);
    let finalize = v8::Local::<v8::Function>::try_from(v8::Local::new(tc, finalize.clone()))
        .map_err(|_| anyhow::anyhow!("finalize 非函数（41 段装配缺陷）"))?;
    let receiver = v8::undefined(tc).into();
    finalize.call(tc, receiver, &[]).map(|_| ()).ok_or_else(|| {
        let detail = tc
            .exception()
            .map(|exception| exception.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "无异常细节".to_string());
        anyhow::anyhow!("玩家面 finalize 失败：{detail}")
    })
}

/// 调用玩家模块的 default(install) 导出。install 必须同步返回；异常
/// 文本随错误返回，供装配失败诊断。
fn call_player_install(
    runtime: &mut JsRuntime,
    mod_id: deno_core::ModuleId,
    install_api: &v8::Global<v8::Value>,
    namespace: &str,
) -> anyhow::Result<()> {
    let module_namespace = runtime
        .get_module_namespace(mod_id)
        .map_err(|error| anyhow::anyhow!("玩家模块 {namespace} 命名空间不可用：{error}"))?;
    deno_core::scope!(scope, runtime);
    v8::tc_scope!(let tc, scope);
    let namespace_obj = v8::Local::new(tc, module_namespace.clone());
    let default_key = v8::String::new_from_utf8(tc, b"default", v8::NewStringType::Normal)
        .ok_or_else(|| anyhow::anyhow!("键名构造失败"))?;
    let default = namespace_obj
        .get(tc, default_key.into())
        .ok_or_else(|| anyhow::anyhow!("玩家模块 {namespace} 缺少 default 导出"))?;
    let install = v8::Local::<v8::Function>::try_from(default)
        .map_err(|_| anyhow::anyhow!("玩家模块 {namespace} 的 default 导出须为函数"))?;
    let api_local = v8::Local::new(tc, install_api.clone());
    let receiver = v8::undefined(tc).into();
    let result = match install.call(tc, receiver, &[api_local]) {
        Some(result) => result,
        None => {
            let detail = tc
                .exception()
                .map(|exception| exception.to_rust_string_lossy(tc))
                .unwrap_or_else(|| "无异常细节".to_string());
            anyhow::bail!("玩家模块 {namespace} 的 install 执行失败：{detail}");
        }
    };
    if result.is_promise() {
        anyhow::bail!("玩家模块 {namespace} 的 install 不得返回 Promise（安装必须同步完成）");
    }
    Ok(())
}
