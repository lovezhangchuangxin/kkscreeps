//! 经真实 CLI 子进程验证 serve init/run：SIGTERM 保存与恢复续存、目录
//! 租约互斥、半初始化拒绝与持久化故障的非零退出。
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-serve-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("modules"))?;
        std::fs::write(
            path.join("modules/main.js"),
            "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };",
        )?;
        std::fs::write(
            path.join("scenario.json"),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "seed": 11,
                "shard": {"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}]},
                "bots": [{"player": "p1", "modules": "modules", "entry": "main"}],
            }))?,
        )?;
        Ok(Self(path))
    }

    /// 写服务配置；返回配置路径（state_dir 解析相对配置所在目录）。
    fn config(&self, extra: Value) -> anyhow::Result<PathBuf> {
        let path = self.0.join("serve.json");
        let mut config = json!({
            "config_format_version": 1,
            "state_dir": "state",
            "tick_interval_ms": 40,
            "auto_save_ticks": 5,
            "auto_save_wall_secs": 3600,
            "keep_checkpoints": 3,
            "exit_wait_secs": 10,
        });
        if let (Some(target), Some(source)) = (config.as_object_mut(), extra.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
        std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
        Ok(path)
    }

    fn state(&self) -> PathBuf {
        self.0.join("state")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 子进程守卫：断言失败路径也先 SIGTERM（超时升级 kill），避免孤儿常驻
/// 进程持有目录租约，污染后续用例与本地环境。
struct KillOnDrop(std::process::Child);
impl KillOnDrop {
    fn spawn(command: &mut Command) -> anyhow::Result<Self> {
        Ok(Self(command.spawn()?))
    }
    fn id(&self) -> u32 {
        self.0.id()
    }
    fn child_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
    fn wait(&mut self) -> anyhow::Result<std::process::ExitStatus> {
        Ok(self.0.wait()?)
    }
}
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // 已退出（含测试内已 wait）则无事可做。
        if self.0.try_wait().map(|s| s.is_some()).unwrap_or(true) {
            return;
        }
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        for _ in 0..100 {
            if self.0.try_wait().map(|s| s.is_some()).unwrap_or(true) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command.current_dir(std::env::temp_dir()).args(args);
    command
}

fn init_ok(fixture: &Fixture, config: &Path) -> anyhow::Result<Value> {
    let output = serve(&[
        "serve",
        "init",
        "--config",
        &config.to_string_lossy(),
        "--seed-scenario",
        &fixture.0.join("scenario.json").to_string_lossy(),
    ])
    .output()?;
    anyhow::ensure!(
        output.status.success(),
        "serve init 应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// 等待谓词在超时窗内成立（默认 15s，覆盖 CI 抖动）。
fn wait_for(mut predicate: impl FnMut() -> bool) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if predicate() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    anyhow::bail!("等待条件超时")
}

/// 从已接管的 stdout 逐行读取 JSON（控制应答与最终状态）。
fn read_status_line(reader: &mut BufReader<std::process::ChildStdout>) -> anyhow::Result<Value> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(serde_json::from_str(line.trim())?)
}

/// 主链路：init → 常驻运行（SIGTERM 保存，退出 0）→ 再次运行（无日志
/// 恢复续存，控制通道停止）。
#[test]
#[cfg(unix)]
fn init_run_sigterm_save_then_recover_and_continue() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    let report = init_ok(&fixture, &config)?;
    assert_eq!(report["initial_checkpoint"]["publish_seq"], 1);
    let initial_tick = report["initial_checkpoint"]["tick"].as_u64().unwrap_or(0);
    assert!(fixture.state().join("identity.json").is_file());

    // 常驻运行：输出进文件避免管道缓冲；周期保存应出现多份检查点。
    let stderr_path = fixture.0.join("run1.err");
    let mut child = KillOnDrop::spawn(
        serve(&["serve", "run", "--config", &config.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&stderr_path)?)),
    )?;
    wait_for(|| {
        std::fs::read_dir(fixture.state().join("checkpoints"))
            .map(|entries| entries.count() >= 2)
            .unwrap_or(false)
    })?;
    // SIGTERM：收尾保存后退出 0。
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let status = child.wait()?;
    anyhow::ensure!(status.code() == Some(0), "SIGTERM 应优雅退出 0：{status:?}");
    let identity = std::fs::read(fixture.state().join("identity.json"))?;
    anyhow::ensure!(!identity.is_empty());

    // 二次运行：无日志恢复续存（live_tick 自检查点继续增长），经控制
    // 通道查询并停止。
    let mut child = KillOnDrop::spawn(
        serve(&[
            "serve",
            "run",
            "--config",
            &config.to_string_lossy(),
            "--control-stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()),
    )?;
    // 两次轮询确认恢复后世界仍在推进（live_tick 增长），并核对恢复选择。
    let mut stdout = BufReader::new(child.child_mut().stdout.take().expect("stdout 已接管"));
    writeln!(
        child.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"status"}}"#
    )?;
    let first = read_status_line(&mut stdout)?;
    let first_tick = first["live_tick"].as_u64().unwrap_or(0);
    anyhow::ensure!(
        first["recovery_selected"]
            .as_str()
            .is_some_and(|name| !name.is_empty()),
        "状态应携带恢复选择：{first}"
    );
    anyhow::ensure!(
        first_tick > initial_tick,
        "恢复后的 live_tick 应越过初始检查点：{first}（init tick {initial_tick}）"
    );
    wait_for(|| {
        let sent = writeln!(
            child.child_mut().stdin.as_mut().expect("stdin"),
            r#"{{"command":"status"}}"#
        )
        .is_ok();
        sent && read_status_line(&mut stdout)
            .ok()
            .and_then(|status| status["live_tick"].as_u64())
            .is_some_and(|tick| tick > first_tick)
    })?;
    writeln!(
        child.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"stop"}}"#
    )?;
    let final_line = read_status_line(&mut stdout)?;
    assert_eq!(final_line["ok"], true, "stop 应答: {final_line}");
    let status = child.wait()?;
    anyhow::ensure!(status.code() == Some(0), "控制停止应退出 0：{status:?}");
    Ok(())
}

