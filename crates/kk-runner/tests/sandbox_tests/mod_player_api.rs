//! Mod 玩家面安装的场景族：安装 API 面（类/常量/查询/emit）、mod 记录的
//! 对象装配与 FIND/LOOK、失败门（缺绑定/坏模块/越权导入）与全局面守卫。

use serde_json::json;

use kk_runner::{ModPlayerApi, ModPlayerModule, Sandbox, SandboxConfig, TickPlan};

const COUNTER_PLAYER_JS: &str = r#"
export default function install(api) {
  const { RoomObject } = api.builtins;
  class Counter extends RoomObject {
    get value() { return api.view.field(this, 'value'); }
    get owner() { return api.view.field(this, 'owner'); }
    increment(amount = 1) {
      return api.intents.emit('testmod:increment', this, { amount });
    }
  }
  api.objects.bind('testmod:counter', Counter);
  api.globals.define('Counter', Counter);
  const q = api.queries.install('testmod:counter');
  api.globals.define('FIND_COUNTERS', q.find);
  api.globals.define('LOOK_COUNTERS', q.look);
  // 测试面：直通 emit 预检（错误码路径不经类方法包装）。
  api.globals.define('emitDirect', function emitDirect(key, target, args) {
    return api.intents.emit(key, target, args);
  });
}
"#;

fn counter_api(player_source: &str) -> std::sync::Arc<ModPlayerApi> {
    std::sync::Arc::new(ModPlayerApi {
        modules: std::sync::Arc::new(vec![ModPlayerModule {
            namespace: "testmod".into(),
            entry: "player.js".into(),
            files: [("player.js".to_string(), player_source.to_string())].into(),
        }]),
        types: vec![(
            "testmod:counter".to_string(),
            1000,
            "testmod:counter".to_string(),
        )],
        intents: vec!["testmod:increment".to_string()],
        buildings: Vec::new(),
    })
}

fn counter_config(player_source: &str) -> SandboxConfig {
    SandboxConfig {
        extension_intents: true,
        mod_api: Some(counter_api(player_source)),
        ..SandboxConfig::default()
    }
}

/// bind 与 buildings.bind 只认本包命名空间：清单内但归他人命名空间的键
/// 在安装期拒绝（与 emit 目录的纵深防御同律）。
#[test]
fn bind_rejects_foreign_namespace_types() {
    let foreign_api = |source: &str| {
        std::sync::Arc::new(ModPlayerApi {
            modules: std::sync::Arc::new(vec![ModPlayerModule {
                namespace: "testmod".into(),
                entry: "player.js".into(),
                files: [("player.js".to_string(), source.to_string())].into(),
            }]),
            types: vec![(
                "othermod:thing".to_string(),
                1000,
                "othermod:thing".to_string(),
            )],
            intents: Vec::new(),
            buildings: vec![(
                "othermod:mill".to_string(),
                "obstacle".to_string(),
                vec!["energy".to_string()],
            )],
        })
    };
    for source in [
        "export default function install(api) { api.objects.bind('othermod:thing', class T {}); }",
        "export default function install(api) { api.buildings.bind('othermod:mill', class T {}); }",
    ] {
        let err = Sandbox::new(SandboxConfig {
            mod_api: Some(foreign_api(source)),
            ..SandboxConfig::default()
        })
        .err()
        .expect("跨包绑定须使沙箱创建失败");
        assert!(
            err.to_string().contains("不属于本包命名空间"),
            "错误应含命名空间拒绝：{err}"
        );
    }
}

/// 装了玩家面的世界：一个 mod 计数器对象与最小房间信封。
fn mod_world() -> serde_json::Value {
    let terrain = vec![0u32; 2500];
    json!({
        "time": 3,
        "user": "p1",
        "gcl": 0,
        "visibleRooms": ["W1N1"],
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "rooms": {
            "W1N1": {
                "name": "W1N1",
                "terrain": terrain,
                "energyAvailable": 0,
                "energyCapacityAvailable": 300,
            }
        },
        "objects": [
            {
                "type": "mod", "modType": "testmod:counter",
                "id": "kk0000000000000001", "room": "W1N1", "x": 10, "y": 10,
                "owner": "p1", "value": 5,
                "pos": { "x": 10, "y": 10, "roomName": "W1N1" }
            }
        ]
    })
}

