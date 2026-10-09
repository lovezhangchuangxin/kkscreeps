//! G26.a `--checkpoint-every` 周期保存与目录恢复的端到端验收：多份落盘
//! 与报告字段、恢复重放到尾与直跑等值、坏份降级、零段合法终态、撕裂
//! 日志拒绝、互斥守卫与保留策略。
use super::*;

fn scenario() -> Value {
    json!({
        "version": 1,
        "shard": {"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}]},
        "bots": [{"player": "p1", "modules": "modules"}]
    })
}

/// 直跑参考与分相运行的比较按 observe 终态减 cpu_bucket 口径
///（与 sim_tests/checkpoint.rs 冷恢复验收同规）。
fn without_cpu(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(players) = value["players"].as_object_mut() {
        for player in players.values_mut() {
            if let Some(object) = player.as_object_mut() {
                object.remove("cpu_bucket");
            }
        }
    }
    value
}

/// 与实现同源的规范文件名（`ckpt-{tick:010}.json`）。
fn ckpt_name(tick: u64) -> String {
    format!("ckpt-{tick:010}.json")
}

static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Case(PathBuf);
impl Case {
    fn new(tag: &str) -> Self {
        Self::with_main(
            tag,
            "module.exports.loop = function () { Memory.n = (Memory.n || 0) + 1; };",
        )
    }
    /// 自定义 bot 入口代码（失败轮次等行为用例）。
    fn with_main(tag: &str, code: &str) -> Self {
        // 原子计数后缀：同测试进程内多 Case 只靠 tag 人工不重名维持并
        // 行安全，加序号消除撞名风险。
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-auto-ckpt-{}-{}-{seq}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("modules")).unwrap();
        std::fs::write(path.join("modules/main.js"), code).unwrap();
        std::fs::write(
            path.join("scenario.json"),
            serde_json::to_vec_pretty(&scenario()).unwrap(),
        )
        .unwrap();
        Self(path)
    }
    fn scene(&self) -> PathBuf {
        self.0.join("scenario.json")
    }
    fn command(&self, report: &Path, ticks: u32) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
        command
            .current_dir(std::env::temp_dir())
            .args(["sim", "run", "--scenario"])
            .arg(self.scene())
            .arg("--report")
            .arg(report)
            .arg("--ticks")
            .arg(ticks.to_string());
        command
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(command: &mut Command, report: &Path) -> anyhow::Result<(i32, Value)> {
    let output = command.output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((
        output.status.code().unwrap(),
        serde_json::from_slice(&std::fs::read(report)?)?,
    ))
}

/// 周期保存落盘多份、报告 v10 字段与 RPO 声明、keep 删旧。
#[test]
fn periodic_checkpoints_land_with_report_fields_and_keep() -> anyhow::Result<()> {
    let case = Case::new("land");
    let dir = case.0.join("ckpt");
    let report = case.0.join("r.json");

    // 与终态 --save-checkpoint 组合：两条保存路径互不冲突（路径互斥由
    // 守卫保证），终态文件照常落盘。
    let final_ckpt = case.0.join("final.json");
    let (code, report) = run(
        case.command(&report, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--checkpoint-keep")
            .arg("2")
            .arg("--save-checkpoint")
            .arg(&final_ckpt),
        &report,
    )?;
    assert!(final_ckpt.is_file(), "终态保存与周期保存可同跑");
    assert_eq!(code, 0);
    assert_eq!(report["report_version"], 10, "v10 起顶层带 auto_checkpoint");
    let auto = &report["auto_checkpoint"];
    assert_eq!(auto["every"], 2);
    assert_eq!(auto["saved"], 3, "tick 2/4/6 各一份");
    assert_eq!(auto["last_tick"], 6);
    assert_eq!(auto["keep"], 2);
    assert_eq!(auto["deleted"], 1, "tick 2 的那份被保留策略删除");
    assert!(
        auto["rpo"].as_str().unwrap().contains("不承诺零丢失"),
        "RPO 声明必须如实：{}",
        auto["rpo"]
    );
    assert!(
        auto["rpo"].as_str().unwrap().contains("最多丢 2 tick"),
        "RPO 数值与间隔一致：{}",
        auto["rpo"]
    );
    assert_eq!(auto["delete_errors"], json!([]), "成功路径无删除错误");

    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["ckpt-0000000004.json", "ckpt-0000000006.json"]);

    // 未启用周期保存/目录恢复的运行：v10 两个键均为 null。
    let plain_report = case.0.join("r-plain.json");
    let (_, plain) = run(&mut case.command(&plain_report, 2), &plain_report)?;
    assert!(plain["auto_checkpoint"].is_null(), "未启用时键为 null");
    assert!(plain["recovery"].is_null(), "未启用时键为 null");
    Ok(())
}

/// 目录恢复+续跑与直跑等值（减 CPU 桶）；--verify-replay 组合通过；
/// 日志尾恰与最新份衔接时零段待重放=合法终态。
#[test]
fn directory_recovery_replays_log_and_matches_straight_run() -> anyhow::Result<()> {
    let reference = Case::new("ref");
    let ref_report = reference.0.join("r.json");
    let (_, reference) = run(reference.command(&ref_report, 7).arg("--kkpv"), &ref_report)?;

    let case = Case::new("recover");
    let dir = case.0.join("ckpt");
    let log = case.0.join("input.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report1,
    )?;

    // 模拟崩溃：目录有 tick 2/4/6 三份，日志段到 tick 5——最新份 6 的
    // 世界时间恰为日志末段+1（保存后即崩的形态，零段待重放），续跑
    // 1 轮后应与直跑 7 轮等值。
    let report2 = case.0.join("r2.json");
    let (_, report2) = run(
        case.command(&report2, 1)
            .arg("--load-checkpoint")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log)
            .arg("--verify-replay"),
        &report2,
    )?;
    let recovery = &report2["recovery"];
    assert!(
        recovery["selected"]
            .as_str()
            .unwrap()
            .ends_with("ckpt-0000000006.json"),
        "{}",
        recovery["selected"]
    );
    assert_eq!(
        recovery["checkpoint_time"], 6,
        "零段边界：检查点时间锚世界时间（保存节奏不随热身平移）"
    );
    assert_eq!(
        recovery["replayed_segments"], 1,
        "保存后恰剩末轮一段（日志随热身平移、保存锚世界时间）"
    );
    assert_eq!(recovery["skipped_broken"], 0);
    assert_eq!(recovery["log_cursor"]["last_tick"], 5, "游标随保存落盘");
    assert_eq!(
        recovery["log_cursor"]["last_seq"], 5,
        "游标 seq = 文件内段数"
    );
    assert_eq!(report2["verify_replay"]["status"], "passed");
    assert_eq!(
        without_cpu(&report2["final"]),
        without_cpu(&reference["final"]),
        "目录恢复续跑终态须与直跑逐位相等（减 CPU 桶）"
    );
    Ok(())
}

