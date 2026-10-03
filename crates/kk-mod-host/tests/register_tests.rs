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
    assert_eq!(
        sources["demo-base"].player_api_digest, [0u8; 32],
        "未声明 player 的包摘要为零"
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
        configs: BTreeMap::from([("demo-counter".to_string(), json!({"start": 5}))]),
        freeze: Default::default(),
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

#[test]
fn manifest_failures() {
    let root = temp_root("manifest");
    /// (包名, 覆盖文件, 错误须包含的子串)
    type ManifestCase = (
        &'static str,
        Vec<(&'static str, &'static str)>,
        &'static str,
    );
    let cases: Vec<ManifestCase> = vec![
        (
            "unknown-field",
            vec![(
                "mod.json",
                r#"{"id":"unknown-field","version":"1.0.0","apiVersion":"^1.0.0","extra":1}"#,
            )],
            "mod.json 解析失败",
        ),
        (
            "bad-id",
            vec![(
                "mod.json",
                r#"{"id":"Bad-Id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
            )],
            "不得为 builtin",
        ),
        (
            "builtin-id",
            vec![(
                "mod.json",
                r#"{"id":"builtin","version":"1.0.0","apiVersion":"^1.0.0"}"#,
            )],
            "不得为 builtin",
        ),
        (
            "api-too-new",
            vec![(
                "mod.json",
                r#"{"id":"api-too-new","version":"1.0.0","apiVersion":"^2.0.0"}"#,
            )],
            "不兼容",
        ),
        (
            "short-version",
            vec![(
                "mod.json",
                r#"{"id":"short-version","version":"1.0","apiVersion":"^1.0.0"}"#,
            )],
            "三段数字",
        ),
        (
            "escape-entry",
            vec![(
                "mod.json",
                r#"{"id":"escape-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"../x.js"}"#,
            )],
            "entry 路径",
        ),
        (
            "text-entry",
            vec![(
                "mod.json",
                r#"{"id":"text-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"server.txt"}"#,
            )],
            "entry 路径",
        ),
        (
            "missing-entry",
            vec![(
                "mod.json",
                r#"{"id":"missing-entry","version":"1.0.0","apiVersion":"^1.0.0","entry":"nope.js"}"#,
            )],
            "声明的模块缺失",
        ),
    ];
    for (name, files, needle) in cases {
        write_pkg(&root, name, &files);
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(root.join(name));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dependency_failures() {
    let root = temp_root("graph");
    write_pkg(
        &root,
        "needs-ghost",
        &[(
            "mod.json",
            r#"{"id":"needs-ghost","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"ghost":"^1.0.0"}}"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("依赖的包不存在：ghost"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("graph-range");
    write_pkg(
        &root,
        "needs-newer-base",
        &[(
            "mod.json",
            r#"{"id":"needs-newer-base","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"demo-base":"^2.0.0"}}"#,
        )],
    );
    std::fs::create_dir_all(root.join("demo-base")).expect("建依赖目录");
    std::fs::write(
        root.join("demo-base/mod.json"),
        r#"{"id":"demo-base","version":"1.1.0","apiVersion":"^1.0.0"}"#,
    )
    .expect("写依赖 manifest");
    std::fs::write(
        root.join("demo-base/server.js"),
        "export default function register() {}",
    )
    .expect("写依赖入口");
    let error = register_root_error(&root);
    assert!(error.contains("需要 ^2.0.0，实得 1.1.0"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("graph-cycle");
    for (name, dependency) in [("cycle-a", "cycle-b"), ("cycle-b", "cycle-a")] {
        write_pkg(
            &root,
            name,
            &[(
                "mod.json",
                format!(
                    r#"{{"id":"{name}","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{{"{dependency}":"^1.0.0"}}}}"#
                )
                .as_str(),
            )],
        );
    }
    let error = register_root_error(&root);
    assert!(error.contains("依赖环"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn register_failures() {
    /// (包名, server.js 内容, 错误须包含的子串)
    type RegisterCase = (&'static str, &'static str, &'static str);
    let cases: Vec<RegisterCase> = vec![
        (
            "foreign-key",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('other-pkg:thing', { version: 1, fields: {} });
}
"#,
            "必须以",
        ),
        (
            "builtin-squat",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('builtin:thing', { version: 1, fields: {} });
}
"#,
            "必须以",
        ),
        (
            "missing-process",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.intents.define('missing-process:hit', { scope: 'room', args: { type: 'bool' } });
}
"#,
            "需要一个函数",
        ),
        (
            "duplicate-handler",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'dup' }, function a() {});
  api.handlers.after('builtin:link.tick', { id: 'dup' }, function b() {});
}
"#,
            "重复注册",
        ),
        (
            "raw-unknown-kind",
            r#"
export default function register() {
  Deno.core.ops.op_mod_declare('weird-kind', {});
}
"#,
            "未知声明种类",
        ),
        (
            "rule-without-consumers",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.rules.define('rule-without-consumers:cap', { type: 'i32', default: 5 });
}
"#,
            "消费者",
        ),
        (
            "async-register",
            r#"
export default async function register() {}
"#,
            "不得返回 Promise",
        ),
        (
            "throws-in-register",
            r#"
export default function register() {
  throw new Error('注册期抛错');
}
"#,
            "注册期抛错",
        ),
        (
            "escape-import",
            r#"
import { value } from '../outside.js';
export default function register() {}
"#,
            "包根之外",
        ),
        (
            "bare-import",
            r#"
import fs from 'fs';
export default function register() {}
"#,
            "外部导入",
        ),
    ];
    let root = temp_root("register");
    for (name, server, needle) in cases {
        write_pkg(&root, name, &[("server.js", server)]);
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(root.join(name));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cross_package_import_rejected() {
    let root = temp_root("cross");
    write_pkg(
        &root,
        "importer",
        &[(
            "server.js",
            r#"
import { helper } from '../provider/helper.js';
export default function register() {}
"#,
        )],
    );
    write_pkg(
        &root,
        "provider",
        &[("helper.js", "export function helper() {}\n")],
    );
    let error = register_root_error(&root);
    assert!(error.contains("跨包导入被拒绝"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn seal_blocks_late_registration() {
    let root = temp_root("seal");
    write_pkg(
        &root,
        "leaky",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  globalThis.__leaked = api;
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let probe = realm
        .evaluate_probe(
            r#"(() => {
                try {
                    globalThis.__leaked.rules.define('leaky:late', { type: 'i32', default: 1 });
                    return 'late-registration-unexpectedly-succeeded';
                } catch (error) {
                    return 'sealed:' + (error instanceof Error);
                }
            })()"#,
        )
        .expect("探针可执行");
    assert_eq!(
        probe,
        json!("sealed:true"),
        "密封后经泄漏的 api 注册必须抛错"
    );
    let raw = realm
        .evaluate_probe(
            r#"(() => {
                try {
                    Deno.core.ops.op_mod_declare('rule', {});
                    return 'raw-declare-unexpectedly-succeeded';
                } catch (error) {
                    return 'sealed:' + (error instanceof Error);
                }
            })()"#,
        )
        .expect("探针可执行");
    assert_eq!(raw, json!("sealed:true"), "密封后直连 op 声明必须抛错");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn second_register_rejected() {
    let root = temp_root("twice");
    write_pkg(&root, "plain", &[]);
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("首次注册成功");
    let error = realm
        .register(&packages, RegisterOptions::default())
        .expect_err("二次注册必须失败");
    assert!(error.to_string().contains("已密封"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn duplicate_package_id_rejected() {
    let root = temp_root("dup-id");
    write_pkg(
        &root,
        "first",
        &[(
            "mod.json",
            r#"{"id":"same-id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
        )],
    );
    write_pkg(
        &root,
        "second",
        &[(
            "mod.json",
            r#"{"id":"same-id","version":"1.0.0","apiVersion":"^1.0.0"}"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("重复的包 id"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn symlink_rejected() {
    let root = temp_root("symlink");
    let outside = root.join("outside.js");
    std::fs::write(&outside, "export const x = 1;\n").expect("写包外文件");
    write_pkg(&root, "with-link", &[]);
    std::os::unix::fs::symlink(outside, root.join("with-link").join("linked.js"))
        .expect("建符号链接");
    let error = register_root_error(&root);
    assert!(error.contains("符号链接"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn package_digests_track_server_bytes_only_for_code() {
    let packages = discover_packages(&fixtures_root()).expect("夹具可装载");
    let counter: &ModPackage = packages
        .values()
        .find(|p| p.id() == "demo-counter")
        .expect("counter 夹具存在");
    let mut modified = counter.clone();
    modified.files.insert(
        Path::new("README.md").to_path_buf(),
        b"docs change".to_vec(),
    );
    assert_ne!(
        modified.code_digest(),
        counter.code_digest(),
        "服务端文件变化必须改变代码摘要"
    );
    let mut manifest_changed = counter.clone();
    manifest_changed.files.insert(
        Path::new("mod.json").to_path_buf(),
        br#"{"id":"demo-counter","version":"0.3.1","apiVersion":"^1.0.0","dependencies":{"demo-base":"^2.0.0"},"player":"player.js"}"#
            .to_vec(),
    );
    assert_ne!(
        manifest_changed.code_digest(),
        counter.code_digest(),
        "manifest 字节属于包身份，改动必须改变代码摘要"
    );
    let mut player_changed = counter.clone();
    player_changed.files.insert(
        Path::new("player.js").to_path_buf(),
        b"export default function install(v) {}".to_vec(),
    );
    assert_eq!(
        player_changed.code_digest(),
        counter.code_digest(),
        "玩家模块不参与服务端代码摘要"
    );
    assert_ne!(
        player_changed.player_api_digest(),
        counter.player_api_digest(),
        "玩家模块变化改变玩家面摘要"
    );
}

#[test]
fn dynamic_import_rejected() {
    let root = temp_root("dyn-import");
    write_pkg(
        &root,
        "dyn",
        &[
            (
                "server.js",
                r#"
await import('./other.js');
export default function register() {}
"#,
            ),
            ("other.js", "export const x = 1;\n"),
        ],
    );
    let error = register_root_error(&root);
    assert!(error.contains("不支持动态导入"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn f64_domain_requires_explicit_bounds() {
    let root = temp_root("f64-bounds");
    write_pkg(
        &root,
        "loose-float",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.rules.define('loose-float:ratio', { type: 'f64', default: 0.5, consumers: [] });
}
"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(
        error.contains("f64 数值域须显式给出 min 与 max"),
        "实得 {error}"
    );
    let _ = std::fs::remove_dir_all(&root);

    let root = temp_root("f64-bounded");
    write_pkg(
        &root,
        "bounded-float",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const gauge = api.objects.define('bounded-float:gauge', {
    version: 1,
    playerClass: 'Gauge',
    fields: {
      ratio: { type: 'f64', min: 0, max: 0.75, default: 0.5 },
    },
  });
  api.intents.define('bounded-float:calibrate', {
    scope: 'object',
    target: gauge,
    args: { type: 'record', fields: { to: { type: 'f64', min: 0, max: 0.75 } } },
    process() {},
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("显式双侧界的 f64 可注册");
    let gauge = key("bounded-float:gauge");
    let handle = registered
        .plan
        .projection()
        .resolve(&gauge)
        .expect("类型已注册");
    assert!(
        registered.plan.projection().get(handle).is_some(),
        "有界 f64 字段通过冻结"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cross_namespace_ref_field_allowed() {
    let root = temp_root("cross-ref");
    write_pkg(
        &root,
        "provider",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('provider:thing', {
    version: 1,
    playerClass: 'Thing',
    fields: { tag: { type: 'string', maxBytes: 16 } },
  });
}
"#,
        )],
    );
    write_pkg(
        &root,
        "consumer",
        &[(
            "mod.json",
            r#"{"id":"consumer","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"provider":"^1.0.0"}}"#,
        )],
    );
    std::fs::write(
        root.join("consumer/server.js"),
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('consumer:holder', {
    version: 1,
    playerClass: 'Holder',
    fields: {
      target: { type: 'ref', target: 'provider:thing', version: 1 },
    },
  });
}
"#,
    )
    .expect("写 consumer 入口");
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("跨命名空间 ref 字段可注册");
    let holder = key("consumer:holder");
    let handle = registered
        .plan
        .projection()
        .resolve(&holder)
        .expect("类型已注册");
    assert!(
        registered.plan.projection().get(handle).is_some(),
        "ref 字段通过冻结（引用与挂载同为跨包面）"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn failed_register_poisons_realm() {
    let root = temp_root("poison");
    write_pkg(
        &root,
        "doomed",
        &[("server.js", "export default async function register() {}")],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let first = realm.register(&packages, RegisterOptions::default());
    assert!(first.is_err(), "async register 必须失败");
    let error = realm
        .register(&packages, RegisterOptions::default())
        .expect_err("失败后的 realm 不可再用");
    assert!(
        error.to_string().contains("已密封"),
        "失败应毒化 realm，实得 {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dependency_order_is_not_lexicographic() {
    let root = temp_root("topo");
    write_pkg(
        &root,
        "aaa",
        &[(
            "mod.json",
            r#"{"id":"aaa","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"zzz":"^1.0.0"}}"#,
        )],
    );
    write_pkg(&root, "zzz", &[]);
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let order: Vec<&str> = registered
        .contributions
        .iter()
        .map(|c| c.source.namespace.as_str())
        .collect();
    assert_eq!(
        order,
        vec!["zzz", "aaa"],
        "拓扑序由依赖边决定，不受字典序影响"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn module_path_negative_shapes() {
    let root = temp_root("path-shapes");
    let cases: Vec<(&str, &str, &str)> = vec![
        ("abs-entry", r#","entry":"/x.js""#, "entry 路径"),
        ("drive-entry", r#","entry":"C:/x.js""#, "entry 路径"),
        ("backslash-entry", r#","entry":"a\\b.js""#, "entry 路径"),
        ("dot-entry", r#","entry":"./x.js""#, "entry 路径"),
        ("player-escape", r#","player":"../up.js""#, "player 路径"),
        ("player-text", r#","player":"notes.txt""#, "player 路径"),
        (
            "player-equals-entry",
            r#","player":"server.js""#,
            "不得指向同一文件",
        ),
    ];
    for (name, fragment, needle) in cases {
        let manifest =
            format!(r#"{{"id":"{name}","version":"1.0.0","apiVersion":"^1.0.0"{fragment}}}"#);
        write_pkg(&root, name, &[("mod.json", manifest.as_str())]);
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(root.join(name));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn dotted_field_name_and_loose_numeric_options_rejected() {
    let root = temp_root("strict-shapes");
    write_pkg(
        &root,
        "dotty",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('dotty:thing', {
    version: 1,
    playerClass: 'Thing',
    fields: { 'a.b': { type: 'bool' } },
  });
}
"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("不得为空、含点号或冒号"), "实得 {error}");
    let _ = std::fs::remove_dir_all(root.join("dotty"));

    write_pkg(
        &root,
        "stringly",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('stringly:thing', {
    version: 1,
    playerClass: 'Thing',
    fields: { count: { type: 'i32', min: '5' } },
  });
}
"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(error.contains("字段 min 须为整数"), "实得 {error}");
    let _ = std::fs::remove_dir_all(root.join("stringly"));

    write_pkg(
        &root,
        "loose-rule-bound",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'watch' }, function w() {});
  api.rules.define('loose-rule-bound:cap', {
    type: 'i32', min: '5', default: 6,
    consumers: ['loose-rule-bound:handler.watch'],
  });
}
"#,
        )],
    );
    let error = register_root_error(&root);
    assert!(
        error.contains("字段 min 须为整数"),
        "规则边界同样不得静默落默认：实得 {error}"
    );
    let _ = std::fs::remove_dir_all(root.join("loose-rule-bound"));

    write_pkg(
        &root,
        "float-rule-bound",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'watch' }, function w() {});
  api.rules.define('float-rule-bound:ratio', {
    type: 'f64', min: 0, max: 0.75, default: 0.5,
    consumers: ['float-rule-bound:handler.watch'],
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("f64 规则支持小数端点");
    let _ = std::fs::remove_dir_all(root.join("float-rule-bound"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn oversized_file_rejected_before_full_read() {
    let root = temp_root("oversize");
    write_pkg(&root, "big", &[]);
    let big = root.join("big/blob.bin");
    let chunk = vec![0u8; 1024 * 1024];
    let mut file = std::fs::File::create(&big).expect("建大文件");
    for _ in 0..9 {
        std::io::Write::write_all(&mut file, &chunk).expect("写大文件");
    }
    let error = register_root_error(&root);
    assert!(error.contains("超过装载上限"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn queue_mode_and_max_len_pass_through() {
    let root = temp_root("queue-mode");
    write_pkg(
        &root,
        "queued",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const sensor = api.objects.define('queued:sensor', {
    version: 1,
    playerClass: 'Sensor',
    fields: {},
  });
  api.intents.define('queued:pulse', {
    scope: 'object',
    target: sensor,
    args: { type: 'bool' },
    mode: 'queue',
    queueMax: 5,
    process() {},
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let intent = registered
        .plan
        .resolve_intent(&key("queued:pulse"))
        .expect("动作已注册");
    let intent = registered.plan.get_intent(intent).expect("句柄有效");
    assert_eq!(
        intent.merge,
        kk_mod_api::Merge::Queue { max_len: 5 },
        "queue 模式与上限透传到冻结计划"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn symlinked_package_directory_rejected() {
    let root = temp_root("dir-link");
    let real = root.join("real-pkg");
    std::fs::create_dir_all(&real).expect("建真实目录");
    std::fs::write(
        real.join("mod.json"),
        r#"{"id":"real-pkg","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .expect("写 manifest");
    std::fs::write(
        real.join("server.js"),
        "export default function register() {}",
    )
    .expect("写入口");
    std::os::unix::fs::symlink(&real, root.join("linked")).expect("建目录链接");
    let error = register_root_error(&root);
    assert!(error.contains("包目录不得为符号链接"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn invoke_dispatches_handler_with_state_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_root("invoke");
    write_pkg(
        &root,
        "tally",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'tally' }, function tally(ctx) {
    const count = (ctx.state && ctx.state.count) || 0;
    ctx.state = { count: count + 1, room: ctx.room, tick: ctx.tick };
    return 'seen';
  });
  api.handlers.after('builtin:link.tick', { id: 'async-bad' }, function bad() {
    return { then() {} };
  });
  api.handlers.after('builtin:link.tick', { id: 'throwing' }, function boom() {
    throw new Error('处理器抛错');
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");

    let mut state = serde_json::Value::Null;
    for tick in [7u64, 8] {
        let ctx = json!({"tick": tick, "room": "E1N1", "state": state});
        let outcome = realm
            .invoke("tally:handler.tally", ctx)
            .expect("处理器可调用");
        state = outcome["state"].clone();
        assert_eq!(outcome["result"], json!("seen"));
        assert_eq!(state["count"], json!(tick - 6), "状态在调用间由调用方续传");
        assert_eq!(state["room"], json!("E1N1"));
    }

    let promise = realm
        .invoke(
            "tally:handler.async-bad",
            json!({"tick": 1, "room": "E1N1", "state": null}),
        )
        .expect_err("thenable 结果必须被拒");
    assert!(promise.to_string().contains("同步完成"), "实得 {promise}");
    let thrown = realm
        .invoke(
            "tally:handler.throwing",
            json!({"tick": 1, "room": "E1N1", "state": null}),
        )
        .expect_err("处理器异常必须传播");
    assert!(thrown.to_string().contains("处理器抛错"), "实得 {thrown}");
    let unknown = realm
        .invoke("tally:handler.ghost", json!({}))
        .expect_err("未知绑定必须失败");
    assert!(
        unknown.to_string().contains("没有注册函数"),
        "实得 {unknown}"
    );
    assert!(
        registered.executables.contains(&key("tally:handler.tally")),
        "派发键来自密封绑定表"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn invoke_null_result_and_state_clear() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_root("invoke-null");
    write_pkg(
        &root,
        "nullable",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'echo' }, function echo(ctx) {
    if (ctx.tick === 2) {
      ctx.state = null;
      return null;
    }
    ctx.state = { seen: ctx.tick };
    return null;
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let out = realm
        .invoke(
            "nullable:handler.echo",
            json!({"tick": 1, "room": "W1N1", "state": null}),
        )
        .expect("null 返回可用");
    assert_eq!(out["result"], serde_json::Value::Null);
    assert_eq!(out["state"]["seen"], serde_json::json!(1));
    let cleared = realm
        .invoke(
            "nullable:handler.echo",
            json!({"tick": 2, "room": "W1N1", "state": json!({"seen": 1})}),
        )
        .expect("清空调用可用");
    assert_eq!(cleared["state"], serde_json::Value::Null, "写 null 即清空");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
