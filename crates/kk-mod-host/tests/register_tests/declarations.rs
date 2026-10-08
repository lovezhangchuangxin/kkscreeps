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
            // G07.b：suppressedBy 转换形态（非数组 / 非字符串元素）。
            "suppressed-not-array",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.intents.define('suppressed-not-array:hit', {
    scope: 'room', args: { type: 'bool' }, suppressedBy: 'plain-string',
    process() {},
  });
}
"#,
            "须为字符串数组",
        ),
        (
            "suppressed-not-string",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.intents.define('suppressed-not-string:hit', {
    scope: 'room', args: { type: 'bool' }, suppressedBy: [42],
    process() {},
  });
}
"#,
            "须为字符串",
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
fn suppressed_by_null_registers_as_undeclared() {
    // G07.b：suppressedBy: null 与缺省同判（与 queueMax/mode 容忍一致），
    // 空数组才是显式空形态——注册不因 null 失败。
    let root = temp_root("suppress-null");
    write_pkg(
        &root,
        "nullable",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.intents.define('nullable:hit', {
    scope: 'room', args: { type: 'bool' }, suppressedBy: null,
    process() {},
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("suppressedBy=null 等价未声明，注册成功");
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

/// 命名空间资源声明：成功进入计划与组合报告；形状与归属错误整体拒绝。
#[test]
fn resource_declarations_enter_plan_and_report() {
    let root = temp_root("resources-ok");
    write_pkg(
        &root,
        "miner",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('miner:crystal');
  api.resources.define('miner:dust', {});
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let keys: Vec<&str> = registered
        .plan
        .resources()
        .iter()
        .map(kk_mod_api::Key::as_str)
        .collect();
    assert_eq!(keys, ["miner:crystal", "miner:dust"]);
    let catalog = kk_mod_api::resources::ResourceCatalog::from_plan(&registered.plan);
    assert!(catalog.contains("energy") && catalog.contains("miner:crystal"));
    let reported: Vec<&str> = registered
        .composition
        .resources
        .iter()
        .map(|entry| entry.key.as_str())
        .collect();
    assert_eq!(reported, ["miner:crystal", "miner:dust"]);
    assert_eq!(
        registered.composition.resources[0].source, "miner",
        "报告记录定义来源"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn resource_declaration_failures() {
    type ResourceCase = (&'static str, &'static str, &'static str);
    let cases: Vec<ResourceCase> = vec![
        (
            "foreign-resource",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('other:spark');
}
"#,
            "必须以",
        ),
        (
            "unknown-resource-option",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('unknown-resource-option:spark', { capacity: 5 });
}
"#,
            "声明了未知选项",
        ),
        (
            "resource-spec-shape",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('resource-spec-shape:spark', 3);
}
"#,
            "的说明须为对象",
        ),
        (
            "duplicate-resource",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('duplicate-resource:spark');
  api.resources.define('duplicate-resource:spark');
}
"#,
            "重复声明的资源",
        ),
        (
            "raw-resource-extra-member",
            r#"
export default function register() {
  Deno.core.ops.op_mod_declare('resource', { key: 'raw-resource-extra-member:spark', capacity: 1 });
}
"#,
            "未知选项",
        ),
    ];
    let root = temp_root("resources-bad");
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

/// 建筑声明：成功进入计划与组合报告，配方数据完整透传。
#[test]
fn building_declarations_enter_plan_and_report() {
    let root = temp_root("buildings-ok");
    write_pkg(
        &root,
        "forge",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.resources.define('forge:ore');
  api.resources.define('forge:ingot');
  api.buildings.define('forge:foundry', {
    playerClass: 'Foundry',
    cost: { energy: 5000, 'forge:ore': 200 },
    hitsMax: 5000,
    pathing: 'obstacle',
    capacity: 1000,
    production: {
      interval: 12,
      inputs: { energy: 60, 'forge:ore': 30 },
      outputs: { 'forge:ingot': 10 },
    },
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
    let catalog =
        kk_mod_api::buildings::BuildingCatalog::from_plan(&registered.plan).expect("玩家类名合法");
    let b = catalog.get("forge:foundry").expect("建筑进入目录");
    assert_eq!(
        b.cost,
        vec![("energy".to_string(), 5000), ("forge:ore".to_string(), 200)]
    );
    assert_eq!(b.progress_total(), 5000);
    assert_eq!(b.hits_max, 5000);
    assert_eq!(b.pathing, kk_mod_api::buildings::BuildingPathing::Obstacle);
    assert_eq!(b.store_capacity, Some(1000));
    let production = b.production.as_ref().expect("生产配方透传");
    assert_eq!(production.interval, 12);
    assert_eq!(
        production.inputs,
        vec![("energy".to_string(), 60), ("forge:ore".to_string(), 30)]
    );
    assert_eq!(production.outputs, vec![("forge:ingot".to_string(), 10)]);
    // 组合报告记录建筑目录与定义来源。
    assert_eq!(registered.composition.buildings.len(), 1);
    assert_eq!(
        registered.composition.buildings[0].key.as_str(),
        "forge:foundry"
    );
    assert_eq!(registered.composition.buildings[0].source, "forge");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn building_declaration_failures() {
    type BuildingCase = (&'static str, &'static str, &'static str);
    let cases: Vec<BuildingCase> = vec![
        (
            "foreign-building-key",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('other:mill', { playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'obstacle' });
}
"#,
            "必须以",
        ),
        (
            "unknown-building-option",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('unknown-building-option:mill', { playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'obstacle', hitMax: 200 });
}
"#,
            "未知选项",
        ),
        (
            "missing-building-cost",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('missing-building-cost:mill', { playerClass: 'Mill', hitsMax: 100, pathing: 'obstacle' });
}
"#,
            "缺少成本表",
        ),
        (
            "bad-pathing",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('bad-pathing:mill', { playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'sky' });
}
"#,
            "pathing",
        ),
        (
            "missing-player-class",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('missing-player-class:mill', { cost: { energy: 100 }, hitsMax: 100, pathing: 'road' });
}
"#,
            "playerClass",
        ),
        (
            "unknown-production-option",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('unknown-production-option:mill', { playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'plain', production: { rate: 5 } });
}
"#,
            "生产配方声明了未知选项",
        ),
        (
            "outputs-without-capacity",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('outputs-without-capacity:mill', { playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'plain', production: { interval: 10, outputs: { energy: 5 } } });
}
"#,
            "必须声明仓库容量",
        ),
        (
            "cost-unknown-resource",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.buildings.define('cost-unknown-resource:mill', { playerClass: 'Mill', cost: { 'cost-unknown-resource:ghost': 10 }, hitsMax: 100, pathing: 'plain' });
}
"#,
            "引用未知资源",
        ),
        (
            "raw-building-extra-member",
            r#"
export default function register() {
  Deno.core.ops.op_mod_declare('building', { key: 'raw-building-extra-member:mill', playerClass: 'Mill', cost: { energy: 100 }, hitsMax: 100, pathing: 'plain', hitMax: 1 });
}
"#,
            "未知选项",
        ),
    ];
    let root = temp_root("buildings-bad");
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

/// G09.d：对象反击比率 retaliationRatio 从 JS 浮点转毫精度整数入冻结
/// 描述符；非法比率与无耐久声明在注册/冻结层拒绝。
#[test]
fn object_retaliation_ratio_converts_and_rejects_bad_values() {
    // 合法：0.5 → 500 毫。
    let root = std::env::temp_dir().join("kk-mod-host-retaliation-ok");
    let _ = std::fs::remove_dir_all(&root);
    write_pkg(
        &root,
        "retaliation-ok-spike",
        &[
            (
                "mod.json",
                r#"{"id": "retaliation-ok-spike", "version": "1.0.0", "apiVersion": "^1.0.0"}"#,
            ),
            (
                "server.js",
                r#"
export default function register(api) {
  api.objects.define('retaliation-ok-spike:spike', {
    version: 1, playerClass: 'Spike', hitsMax: 300, retaliationRatio: 0.5,
    fields: {},
  });
}
"#,
            ),
        ],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let ty = registered
        .plan
        .projection()
        .types()
        .iter()
        .find(|ty| ty.descriptor.key.as_str() == "retaliation-ok-spike:spike")
        .expect("类型已冻结");
    assert_eq!(ty.descriptor.retaliation_milli, Some(500));
    assert_eq!(ty.descriptor.hits_max, Some(300));
    let _ = std::fs::remove_dir_all(&root);

    // 非法：超界、零、无耐久声明。
    for (name, ratio, needle) in [
        ("retaliation-too-big", "1.5", "retaliationRatio 须为 (0,1]"),
        ("retaliation-zero", "0", "retaliationRatio 须为 (0,1]"),
        ("retaliation-no-hits", "0.5", "反击声明要求耐久声明"),
    ] {
        let hits = if name == "retaliation-no-hits" {
            ""
        } else {
            "hitsMax: 300, "
        };
        let root = std::env::temp_dir().join(format!("kk-mod-host-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        write_pkg(
            &root,
            name,
            &[
                (
                    "mod.json",
                    &format!(r#"{{"id": "{name}", "version": "1.0.0", "apiVersion": "^1.0.0"}}"#),
                ),
                (
                    "server.js",
                    &format!(
                        r#"
export default function register(api) {{
  api.objects.define('{name}:spike', {{
    version: 1, playerClass: 'Spike', {hits}retaliationRatio: {ratio},
    fields: {{}},
  }});
}}
"#
                    ),
                ),
            ],
        );
        let error = register_root_error(&root);
        assert!(
            error.contains(needle),
            "包 {name} 的错误应包含 {needle:?}，实得 {error}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