pub(super) fn mod_player_api_surface_impl() -> anyhow::Result<()> {
    // vanilla 沙箱：没有任何玩家面全局（与全局面白名单守卫同口径）。
    let mut plain = Sandbox::new(SandboxConfig::default())?;
    plain.begin_tick(&json!({ "time": 1, "user": "p1", "rooms": {}, "objects": [] }))?;
    let v = plain.run_expr("typeof Counter + ':' + typeof FIND_COUNTERS")?;
    assert_eq!(
        v["value"], "undefined:undefined",
        "vanilla 分片无玩家面全局"
    );
    drop(plain);

    let mut sb = Sandbox::new(counter_config(COUNTER_PLAYER_JS))?;
    sb.begin_tick(&mod_world())?;
    // 安装产物：类、查询常量、look 常量；绑定不可写、不可配置。
    let v = sb.run_expr(
        r#"(()=>{
            const d = Object.getOwnPropertyDescriptor(globalThis, 'Counter');
            return [
                typeof Counter, FIND_COUNTERS, LOOK_COUNTERS,
                !!d && d.enumerable && !d.writable && !d.configurable,
                Counter.name,
            ];
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        json!(["function", 1000, "testmod:counter", true, "Counter"]),
        "玩家面全局按清单安装"
    );
    // 越权全局被拒（内建名与其他已装全局不可覆盖）——安装期失败。
    for source in [
        "export default function install(api){ api.globals.define('Game', 1); }",
        "export default function install(api){ api.objects.bind('testmod:counter', class X {}); api.globals.define('A', 1); api.globals.define('A', 2); }",
        "export default function install(api){ api.globals.define('bad name', 1); }",
    ] {
        let err = Sandbox::new(counter_config(source))
            .err()
            .expect("越权/非法全局定义须失败");
        assert!(err.to_string().contains("全局"), "错误指向全局定义：{err}");
    }
    // 必需类型缺绑定 → finalize 失败。
    let err = Sandbox::new(counter_config(
        "export default function install(api) { api.globals.define('Noop', 1); }",
    ))
    .err()
    .expect("缺必需绑定须失败");
    assert!(
        err.to_string().contains("未绑定玩家面类型"),
        "错误指向缺绑定：{err}"
    );
    // 意图目录非空但原语缺席（ext 门与清单不同源）→ 安装期失败。
    let err = Sandbox::new(SandboxConfig {
        mod_api: Some(counter_api(COUNTER_PLAYER_JS)),
        ..SandboxConfig::default()
    })
    .err()
    .expect("原语缺席须失败");
    assert!(
        err.to_string().contains("原语缺席"),
        "错误指向装配缺陷：{err}"
    );
    Ok(())
}

