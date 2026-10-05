//! `mod init` 模板闭环：生成的包经真实注册、场景断言与检查点续跑。
use std::path::{Path, PathBuf};
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
        "kkscreeps-mod-init-{}-{tag}-{}",
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

#[test]
fn init_template_runs_asserts_and_resumes_across_checkpoint() {
    let ws = workspace("full");
    let root = ws.0.join("mods");
    // 1) 生成模板；重复生成拒绝且不覆盖。
    let out = cli(&[
        "mod",
        "init",
        "--root",
        root.to_str().unwrap(),
        "--id",
        "starter",
    ]);
    assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    let package = root.join("starter");
    for name in [
        "mod.json",
        "server.js",
        "player.js",
        "main.js",
        "scenario.json",
        "README.md",
    ] {
        assert!(package.join(name).is_file(), "缺少 {name}");
    }
    let dup = cli(&[
        "mod",
        "init",
        "--root",
        root.to_str().unwrap(),
        "--id",
        "starter",
    ]);
    assert_eq!(code_of(&dup), 1, "重复 init 须拒绝");
    assert!(String::from_utf8_lossy(&dup.stderr).contains("不覆盖"));

    // 2) 组合检查通过（报告须放在包根之外）。
    let inspect_report = ws.0.join("inspect.json");
    let out = cli(&[
        "mod",
        "inspect",
        "--root",
        root.to_str().unwrap(),
        "--report",
        inspect_report.to_str().unwrap(),
    ]);
    assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));

    // 3) 直接跑满 3 轮：对象计数值、玩家读取与 namespace state 断言通过。
    let scene = package.join("scenario.json");
    let run = |ticks: &str, report: &Path, option: Option<(&str, &Path)>| -> i32 {
        let mut args = vec![
            "sim".to_string(),
            "run".to_string(),
            "--scenario".to_string(),
            scene.to_string_lossy().into_owned(),
            "--ticks".to_string(),
            ticks.to_string(),
            "--report".to_string(),
            report.to_string_lossy().into_owned(),
        ];
        if let Some((flag, value)) = option {
            args.push(flag.to_string());
            args.push(value.to_string_lossy().into_owned());
        }
        cli(&args.iter().map(String::as_str).collect::<Vec<_>>())
            .status
            .code()
            .expect("子进程有退出码")
    };
    assert_eq!(
        run("3", &ws.0.join("full.json"), None),
        0,
        "模板场景断言应直接通过"
    );

    // 4) 保存恢复：1 轮保存（断言未满足，退出码 2 是预期），加载续跑 2 轮。
    let checkpoint = ws.0.join("cp.json");
    assert_eq!(
        run(
            "1",
            &ws.0.join("first.json"),
            Some(("--save-checkpoint", &checkpoint))
        ),
        2,
        "未满足断言时保存退出码为 2"
    );
    assert_eq!(
        run(
            "2",
            &ws.0.join("resumed.json"),
            Some(("--load-checkpoint", &checkpoint))
        ),
        0,
        "加载续跑后断言通过：计数跨检查点恰一次延续"
    );
    let full: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.0.join("full.json")).unwrap()).unwrap();
    let resumed: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ws.0.join("resumed.json")).unwrap()).unwrap();
    // 先断言具体值，避免报告键缺失时 Null==Null 退化为恒真。
    assert_eq!(full["final"]["objects"]["ctr"]["fields"]["value"], 3);
    assert_eq!(
        resumed["final"]["objects"]["ctr"]["fields"]["value"], 3,
        "加载续跑后计数恰一次延续"
    );
    assert_eq!(
        full["final"]["objects"]["ctr"],
        resumed["final"]["objects"]["ctr"]
    );
    assert!(full["plan_hash"].as_str().is_some_and(|v| !v.is_empty()));
    assert_eq!(full["plan_hash"], resumed["plan_hash"]);
}

#[test]
fn init_templates_under_one_root_do_not_collide_on_player_globals() {
    // 同一 Mod 根下两个模板包：玩家类与 FIND 常量从包 ID 派生，
    // 组合注册不冲突、两个场景都能独立通过。
    let ws = workspace("pair");
    let root = ws.0.join("mods");
    for id in ["alpha-pkg", "beta_pkg"] {
        let out = cli(&["mod", "init", "--root", root.to_str().unwrap(), "--id", id]);
        assert_eq!(code_of(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    }
    for id in ["alpha-pkg", "beta_pkg"] {
        let report = ws.0.join(format!("{id}.json"));
        let out = cli(&[
            "sim",
            "run",
            "--scenario",
            root.join(id).join("scenario.json").to_str().unwrap(),
            "--ticks",
            "3",
            "--report",
            report.to_str().unwrap(),
        ]);
        assert_eq!(
            code_of(&out),
            0,
            "{id}：{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn init_rejects_invalid_ids_and_existing_targets() {
    let ws = workspace("guards");
    // 非法命名空间（大写/前导数字/下划线/冒号/空格）与路径分隔符被拒
    // 绝，且不落任何文件。
    for id in ["Starter", "a/b", "..", "a b", "1st", "_lead", "a:b"] {
        let out = cli(&["mod", "init", "--root", ws.0.to_str().unwrap(), "--id", id]);
        assert_ne!(code_of(&out), 0, "ID {id:?} 须被拒绝");
    }
    for id in ["Starter", "1st", "_lead"] {
        assert!(!ws.0.join(id).exists(), "{id} 不应留下目录");
    }
    // 根目录内已有同名目录时拒绝。
    std::fs::create_dir_all(ws.0.join("taken")).unwrap();
    let out = cli(&[
        "mod",
        "init",
        "--root",
        ws.0.to_str().unwrap(),
        "--id",
        "taken",
    ]);
    assert_eq!(code_of(&out), 1);
}
