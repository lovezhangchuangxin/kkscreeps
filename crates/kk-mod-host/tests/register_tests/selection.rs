//! 新世界启动期的显式内容选择：展开、诊断与实际安装内容。
use super::*;
use kk_mod_host::ContentSelection;
use std::collections::BTreeSet;

fn selection(include: &[&str], exclude: &[&str]) -> RegisterOptions {
    RegisterOptions {
        selection: ContentSelection {
            include: include.iter().map(|id| id.to_string()).collect(),
            exclude: exclude.iter().map(|id| id.to_string()).collect(),
        },
        ..Default::default()
    }
}

fn register_error(packages: &BTreeMap<String, ModPackage>, options: RegisterOptions) -> String {
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(packages, options)
        .expect_err("期望注册失败")
        .to_string()
}

/// 独立包 alpha（自有类型 + 玩家面）、依赖 alpha 的 beta、独立包 gamma。
fn content_root(label: &str) -> PathBuf {
    let root = temp_root(label);
    write_pkg(
        &root,
        "alpha",
        &[
            (
                "mod.json",
                r#"{"id":"alpha","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
            ),
            (
                "server.js",
                r#"export default function register(api){
  api.objects.define('alpha:node', {
    version: 1, playerClass: 'AlphaNode', visibility: 'room-visible', positionFields: ['x','y'],
    fields: { x: {type:'u32',min:0,max:49}, y: {type:'u32',min:0,max:49}, value: {type:'u32',default:0} },
    tick(ctx) { ctx.target.fields.value++; },
  });
}"#,
            ),
            (
                "player.js",
                r#"export default function install(api) {
  class AlphaNode extends api.builtins.RoomObject {}
  api.objects.bind('alpha:node', AlphaNode);
  api.globals.define('AlphaNode', AlphaNode);
}"#,
            ),
        ],
    );
    write_pkg(
        &root,
        "beta",
        &[
            (
                "mod.json",
                r#"{"id":"beta","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"alpha":"^1.0.0"}}"#,
            ),
            (
                "server.js",
                r#"export default function register(api){
  api.rules.override('builtin:link.capacity', 700);
}"#,
            ),
        ],
    );
    write_pkg(
        &root,
        "gamma",
        &[(
            "server.js",
            r#"export default function register(api){
  api.handlers.before('builtin:link.tick', {id:'g'}, () => {});
}"#,
        )],
    );
    root
}

fn selected_ids(registered: &kk_mod_host::Registered) -> Vec<String> {
    registered
        .plan
        .sources()
        .keys()
        .filter(|ns| ns.as_str() != "builtin")
        .cloned()
        .collect()
}