pub(super) fn mod_player_api_record_face_impl() -> anyhow::Result<()> {
    // 专用配置：counter（有对象）+ vault（注册并绑定、但本房无对象——
    // 空桶语义）。模块源在标准 counter 安装尾部补绑 vault。
    let patched = COUNTER_PLAYER_JS.replace(
        "  api.globals.define('emitDirect', function emitDirect(key, target, args) {",
        "  api.objects.bind('testmod:vault', class Vault {});\n  api.globals.define('emitDirect', function emitDirect(key, target, args) {",
    );
    assert!(
        patched != COUNTER_PLAYER_JS,
        "补绑锚点须命中（夹具格式变化时更新锚点）"
    );
    let files = [("player.js".to_string(), patched)].into();
    let template = counter_api(COUNTER_PLAYER_JS);
    let api = ModPlayerApi {
        modules: std::sync::Arc::new(vec![ModPlayerModule {
            namespace: "testmod".into(),
            entry: "player.js".into(),
            files,
        }]),
        types: vec![
            (
                "testmod:counter".to_string(),
                1000,
                "testmod:counter".to_string(),
            ),
            (
                "testmod:vault".to_string(),
                1001,
                "testmod:vault".to_string(),
            ),
        ],
        intents: template.intents.clone(),
        buildings: Vec::new(),
    };
    let config = SandboxConfig {
        extension_intents: true,
        mod_api: Some(std::sync::Arc::new(api)),
        ..SandboxConfig::default()
    };
    let mut sb = Sandbox::new(config)?;
    match sb.begin_tick(&mod_world())? {
        TickPlan::Run { .. } => {}
        other => anyhow::bail!("期望 Run 计划：{other:?}"),
    }
    // 对象装配：实例、instanceof、字段 getter 经当轮投影读取。
    let v = sb.run_expr(
        r#"(()=>{
            const c = Game.getObjectById('kk0000000000000001');
            return [
                c instanceof Counter,
                c instanceof RoomObject,
                c.value, c.owner, c.pos.x, c.pos.y, c.pos.roomName, c.id,
            ];
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        json!([true, true, 5, "p1", 10, 10, "W1N1", "kk0000000000000001"]),
        "mod 对象装配为玩家类实例，字段经当轮投影读取"
    );
    // FIND/LOOK：注册常量命中；look 键序在内建之后按键序追加。
    let v = sb.run_expr(
        r#"(()=>{
            const room = Game.rooms.W1N1;
            const found = room.find(FIND_COUNTERS);
            const look = room.lookAt(10, 10);
            const lookFor = room.lookForAt(LOOK_COUNTERS, 10, 10);
            return [
                found.length, found[0] === Game.getObjectById('kk0000000000000001'),
                look.length, look[0].type, look[1].type,
                look[1]['testmod:counter'] === found[0],
                lookFor.length, lookFor[0] === found[0],
            ];
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        json!([1, true, 2, "terrain", "testmod:counter", true, 1, true]),
        "FIND/LOOK 走注册常量；lookAt 内建固定序在前、Mod 键序在后"
    );
    // 注册但本房无对象的类型：FIND 空数组、lookForAt 空数组（桶预初始
    // 化——与内建类型同待遇，不是 ERR_INVALID_ARGS）。
    let v = sb.run_expr(
        r#"(()=>{
            const room = Game.rooms.W1N1;
            return [
                room.find(1001).length,
                JSON.stringify(room.lookForAt('testmod:vault', 5, 5)),
            ];
        })()"#,
    )?;
    assert_eq!(v["value"], json!([0, "[]"]), "空桶 Mod 类型与内建同语义");
    // emit：合法意图进容器；未知键/坏目标返回错误码且不进容器。
    let v = sb.run_expr(
        r#"(()=>{
            const c = Game.getObjectById('kk0000000000000001');
            return [
                c.increment(3),
                emitDirect('testmod:unknown', c, {}),
                emitDirect('testmod:increment', null, {}),
            ];
        })()"#,
    )?;
    assert_eq!(
        v["value"],
        json!([0, -10, -10]),
        "emit 合法返回 OK；未知键与坏目标返回 ERR_INVALID_ARGS"
    );
    let end = sb.end_tick()?;
    assert_eq!(end["ok"], true, "end_tick 正常：{end}");
    let intents = sb.take_intents().expect("intentsList 带出");
    assert_eq!(
        intents["kk0000000000000001"]["testmod:increment"]["amount"], 3,
        "emit 进入扩展意图容器"
    );
    assert_eq!(end["intentsCpu"], 0.2, "一个扩展意图键计 0.2");
    Ok(())
}