/// 从更旧的检查点恢复：重放段更长，终态仍与直跑等值（重放确定性）。
#[test]
fn recovery_from_older_checkpoint_matches_straight_run() -> anyhow::Result<()> {
    let reference = Case::new("ref2");
    let ref_report = reference.0.join("r.json");
    let (_, reference) = run(&mut reference.command(&ref_report, 7), &ref_report)?;

    let case = Case::new("older");
    let dir = case.0.join("ckpt");
    let log = case.0.join("input.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report1,
    )?;
    // 摘掉较新的两份，强制选 tick 2：恢复 2 + 重放段 2..5 + 续跑 1 = 7。
    std::fs::remove_file(dir.join("ckpt-0000000004.json"))?;
    std::fs::remove_file(dir.join("ckpt-0000000006.json"))?;

    let report2 = case.0.join("r2.json");
    let (_, report2) = run(
        case.command(&report2, 1)
            .arg("--load-checkpoint")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report2,
    )?;
    assert_eq!(report2["recovery"]["checkpoint_time"], 2);
    assert_eq!(
        report2["recovery"]["replayed_segments"], 5,
        "段 2..6 重放（热身平移起点）"
    );
    assert_eq!(report2["final"]["time"], 8);
    assert_eq!(
        without_cpu(&report2["final"]),
        without_cpu(&reference["final"]),
        "所选检查点不影响终态：重放是确定性的"
    );
    Ok(())
}

