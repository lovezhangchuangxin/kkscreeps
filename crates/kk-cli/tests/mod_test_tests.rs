//! `mod test` 汇总：模板包一键断言、失败传播与无场景包跳过。
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace(tag: &str) -> Workspace {
    let path = std::env::temp_dir().join(format!(
        "kkscreeps-mod-test-cli-{}-{tag}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    Workspace(path)
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .args(args)
        .output()
        .expect("子进程正常退出")
}

fn code_of(output: &std::process::Output) -> i32 {
    output.status.code().expect("子进程有退出码")
}

/// 无场景的占位包：只验证 discover 通过，不参与断言。
fn sceneless_package(root: &std::path::Path, id: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("mod.json"),
        format!(r#"{{"id":"{id}","version":"1.0.0","apiVersion":"^1.0.0","entry":"server.js"}}"#),
    )
    .unwrap();
    std::fs::write(
        dir.join("server.js"),
        "export default function register(api) {}",
    )
    .unwrap();
}

#[test]
fn mod_test_passes_templates_skips_sceneless_and_propagates_failures() -> anyhow::Result<()> {
    let ws = workspace("full");
    let root = ws.0.join("mods");
    for id in ["alpha", "beta"] {
        let out = cli(&["mod", "init", "--root", root.to_str().unwrap(), "--id", id]);
        assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    }
    sceneless_package(&root, "plain");

    // 全过：两个模板断言通过、无场景包跳过，退出码 0。
    let report = ws.0.join("summary-ok.json");
    let out = cli(&[
        "mod",
        "test",
        "--root",
        root.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    let outcomes: Vec<(String, String)> = summary["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["id"].as_str().unwrap().to_string(),
                r["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        vec![
            ("alpha".into(), "passed".into()),
            ("beta".into(), "passed".into()),
            ("plain".into(), "skipped".into()),
        ]
    );
    assert_eq!(summary["tool"], "kkscreeps mod test");
    assert_eq!(summary["kkpv"], false);
    assert!(
        summary["summary"]
            .as_str()
            .is_some_and(|t| t.contains("2 通过"))
    );
    assert_eq!(summary["results"][0]["detail"]["status"], "passed");
    assert!(
        summary["results"][0]["detail"]["plan_hash"]
            .as_str()
            .is_some_and(|h| !h.is_empty())
    );

    // 篡改一个场景断言：该包 failed、退出码 2、汇总含机器细节。
    let scenario = root.join("alpha").join("scenario.json");
    let mut scene: serde_json::Value = serde_json::from_slice(&std::fs::read(&scenario)?)?;
    scene["assertions"][0]["value"] = serde_json::json!(99);
    std::fs::write(&scenario, serde_json::to_vec_pretty(&scene)?)?;
    let failed_report = ws.0.join("summary-failed.json");
    let out = cli(&[
        "mod",
        "test",
        "--root",
        root.to_str().unwrap(),
        "--report",
        failed_report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 2, "断言失败须传播 sim 的退出码 2");
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&failed_report)?)?;
    let alpha = summary["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "alpha")
        .unwrap();
    assert_eq!(alpha["outcome"], "failed");
    assert_eq!(alpha["exit_code"], 2);
    assert_eq!(alpha["detail"]["status"], "assertion_failed");
    Ok(())
}

#[test]
fn mod_test_locates_scenarios_by_package_directory_not_id() -> anyhow::Result<()> {
    // 目录改名后 manifest id 不变：mod test 按包真实目录定位场景，
    // 不得把改名包静默记为 skipped（假阴性）。
    let ws = workspace("renamed");
    let root = ws.0.join("mods");
    let out = cli(&[
        "mod",
        "init",
        "--root",
        root.to_str().unwrap(),
        "--id",
        "gamma",
    ]);
    assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    std::fs::rename(root.join("gamma"), root.join("gamma-renamed"))?;
    let report = ws.0.join("summary.json");
    let out = cli(&[
        "mod",
        "test",
        "--root",
        root.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(summary["results"][0]["id"], "gamma");
    assert_eq!(summary["results"][0]["outcome"], "passed", "改名包仍被测试");
    assert!(
        summary["results"][0]["scenario"]
            .as_str()
            .is_some_and(|path| path.ends_with("gamma-renamed/scenario.json"))
    );
    Ok(())
}

#[test]
fn mod_test_reports_setup_failed_for_broken_scenario() -> anyhow::Result<()> {
    let ws = workspace("broken");
    let root = ws.0.join("mods");
    let out = cli(&[
        "mod",
        "init",
        "--root",
        root.to_str().unwrap(),
        "--id",
        "delta",
    ]);
    assert_eq!(code_of(&out), 0);
    let scenario = root.join("delta").join("scenario.json");
    std::fs::write(&scenario, "{ not valid json")?;
    let report = ws.0.join("summary.json");
    let out = cli(&[
        "mod",
        "test",
        "--root",
        root.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 1, "装载失败传播退出码 1");
    let summary: serde_json::Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    let delta = &summary["results"][0];
    assert_eq!(delta["outcome"], "failed");
    assert_eq!(delta["exit_code"], 1);
    assert_eq!(delta["detail"]["status"], "setup_failed");
    Ok(())
}

#[test]
fn mod_test_rejects_empty_and_missing_roots() -> anyhow::Result<()> {
    let ws = workspace("guards");
    let report = ws.0.join("summary.json");
    // 根不存在。
    let out = cli(&[
        "mod",
        "test",
        "--root",
        ws.0.join("missing").to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 1);
    // 根存在但无包。
    std::fs::create_dir_all(ws.0.join("empty"))?;
    let out = cli(&[
        "mod",
        "test",
        "--root",
        ws.0.join("empty").to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 1);
    Ok(())
}
