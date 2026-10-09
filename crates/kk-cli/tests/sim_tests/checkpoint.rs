//! 真正退出子进程后恢复：原始断言起点、alias、玩家与 Mod 状态均连续。
use super::*;

fn command(scene: &Path, report: &Path, ticks: u32, binary: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string());
    if binary {
        command.arg("--kkpv");
    }
    command
}

fn result(command: &mut Command, report: &Path) -> anyhow::Result<(i32, Value)> {
    let output = command.output()?;
    let value = serde_json::from_slice(&std::fs::read(report)?)?;
    Ok((output.status.code().unwrap(), value))
}

fn without_timing_cpu(mut value: Value) -> Value {
    // 真实 CPU 费用由每次执行实际测量；世界/玩法等价不能要求独立运行计时相等。
    for player in value["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    value
}

#[test]
fn cold_cli_resume_matches_uninterrupted_multiplayer_mod_world() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; RawMemory.setActiveSegments([6]); RawMemory.segments[6] = String(Memory.n); };")?;
    let mods = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../mods/official")
        .canonicalize()?;
    let scene = fixture.scene(&json!({"version": 1,
        "shard": {"players": ["p1", "p2"], "rooms": [{"name": "W1N1", "owner": "p1"}, {"name": "W2N2", "owner": "p2"}]},
        "bots": [{"player": "p1", "modules": "modules", "memory": {"n": 0}}, {"player": "p2", "modules": "modules", "memory": {"n": 0}}],
        "mods": {"root": mods, "selection": {"include": ["nursery", "clockwork"]}},
        "objects": [
            {"alias": "emitter", "room": "W1N1", "kind": "mod", "spec": {"typeKey": "nursery:emitter", "fields": {"x": 10, "y": 10}}},
            {"alias": "counter", "room": "W2N2", "kind": "mod", "spec": {"typeKey": "clockwork:counter", "fields": {"x": 10, "y": 10}}}
        ],
        "assertions": [{"pointer": "/players/p1/memory/n", "op": "delta_gte", "value": 12}, {"pointer": "/objects/counter/fields/value", "op": "eq", "value": 12}]
    }))?;
    for binary in [false, true] {
        let reference = fixture.0.join(format!("ref-{binary}.json"));
        let checkpoint = fixture.0.join(format!("saved-{binary}.json"));
        let first = fixture.0.join(format!("first-{binary}.json"));
        let resumed = fixture.0.join(format!("resumed-{binary}.json"));
        let (code, expected) = result(&mut command(&scene, &reference, 12, binary), &reference)?;
        assert_eq!(code, 0, "{expected}");
        let (code, first_report) = result(
            command(&scene, &first, 7, binary)
                .arg("--save-checkpoint")
                .arg(&checkpoint),
            &first,
        )?;
        assert_eq!(
            code, 2,
            "最终断言尚未满足，但合法提交状态可保存：{first_report}"
        );
        let (code, actual) = result(
            command(&scene, &resumed, 5, binary)
                .arg("--load-checkpoint")
                .arg(&checkpoint),
            &resumed,
        )?;
        assert_eq!(code, 0, "{actual}");
        assert_eq!(actual["start_time"], 8);
        assert_eq!(actual["end_time"], 13);
        assert_eq!(actual["world_ticks_committed"], 5);
        assert_eq!(
            actual["initial"], expected["initial"],
            "delta 断言沿用原始起点"
        );
        assert_eq!(actual["aliases"], expected["aliases"]);
        assert_eq!(actual["composition"], expected["composition"]);
        assert_eq!(
            actual["execution_strategy"], expected["execution_strategy"],
            "执行策略身份跨冷恢复一致"
        );
        let strategy = expected["execution_strategy"]
            .as_str()
            .expect("本场景带 Mod，策略身份必为字符串");
        assert_eq!(strategy.len(), 64, "64 位十六进制摘要");
        assert_eq!(
            without_timing_cpu(actual["final"].clone()),
            without_timing_cpu(expected["final"].clone())
        );
        let original = std::fs::read(&checkpoint)?;
        let (code, _) = result(
            command(&scene, &resumed, 1, binary)
                .arg("--load-checkpoint")
                .arg(&checkpoint)
                .arg("--save-checkpoint")
                .arg(&checkpoint),
            &resumed,
        )?;
        assert_eq!(code, 2, "续跑到第 8 tick，绝对最终断言仍未满足");
        assert_ne!(
            std::fs::read(&checkpoint)?,
            original,
            "同一路径可发布下一份完整检查点"
        );
    }
    Ok(())
}

#[test]
fn cli_rejects_input_identity_drift_and_output_collisions() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };")?;
    let scene = fixture.scene(&simple_scene())?;
    let checkpoint = fixture.0.join("checkpoint.json");
    let report = fixture.0.join("report.json");
    let (code, _) = result(
        command(&scene, &report, 2, false)
            .arg("--save-checkpoint")
            .arg(&checkpoint),
        &report,
    )?;
    assert_eq!(code, 0);
    let original = std::fs::read(&checkpoint)?;
    let output = command(&scene, &checkpoint, 2, false)
        .arg("--load-checkpoint")
        .arg(&checkpoint)
        .output()?;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(std::fs::read(&checkpoint)?, original);
    fixture.module("exports.loop = () => { Memory.changed = true; };")?;
    let (code, value) = result(
        command(&scene, &report, 2, false)
            .arg("--load-checkpoint")
            .arg(&checkpoint),
        &report,
    )?;
    assert_eq!(code, 1, "{value}");
    assert!(value["error"].as_str().unwrap().contains("玩家模块身份"));
    assert_eq!(std::fs::read(&checkpoint)?, original);
    let module = fixture.0.join("modules/save.json");
    let (code, _) = result(
        command(&scene, &report, 2, false)
            .arg("--save-checkpoint")
            .arg(&module),
        &report,
    )?;
    assert_eq!(code, 1);
    assert!(!module.exists());
    Ok(())
}

#[test]
fn checkpoint_io_failure_reports_actual_committed_world_without_replacing_prior_destination()
-> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };")?;
    let scene = fixture.scene(&simple_scene())?;
    let destination = fixture.0.join("existing-directory");
    std::fs::create_dir(&destination)?;
    std::fs::write(destination.join("keep"), "previous content")?;
    let report = fixture.0.join("io-report.json");
    let (code, report) = result(
        command(&scene, &report, 2, false)
            .arg("--save-checkpoint")
            .arg(&destination),
        &report,
    )?;
    assert_eq!(code, 1);
    assert_eq!(report["status"], "checkpoint_failed");
    assert_eq!(report["world_ticks_committed"], 2);
    assert_eq!(report["final"]["players"]["p1"]["memory"]["n"], 2);
    assert_eq!(report["failure"]["stage"], "checkpoint");
    assert_eq!(
        std::fs::read_to_string(destination.join("keep"))?,
        "previous content"
    );
    Ok(())
}