/// 坏最新份降级：信封 SHA 校验失败的 tick 6 被跳过，选 tick 4 并重放
/// 段 4/5；报告记录跳过数。
#[test]
fn broken_newest_checkpoint_downgrades_to_previous() -> anyhow::Result<()> {
    let case = Case::new("downgrade");
    let dir = case.0.join("ckpt");
    let log = case.0.join("input.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report1,
    )?;
    let newest = dir.join(ckpt_name(6));
    let mut corrupted = std::fs::read(&newest)?;
    let mid = corrupted.len() / 2;
    corrupted[mid] = corrupted[mid].wrapping_add(1);
    std::fs::write(&newest, corrupted)?;
    // 非规范命名的手工文件不是策略管理对象：选择器忽略（不计坏份）。
    std::fs::write(dir.join("ckpt-9.json"), b"not canonical")?;

    let report2 = case.0.join("r2.json");
    let (_, report2) = run(
        case.command(&report2, 1)
            .arg("--load-checkpoint")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report2,
    )?;
    let recovery = &report2["recovery"];
    assert_eq!(recovery["skipped_broken"], 1, "坏最新份被跳过");
    assert_eq!(recovery["checkpoint_time"], 4);
    assert_eq!(recovery["replayed_segments"], 3, "段 4..6 重放到日志尾");
    assert_eq!(report2["final"]["time"], 8);

    let reference = Case::new("refd");
    let ref_report = reference.0.join("r.json");
    let (_, reference) = run(&mut reference.command(&ref_report, 7), &ref_report)?;
    assert_eq!(
        without_cpu(&report2["final"]),
        without_cpu(&reference["final"]),
        "降级恢复的终态仍与直跑等值"
    );
    Ok(())
}

/// 撕裂日志：恢复在日志读取处失败（exit 1）；错误指明不自动截断。
#[test]
fn torn_log_recovery_fails_without_auto_truncation() -> anyhow::Result<()> {
    let case = Case::new("torn");
    let dir = case.0.join("ckpt");
    let log = case.0.join("input.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 3)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report1,
    )?;
    let bytes = std::fs::read(&log)?;
    std::fs::write(&log, &bytes[..bytes.len() - 9])?;

    let report2 = case.0.join("r2.json");
    let output = case
        .command(&report2, 1)
        .arg("--load-checkpoint")
        .arg(&dir)
        .arg("--input-log")
        .arg(&log)
        .output()?;
    assert_eq!(output.status.code(), Some(1), "撕裂日志必须拒绝恢复");
    let report2: Value = serde_json::from_slice(&std::fs::read(&report2)?)?;
    assert_eq!(report2["status"], "setup_failed");
    let error = report2["error"].as_str().unwrap();
    assert!(
        error.contains("人工截断") || error.contains("撕裂"),
        "{error}"
    );
    Ok(())
}

/// 互斥与前置守卫：every 无目录、目录无 every、keep 无 every、目录
/// 恢复缺日志、mods_switch 组合、报告指进检查点目录。
#[test]
fn guards_reject_invalid_combinations() -> anyhow::Result<()> {
    let case = Case::new("guards");
    let dir = case.0.join("ckpt");
    std::fs::create_dir_all(&dir)?;

    let check = |name: &str, extra: &[&str], needle: &str| -> anyhow::Result<Value> {
        let report = case.0.join(format!("{name}.json"));
        let mut command = case.command(&report, 1);
        for arg in extra {
            command.arg(arg);
        }
        let output = command.output()?;
        assert_eq!(
            output.status.code(),
            Some(1),
            "{name}: {args:?}",
            args = extra
        );
        let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
        assert!(
            value["error"].as_str().unwrap().contains(needle),
            "{name}: {}",
            value["error"]
        );
        Ok(value)
    };

    let every_no_dir = check(
        "every-no-dir",
        &["--checkpoint-every", "2"],
        "--checkpoint-dir",
    )?;
    assert_eq!(
        every_no_dir["auto_checkpoint"]["status"], "setup_failed",
        "v10：周期保存启用即非 null（两路同升）"
    );
    check(
        "dir-no-every",
        &["--checkpoint-dir", "ckpt"],
        "--checkpoint-every",
    )?;
    check(
        "keep-no-every",
        &["--checkpoint-keep", "2"],
        "--checkpoint-every",
    )?;
    let recover_no_log = check(
        "recover-no-log",
        &["--load-checkpoint", dir.to_str().unwrap()],
        "--input-log",
    )?;
    assert_eq!(
        recover_no_log["recovery"]["status"], "setup_failed",
        "v10：目录形态恢复启用即非 null（两路同升）"
    );

    // 同目录「恢复+继续周期保存」当前被路径互斥守卫拒绝（目录由自动
    // 策略管理，放宽属后续切片）；守卫失败发生在报告写入之前，从
    // stderr 断言且不产生报告文件。
    let same_report = case.0.join("same-dir.json");
    let output = case
        .command(&same_report, 1)
        .arg("--load-checkpoint")
        .arg(&dir)
        .arg("--input-log")
        .arg(case.0.join("log.jsonl"))
        .arg("--checkpoint-every")
        .arg("2")
        .arg("--checkpoint-dir")
        .arg(&dir)
        .output()?;
    assert_eq!(output.status.code(), Some(1), "同目录恢复+保存暂被守卫拒绝");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("周期检查点目录"),
        "守卫信息指明目录互斥：{stderr}"
    );
    assert!(!same_report.exists(), "守卫失败不写报告");
    Ok(())
}

