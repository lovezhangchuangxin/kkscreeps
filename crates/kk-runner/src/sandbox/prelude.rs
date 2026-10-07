//! 内嵌玩家 API 的来源、依赖顺序和装配阶段；目录名只表达职责。
use super::*;

const LODASH_SRC: &str = include_str!("../../assets/lodash-3.10.1.js");
const CONSTANTS_JSON: &str = include_str!("../../assets/constants.json");
/// 按依赖顺序装配共享工具、模块加载、玩家 API 与宿主函数。
/// 各段通过临时全局 __kkp 共享闭包状态；Rust 捕获宿主函数句柄后删除 __kkp。
/// 因此 host.js 只能注册句柄，不能提前删除共享容器。
const SEGMENTS: &[(&str, &str)] = &[
    (
        "kk-prelude-runtime-shared",
        include_str!("../prelude/runtime/shared.js"),
    ),
    (
        "kk-prelude-runtime-buffer",
        include_str!("../prelude/runtime/buffer.js"),
    ),
    (
        "kk-prelude-runtime-commonjs",
        include_str!("../prelude/runtime/commonjs.js"),
    ),
    (
        "kk-prelude-game-registry",
        include_str!("../prelude/game/registry.js"),
    ),
    (
        "kk-prelude-runtime-deprecation",
        include_str!("../prelude/runtime/deprecation.js"),
    ),
    (
        "kk-prelude-game-targets",
        include_str!("../prelude/game/targets.js"),
    ),
    (
        "kk-prelude-game-kkpv",
        include_str!("../prelude/game/kkpv.js"),
    ),
    (
        "kk-prelude-game-room-position",
        include_str!("../prelude/game/room-position.js"),
    ),
    (
        "kk-prelude-game-room-terrain",
        include_str!("../prelude/game/room/terrain.js"),
    ),
    (
        "kk-prelude-game-room-visual",
        include_str!("../prelude/game/room/visual.js"),
    ),
    (
        "kk-prelude-game-room-queries",
        include_str!("../prelude/game/room/queries.js"),
    ),
    (
        "kk-prelude-game-room-creation",
        include_str!("../prelude/game/room/creation.js"),
    ),
    (
        "kk-prelude-game-room-pathfinding",
        include_str!("../prelude/game/room/pathfinding.js"),
    ),
    (
        "kk-prelude-game-room",
        include_str!("../prelude/game/room.js"),
    ),
    (
        "kk-prelude-game-creep",
        include_str!("../prelude/game/creep.js"),
    ),
    (
        "kk-prelude-game-creep-resources",
        include_str!("../prelude/game/creep-resources.js"),
    ),
    (
        "kk-prelude-game-structures",
        include_str!("../prelude/game/structures.js"),
    ),
    (
        "kk-prelude-game-structures-spawn",
        include_str!("../prelude/game/structures/spawn.js"),
    ),
    (
        "kk-prelude-game-structures-fields",
        include_str!("../prelude/game/structures/fields.js"),
    ),
    (
        "kk-prelude-game-structure-api",
        include_str!("../prelude/game/structure-api.js"),
    ),
    (
        "kk-prelude-game-tombstones",
        include_str!("../prelude/game/tombstones.js"),
    ),
    (
        "kk-prelude-game-sites-flags",
        include_str!("../prelude/game/sites-flags.js"),
    ),
    (
        "kk-prelude-game-resources",
        include_str!("../prelude/game/resources.js"),
    ),
    (
        "kk-prelude-game-path-finder",
        include_str!("../prelude/game/path-finder.js"),
    ),
    (
        "kk-prelude-game-memory",
        include_str!("../prelude/game/memory.js"),
    ),
    (
        "kk-prelude-game-market",
        include_str!("../prelude/game/market.js"),
    ),
    (
        "kk-prelude-game-map",
        include_str!("../prelude/game/map.js"),
    ),
    (
        "kk-prelude-game-intents",
        include_str!("../prelude/game/intents.js"),
    ),
    (
        "kk-prelude-runtime-world-tick",
        include_str!("../prelude/runtime/world-tick.js"),
    ),
    (
        "kk-prelude-game-game",
        include_str!("../prelude/game/game.js"),
    ),
    ("kk-prelude-host", include_str!("../prelude/host.js")),
];

/// 装配基础环境、游戏 API 与宿主入口，再按配置加载扩展意图原语。
/// 清单顺序是唯一执行顺序；不按目录遍历或文件名排序。
pub(super) fn install_base(runtime: &mut JsRuntime, config: &SandboxConfig) -> anyhow::Result<()> {
    runtime.execute_script("kk-lodash", lodash_script())?;
    runtime.execute_script("kk-constants", constants_script(&config.constant_overrides))?;
    for &(name, source) in SEGMENTS {
        runtime.execute_script(name, source.to_string())?;
    }
    if config.extension_intents {
        runtime.execute_script(
            "kk-prelude-mods-extension-intents",
            include_str!("../prelude/mods/extension-intents.js").to_string(),
        )?;
    }
    Ok(())
}

/// Mod 安装内部先解绑再执行作者模块；无 Mod 路径在这里统一解绑。
pub(super) fn unlink(runtime: &mut JsRuntime) -> anyhow::Result<()> {
    runtime.execute_script("kk-host-unlink", "delete globalThis.__kkp".to_string())?;
    Ok(())
}

fn lodash_script() -> String {
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

fn constants_script(overrides: &std::collections::BTreeMap<String, serde_json::Value>) -> String {
    // 将嵌入的常量 JSON 构造成对象，叠加宿主按有效规则提供的覆盖，
    // 再平铺至玩家全局；API 各段按需捕获常量。
    let extra = serde_json::to_string(overrides).expect("常量覆盖可序列化");
    format!(
        "(function(){{ var C = {CONSTANTS_JSON}; var O = {extra}; for (var k in O) {{ if (!Object.prototype.hasOwnProperty.call(C, k)) throw new Error('常量覆盖键不在常量表: ' + k); C[k] = O[k]; }} for (var k in C) globalThis[k] = C[k]; }})();"
    )
}

/// 装配 Mod 玩家面：清单注入 → 安装运行时（mods/player-api.js）→ 逐包装载玩家模块
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
        "kk-prelude-mods-player-api",
        include_str!("../prelude/mods/player-api.js").to_string(),
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
        .map_err(|_| anyhow::anyhow!("安装 API 工厂非函数（玩家安装运行时装配缺陷）"))?;
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
        .map_err(|_| anyhow::anyhow!("finalize 非函数（玩家安装运行时装配缺陷）"))?;
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
