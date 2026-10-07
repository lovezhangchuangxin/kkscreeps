//! 捕获宿主 V8 句柄并调用生命周期入口；注册近堆中断回调。
use super::*;

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
