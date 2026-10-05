//! 有效规则读取：处理器经公共 SDK 读取冻结后的实际值，而不是注册闭包
//! 里缓存的声明默认值；注册期读取与未知键都有明确失败。

use super::*;
use std::collections::BTreeSet;

/// gate 包声明两条自有规则并在处理器内读取；tuner 包覆盖它们与一条
/// 内建规则。返回 gate 读取处理器的 server.js 源。
const GATE_SOURCE: &str = r#"
export default function register(api) {
  api.rules.define('gate:threshold', {
    type: 'i32', min: 1, max: 100, default: 3, consumers: ['gate:handler.read'],
  });
  api.rules.define('gate:rate', {
    type: 'f64', min: 0, max: 1, default: 0.25, consumers: ['gate:handler.read'],
  });
  api.handlers.after('builtin:link.tick', { id: 'read' }, ctx => {
    ctx.state = {
      threshold: api.rules.effective('gate:threshold'),
      rate: api.rules.effective('gate:rate'),
      capacity: api.rules.effective('builtin:link.capacity'),
    };
  });
}
"#;

fn gate_root(label: &str, tuner_body: &str) -> PathBuf {
    let root = temp_root(label);
    write_pkg(&root, "gate", &[("server.js", GATE_SOURCE)]);
    write_pkg(&root, "tuner", &[("server.js", tuner_body)]);
    root
}

fn read_state(realm: &mut ModRealm) -> Value {
    realm
        .invoke(
            "gate:handler.read",
            json!({"tick": 1, "room": "W1N1", "state": null}),
        )
        .expect("读取处理器可调用")["state"]
        .clone()
}