#[test]
fn selection_expands_dependency_closure_and_reports_identity() {
    let root = content_root("expands");
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, selection(&["beta"], &[]))
        .unwrap();
    assert_eq!(selected_ids(&registered), vec!["alpha", "beta"]);
    let report = &registered.composition;
    assert_eq!(report.report_version, 3);
    assert_eq!(report.selection.requested, vec!["beta"]);
    assert_eq!(report.selection.excluded, Vec::<String>::new());
    assert_eq!(report.selection.selected, vec!["alpha", "beta"]);
    assert_eq!(report.selection.auto_added, vec!["alpha"]);
    assert_eq!(report.selection.skipped, vec!["gamma"]);
    // 未选包的处理器完全不在计划内；玩家面只导出已选包。
    assert!(!registered.executables.contains(&key("gamma:handler.g")));
    assert_eq!(
        registered
            .player_modules
            .iter()
            .map(|m| m.namespace.clone())
            .collect::<Vec<_>>(),
        vec!["alpha"]
    );
    assert!(
        registered
            .plan
            .projection()
            .resolve(&key("alpha:node"))
            .is_some()
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn selection_changes_plan_hash_for_same_discovered_root() {
    let root = content_root("hash");
    let packages = discover_packages(&root).unwrap();
    let mut full = ModRealm::new().unwrap();
    let full = full
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let mut only = ModRealm::new().unwrap();
    let only = only
        .register(&packages, selection(&["alpha"], &[]))
        .unwrap();
    assert_ne!(full.plan.hash(), only.plan.hash());
    assert_eq!(
        full.composition.selection.selected,
        vec!["alpha", "beta", "gamma"]
    );
    assert_eq!(only.composition.selection.selected, vec!["alpha"]);
    assert_eq!(
        full.composition.selection.auto_added,
        Vec::<String>::new(),
        "无显式选择时不报告自动加入"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn same_installed_set_from_different_inputs_shares_plan_hash() {
    let root = content_root("same-set");
    let packages = discover_packages(&root).unwrap();
    let mut included = ModRealm::new().unwrap();
    let included = included
        .register(&packages, selection(&["alpha", "gamma"], &[]))
        .unwrap();
    let mut excluded = ModRealm::new().unwrap();
    let excluded = excluded
        .register(&packages, selection(&[], &["beta"]))
        .unwrap();
    // 安装集合都是 {alpha, gamma}：计划身份一致，但报告保留各自输入。
    assert_eq!(included.plan.hash(), excluded.plan.hash());
    assert_eq!(
        included.composition.selection.selected,
        vec!["alpha", "gamma"]
    );
    assert_eq!(
        included.composition.selection.requested,
        vec!["alpha", "gamma"]
    );
    assert_eq!(
        excluded.composition.selection.requested,
        Vec::<String>::new()
    );
    assert_eq!(excluded.composition.selection.excluded, vec!["beta"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn content_selection_serde_shape_is_locked() {
    let empty: ContentSelection = serde_json::from_str("{}").unwrap();
    assert!(empty.is_empty());
    let parsed: ContentSelection =
        serde_json::from_str(r#"{"include":["a","a"],"exclude":[]}"#).unwrap();
    assert_eq!(parsed.include, BTreeSet::from(["a".to_string()]));
    assert!(serde_json::from_str::<ContentSelection>(r#"{"include":["a"],"unknown":1}"#).is_err());
    assert!(serde_json::from_str::<ContentSelection>(r#"{"include":"a"}"#).is_err());
    assert!(serde_json::from_str::<ContentSelection>("null").is_err());
}

#[test]
fn no_selection_keeps_current_behavior() {
    let root = content_root("none");
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    assert_eq!(selected_ids(&registered), vec!["alpha", "beta", "gamma"]);
    let selection = &registered.composition.selection;
    assert!(selection.requested.is_empty());
    assert!(selection.excluded.is_empty());
    assert_eq!(selection.selected, vec!["alpha", "beta", "gamma"]);
    assert!(selection.auto_added.is_empty());
    assert!(selection.skipped.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unknown_selection_ids_and_builtin_are_rejected() {
    let root = content_root("unknown");
    let packages = discover_packages(&root).unwrap();
    let error = register_error(&packages, selection(&["ghost"], &[]));
    assert!(
        error.contains("选择的包不存在：ghost") && error.contains("alpha"),
        "实得 {error}"
    );
    let error = register_error(&packages, selection(&[], &["ghost"]));
    assert!(error.contains("排除的包不存在：ghost"), "实得 {error}");
    let error = register_error(&packages, selection(&["builtin"], &[]));
    assert!(error.contains("builtin 是固定内建贡献"), "实得 {error}");
    let error = register_error(&packages, selection(&["alpha"], &["alpha"]));
    assert!(
        error.contains("不能同时显式选择与排除：alpha"),
        "实得 {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_dependency_error_carries_dependency_path() {
    let root = temp_root("selection-missing");
    write_pkg(
        &root,
        "outer",
        &[(
            "mod.json",
            r#"{"id":"outer","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"inner":"^1.0.0"}}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let error = register_error(&packages, selection(&["outer"], &[]));
    assert!(
        error.contains("缺失依赖：outer → inner") && error.contains("不在包根目录"),
        "实得 {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exclusion_collision_reports_dependency_path_without_rejoin() {
    let root = temp_root("selection-collision");
    write_pkg(
        &root,
        "top",
        &[(
            "mod.json",
            r#"{"id":"top","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"middle":"^1.0.0"}}"#,
        )],
    );
    write_pkg(
        &root,
        "middle",
        &[(
            "mod.json",
            r#"{"id":"middle","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"bottom":"^1.0.0"}}"#,
        )],
    );
    write_pkg(&root, "bottom", &[]);
    let packages = discover_packages(&root).unwrap();
    let error = register_error(&packages, selection(&["top"], &["bottom"]));
    assert!(
        error.contains("显式排除 bottom 与必需依赖相撞") && error.contains("top → middle → bottom"),
        "实得 {error}"
    );
    // include 为空时，种子是全部留存包：任何指向被排除包的必需依赖
    // 路径（此处按字典序先从 middle 展开）都构成拒绝证据。
    let error = register_error(&packages, selection(&[], &["bottom"]));
    assert!(
        error.contains("显式排除 bottom 与必需依赖相撞") && error.contains("→ bottom"),
        "实得 {error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exclusion_without_selection_installs_the_rest() {
    let root = content_root("exclude-rest");
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, selection(&[], &["beta"]))
        .unwrap();
    assert_eq!(selected_ids(&registered), vec!["alpha", "gamma"]);
    let selection = &registered.composition.selection;
    assert_eq!(selection.excluded, vec!["beta"]);
    assert_eq!(selection.selected, vec!["alpha", "gamma"]);
    assert_eq!(selection.skipped, vec!["beta"]);
    assert!(selection.auto_added.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unselected_broken_package_does_not_block_selection() {
    let root = temp_root("selection-broken");
    write_pkg(&root, "healthy", &[]);
    write_pkg(
        &root,
        "broken",
        &[(
            "mod.json",
            r#"{"id":"broken","version":"1.0.0","apiVersion":"^1.0.0","dependencies":{"ghost":"^1.0.0"}}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    // 不做选择时既有行为保留：坏包照常阻塞装载。
    assert!(register_error(&packages, RegisterOptions::default()).contains("依赖的包不存在"));
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, selection(&["healthy"], &[]))
        .unwrap();
    assert_eq!(selected_ids(&registered), vec!["healthy"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn handler_dependency_on_unselected_package_keeps_freeze_error() {
    let root = temp_root("selection-handler");
    write_pkg(
        &root,
        "dep",
        &[(
            "server.js",
            r#"export default function register(api){
  api.handlers.before('builtin:link.tick', {id:'x'}, () => {});
}"#,
        )],
    );
    write_pkg(
        &root,
        "user",
        &[(
            "server.js",
            r#"export default function register(api){
  api.handlers.before('builtin:link.tick', {id:'y', dependsOn:['dep:handler.x']}, () => {});
}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    // 未选 dep：沿用未知处理器依赖的冻结错误，不静默跳过。
    let error = register_error(&packages, selection(&["user"], &[]));
    assert!(
        error.contains("未知处理器依赖 dep:handler.x"),
        "实得 {error}"
    );
    // 显式选择后同一注册成功：handler 引用不参与选择展开。
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, selection(&["user", "dep"], &[]))
        .unwrap();
    assert_eq!(selected_ids(&registered), vec!["dep", "user"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn configs_and_disabled_for_unselected_packages_are_rejected() {
    let root = content_root("configs");
    let packages = discover_packages(&root).unwrap();
    let mut options = selection(&["alpha"], &[]);
    options.configs.insert("beta".into(), json!({"any":1}));
    let error = register_error(&packages, options);
    assert!(error.contains("配置指定了未选择的包 beta"), "实得 {error}");
    let mut realm = ModRealm::new().unwrap();
    let error = realm
        .register_with_disabled(
            &packages,
            selection(&["alpha"], &[]),
            &BTreeSet::from(["gamma".to_string()]),
        )
        .expect_err("未选包不可停用")
        .to_string();
    assert!(error.contains("未被内容选择安装"), "实得 {error}");
    let _ = std::fs::remove_dir_all(&root);
}