/// 控制通道容错：非法行与无故障 retry 得 ok:false 应答且不崩通道，
/// 世界继续推进后正常停止。
#[test]
#[cfg(unix)]
fn control_channel_rejects_invalid_input_and_reports_retry_without_fault() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    init_ok(&fixture, &config)?;
    let mut child = KillOnDrop::spawn(
        serve(&[
            "serve",
            "run",
            "--config",
            &config.to_string_lossy(),
            "--control-stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null()),
    )?;
    let mut stdout = BufReader::new(child.child_mut().stdout.take().expect("stdout 已接管"));
    // 非 JSON 行 → 错误应答，不杀通道。
    writeln!(child.child_mut().stdin.as_mut().expect("stdin"), "not-json")?;
    let reply = read_status_line(&mut stdout)?;
    anyhow::ensure!(
        reply["ok"] == false && reply["error"].as_str().is_some(),
        "非法行应得 ok:false 错误应答：{reply}"
    );
    // 无故障 retry → ok:false（不是无声成功）。
    writeln!(
        child.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"retry"}}"#
    )?;
    let reply = read_status_line(&mut stdout)?;
    anyhow::ensure!(reply["ok"] == false, "无故障 retry 应答 ok:false：{reply}");
    // 通道仍活：status 继续应答且世界在推进。
    writeln!(
        child.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"status"}}"#
    )?;
    let first = read_status_line(&mut stdout)?;
    let first_tick = first["live_tick"].as_u64().unwrap_or(0);
    wait_for(|| {
        writeln!(
            child.child_mut().stdin.as_mut().expect("stdin"),
            r#"{{"command":"status"}}"#
        )
        .is_ok()
            && read_status_line(&mut stdout)
                .ok()
                .and_then(|status| status["live_tick"].as_u64())
                .is_some_and(|tick| tick > first_tick)
    })?;
    writeln!(
        child.child_mut().stdin.as_mut().expect("stdin"),
        r#"{{"command":"stop"}}"#
    )?;
    let final_line = read_status_line(&mut stdout)?;
    assert_eq!(final_line["ok"], true, "stop 应答: {final_line}");
    let status = child.wait()?;
    anyhow::ensure!(status.code() == Some(0), "控制停止应退出 0：{status:?}");
    Ok(())
}