/// mods_switch 场景与周期保存互斥；报告路径指进检查点目录被守卫拒绝。
#[test]
fn switch_scenario_and_dir_outputs_are_rejected() -> anyhow::Result<()> {
    let scene = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-switch/scenario-add.json")
        .canonicalize()?;
    let fixture = Fixture::new()?;
    let report = fixture.0.join("r.json");
    let output = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .arg("--report")
        .arg(&report)
        .args(["--ticks", "2"])
        .arg("--checkpoint-every")
        .arg("1")
        .arg("--checkpoint-dir")
        .arg(fixture.0.join("ckpt"))
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert!(
        value["error"].as_str().unwrap().contains("mods_switch"),
        "{}",
        value["error"]
    );

    let case = Case::new("inside");
    let dir = case.0.join("ckpt");
    std::fs::create_dir_all(&dir)?;
    let inside_report = dir.join("r.json");
    let output = case
        .command(&inside_report, 1)
        .arg("--checkpoint-every")
        .arg("1")
        .arg("--checkpoint-dir")
        .arg(&dir)
        .output()?;
    assert_eq!(output.status.code(), Some(1), "目录内输出必须被守卫拒绝");
    // validate_paths 失败发生在报告写入之前（报告本身就在被拒目录内），
    // 诊断只走 stderr。
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("周期检查点目录"),
        "守卫必须拒绝目录内输出：{stderr}"
    );
    assert!(!inside_report.exists(), "守卫失败不写报告");
    Ok(())
}

/// 检查点领先日志（time > 日志末段+1）：日志曾在检查点之前被摘除、世
/// 界连续性断裂——硬错误不降级，也不自动截断日志凑数。
#[test]
fn checkpoint_beyond_log_tail_is_hard_error() -> anyhow::Result<()> {
    let case = Case::new("beyond");
    let dir = case.0.join("ckpt");
    let long_log = case.0.join("long.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&long_log),
        &report1,
    )?;

    // 短日志只记录到 tick 1：目录最新份 tick 6 领先其末段 4 轮，任何
    // 配对解释都无法弥合，必须拒绝而非降级选旧份。
    let short_log = case.0.join("short.jsonl");
    let report2 = case.0.join("r2.json");
    let _ = run(
        case.command(&report2, 2).arg("--input-log").arg(&short_log),
        &report2,
    )?;

    let report3 = case.0.join("r3.json");
    let output = case
        .command(&report3, 1)
        .arg("--load-checkpoint")
        .arg(&dir)
        .arg("--input-log")
        .arg(&short_log)
        .output()?;
    assert_eq!(output.status.code(), Some(1), "检查点领先日志必须硬错误");
    let report3: Value = serde_json::from_slice(&std::fs::read(&report3)?)?;
    assert_eq!(report3["status"], "setup_failed");
    let error = report3["error"].as_str().unwrap();
    assert!(error.contains("不覆盖检查点游标"), "{error}");
    assert_eq!(report3["recovery"]["status"], "setup_failed");
    Ok(())
}

