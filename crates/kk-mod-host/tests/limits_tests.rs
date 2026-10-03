//! 真实死循环与分配探针放在子进程，外层期限防止错误实现挂死整个测试任务。
use kk_mod_host::{
    ChainContext, ChainHost, ChainTarget, ModRealm, ModVmLimits, RegisterOptions, discover_packages,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Host;
struct SleepingHost(Duration);
impl ChainHost for SleepingHost {
    fn read(&mut self, binding: &str) -> Result<ChainContext, String> {
        Host.read(binding)
    }
    fn write(&mut self, binding: &str, value: Value) -> Result<(), String> {
        Host.write(binding, value)
    }
    fn native(&mut self) -> Result<(), String> {
        std::thread::sleep(self.0);
        Ok(())
    }
}
impl ChainHost for Host {
    fn read(&mut self, _: &str) -> Result<ChainContext, String> {
        Ok(ChainContext {
            args: Value::Null,
            room: "W1N1".into(),
            state: Value::Null,
            target: ChainTarget {
                fields: json!({"cooldown":0}),
                id: "test".into(),
            },
            tick: 0,
        })
    }
    fn write(&mut self, _: &str, _: Value) -> Result<(), String> {
        Ok(())
    }
    fn native(&mut self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn resource_fault_matrix() -> Result<(), Box<dyn std::error::Error>> {
    for case in [
        "module-cpu",
        "register-cpu",
        "module-wall",
        "module-wall-prepare",
        "module-wall-name",
        "module-cpu-prepare",
        "module-cpu-name",
        "before-cpu",
        "wrap-cpu",
        "after-cpu",
        "center-cpu",
        "array-heap",
        "external-memory",
        "cumulative",
        "microtasks",
    ] {
        let mut child = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "resource_fault_child",
                "--ignored",
                "--nocapture",
            ])
            .env("KK_MOD_LIMIT_CASE", case)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        let start = Instant::now();
        while child.try_wait()?.is_none() {
            if start.elapsed() > Duration::from_secs(10) {
                child.kill()?;
                let output = child.wait_with_output()?;
                panic!(
                    "{case} 未在外层期限内结束：{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output()?;
        assert!(
            output.status.success(),
            "{case}: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[test]
#[ignore = "只由带外层期限的父测试启动"]
fn resource_fault_child() -> Result<(), Box<dyn std::error::Error>> {
    let case = std::env::var("KK_MOD_LIMIT_CASE")?;
    let root = Root(std::env::temp_dir().join(format!("kk-mod-limits-{}", std::process::id())));
    let dir = root.0.join("test");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"test","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )?;
    let source = match case.as_str() {
        "module-cpu" | "module-wall" => "while(true){} export default function register(){}".into(),
        "module-wall-prepare" | "module-cpu-prepare" => "Error.prepareStackTrace=()=>{while(true){}};while(true){} export default function register(){}".into(),
        "module-wall-name" | "module-cpu-name" => "Object.defineProperty(Error.prototype,'name',{get(){while(true){}}});while(true){} export default function register(){}".into(),
        "register-cpu" => "export default function register(){while(true){}}".into(),
        "microtasks" => "await new Promise(()=>{const spin=()=>Promise.resolve().then(spin);spin()}); export default function register(){}".into(),
        "array-heap" => "export default function register(api){api.handlers.after('builtin:link.tick',{id:'bad'},()=>{globalThis.a=[];while(true){a.push(Array(8192).fill(Math.random()));}})}".into(),
        "external-memory" => "export default function register(api){api.handlers.after('builtin:link.tick',{id:'bad'},()=>{globalThis.a=new ArrayBuffer(48*1024*1024);})}".into(),
        "cumulative" => "export default function register(api){api.handlers.after('builtin:link.tick',{id:'bad'},()=>{let n=0;for(let i=0;i<2000;i++)n+=Math.sqrt(i);globalThis.n=n;})}".into(),
        other => {
            let kind = match other { "before-cpu" => "before", "wrap-cpu" => "wrap", "after-cpu" => "after", _ => "replace" };
            format!("export default function register(api){{api.handlers.{kind}('builtin:link.tick',{{id:'bad'}},()=>{{while(true){{}}}})}}")
        }
    };
    std::fs::write(dir.join("server.js"), source)?;
    let packages = discover_packages(&root.0)?;
    let mut limits = ModVmLimits::default();
    if case.starts_with("module-cpu") || case == "register-cpu" {
        limits.registration.cpu = Duration::from_millis(10);
    }
    limits.settlement.cpu = Duration::from_millis(10);
    limits.settlement.wall = Duration::from_secs(2);
    if case.starts_with("module-wall") || case == "microtasks" {
        limits.registration.cpu = Duration::from_secs(2);
        limits.registration.wall = Duration::from_millis(30);
    }
    if case.contains("heap") || case.contains("memory") {
        limits.memory_bytes = 16 * 1024 * 1024;
        limits.settlement.cpu = Duration::from_secs(2);
    }
    let mut realm = ModRealm::with_limits(limits.clone())?;
    let options = RegisterOptions {
        vm_limits: limits,
        ..Default::default()
    };
    let start = Instant::now();
    let error = if case.starts_with("module-") || case == "register-cpu" || case == "microtasks" {
        realm.register(&packages, options).unwrap_err().to_string()
    } else {
        // 正常注册用宽限额，保持本例只测试对应结算路径。
        realm.register(&packages, options)?;
        realm.begin_settlement()?;
        if case == "cumulative" {
            let mut error = None;
            let mut successes = 0;
            for _ in 0..10000 {
                match realm.invoke_chain("builtin:link.tick", Box::new(Host)) {
                    Ok(()) => successes += 1,
                    Err(e) => {
                        error = Some(e.to_string());
                        break;
                    }
                }
            }
            assert!(successes > 1, "应累计多个单次未超限调用");
            error.expect("整轮累计 CPU 应超限")
        } else {
            realm
                .invoke_chain("builtin:link.tick", Box::new(Host))
                .unwrap_err()
                .to_string()
        }
    };
    let expected = if case.contains("heap") || case.contains("memory") {
        "内存预算"
    } else if case.contains("wall") || case == "microtasks" {
        "墙钟预算"
    } else {
        "CPU 预算"
    };
    assert!(error.contains(expected), "{case}: {error}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "资源故障应有界返回"
    );
    assert!(
        realm
            .invoke("test:handler.bad", Value::Null)
            .unwrap_err()
            .to_string()
            .contains("毒化")
    );
    Ok(())
}

#[test]
fn invalid_limits_are_rejected_before_vm_creation() {
    let limits = ModVmLimits {
        memory_bytes: 1,
        ..Default::default()
    };
    assert!(ModRealm::with_limits(limits).is_err());
    let options = RegisterOptions::default();
    let packages = BTreeMap::new();
    let mut realm = ModRealm::new().unwrap();
    let mut mismatched = options.clone();
    mismatched.vm_limits.memory_bytes *= 2;
    assert!(realm.register(&packages, mismatched).is_err());
    assert!(
        realm
            .register(&packages, options)
            .unwrap_err()
            .to_string()
            .contains("密封")
    );
}

#[test]
fn native_wait_is_not_cpu_and_queued_checks_use_current_call_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let root = Root(std::env::temp_dir().join(format!("kk-mod-limit-wait-{}", std::process::id())));
    let dir = root.0.join("test");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        r#"{"id":"test","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )?;
    std::fs::write(
        dir.join("server.js"),
        "export default function register(api){api.handlers.wrap('builtin:link.tick',{id:'pass'},(ctx,next)=>next());}",
    )?;
    let packages = discover_packages(&root.0)?;
    let mut limits = ModVmLimits::default();
    limits.settlement.cpu = Duration::from_millis(10);
    limits.settlement.wall = Duration::from_secs(1);
    let mut realm = ModRealm::with_limits(limits.clone())?;
    realm.register(
        &packages,
        RegisterOptions {
            vm_limits: limits,
            ..Default::default()
        },
    )?;
    // Rust op 暂停时中断请求排队；返回后及下一次调用只能按当时生效的限额判断。
    for _ in 0..3 {
        realm.invoke_chain(
            "builtin:link.tick",
            Box::new(SleepingHost(Duration::from_millis(50))),
        )?;
        realm.invoke_chain("builtin:link.tick", Box::new(Host))?;
    }
    Ok(())
}
