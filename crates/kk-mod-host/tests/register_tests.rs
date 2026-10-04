//! 注册期端到端测试：demo 夹具全链（装载→拓扑→注册→冻结）、确定性、
//! 配置指纹，与 manifest/依赖图/注册/加载器的失败矩阵。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kk_mod_api::{Key, Number};
use kk_mod_host::{ModPackage, ModRealm, RegisterOptions, discover_packages};
use serde_json::{Value, json};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mods")
}

fn register_fixtures(options: RegisterOptions) -> (ModRealm, kk_mod_host::Registered) {
    let packages = discover_packages(&fixtures_root()).expect("夹具可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm.register(&packages, options).expect("夹具可注册");
    (realm, registered)
}

/// 在临时目录写一个最小 Mod 包；manifest 可覆盖，server.js 缺省为空注册。
fn write_pkg(root: &Path, name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).expect("建目录");
    let has_manifest = files.iter().any(|(path, _)| *path == "mod.json");
    if !has_manifest {
        let manifest = json!({
            "id": name,
            "version": "1.0.0",
            "apiVersion": "^1.0.0",
        });
        std::fs::write(
            dir.join("mod.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .expect("写 manifest");
    }
    let has_entry = files.iter().any(|(path, _)| *path == "server.js");
    if !has_entry {
        std::fs::write(
            dir.join("server.js"),
            "export default function register() {}\n",
        )
        .expect("写入口");
    }
    for (path, body) in files {
        let target = dir.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("建子目录");
        }
        std::fs::write(target, body).expect("写包文件");
    }
    dir
}

fn temp_root(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kk-mod-host-tests-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时根");
    dir
}

/// 注册一个临时根下的全部包，返回错误文本。
fn register_root_error(root: &Path) -> String {
    match discover_packages(root) {
        Err(error) => error.to_string(),
        Ok(packages) => {
            let mut realm = ModRealm::new().expect("realm 可创建");
            if let Err(error) = realm.register(&packages, RegisterOptions::default()) {
                return error.to_string();
            }
            panic!("期望注册失败，实际成功：{}", root.display());
        }
    }
}

fn key(value: &str) -> Key {
    Key::new(value).expect("测试键合法")
}

#[test]
fn demo_fixtures_register_and_freeze() {
    let (_realm, registered) = register_fixtures(RegisterOptions::default());
    let plan = &registered.plan;

    // 拓扑序：被依赖的 demo-base 在前；builtin 不计入 Mod 贡献列表。
    let order: Vec<&str> = registered
        .contributions
        .iter()
        .map(|c| c.source.namespace.as_str())
        .collect();
    assert_eq!(order, vec!["demo-base", "demo-counter"]);
    assert!(plan.sources().contains_key("builtin"));

    // counter 类型与字段。
    let counter = key("demo-counter:counter");
    let handle = plan.projection().resolve(&counter).expect("类型已注册");
    let projected = plan.projection().get(handle).expect("句柄有效");
    assert_eq!(projected.descriptor.player_class, "Counter");
    assert_eq!(
        projected.descriptor.owner_field,
        Some(key("demo-counter:owner"))
    );
    let value_field = plan
        .projection()
        .field(handle, &key("demo-counter:value"))
        .expect("字段已注册");
    let value_field = plan
        .projection()
        .get_field(handle, value_field)
        .expect("字段句柄有效");
    assert_eq!(
        value_field.descriptor.binding,
        key("demo-counter:counter.field.value")
    );
    match &value_field.descriptor.default {
        kk_mod_api::FieldDefault::Value(Value::Number(value)) => {
            assert_eq!(value.as_i64(), Some(0), "未配置时 start 默认 0");
        }
        other => panic!("value 字段应有数值默认值，实得 {other:?}"),
    }

    // increment 动作：对象域、接收者、参数域与自动入口。
    let intent = plan
        .resolve_intent(&key("demo-counter:increment"))
        .expect("动作已注册");
    let intent = plan.get_intent(intent).expect("动作句柄有效");
    assert_eq!(
        intent.receiver.as_ref().expect("对象动作有接收者").key,
        counter
    );
    let slot = plan
        .resolve_slot(&key("demo-counter:increment.slot"))
        .expect("自动入口已注册");
    let slot = plan.get_slot(slot).expect("入口句柄有效");
    match &slot.dispatch {
        kk_mod_api::Dispatch::Native(binding) => {
            assert_eq!(*binding, key("demo-counter:increment.process"));
        }
        other => panic!("无处理器的自动入口应为 Native，实得 {other:?}"),
    }

    // 既有入口上的 wrap：builtin:link.tick 变 Chained，wrap 处理器在内层。
    let link_tick = plan
        .resolve_slot(&key("builtin:link.tick"))
        .expect("link tick 入口存在");
    let link_tick = plan.get_slot(link_tick).expect("入口句柄有效");
    match &link_tick.dispatch {
        kk_mod_api::Dispatch::Chained { wraps, .. } => {
            assert_eq!(wraps, &vec![key("demo-counter:handler.source-audit")]);
        }
        other => panic!("wrap 后应为 Chained，实得 {other:?}"),
    }

    // 跨包挂载：widget 获得 counterTag 字段，绑定归挂载方命名空间。
    let widget = key("demo-base:widget");
    let widget_handle = plan.projection().resolve(&widget).expect("widget 已注册");
    let widget_type = plan.projection().get(widget_handle).expect("句柄有效");
    let tag = widget_type
        .fields
        .iter()
        .find(|f| f.descriptor.key == key("demo-counter:counterTag"))
        .expect("挂载字段并入目标类型");
    assert_eq!(tag.descriptor.binding, key("demo-counter:mount.counterTag"));

    // 规则默认值与覆盖生效性。
    let ceiling = plan.rules().resolve(&key("demo-base:recharge.ceiling"));
    let ceiling = ceiling.expect("规则已注册");
    assert_eq!(plan.rules().get(ceiling), Some(&Number::Signed(1000)));
    let capacity = plan
        .rules()
        .resolve(&key("builtin:link.capacity"))
        .expect("link 容量规则存在");
    assert_eq!(
        plan.rules().get(capacity),
        Some(&Number::Signed(700)),
        "唯一覆盖候选自动选中"
    );

    // 绑定函数表：处理函数与处理器都有注册函数。
    for executable in [
        "demo-counter:increment.process",
        "demo-counter:handler.source-audit",
        "demo-base:recharge.process",
        "demo-base:handler.recharge-audit",
    ] {
        assert!(
            registered.executables.contains(&key(executable)),
            "缺少可执行绑定 {executable}"
        );
    }
    assert!(
        plan.required_bindings()
            .contains(&key("demo-counter:counter.field.value")),
        "字段绑定进入计划绑定目录"
    );

    // 指纹来源：玩家面摘要只在声明了 player 的包非零。
    let sources = plan.sources();
    assert_eq!(
        sources["demo-counter"].revision, "0.3.1",
        "revision 取 manifest 版本"
    );
    assert_ne!(
        sources["demo-counter"].player_api_digest, [0u8; 32],
        "声明 player 的包有玩家面摘要"
    );
    assert_ne!(
        sources["demo-base"].player_api_digest, [0u8; 32],
        "两个夹具包都声明 player 面，摘要非零"
    );
}

#[test]
fn registration_is_deterministic() {
    let (_, first) = register_fixtures(RegisterOptions::default());
    let (_, second) = register_fixtures(RegisterOptions::default());
    assert_eq!(
        first.plan.hash().bytes(),
        second.plan.hash().bytes(),
        "同输入两次注册得到同一计划哈希"
    );
    assert_eq!(first.contributions, second.contributions);
    assert_eq!(first.executables, second.executables);
}

#[test]
fn config_reaches_registration_and_digest() {
    let options = RegisterOptions {
        profiler: None,
        configs: BTreeMap::from([("demo-counter".to_string(), json!({"start": 5}))]),
        freeze: Default::default(),
        vm_limits: Default::default(),
        fault_disable_threshold: None,
    };
    let (_, configured) = register_fixtures(options.clone());
    let (_, plain) = register_fixtures(RegisterOptions::default());

    let counter = key("demo-counter:counter");
    let handle = configured
        .plan
        .projection()
        .resolve(&counter)
        .expect("类型已注册");
    let field = configured
        .plan
        .projection()
        .field(handle, &key("demo-counter:value"))
        .expect("字段已注册");
    let field = configured
        .plan
        .projection()
        .get_field(handle, field)
        .expect("字段句柄有效");
    match &field.descriptor.default {
        kk_mod_api::FieldDefault::Value(Value::Number(value)) => {
            assert_eq!(value.as_i64(), Some(5), "配置 start 进入字段默认值");
        }
        other => panic!("配置后应有数值默认值，实得 {other:?}"),
    }
    assert_ne!(
        configured.plan.hash().bytes(),
        plain.plan.hash().bytes(),
        "配置参与计划身份"
    );
    assert_ne!(
        configured.plan.sources()["demo-counter"].config_digest,
        plain.plan.sources()["demo-counter"].config_digest,
        "配置摘要有别于未配置"
    );
    assert_eq!(
        plain.plan.sources()["demo-counter"].config_digest,
        [0u8; 32],
        "未配置的包配置摘要为零"
    );
}

#[path = "register_tests/package_loading.rs"]
mod package_loading;

#[path = "register_tests/declarations.rs"]
mod declarations;

#[path = "register_tests/invocation.rs"]
mod invocation;
