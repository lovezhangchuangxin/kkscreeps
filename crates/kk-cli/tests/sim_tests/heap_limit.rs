//! 场景字段 player_heap_limit_bytes 的端到端行为：报告记录生效值、
//! 检查点工作者身份携带该值，以及续跑时声明值与恢复工作者的硬约束。
use super::*;

fn command(scene: &Path, report: &Path, ticks: u32) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string());
    command
}

#[test]
fn heap_limit_scenario_field_reaches_workers_and_report() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };")?;
    // 未声明字段：报告记录整卡默认 64MiB。
    let scene = fixture.scene(&simple_scene())?;
    let report = fixture.0.join("default.json");
    let (code, value) = run(&scene, &report, 2, false, 1)?;
    assert_eq!(code, 0, "{value}");
    assert_eq!(
        value["player_heap_limit_bytes"].as_u64(),
        Some(64 * 1024 * 1024),
        "缺省报告整卡默认堆上限"
    );
    // 声明 32MiB：报告与检查点工作者身份都携带声明值。
    let mut declared = simple_scene();
    declared["player_heap_limit_bytes"] = json!(32 * 1024 * 1024);
    let scene = fixture.scene(&declared)?;
    let report = fixture.0.join("declared.json");
    let checkpoint = fixture.0.join("checkpoint.json");
    let output = command(&scene, &report, 2)
        .arg("--save-checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert_eq!(output.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(
        value["player_heap_limit_bytes"].as_u64(),
        Some(32 * 1024 * 1024),
        "报告记录场景声明值"
    );
    let saved = kk_runtime::checkpoint::read_checkpoint(&checkpoint)?;
    assert_eq!(
        saved["workers"]["p1"]["heap_limit_bytes"].as_u64(),
        Some(32 * 1024 * 1024),
        "工作者身份携带场景声明值"
    );
    Ok(())
}

#[test]
fn resume_rejects_scenario_heap_limit_drift() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };")?;
    let mut declared = simple_scene();
    declared["player_heap_limit_bytes"] = json!(32 * 1024 * 1024);
    let scene = fixture.scene(&declared)?;
    let report = fixture.0.join("report.json");
    let checkpoint = fixture.0.join("checkpoint.json");
    let output = command(&scene, &report, 2)
        .arg("--save-checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert_eq!(output.status.code(), Some(0));
    // 场景声明改为 48MiB 后续跑：工作者身份随检查点恢复，声明值与
    // 恢复工作者不一致必须在启动前拒绝，而不是静默忽略场景值。
    declared["player_heap_limit_bytes"] = json!(48 * 1024 * 1024);
    let scene = fixture.scene(&declared)?;
    let output = command(&scene, &report, 1)
        .arg("--load-checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("堆上限不一致"),
        "拒绝信息指向堆上限身份：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 声明值与检查点一致时正常续跑。
    declared["player_heap_limit_bytes"] = json!(32 * 1024 * 1024);
    let scene = fixture.scene(&declared)?;
    let output = command(&scene, &report, 1)
        .arg("--load-checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}
