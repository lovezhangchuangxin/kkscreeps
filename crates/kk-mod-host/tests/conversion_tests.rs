//! 转换可以执行 getter/Proxy，必须在有界子进程中验证重入与异常边界。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use kk_mod_host::{ModRealm, RegisterOptions, discover_packages};
use serde_json::json;

#[test]
fn conversion_boundaries_are_bounded_and_preserve_registration_failure() {
    for case in [
        "register-symbol",
        "register-caught-throw",
        "register-hostile-exception",
        "register-nested-fault",
        "register-nested-valid",
        "register-proxy",
        "module-caught-fault",
        "inactive-gate",
        "sealed-gate",
        "invoke-symbol",
        "invoke-throw",
        "seal-symbol",
        "seal-throw",
        "probe-symbol",
        "probe-throw",
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "conversion_child", "--ignored", "--nocapture"])
            .env("KK_MOD_CONVERSION_CASE", case)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!("{case} 超时：{}", String::from_utf8_lossy(&output.stderr));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{case}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
#[ignore = "只由带外层期限的父测试启动"]
fn conversion_child() {
    let case = std::env::var("KK_MOD_CONVERSION_CASE").unwrap();
    let mut realm = ModRealm::new().unwrap();
    if case == "inactive-gate" || case.starts_with("probe-") {
        check_probe(&mut realm, &case);
        return;
    }
    let root = std::env::temp_dir().join(format!("kk-mod-conversion-{}", std::process::id()));
    let dir = root.join("test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"test","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )
    .unwrap();
    let body = match case.as_str() {
        "register-symbol" => {
            "try{Deno.core.ops.op_mod_declare('rule',{value:Symbol('bad')});}catch(e){}"
        }
        "register-caught-throw" => {
            "try{Deno.core.ops.op_mod_declare('rule',{get value(){throw new Error('bad getter')}});}catch(e){} api.rules.define('test:ok',{type:'i32',consumers:['test:handler.watch'],default:1});"
        }
        "register-hostile-exception" => {
            "try{Deno.core.ops.op_mod_declare('rule',{get value(){throw {toString(){while(true){}},get message(){while(true){}}};}});}catch(e){}"
        }
        "register-nested-fault" => {
            "Deno.core.ops.op_mod_declare('rule',{key:'test:outer',type:'i32',consumers:['test:handler.watch'],get default(){try{Deno.core.ops.op_mod_declare('rule',{value:Symbol('bad')});}catch(e){} return 1;}});"
        }
        "register-nested-valid" => {
            "Deno.core.ops.op_mod_declare('rule',{key:'test:outer',type:'i32',consumers:['test:handler.watch'],get default(){api.rules.define('test:inner',{type:'i32',consumers:['test:handler.watch'],default:2});return 1;}});"
        }
        "register-proxy" => {
            "const value={key:'test:outer',type:'i32',consumers:['test:handler.watch'],default:1};Deno.core.ops.op_mod_declare('rule',new Proxy(value,{ownKeys(t){api.rules.define('test:inner',{type:'i32',consumers:['test:handler.watch'],default:2});return Reflect.ownKeys(t)}}));"
        }
        "module-caught-fault" => "globalThis.registerCalled=true;",
        "sealed-gate" => "",
        "invoke-symbol" => {
            "api.handlers.after('builtin:link.tick',{id:'bad'},ctx=>{ctx.state={bad:Symbol('bad')};});"
        }
        "invoke-throw" => {
            "api.handlers.after('builtin:link.tick',{id:'bad'},ctx=>{ctx.state={get bad(){throw new Error('bad getter')}};});"
        }
        "seal-symbol" => "Array.from=()=>[Symbol('bad')];",
        "seal-throw" => {
            "Array.from=()=>Object.defineProperty([0],'0',{get(){throw new Error('bad getter')}});"
        }
        _ => panic!("未知用例 {case}"),
    };
    let prefix = if case == "module-caught-fault" {
        "try{Deno.core.ops.op_mod_declare('rule',{value:Symbol('bad')});}catch(e){}"
    } else {
        ""
    };
    std::fs::write(dir.join("server.js"), format!("{prefix} export default function register(api){{api.handlers.after('builtin:link.tick',{{id:'watch'}},()=>{{}});{body}}}")).unwrap();
    let packages = discover_packages(&root).unwrap();
    let result = realm.register(&packages, RegisterOptions::default());
    if case.starts_with("register-")
        && !matches!(case.as_str(), "register-nested-valid" | "register-proxy")
        || case.starts_with("seal-")
        || case == "module-caught-fault"
    {
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("转换失败") || error.contains("序列化失败"),
            "{case}: {error}"
        );
        assert!(
            realm
                .register(&packages, RegisterOptions::default())
                .unwrap_err()
                .to_string()
                .contains("密封")
        );
        if case == "module-caught-fault" {
            assert_eq!(
                realm
                    .evaluate_probe("globalThis.registerCalled === true")
                    .unwrap(),
                json!(false)
            );
        }
    } else {
        let registered = result.unwrap();
        if case.starts_with("register-") {
            assert_eq!(registered.contributions[0].rules.len(), 2);
        } else if case == "sealed-gate" {
            check_probe(&mut realm, &case);
        } else {
            let error = realm
                .invoke("test:handler.bad", json!({"state":null}))
                .unwrap_err()
                .to_string();
            assert!(error.contains("序列化失败"), "{case}: {error}");
            assert_eq!(realm.evaluate_probe("42").unwrap(), json!(42));
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn check_probe(realm: &mut ModRealm, case: &str) {
    if case.ends_with("gate") {
        let result = realm.evaluate_probe("(()=>{let touched=0;try{Deno.core.ops.op_mod_declare('rule',new Proxy({get value(){touched++;return Symbol('bad')}},{ownKeys(t){touched++;return Reflect.ownKeys(t)}}));}catch(e){} return touched;})()").unwrap();
        assert_eq!(result, json!(0), "关闭的注册能力不得访问载荷");
    } else {
        let code = if case == "probe-symbol" {
            "({bad:Symbol('bad')})"
        } else {
            "({get bad(){throw new Error('bad getter')}})"
        };
        let error = realm.evaluate_probe(code).unwrap_err().to_string();
        assert!(error.contains("序列化失败"), "{case}: {error}");
        assert_eq!(realm.evaluate_probe("42").unwrap(), json!(42));
    }
}
