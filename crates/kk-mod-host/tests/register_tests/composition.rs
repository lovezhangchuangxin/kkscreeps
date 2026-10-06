//! 真实 JS 声明进入冻结图，诊断保留准确规则来源和闭包。
use super::*;
use kk_mod_api::{Dispatch, HandlerKind};
use std::collections::BTreeSet;

#[test]
fn js_handler_dependency_keys_reverse_after_and_canonical_sets_reach_freeze() {
    let root = temp_root("composition-keys");
    write_pkg(
        &root,
        "test",
        &[(
            "server.js",
            r#"export default function register(api){
for(const kind of ['before','wrap','after']){
 const z=api.handlers[kind]('builtin:link.tick',{id:'z.'+kind},()=>{});
 if(z!=='test:handler.z.'+kind)throw Error('returned key');
 api.handlers[kind]('builtin:link.tick',{id:'a.'+kind,dependsOn:[z,z]},()=>{});
}}
"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let slot = registered
        .composition
        .slots
        .iter()
        .find(|s| s.key == key("builtin:link.tick"))
        .unwrap();
    let Dispatch::Chained {
        before,
        wraps,
        after,
        ..
    } = &slot.frozen
    else {
        panic!("chained expected");
    };
    assert_eq!(
        before,
        &[key("test:handler.z.before"), key("test:handler.a.before")]
    );
    assert_eq!(
        wraps,
        &[key("test:handler.z.wrap"), key("test:handler.a.wrap")]
    );
    assert_eq!(
        after,
        &[key("test:handler.a.after"), key("test:handler.z.after")]
    );
    for handler in &registered.composition.handlers {
        if handler.key.as_str().contains("handler.a.") {
            assert_eq!(handler.depends_on.len(), 1);
        }
    }
    assert_eq!(
        registered.composition.plan_hash,
        registered.plan.hash().hex()
    );
    let disabled = registered
        .composition
        .with_disabled(&BTreeSet::from(["test".into()]))
        .unwrap();
    let active = disabled
        .slots
        .iter()
        .find(|s| s.key == key("builtin:link.tick"))
        .unwrap();
    assert!(
        matches!(active.execution, kk_mod_host::ExecutionMode::Hosted),
        "停用不重装引擎 Native 表"
    );
    assert_eq!(
        active.active.as_ref().unwrap().center,
        "builtin:link.tick.native"
    );
    assert!(active.active.as_ref().unwrap().before.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn malformed_missing_cyclic_cross_kind_cross_slot_and_replace_dependencies_fail() {
    let root = temp_root("composition-invalid");
    for (body, expected) in [
        (
            "api.handlers.before('builtin:link.tick',{id:'a',depends_on:['test:handler.b']},()=>{});",
            "未知处理器选项",
        ),
        (
            "try{api.handlers.before('builtin:link.tick',{id:'a',get dependsOn(){throw Error('dependency-getter')}},()=>{})}catch(e){};",
            "转换失败",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:null},()=>{});",
            "须为稳定键数组",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:'test:handler.b'},()=>{});",
            "须为稳定键数组",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:[1]},()=>{});",
            "成员须为稳定键字符串",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['invalid']},()=>{});",
            "键须为",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['missing:handler.b']},()=>{});",
            "未知处理器依赖",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['test:handler.a']},()=>{});",
            "有环",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['test:handler.b']},()=>{});api.handlers.before('builtin:link.tick',{id:'b',dependsOn:['test:handler.a']},()=>{});",
            "有环",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['test:handler.b']},()=>{});api.handlers.after('builtin:link.tick',{id:'b'},()=>{});",
            "同一种 kind",
        ),
        (
            "api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['test:handler.b']},()=>{});api.handlers.before('builtin:link.transfer',{id:'b'},()=>{});",
            "同一入口",
        ),
        (
            "api.handlers.replace('builtin:link.tick',{id:'a',dependsOn:['test:handler.b']},()=>{});api.handlers.replace('builtin:link.tick',{id:'b'},()=>{});",
            "replace 处理器冲突",
        ),
    ] {
        write_pkg(
            &root,
            "test",
            &[(
                "server.js",
                &format!("export default function register(api){{{body}}}"),
            )],
        );
        let error = register_root_error(&root);
        assert!(error.contains(expected), "{body}: {error}");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn report_rule_winners_use_freeze_selection_even_when_values_are_equal_or_disabled() {
    let root = temp_root("composition-rules");
    for id in ["a", "z"] {
        write_pkg(
            &root,
            id,
            &[(
                "server.js",
                "export default function register(api){api.rules.override('builtin:link.capacity',700);}",
            )],
        );
    }
    let packages = discover_packages(&root).unwrap();
    let mut options = RegisterOptions::default();
    options
        .freeze
        .rule_sources
        .insert(key("builtin:link.capacity"), "z".into());
    let mut realm = ModRealm::new().unwrap();
    let registered = realm.register(&packages, options).unwrap();
    let report = registered
        .composition
        .with_disabled(&BTreeSet::from(["z".into()]))
        .unwrap();
    let rule = report
        .rules
        .iter()
        .find(|r| r.key == key("builtin:link.capacity"))
        .unwrap();
    assert_eq!(rule.default, Number::Signed(800));
    assert_eq!(rule.value, Number::Signed(700));
    assert_eq!(rule.selected_source, "z");
    assert_eq!(rule.definition_source, "builtin");
    assert_eq!(rule.overrides.len(), 2);
    assert_eq!(report.plan_hash, registered.plan.hash().hex());
    assert!(
        report
            .handlers
            .iter()
            .all(|h| h.kind != HandlerKind::Replace)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn direct_host_rejects_disabled_masks_missing_handler_dependents() {
    let root = temp_root("composition-mask");
    write_pkg(
        &root,
        "a",
        &[(
            "server.js",
            "export default function register(api){api.handlers.before('builtin:link.tick',{id:'a',dependsOn:['z:handler.z']},()=>{});}",
        )],
    );
    write_pkg(
        &root,
        "z",
        &[(
            "server.js",
            "export default function register(api){api.handlers.before('builtin:link.tick',{id:'z'},()=>{});}",
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let error = realm
        .register_with_disabled(
            &packages,
            RegisterOptions::default(),
            &BTreeSet::from(["z".into()]),
        )
        .unwrap_err();
    assert!(error.to_string().contains("停用集合未覆盖依赖闭包"));
    assert!(
        realm
            .register(&packages, RegisterOptions::default())
            .is_err(),
        "失败 realm 保持密封"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn dependency_getter_can_register_a_handler_without_borrowing_registry_across_js() {
    let root = temp_root("composition-getter");
    write_pkg(
        &root,
        "test",
        &[(
            "server.js",
            r#"export default function register(api){
api.handlers.before('builtin:link.tick',{id:'a',get dependsOn(){const b=api.handlers.before('builtin:link.tick',{id:'b'},()=>{});return [b];}},()=>{});
}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let active = registered
        .composition
        .slots
        .iter()
        .find(|s| s.key == key("builtin:link.tick"))
        .unwrap()
        .active
        .as_ref()
        .unwrap();
    assert_eq!(active.before, vec!["test:handler.b", "test:handler.a"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mod_level_cycle_across_kinds_disables_as_a_group_without_handler_cycle() {
    let root = temp_root("composition-group");
    write_pkg(
        &root,
        "a",
        &[(
            "server.js",
            "export default function register(api){api.handlers.before('builtin:link.tick',{id:'before',dependsOn:['z:handler.before']},()=>{});api.handlers.after('builtin:link.tick',{id:'after'},()=>{});}",
        )],
    );
    write_pkg(
        &root,
        "z",
        &[(
            "server.js",
            "export default function register(api){api.handlers.before('builtin:link.tick',{id:'before'},()=>{});api.handlers.after('builtin:link.tick',{id:'after',dependsOn:['a:handler.after']},()=>{});}",
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let group = BTreeSet::from(["a".into(), "z".into()]);
    assert_eq!(registered.composition.disable_closures["a"], group);
    assert_eq!(registered.composition.disable_closures["z"], group);
    let preview = registered
        .composition
        .with_disabled(&BTreeSet::from(["a".into()]))
        .unwrap();
    assert_eq!(preview.disabled_mods, group);
    let mut realm = ModRealm::new().unwrap();
    let stopped = realm
        .register_with_disabled(&packages, RegisterOptions::default(), &group)
        .unwrap();
    assert_eq!(stopped.composition.disabled_mods, preview.disabled_mods);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn prototype_dependency_property_is_canonicalized_in_the_declaration_op() {
    let root = temp_root("composition-prototype");
    write_pkg(
        &root,
        "test",
        &[(
            "server.js",
            r#"export default function register(api){
const options=Object.create({get dependsOn(){return ['test:handler.z'];}});options.id='a';
api.handlers.before('builtin:link.tick',options,()=>{});
api.handlers.before('builtin:link.tick',{id:'z'},()=>{});
}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let active = registered
        .composition
        .slots
        .iter()
        .find(|s| s.key == key("builtin:link.tick"))
        .unwrap()
        .active
        .as_ref()
        .unwrap();
    assert_eq!(active.before, vec!["test:handler.z", "test:handler.a"]);
    let _ = std::fs::remove_dir_all(root);
}

/// 执行策略身份摘要：确定性（同输入同值）、停用敏感（掩码/闭包变化
/// 即变化）、诊断字段不敏感（无执行语义的派生不入摘要）。
#[test]
fn execution_strategy_digest_is_deterministic_disable_sensitive_and_stable() {
    let root = temp_root("strategy-digest");
    write_pkg(
        &root,
        "test",
        &[(
            "server.js",
            r#"export default function register(api){
api.handlers.before('builtin:link.tick',{id:'a'},()=>{});
api.handlers.replace('builtin:link.tick',{id:'r'},()=>{});
}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let base = registered.composition.execution_strategy_digest().unwrap();
    // 同一组合重算等值（规范化确定性）。
    assert_eq!(
        base,
        registered.composition.execution_strategy_digest().unwrap()
    );

    // 停用改写掩码与槽派生 → 身份变化。
    let disabled = registered
        .composition
        .with_disabled(&BTreeSet::from(["test".into()]))
        .unwrap();
    let masked = disabled.execution_strategy_digest().unwrap();
    assert_ne!(base, masked, "停用掩码必须改变执行策略身份");
    // 停用后链派生变化：before 处理器（test:handler.a）被掩码滤除、
    // 替换中心（test:handler.r）回退原生中心——active 链内容进摘要。
    let chain = |report: &kk_mod_host::CompositionReport| {
        report
            .slots
            .iter()
            .find(|s| s.key == key("builtin:link.tick"))
            .and_then(|s| s.active.as_ref())
            .unwrap()
            .clone()
    };
    let active = chain(&registered.composition);
    let masked_chain = chain(&disabled);
    assert!(active.before.iter().any(|h| h == "test:handler.a"));
    assert!(
        !masked_chain.before.iter().any(|h| h == "test:handler.a"),
        "停用后 before 链不得含停用包处理器"
    );
    assert_eq!(active.center, "test:handler.r");
    assert!(masked_chain.native_center, "替换中心停用后回退原生中心");

    // 重新注册同一包集合 → 身份回到基线（与恢复路径自检同一口径）。
    let mut fresh = ModRealm::new().unwrap();
    let reregistered = fresh
        .register(&packages, RegisterOptions::default())
        .unwrap();
    assert_eq!(
        base,
        reregistered
            .composition
            .execution_strategy_digest()
            .unwrap(),
        "同包集合重注册必须回到同一执行策略身份"
    );
}
