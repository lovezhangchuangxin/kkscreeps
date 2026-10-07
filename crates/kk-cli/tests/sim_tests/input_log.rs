//! G25.a `sim run --input-log` 的端到端验收：逐轮记段、报告摘要与读回
//! 对账、同路径续跑段号延续、损坏日志拒绝续接。
use super::*;

use kk_runtime::embed::read_input_log;

fn bot_module() -> &'static str {
    // 逐轮写 Memory：玩家段的 raw_memory 写回随段可验。
    "module.exports.loop = function () { Memory.seen = (Memory.seen || 0) + 1; };"
}

fn scenario() -> Value {
    json!({
        "version": 1,
        "shard": {"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}]},
        "bots": [{"player": "p1", "modules": "modules"}]
    })
}

fn sim_command(scene: &Path, report: &Path, log: &Path, ticks: u32) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string())
        .arg("--input-log")
        .arg(log);
    command
}

/// 三轮全部记段（source=players）、报告 v8 摘要与磁盘读回对账、
/// RawMemory 写回随段可验、续跑段号延续、损坏日志拒绝续接。
#[test]
fn input_log_records_commits_and_resumes() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&scenario())?;
    fixture.module(bot_module())?;
    let log = fixture.0.join("input.jsonl");
    let report = fixture.0.join("r1.json");

    let output = sim_command(&scene, &report, &log, 3).output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["status"], "passed");
    assert_eq!(report["report_version"], 8, "v8 起顶层带 input_log 字段");
    assert_eq!(report["input_log"]["segments"], 3);
    assert_eq!(report["input_log"]["read_back_segments"], 3);
    assert_eq!(report["input_log"]["validation"], "ok");
    assert_eq!(report["input_log"]["first_tick"], 0);
    assert_eq!(report["input_log"]["last_tick"], 2);

    // 独立读回：三段全为玩家轮，段号 1..=3，写回随段落位。
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 3);
    for (index, segment) in data.segments.iter().enumerate() {
        assert_eq!(segment["seq"], index as u64 + 1);
        assert_eq!(segment["source"], "players");
        let raw = segment["players"]["p1"]["raw_memory"].as_str().unwrap();
        assert!(raw.contains("seen"), "玩家写回候选随段落位：{raw}");
    }

    // 同场景同日志路径续跑：先整读校验再续接，段号延续到 5。
    let report2 = fixture.0.join("r2.json");
    let output = sim_command(&scene, &report2, &log, 2).output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report2: Value = serde_json::from_slice(&std::fs::read(&report2)?)?;
    assert_eq!(report2["input_log"]["segments"], 5, "段数按整个文件计");
    assert_eq!(report2["input_log"]["validation"], "ok");
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 5);
    assert_eq!(data.segments[4]["seq"], 5);
    // 续跑是全新分片（未带检查点），世界 tick 从 0 重新开始；日志只保证
    // 段号结构延续。世界连续性须与检查点配对，是 G25.b 的回放规则。
    assert_eq!(data.segments[4]["tick"], 1);

    // 损坏（中段字节翻转）后拒绝续接：整读校验失败属 setup 失败。
    let text = std::fs::read_to_string(&log)?;
    let corrupted = text.replacen("\"source\":\"players\"", "\"source\":\"playerz\"", 2);
    assert_ne!(corrupted, text, "字节翻转确实生效");
    std::fs::write(&log, corrupted)?;
    let report3 = fixture.0.join("r3.json");
    let output = sim_command(&scene, &report3, &log, 1).output()?;
    assert_eq!(output.status.code(), Some(1), "损坏日志必须拒绝续接");
    let report3: Value = serde_json::from_slice(&std::fs::read(&report3)?)?;
    assert_eq!(report3["status"], "setup_failed");
    assert!(report3["error"].as_str().unwrap().contains("输入日志"));
    // v8 契约：--input-log 启用时 setup_failed 报告同样带非 null 的键。
    assert_eq!(report3["input_log"]["status"], "setup_failed");
    assert_eq!(report3["input_log"]["path"], log.display().to_string());
    Ok(())
}

/// 检查点配对：同跑 --save-checkpoint + --input-log；随后 --load-checkpoint
/// 加同一日志续跑。恢复摘除运行期日志，sim 在恢复后重新挂载，段号跨进程
/// 延续且续跑段的世界 tick 从恢复点继续（与检查点配对的世界连续性）。
#[test]
fn input_log_pairs_with_checkpoint_across_processes() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&scenario())?;
    fixture.module(bot_module())?;
    let log = fixture.0.join("pair.jsonl");
    let checkpoint = fixture.0.join("cp.json");
    let report = fixture.0.join("p1.json");

    let mut command = sim_command(&scene, &report, &log, 2);
    command.arg("--save-checkpoint").arg(&checkpoint);
    let output = command.output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["input_log"]["segments"], 2);
    assert!(checkpoint.exists(), "检查点与日志同跑都落盘");

    let report2 = fixture.0.join("p2.json");
    let mut command = sim_command(&scene, &report2, &log, 2);
    command.arg("--load-checkpoint").arg(&checkpoint);
    let output = command.output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report2: Value = serde_json::from_slice(&std::fs::read(&report2)?)?;
    assert_eq!(report2["input_log"]["segments"], 4, "段号跨进程延续");
    assert_eq!(
        report2["input_log"]["last_tick"], 3,
        "恢复后续跑从检查点 tick 继续"
    );

    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 4);
    let ticks: Vec<u64> = data
        .segments
        .iter()
        .filter_map(|s| s["tick"].as_u64())
        .collect();
    assert_eq!(ticks, [0, 1, 2, 3], "恢复配对下世界 tick 连续");
    Ok(())
}

/// 场景声明 mods_switch 时：切换摘除运行期日志（生命周期语义），报告
/// 如实标注 detached 而非误报 I/O 失败，切换前已写段仍通过读回校验。
#[test]
fn input_log_detaches_on_mods_switch() -> anyhow::Result<()> {
    let scene = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-switch/scenario-add.json")
        .canonicalize()?;
    let fixture = Fixture::new()?;
    let log = fixture.0.join("switch.jsonl");
    let report = fixture.0.join("s.json");

    // 场景断言钉死 /time==6：与 switch.rs 同口径跑满 6 tick。
    let output = sim_command(&scene, &report, &log, 6).output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["status"], "passed");
    let summary = &report["input_log"];
    assert_eq!(summary["status"], "detached", "切换摘除不得误报为 I/O 失败");
    assert_eq!(summary["validation"], "ok");
    // 该场景 after_tick=2：前两轮记段，切换及其后轮次不记。
    assert_eq!(summary["read_back_segments"], 2);
    assert_eq!(summary["last_tick"], 1);
    Ok(())
}
