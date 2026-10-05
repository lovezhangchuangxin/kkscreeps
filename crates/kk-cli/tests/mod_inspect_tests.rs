//! 独立诊断命令检查组合、规则来源、闭包和错误报告；不推进世界。
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kk-mod-inspect-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn run(&self, root: &Path, args: &[&str]) -> (i32, Value) {
        let report = self.0.join("report.json");
        let output = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
            .args(["mod", "inspect", "--root"])
            .arg(root)
            .args(args)
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap();
        let json = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
        (output.status.code().unwrap(), json)
    }
    fn package(&self, id: &str, source: &str) {
        let dir = self.0.join("mods").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mod.json"),
            json!({"id":id,"version":"1.0.0","apiVersion":"^1.0.0"}).to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("server.js"), source).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn slot<'a>(r: &'a Value, key: &str) -> &'a Value {
    r["composition"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == key)
        .unwrap()
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-composition")
}

#[test]
fn inspect_reports_real_graph_rule_sources_and_disable_preview() {
    let f = Fixture::new();
    let (code, plain) = f.run(&root(), &[]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(plain["composition"]["disabled_mods"], json!([]));
    assert_eq!(
        slot(&plain, "base:counter.tick")["active"]["before"],
        json!(["z-gate:handler.before", "a-audit:handler.before"])
    );
    assert_eq!(
        slot(&plain, "base:counter.tick")["active"]["after"],
        json!(["a-audit:handler.after", "z-gate:handler.after"])
    );
    assert_eq!(
        plain["composition"]["disable_closures"]["z-gate"],
        json!(["a-audit", "dependent", "z-gate"])
    );
    let rule = plain["composition"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "builtin:link.capacity")
        .unwrap();
    assert_eq!(rule["selected_source"], "z-gate");
    assert_eq!(rule["value"], json!({"Signed":700}));
    let (code, disabled) = f.run(&root(), &["--disable", "z-gate"]);
    assert_eq!(code, 0, "{disabled}");
    assert_eq!(
        disabled["composition"]["disabled_mods"],
        plain["composition"]["disable_closures"]["z-gate"]
    );
    assert_eq!(
        disabled["composition"]["plan_hash"],
        plain["composition"]["plan_hash"]
    );
    assert_eq!(
        disabled["composition"]["rules"],
        plain["composition"]["rules"]
    );
    assert_eq!(
        slot(&disabled, "base:counter.tick")["active"]["wraps"],
        json!([])
    );
    assert_eq!(plain["executable_sha256"].as_str().unwrap().len(), 64);
    let (code, owner) = f.run(&root(), &["--disable", "base"]);
    assert_eq!(code, 0, "{owner}");
    assert_eq!(slot(&owner, "base:counter.tick")["execution"], "inactive");
    assert!(slot(&owner, "base:counter.tick")["active"].is_null());
}

#[test]
fn inspect_failures_replace_stale_success_and_selected_sources_are_explicit() {
    let f = Fixture::new();
    for id in ["a", "z"] {
        f.package(id,"export default function register(api){api.rules.override('builtin:link.capacity',700);}");
    }
    let root = f.0.join("mods");
    let (code, selected) = f.run(&root, &["--rule-source", "builtin:link.capacity=z"]);
    assert_eq!(code, 0, "{selected}");
    for args in [
        vec![],
        vec![
            "--disable",
            "missing",
            "--rule-source",
            "builtin:link.capacity=z",
        ],
        vec!["--rule-source", "builtin:link.capacity=missing"],
        vec!["--rule-source", "bad"],
        vec![
            "--rule-source",
            "builtin:link.capacity=z",
            "--rule-source",
            "builtin:link.capacity=a",
        ],
    ] {
        let (code, error) = f.run(&root, &args);
        assert_eq!(code, 1);
        assert_eq!(error["status"], "setup_failed");
        assert!(error["composition"].is_null());
    }
    let configs = f.0.join("configs.json");
    std::fs::write(&configs, "{\"missing\":{}}").unwrap();
    let (code, error) = f.run(&root, &["--configs", configs.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(error["error"].as_str().unwrap().contains("未加载"));
    f.package("z","export default function register(api){api.handlers.before('builtin:link.tick',{id:'z',dependsOn:['missing:handler.x']},()=>{});}");
    let (code, error) = f.run(&root, &[]);
    assert_eq!(code, 1);
    assert!(error["error"].as_str().unwrap().contains("未知处理器依赖"));
}

#[test]
fn inspect_cannot_overwrite_package_or_config_inputs() {
    let f = Fixture::new();
    f.package("test", "export default function register(){}");
    let root = f.0.join("mods");
    let config = f.0.join("configs.json");
    std::fs::write(&config, "{}").unwrap();
    for path in [root.join("test/mod.json"), config.clone()] {
        let before = std::fs::read(&path).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
            .args(["mod", "inspect", "--root"])
            .arg(&root)
            .arg("--configs")
            .arg(&config)
            .arg("--report")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
