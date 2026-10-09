//! 持续世界 NPC 战斗的双通道、跨进程冷恢复与完整状态输入回放。
use super::*;

fn stage(
    scene: &Path,
    root: &Path,
    name: &str,
    binary: bool,
    ticks: u32,
    log: &Path,
    checkpoint_paths: (Option<&Path>, Option<&Path>),
) -> anyhow::Result<Value> {
    let report = root.join(format!("{name}.json"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--ticks")
        .arg(ticks.to_string())
        .arg("--report")
        .arg(&report)
        .arg("--input-log")
        .arg(log)
        .arg("--verify-replay");
    if binary {
        command.arg("--kkpv");
    }
    let (save, load) = checkpoint_paths;
    if let Some(path) = save {
        command.arg("--save-checkpoint").arg(path);
    }
    if let Some(path) = load {
        command.arg("--load-checkpoint").arg(path);
    }
    let output = command.output()?;
    let value: Value = serde_json::from_slice(&std::fs::read(report)?)?;
    assert!(
        output.status.success(),
        "{name}: {} / {value}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(value["status"], "passed");
    assert_eq!(value["world_ticks_committed"], ticks);
    assert_eq!(value["players"]["bot"]["successful_ticks"], ticks);
    assert_eq!(value["verify_replay"]["status"], "passed");
    assert_eq!(value["verify_replay"]["segments"], ticks);
    assert_eq!(value["input_log"]["validation"], "ok");
    Ok(value)
}

#[test]
fn invader_battle_actual_damage_healing_movement_replay_and_cold_resume_match() -> anyhow::Result<()>
{
    let fixture = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-invader-battle/scenario.json")
        .canonicalize()?;
    let mut finals = Vec::new();
    for binary in [false, true] {
        let log = fixture.0.join(format!("straight-{binary}.jsonl"));
        let straight = stage(
            &scene,
            &fixture.0,
            &format!("straight-{binary}"),
            binary,
            200,
            &log,
            (None, None),
        )?;
        let log = fixture.0.join(format!("split-{binary}.jsonl"));
        let checkpoint = fixture.0.join(format!("split-{binary}.checkpoint.json"));
        let prefix = stage(
            &scene,
            &fixture.0,
            &format!("prefix-{binary}"),
            binary,
            100,
            &log,
            (Some(&checkpoint), None),
        )?;
        assert_eq!(prefix["input_log"]["segments"], 100);
        let saved: Value = serde_json::from_slice(&std::fs::read(&checkpoint)?)?;
        let payload: Value = serde_json::from_str(saved["payload"].as_str().unwrap())?;
        assert_eq!(payload["checkpoint_format_version"], 7);
        assert!(
            !payload["engine"]["world"]["rooms"]["W1N1"]["invader_memory"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        let resumed = stage(
            &scene,
            &fixture.0,
            &format!("resumed-{binary}"),
            binary,
            100,
            &log,
            (None, Some(&checkpoint)),
        )?;
        assert_eq!(resumed["input_log"]["segments"], 200);
        assert_eq!(resumed["start_time"], prefix["end_time"]);
        assert_eq!(straight["final"]["world"], resumed["final"]["world"]);
        assert_eq!(
            straight["final"]["players"]["bot"]["memory"],
            resumed["final"]["players"]["bot"]["memory"]
        );
        let stats = &resumed["final"]["players"]["bot"]["memory"]["stats"];
        for (key, min) in [
            ("npcMelee", 30),
            ("npcRanged", 10),
            ("npcHealed", 12),
            ("npcRecovered", 100),
            ("guardInjured", 100),
            ("npcMoved", 1),
        ] {
            assert!(stats[key].as_u64().unwrap() >= min, "{key}");
        }
        assert_eq!(stats["eventsThroughTick"], 199);
        finals.push(resumed);
    }
    assert_eq!(finals[0]["final"]["world"], finals[1]["final"]["world"]);
    assert_eq!(
        finals[0]["final"]["players"]["bot"]["memory"],
        finals[1]["final"]["players"]["bot"]["memory"]
    );
    Ok(())
}