/// 周期保存 I/O 失败：status=checkpoint_failed、退出 1、保存计数停在
/// 失败前（预置同名目录使原子 rename 落不下去——与终态保存失败用例
/// 同一手法）。
#[test]
fn periodic_save_io_failure_fails_the_run() -> anyhow::Result<()> {
    let case = Case::new("iofail");
    let dir = case.0.join("ckpt");
    std::fs::create_dir_all(dir.join(ckpt_name(2)))?;
    let report = case.0.join("r.json");
    let output = case
        .command(&report, 3)
        .arg("--checkpoint-every")
        .arg("2")
        .arg("--checkpoint-dir")
        .arg(&dir)
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(1),
        "保存失败必须使运行失败（退出 1）"
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(value["status"], "checkpoint_failed");
    assert_eq!(value["failure"]["stage"], "checkpoint");
    assert_eq!(value["failure"]["tick"], 2, "首个到点保存即失败");
    assert_eq!(value["auto_checkpoint"]["saved"], 0, "失败前无成功保存");
    assert_eq!(
        value["world_ticks_committed"], 1,
        "失败前轮次已提交并如实上报（热身拍：到点保存在首轮后）"
    );
    Ok(())
}

/// 崩溃→恢复→继续打点的闭环第二段：恢复后的运行可继续周期保存到新
/// 目录，按世界时间对齐落点；日志续接不双重记录；分相终态与直跑等值。
#[test]
fn recovered_run_continues_periodic_checkpointing() -> anyhow::Result<()> {
    let reference = Case::new("ref3");
    let ref_report = reference.0.join("r.json");
    let (_, reference) = run(&mut reference.command(&ref_report, 8), &ref_report)?;

    let case = Case::new("continue");
    let first_dir = case.0.join("ckpt1");
    let second_dir = case.0.join("ckpt2");
    let log = case.0.join("input.jsonl");
    let report1 = case.0.join("r1.json");
    let _ = run(
        case.command(&report1, 6)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&first_dir)
            .arg("--input-log")
            .arg(&log),
        &report1,
    )?;

    // 恢复到 tick 6 后再跑 2 轮：结算后时间 7/8，仅 8 为 2 的倍数，
    // 新目录恰好落一份 ckpt-0000000008.json。
    let report2 = case.0.join("r2.json");
    let (_, report2) = run(
        case.command(&report2, 2)
            .arg("--load-checkpoint")
            .arg(&first_dir)
            .arg("--input-log")
            .arg(&log)
            .arg("--checkpoint-every")
            .arg("2")
            .arg("--checkpoint-dir")
            .arg(&second_dir),
        &report2,
    )?;
    assert_eq!(report2["recovery"]["checkpoint_time"], 6);
    assert_eq!(report2["recovery"]["replayed_segments"], 1);
    let auto = &report2["auto_checkpoint"];
    assert_eq!(
        auto["saved"], 1,
        "恢复后按世界时间对齐：时间 7 跳过、8 保存"
    );
    assert_eq!(auto["last_tick"], 8);
    let mut names: Vec<String> = std::fs::read_dir(&second_dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![ckpt_name(8)]);
    assert_eq!(report2["final"]["time"], 9);
    let lines = std::fs::read_to_string(&log)?.lines().count();
    assert_eq!(
        lines, 9,
        "header + 6 旧段 + 2 新段（重放不写段），续接不双重记录"
    );
    assert_eq!(
        without_cpu(&report2["final"]),
        without_cpu(&reference["final"]),
        "分相（6 轮 + 恢复续 2 轮）与直跑 8 轮终态等值"
    );
    Ok(())
}

/// 间隔边界：every=1 每轮保存（keep 淘汰后留最新两份）；间隔大于总轮
/// 数时零保存、last_tick 为 null、目录为空。
#[test]
fn interval_edges_cover_every_tick_and_beyond_run_length() -> anyhow::Result<()> {
    let case = Case::new("interval");
    let dir = case.0.join("ckpt");
    let report = case.0.join("r.json");
    let (_, report) = run(
        case.command(&report, 3)
            .arg("--checkpoint-every")
            .arg("1")
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--checkpoint-keep")
            .arg("2"),
        &report,
    )?;
    let auto = &report["auto_checkpoint"];
    assert_eq!(auto["saved"], 3, "每轮一份：tick 1/2/3");
    assert_eq!(auto["deleted"], 1, "保留 2 份删最旧");
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![ckpt_name(3), ckpt_name(4)]);

    let dir2 = case.0.join("ckpt2");
    let report2 = case.0.join("r2.json");
    let (_, report2) = run(
        case.command(&report2, 2)
            .arg("--checkpoint-every")
            .arg("100")
            .arg("--checkpoint-dir")
            .arg(&dir2),
        &report2,
    )?;
    let auto2 = &report2["auto_checkpoint"];
    assert_eq!(auto2["saved"], 0, "间隔大于总轮数：零保存");
    assert!(auto2["last_tick"].is_null(), "无保存则无最后 tick");
    assert_eq!(std::fs::read_dir(&dir2)?.count(), 0, "目录为空");
    Ok(())
}

