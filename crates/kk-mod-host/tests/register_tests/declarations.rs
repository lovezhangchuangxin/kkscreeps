//! 注册声明、冻结封印与 realm 失败状态。
use super::*;

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