#[test]
fn handlers_read_frozen_effective_values_not_registration_defaults() {
    let root = gate_root(
        "rules-read",
        r#"export default function register(api){
api.rules.override('gate:threshold', 5);
api.rules.override('builtin:link.capacity', 700);
}"#,
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    realm
        .register(&packages, RegisterOptions::default())
        .unwrap();
    let state = read_state(&mut realm);
    // 单方覆盖自动选中：读到 tuner 的 5 与 700，而不是声明的默认 3/800。
    assert_eq!(state["threshold"], json!(5));
    assert_eq!(state["capacity"], json!(700));
    // 无覆盖的浮点规则返回声明默认值本身。
    assert_eq!(state["rate"], json!(0.25));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_multi_override_selection_reaches_js_reads() {
    let root = temp_root("rules-selection");
    write_pkg(&root, "gate", &[("server.js", GATE_SOURCE)]);
    write_pkg(
        &root,
        "left",
        &[(
            "server.js",
            "export default function register(api){api.rules.override('gate:threshold', 5);}",
        )],
    );
    write_pkg(
        &root,
        "right",
        &[(
            "server.js",
            "export default function register(api){api.rules.override('gate:threshold', 9);}",
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut options = RegisterOptions::default();
    options
        .freeze
        .rule_sources
        .insert(key("gate:threshold"), "right".into());
    let mut realm = ModRealm::new().unwrap();
    realm.register(&packages, options).unwrap();
    let state = read_state(&mut realm);
    // 多方覆盖按冻结选项显式选择；值相等也不能反推来源，这里值不同可核验。
    assert_eq!(state["threshold"], json!(9));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn registration_window_and_unknown_keys_are_rejected() {
    // 注册期（register 回调内）读取：有效值尚未冻结，必须失败且不能
    // 退化为返回声明默认值。
    let root = temp_root("rules-window");
    write_pkg(
        &root,
        "early",
        &[(
            "server.js",
            r#"export default function register(api){
api.rules.define('early:value', { type: 'u32', min: 0, max: 9, default: 4, consumers: [] });
api.rules.effective('early:value');
}"#,
        )],
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let error = realm
        .register(&packages, RegisterOptions::default())
        .unwrap_err();
    assert!(
        error.to_string().contains("注册与冻结完成前不可读取"),
        "实得 {error}"
    );
    let _ = std::fs::remove_dir_all(root);

    // 注册代码直接调用 installRules 抢装或伪造载荷：注册失败并密封 realm，
    // 宿主安装必须仍以冻结计划为唯一来源。
    let root = temp_root("rules-install-guard");
    for body in [
        "import { installRules } from '@kkscreeps/mod'; export default function register(){ installRules([{key:'fake:rule', value:1}]); }",
        "import { installRules } from '@kkscreeps/mod'; export default function register(){ installRules('bad'); }",
    ] {
        write_pkg(&root, "hijack", &[("server.js", body)]);
        let packages = discover_packages(&root).unwrap();
        let mut realm = ModRealm::new().unwrap();
        let error = realm
            .register(&packages, RegisterOptions::default())
            .unwrap_err();
        assert!(
            error.to_string().contains("只能在密封后安装一次"),
            "实得 {error}"
        );
    }
    let _ = std::fs::remove_dir_all(root);

    // 执行期未知键与畸形键：处理器异常沿既有故障路径传播。
    let root = temp_root("rules-unknown");
    for (expression, expected) in [
        ("api.rules.effective('gate:absent')", "未知规则键"),
        ("api.rules.effective('no-colon')", "形状无效"),
    ] {
        write_pkg(
            &root,
            "gate",
            &[(
                "server.js",
                &format!(
                    r#"export default function register(api){{
api.handlers.after('builtin:link.tick', {{ id: 'read' }}, () => {{ {expression}; }});
}}"#
                ),
            )],
        );
        let packages = discover_packages(&root).unwrap();
        let mut realm = ModRealm::new().unwrap();
        realm
            .register(&packages, RegisterOptions::default())
            .unwrap();
        let error = realm
            .invoke(
                "gate:handler.read",
                json!({"tick": 1, "room": "W1N1", "state": null}),
            )
            .unwrap_err();
        assert!(error.to_string().contains(expected), "实得 {error}");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn disabled_override_keeps_serving_the_frozen_selection() {
    let root = gate_root(
        "rules-disabled",
        r#"export default function register(api){
api.rules.override('gate:threshold', 5);
}"#,
    );
    let packages = discover_packages(&root).unwrap();
    let mut realm = ModRealm::new().unwrap();
    let registered = realm
        .register_with_disabled(
            &packages,
            RegisterOptions::default(),
            &BTreeSet::from(["tuner".into()]),
        )
        .unwrap();
    // 停用只屏蔽结算执行：规则目录与已冻结选择保留，读取继续返回
    // 原覆盖值，而不是回退默认 3。
    let state = read_state(&mut realm);
    assert_eq!(state["threshold"], json!(5));
    assert_eq!(state["rate"], json!(0.25));
    let rule = registered
        .composition
        .rules
        .iter()
        .find(|r| r.key == key("gate:threshold"))
        .unwrap();
    assert_eq!(rule.value, Number::Signed(5));
    assert_eq!(rule.selected_source, "tuner");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn caught_failed_installation_cannot_add_rules_outside_the_frozen_plan() {
    let root = temp_root("rules-install-atomic");
    for payload in [
        "[{key:'gate:ghost',value:77},null]",
        "[{key:'gate:ghost',value:77},{key:'gate:ghost',value:88}]",
        "[{key:'gate:ghost',value:77},{get key(){throw Error('getter');},value:1}]",
        "[{key:'gate:ghost',value:77},{key:'bad-key',value:1}]",
    ] {
        let source = format!(
            r#"import {{ seal, installRules }} from '@kkscreeps/mod';
export default function register(api) {{
  api.handlers.after('builtin:link.tick', {{id:'read'}}, ctx => {{
    ctx.state = {{capacity:api.rules.effective('builtin:link.capacity')}};
  }});
  api.handlers.after('builtin:link.tick', {{id:'ghost'}}, ctx => {{
    ctx.state = {{value:api.rules.effective('gate:ghost')}};
  }});
  seal();
  try {{ installRules({payload}); }} catch (error) {{}}
}}"#
        );
        write_pkg(&root, "gate", &[("server.js", &source)]);
        let packages = discover_packages(&root).unwrap();
        let mut realm = ModRealm::new().unwrap();
        let registered = realm
            .register(&packages, RegisterOptions::default())
            .unwrap();
        assert!(
            registered
                .plan
                .rules()
                .resolve(&key("gate:ghost"))
                .is_none()
        );
        assert_eq!(read_state(&mut realm)["capacity"], 800);
        let error = realm
            .invoke("gate:handler.ghost", json!({"state":null}))
            .unwrap_err();
        assert!(
            error.to_string().contains("未知规则键"),
            "{payload}: {error}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}