/// 失败玩家轮不保存：保存点在失败判定之后——抛错轮（第 3 轮，时间
/// 2）无检查点，此前成功轮照常保存；status=run_failed 而非
/// checkpoint_failed。
#[test]
fn failed_player_round_skips_periodic_save() -> anyhow::Result<()> {
    let case = Case::with_main(
        "failround",
        "module.exports.loop = function () { Memory.n = (Memory.n || 0) + 1; if (Memory.n === 3) { throw new Error('boom'); } };",
    );
    let dir = case.0.join("ckpt");
    let report = case.0.join("r.json");
    let output = case
        .command(&report, 4)
        .arg("--checkpoint-every")
        .arg("1")
        .arg("--checkpoint-dir")
        .arg(&dir)
        .output()?;
    assert_eq!(output.status.code(), Some(2), "玩家失败=运行失败（退出 2）");
    let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(value["status"], "run_failed");
    assert_eq!(value["failure"]["stage"], "player");
    assert_eq!(
        value["auto_checkpoint"]["saved"], 2,
        "tick 1/2 已保存，抛错轮不保存"
    );
    let mut names: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec![ckpt_name(2), ckpt_name(3)]);
    Ok(())
}

#[test]
fn loaded_checkpoint_directory_protects_every_output_before_failure_reporting() -> anyhow::Result<()>
{
    let case = Case::new("protect-load");
    let dir = case.0.join("ckpt");
    let log = case.0.join("log.jsonl");
    let report = case.0.join("first.json");
    run(
        case.command(&report, 2)
            .args(["--checkpoint-every", "2"])
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log),
        &report,
    )?;
    let saved = dir.join(ckpt_name(2));
    let before = std::fs::read(&saved)?;
    for flag in ["--report", "--save-checkpoint", "--input-log"] {
        let safe_report = case.0.join("safe.json");
        let target_report = if flag == "--report" {
            &saved
        } else {
            &safe_report
        };
        let mut command = case.command(target_report, 1);
        command.arg("--load-checkpoint").arg(&dir);
        if flag != "--input-log" {
            command.arg("--input-log").arg(&log);
        }
        if flag != "--report" {
            command.arg(flag).arg(&saved);
        }
        let output = command.output()?;
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("加载检查点目录"));
        assert_eq!(std::fs::read(&saved)?, before);
        assert!(!safe_report.exists(), "守卫失败不写失败报告");
    }
    #[cfg(unix)]
    {
        let alias = case.0.join("alias");
        std::os::unix::fs::symlink(&dir, &alias)?;
        // 场景损坏时也不能向输入目录写 setup_failed 报告。
        std::fs::write(case.scene(), b"broken scenario")?;
        let output = case
            .command(&alias.join(ckpt_name(2)), 1)
            .arg("--load-checkpoint")
            .arg(&dir)
            .arg("--input-log")
            .arg(&log)
            .output()?;
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(std::fs::read(&saved)?, before);
    }
    Ok(())
}

#[test]
fn zero_replay_rejects_another_valid_log_history() -> anyhow::Result<()> {
    let case = Case::new("foreign-history");
    let dir = case.0.join("ckpt");
    let a = case.0.join("a.jsonl");
    let b = case.0.join("b.jsonl");
    let report = case.0.join("first.json");
    run(
        case.command(&report, 2)
            .args(["--checkpoint-every", "2"])
            .arg("--checkpoint-dir")
            .arg(&dir)
            .arg("--input-log")
            .arg(&a),
        &report,
    )?;
    let report = case.0.join("other.json");
    run(case.command(&report, 2).arg("--input-log").arg(&b), &report)?;
    assert_ne!(
        kk_runtime::embed::read_input_log(&a)?.chain,
        kk_runtime::embed::read_input_log(&b)?.chain
    );
    let report = case.0.join("rejected.json");
    let output = case
        .command(&report, 1)
        .arg("--load-checkpoint")
        .arg(&dir)
        .arg("--input-log")
        .arg(&b)
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert!(
        report["error"].as_str().unwrap().contains("历史哈希不匹配"),
        "{report}"
    );
    Ok(())
}