pub(super) fn mod_player_api_module_gates_impl() -> anyhow::Result<()> {
    // default 导出非函数 / install 抛错 / 返回 Promise / 外部导入。
    for (source, needle) in [
        ("export default 1;", "default 导出须为函数"),
        (
            "export default function install(){ throw new Error('boom'); }",
            "boom",
        ),
        (
            "export default function install(){ return Promise.resolve(); }",
            "不得返回 Promise",
        ),
        (
            "import _ from 'lodash';\nexport default function install() {}",
            "not prefixed",
        ),
        (
            "import _ from 'file:///tmp/x.js';\nexport default function install() {}",
            "包内相对导入",
        ),
    ] {
        let err = Sandbox::new(counter_config(source))
            .err()
            .expect("坏模块须使沙箱创建失败");
        assert!(
            err.to_string().contains(needle),
            "错误应含 {needle:?}：{err}"
        );
    }
    // 跨包绝对说明符与查询串/锚点拒绝（同包绝对形式可用——缓存同键）。
    for (source, needle) in [
        (
            "import x from 'kkmodplayer://other/file.js';\nexport default function install() {}",
            "跨包导入被拒绝",
        ),
        (
            "import x from 'kkmodplayer://testmod/player.js?x=1';\nexport default function install() {}",
            "查询串或锚点",
        ),
    ] {
        let err = Sandbox::new(counter_config(source))
            .err()
            .expect("越权说明符须失败");
        assert!(
            err.to_string().contains(needle),
            "错误应含 {needle:?}：{err}"
        );
    }
    // 挂死兜底：安装墙钟期限（缩短到 500ms）使死循环安装失败而不是楔死。
    let start = std::time::Instant::now();
    let err = Sandbox::new(SandboxConfig {
        extension_intents: true,
        mod_api: Some(counter_api(
            "export default function install() { while (true) {} }",
        )),
        install_wall_limit: Some(std::time::Duration::from_millis(500)),
        ..SandboxConfig::default()
    })
    .err()
    .expect("死循环安装须被看门狗中断");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "须及时失败"
    );
    assert!(
        err.to_string().contains("Mod 玩家面安装失败"),
        "错误指向安装：{err}"
    );
    // 近堆兜底：小堆上限 + 无界分配 → 安装失败（进程不中止）。
    let err = Sandbox::new(SandboxConfig {
        extension_intents: true,
        mod_api: Some(counter_api(
            "export default function install(api) { const a = []; while (true) a.push(new Array(100000)); api.objects.bind('testmod:counter', class C {}); }",
        )),
        heap_limit_bytes: 32 * 1024 * 1024,
        install_wall_limit: Some(std::time::Duration::from_secs(30)),
        ..SandboxConfig::default()
    })
    .err()
    .expect("无界分配安装须被近堆回调中断");
    assert!(
        err.to_string().contains("Mod 玩家面安装失败"),
        "错误指向安装：{err}"
    );
    // 包内相对导入可用：入口 import ./util 后再安装。
    let template = counter_api("export default function install() {}");
    let api = ModPlayerApi {
        modules: std::sync::Arc::new(vec![ModPlayerModule {
            namespace: "testmod".into(),
            entry: "player.js".into(),
            files: [
                (
                    "player.js".to_string(),
                    "import { tag } from './util/tag.js';\nexport default function install(api) {\n  if (tag() !== 'ok') throw new Error('util 导入失效');\n  api.objects.bind('testmod:counter', class Counter {});\n}".to_string(),
                ),
                (
                    "util/tag.js".to_string(),
                    "export function tag() { return 'ok'; }".to_string(),
                ),
            ]
            .into(),
        }]),
        types: template.types.clone(),
        intents: template.intents.clone(),
        buildings: Vec::new(),
    };
    let mut sb = Sandbox::new(SandboxConfig {
        extension_intents: true,
        mod_api: Some(std::sync::Arc::new(api)),
        ..SandboxConfig::default()
    })?;
    sb.begin_tick(&json!({ "time": 1, "user": "p1", "rooms": {}, "objects": [] }))?;
    let v = sb.run_expr("typeof Counter")?;
    assert_eq!(v["value"], "undefined", "util 模块未定义全局（类仅绑定）");
    Ok(())
}

pub(super) fn mod_player_api_global_surface_impl() -> anyhow::Result<()> {
    // 启用玩家面的沙箱：新增全局恰为安装声明的集合——没有宿主内部
    // 句柄或临时容器泄漏到玩家面。
    let mut sb = Sandbox::new(counter_config(COUNTER_PLAYER_JS))?;
    sb.begin_tick(&json!({ "time": 1, "user": "p1", "rooms": {}, "objects": [] }))?;
    let v = sb.run_expr("Object.getOwnPropertyNames(globalThis)")?;
    let names: Vec<String> = serde_json::from_value(v["value"].clone())?;
    let leaked: Vec<&String> = names
        .iter()
        .filter(|n| {
            (n.starts_with("__kk") || n.starts_with("__bootstrap"))
                && !matches!(n.as_str(), "__kkEmitExtensionIntent")
        })
        .collect();
    assert!(
        leaked.is_empty(),
        "玩家面不泄漏宿主内部全局: {leaked:?}（全部新增 {names:?}）"
    );
    let v = sb.run_expr("typeof __kkp")?;
    assert_eq!(v["value"], "undefined", "__kkp 在玩家运行前已删除");
    for name in ["Counter", "FIND_COUNTERS", "LOOK_COUNTERS"] {
        assert!(names.contains(&name.to_string()), "应有全局 {name}");
    }
    assert!(
        !names.contains(&"modApiRuntime".to_string()),
        "安装运行时不进全局面"
    );
    Ok(())
}
