//! 作者后台 CLI：公共包、真实世界冷读取和输入输出守卫。
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kk-services-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-services")
}
fn command(report: &std::path::Path) -> Command {
    let root = root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["mod", "services", "--root"])
        .arg(&root)
        .arg("--requests")
        .arg(root.join("requests.json"))
        .arg("--probe")
        .arg(format!(
            "audit:sample={}",
            root.join("sample.txt").display()
        ))
        .arg("--report")
        .arg(report);
    command
}
#[test]
fn services_cli_public_package_runs_async_probe_start_requests_and_stop() {
    let f = Fixture::new();
    let report = f.0.join("report.json");
    let output = command(&report).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(report).unwrap()).unwrap();
    assert_eq!(report["status"], "passed");
    assert_eq!(report["world_unchanged"], true);
    assert_eq!(report["start"]["audit:summary"]["sample"], "local probe\n");
    assert_eq!(report["requests"][0]["result"]["calls"], 1);
    assert_eq!(report["requests"][1]["result"]["calls"], 2);
    assert_eq!(report["stop"]["audit:summary"]["calls"], 2);
}
#[test]
fn services_cli_reads_existing_checkpoint_without_modifying_its_world_or_input_file()
-> anyhow::Result<()> {
    let f = Fixture::new();
    let checkpoint = f.0.join("checkpoint.json");
    let packages = kk_mod_host::discover_packages(&root())?;
    let options = kk_mod_host::RegisterOptions::default();
    let mut e = kk_runtime::EmbedEngine::new();
    e.create_shard_with_mods(
        &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":2}]}),
        &packages,
        options,
    )?;
    e.run_player("p1", "Memory.calls=1")?;
    e.save_checkpoint(&checkpoint, Value::Null)?;
    let original = std::fs::read(&checkpoint)?;
    let report = f.0.join("report.json");
    let output = command(&report)
        .arg("--checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert_eq!(report["world_unchanged"], true);
    assert_eq!(
        report["requests"][0]["result"]["tick"],
        e.world_snapshot()["time"]
    );
    assert_eq!(std::fs::read(checkpoint)?, original);
    Ok(())
}
#[test]
fn services_cli_output_cannot_overwrite_probe_request_or_package_files() {
    let f = Fixture::new();
    let probe = f.0.join("probe.json");
    std::fs::write(&probe, "must remain").unwrap();
    let root = root();
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    let output = command
        .args(["mod", "services", "--root"])
        .arg(&root)
        .arg("--requests")
        .arg(root.join("requests.json"))
        .arg("--probe")
        .arg(format!("audit:sample={}", probe.display()))
        .arg("--report")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(std::fs::read_to_string(&probe).unwrap(), "must remain");
    for report in [root.join("requests.json"), root.join("audit/report.json")] {
        let output = command_for(&root, &report).output().unwrap();
        assert!(!output.status.success());
    }
}
fn command_for(root: &std::path::Path, report: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["mod", "services", "--root"])
        .arg(root)
        .arg("--requests")
        .arg(root.join("requests.json"))
        .arg("--report")
        .arg(report);
    command
}
