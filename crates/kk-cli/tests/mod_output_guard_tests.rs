//! 输出拒绝须发生在注册和落盘之前：输入字节与目录结构保持完整。
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kk-output-guard-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> PathBuf {
        self.0.join("mods")
    }
    fn init(&self, id: &str) {
        let output = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
            .args(["mod", "init", "--id", id, "--root"])
            .arg(self.root())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fn run(&self, command: &str, report: &Path, extra: &[&Path]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
        cmd.args(["mod", command, "--root"])
            .arg(self.root())
            .arg("--report")
            .arg(report);
        if command == "types" {
            cmd.arg("--out").arg(
                extra
                    .first()
                    .copied()
                    .unwrap_or(&self.0.join("generated.d.ts")),
            );
        }
        if let Some(config) = extra.get(1) {
            cmd.arg("--configs").arg(config);
        }
        cmd.output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reports_never_overwrite_package_inputs_or_create_rejected_directories() {
    let f = Fixture::new();
    f.init("alpha");
    for command in ["test", "types", "inspect"] {
        for name in ["mod.json", "scenario.json"] {
            let input = f.root().join("alpha").join(name);
            let before = std::fs::read(&input).unwrap();
            let output = f.run(command, &input, &[]);
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(std::fs::read(&input).unwrap(), before);
        }
        let report = f.root().join("alpha/new/sub/report.json");
        assert_eq!(f.run(command, &report, &[]).status.code(), Some(1));
        assert!(!f.root().join("alpha/new").exists());
    }
    assert!(!f.0.join("generated.d.ts").exists());
}

#[test]
fn reports_cannot_overwrite_config_even_during_setup_failure() {
    let f = Fixture::new();
    f.init("alpha");
    let config = f.0.join("configs.json");
    let out = f.0.join("generated.d.ts");
    std::fs::write(&config, "{ broken config").unwrap();
    for command in ["types", "inspect"] {
        assert_eq!(
            f.run(command, &config, &[&out, &config]).status.code(),
            Some(1)
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "{ broken config");
        assert!(!out.exists());
    }
}

#[cfg(unix)]
#[test]
fn symlink_ancestors_and_aliases_cannot_bypass_input_guards() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.init("alpha");
    let alias = f.0.join("alias");
    symlink(f.root().join("alpha"), &alias).unwrap();
    for command in ["test", "inspect", "types"] {
        let report = alias.join("new/sub/report.json");
        assert_eq!(f.run(command, &report, &[]).status.code(), Some(1));
        assert!(!alias.join("new").exists());
    }
    let out = alias.join("new/sub/types.d.ts");
    assert_eq!(
        f.run("types", &f.0.join("report.json"), &[&out])
            .status
            .code(),
        Some(1)
    );
    assert!(!alias.join("new").exists());
    let config = f.0.join("configs.json");
    std::fs::write(&config, "{}").unwrap();
    let config_alias = f.0.join("alias.json");
    symlink(&config, &config_alias).unwrap();
    assert_eq!(
        f.run("types", &config_alias, &[&f.0.join("types.d.ts"), &config])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
    let hardlink = f.0.join("configs.d.ts");
    std::fs::hard_link(&config, &hardlink).unwrap();
    assert_eq!(
        f.run("types", &f.0.join("report.json"), &[&hardlink, &config])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");
}

#[cfg(unix)]
#[test]
fn declaration_replaces_external_hardlink_without_mutating_package() {
    let f = Fixture::new();
    f.init("alpha");
    let input = f.root().join("alpha/server.js");
    let before = std::fs::read(&input).unwrap();
    let out = f.0.join("types.d.ts");
    std::fs::hard_link(&input, &out).unwrap();
    assert!(
        f.run("types", &f.0.join("report.json"), &[&out])
            .status
            .success()
    );
    assert_eq!(std::fs::read(&input).unwrap(), before);
    assert!(
        std::fs::read_to_string(out)
            .unwrap()
            .contains("export interface AlphaCounter")
    );
}

#[test]
fn escaped_template_ids_compose_and_reserved_builtin_is_rejected() {
    let f = Fixture::new();
    for id in [
        "foo-bar", "foo_bar", "foo--bar", "foo__bar", "foo-hbar", "foo_hbar",
    ] {
        f.init(id);
    }
    let out = f.0.join("types.d.ts");
    let result = f.run("types", &f.0.join("report.json"), &[&out]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = f.run("test", &f.0.join("tests.json"), &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .args(["mod", "init", "--id", "builtin", "--root"])
        .arg(f.root())
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(!f.root().join("builtin").exists());
}

#[test]
fn sim_failure_reports_cannot_overwrite_mod_inputs() {
    let f = Fixture::new();
    f.init("alpha");
    let package = f.root().join("alpha");
    let scenario = package.join("scenario.json");
    let manifest = package.join("mod.json");
    // 损坏 manifest 后运行仍不能以失败报告覆盖输入。
    std::fs::write(&manifest, "broken manifest").unwrap();
    for report in [manifest.clone(), package.join("new/sub/report.json")] {
        let result = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
            .args(["sim", "run", "--scenario"])
            .arg(&scenario)
            .arg("--report")
            .arg(report)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(
            std::fs::read_to_string(&manifest).unwrap(),
            "broken manifest"
        );
        assert!(!package.join("new").exists());
    }
}

#[cfg(unix)]
#[test]
fn package_symlink_pointing_outside_cannot_be_replaced_by_failure_report() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.init("alpha");
    let external = f.0.join("external.json");
    std::fs::write(&external, "original").unwrap();
    let report = f.root().join("alpha/report.json");
    symlink(&external, &report).unwrap();
    for command in ["test", "types", "inspect"] {
        assert_eq!(f.run(command, &report, &[]).status.code(), Some(1));
        assert_eq!(std::fs::read_link(&report).unwrap(), external);
        assert_eq!(std::fs::read_to_string(&external).unwrap(), "original");
    }
}