/// 半初始化目录（检查点在、identity 缺失）：run 与再次 init 都拒绝。
#[test]
fn half_initialized_directory_is_refused() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    init_ok(&fixture, &config)?;
    // 模拟 init 在 identity 写入前崩溃：删除 identity，保留检查点。
    std::fs::remove_file(fixture.state().join("identity.json"))?;
    let output = serve(&["serve", "run", "--config", &config.to_string_lossy()]).output()?;
    anyhow::ensure!(output.status.code() == Some(1), "run 应拒绝半初始化目录");
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(stderr.contains("身份"), "错误应指向身份缺失：{stderr}");
    let output = serve(&[
        "serve",
        "init",
        "--config",
        &config.to_string_lossy(),
        "--seed-scenario",
        &fixture.0.join("scenario.json").to_string_lossy(),
    ])
    .output()?;
    anyhow::ensure!(output.status.code() == Some(1), "init 不得复用半初始化目录");
    Ok(())
}

/// 单写者租约：运行中的目录拒绝第二个 serve run 进程。
#[test]
#[cfg(unix)]
fn second_process_is_rejected_while_lease_held() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({}))?;
    init_ok(&fixture, &config)?;
    let banner = fixture.0.join("lease1.err");
    let mut first = KillOnDrop::spawn(
        serve(&["serve", "run", "--config", &config.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&banner)?)),
    )?;
    // banner 在租约获取与恢复之后打印：出现即证明第一进程持锁。
    wait_for(|| {
        std::fs::read_to_string(&banner)
            .map(|text| text.contains("serve run: world"))
            .unwrap_or(false)
    })?;
    let second = serve(&["serve", "run", "--config", &config.to_string_lossy()]).output()?;
    anyhow::ensure!(
        second.status.code() == Some(1),
        "第二进程应立即被拒: {:?}",
        second.status
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    anyhow::ensure!(stderr.contains("写者"), "错误应指向租约争用：{stderr}");
    unsafe {
        libc::kill(first.id() as i32, libc::SIGTERM);
    }
    let status = first.wait()?;
    anyhow::ensure!(status.code() == Some(0), "第一进程应正常退出：{status:?}");
    Ok(())
}

/// 持久化故障：保存失败冻结推进，SIGTERM 退出非零且如实报告。
#[test]
#[cfg(unix)]
fn persistence_fault_exits_nonzero_after_sigterm() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new()?;
    let config = fixture.config(json!({"auto_save_ticks": 1, "tick_interval_ms": 30}))?;
    init_ok(&fixture, &config)?;
    // 检查点目录只读：周期保存的临时文件创建失败 → PersistenceFault。
    let checkpoints = fixture.state().join("checkpoints");
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o500))?;
    let stderr_path = fixture.0.join("fault.err");
    let mut child = KillOnDrop::spawn(
        serve(&["serve", "run", "--config", &config.to_string_lossy()])
            .stdout(Stdio::null())
            .stderr(Stdio::from(std::fs::File::create(&stderr_path)?)),
    )?;
    wait_for(|| {
        std::fs::read_to_string(&stderr_path)
            .map(|text| text.contains("冻结推进"))
            .unwrap_or(false)
    })?;
    // 权限恢复提前到断言前：失败路径也能由 Fixture 正常清理目录；同时
    // 覆盖"曾在持久化故障下停止，即使收尾保存随后成功也退出 2"的语义。
    std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o755))?;
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let status = child.wait()?;
    anyhow::ensure!(
        status.code() == Some(2),
        "持久化故障停止应退出 2：{status:?}"
    );
    Ok(())
}
