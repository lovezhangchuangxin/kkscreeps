//! 经真实 CLI 验证场景、模块、状态提交和失败退出码。
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-sim-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("modules"))?;
        Ok(Self(path))
    }
    fn scene(&self, value: &Value) -> anyhow::Result<PathBuf> {
        let path = self.0.join("scenario.json");
        std::fs::write(&path, serde_json::to_vec_pretty(value)?)?;
        Ok(path)
    }
    fn module(&self, code: &str) -> anyhow::Result<()> {
        std::fs::write(self.0.join("modules/main.js"), code)?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(
    scene: &Path,
    report: &Path,
    ticks: u32,
    binary: bool,
    concurrency: usize,
) -> anyhow::Result<(i32, Value)> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    // 模块相对场景文件解析，不得依赖调用者当前目录。
    command
        .current_dir(std::env::temp_dir())
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string())
        .arg("--concurrency")
        .arg(concurrency.to_string());
    if binary {
        command.arg("--kkpv");
    }
    let output = command.output()?;
    let code = output.status.code().expect("子进程正常退出");
    let value: Value = serde_json::from_slice(&std::fs::read(report).unwrap_or_default())
        .unwrap_or_else(|_| json!({"stderr": String::from_utf8_lossy(&output.stderr).to_string()}));
    Ok((code, value))
}

fn simple_scene() -> Value {
    json!({"version": 1, "shard": {"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1"}]},
        "bots": [{"player": "p1", "modules": "modules"}], "objects": [], "assertions": []})
}

#[test]
fn economy_bot_moves_renews_colony_and_upgrades_in_both_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-economy/scenario.json")
        .canonicalize()?;
    let mut results = Vec::new();
    for binary in [false, true] {
        let report = fixture.0.join(format!("economy-{binary}.json"));
        let (code, report) = run(&scene, &report, 500, binary, 2)?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["world_ticks_committed"], 500);
        assert_eq!(report["players"]["bot"]["successful_ticks"], 500);
        assert!(
            report["assertions"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["passed"] == true)
        );
        assert_eq!(report["final"]["players"]["bot"]["memory"]["loops"], 500);
        assert!(
            report["final"]["players"]["bot"]["memory"]["spawned"]
                .as_i64()
                .unwrap()
                >= 2
        );
        assert_eq!(
            report["modules"]["bot"]["files"].as_array().unwrap().len(),
            2
        );
        results.push(report);
    }
    assert_eq!(results[0]["final"]["world"], results[1]["final"]["world"]);
    assert_eq!(
        results[0]["final"]["players"]["bot"]["memory"],
        results[1]["final"]["players"]["bot"]["memory"]
    );
    assert_eq!(results[0]["scenario_sha256"], results[1]["scenario_sha256"]);
    assert_eq!(
        results[0]["executable_sha256"],
        results[1]["executable_sha256"]
    );
    Ok(())
}

#[test]
fn two_players_load_cjs_and_seed_memory_without_hidden_setup_ticks() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = function () { if (!Game.rooms[Memory.room] || Object.keys(Game.rooms).length !== 1) throw new Error('wrong visibility'); Memory.n += require('helper').delta; };")?;
    std::fs::write(fixture.0.join("modules/helper.js"), "exports.delta = 2;")?;
    let scene = fixture.scene(&json!({"version": 1,
        "shard": {"players": ["p1", "p2"], "rooms": [{"name": "W1N1", "owner": "p1"}, {"name": "W2N2", "owner": "p2"}]},
        "bots": [{"player": "p1", "modules": "modules", "memory": {"n": 1, "room": "W1N1"}}, {"player": "p2", "modules": "modules", "memory": {"n": 10, "room": "W2N2"}}],
        "assertions": [{"pointer": "/players/p1/memory/n", "op": "delta_gte", "value": 8}, {"pointer": "/players/p2/memory/n", "op": "eq", "value": 18}]
    }))?;
    for concurrency in [1, 2] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("players-{concurrency}.json")),
            4,
            false,
            concurrency,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["initial"]["time"], 0);
        assert_eq!(report["final"]["time"], 4);
        assert_eq!(report["final"]["players"]["p1"]["memory"]["n"], 9);
        assert_eq!(
            report["modules"]["p1"]["sha256"],
            report["modules"]["p2"]["sha256"]
        );
    }
    Ok(())
}

#[test]
fn player_errors_missing_loop_and_cpu_termination_stop_with_committed_state() -> anyhow::Result<()>
{
    let fixture = Fixture::new()?;
    let scene = fixture.scene(&simple_scene())?;
    for (name, code, expected_kind) in [
        (
            "throw",
            "exports.loop = () => { Memory.ran = true; throw new Error('intentional'); };",
            "runtime",
        ),
        ("missing-loop", "exports.other = () => {};", "runtime"),
        (
            "terminated",
            "exports.loop = () => { while (true) {} };",
            "runtime",
        ),
    ] {
        fixture.module(code)?;
        let (code, report) = run(&scene, &fixture.0.join(format!("{name}.json")), 4, false, 2)?;
        assert_eq!(code, 2, "{report}");
        assert_eq!(report["status"], "run_failed");
        assert_eq!(report["world_ticks_committed"], 1);
        assert_eq!(report["ticks_attempted"], 1);
        assert_eq!(report["players"]["p1"]["successful_ticks"], 0);
        assert_eq!(
            report["players"]["p1"]["errors_head"][0]["result"]["kind"],
            expected_kind
        );
        if name == "throw" {
            assert_eq!(report["final"]["players"]["p1"]["memory"]["ran"], true);
        }
        if name == "terminated" {
            assert!(
                report["players"]["p1"]["errors_head"][0]["result"]["error"]
                    .as_str()
                    .unwrap()
                    .contains("硬中断")
            );
        }
    }
    Ok(())
}

#[test]
fn assertions_and_invalid_scenes_fail_without_stale_success_or_input_overwrite()
-> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };")?;
    let report_path = fixture.0.join("result.json");
    let mut value = simple_scene();
    let scene = fixture.scene(&value)?;
    let (code, _) = run(&scene, &report_path, 2, false, 2)?;
    assert_eq!(code, 0);
    value["assertions"] = json!([
        {"pointer": "/players/p1/memory/n", "op": "eq", "value": 3},
        {"pointer": "/players/p1/memory/absent", "op": "delta_gte", "value": 0}
    ]);
    fixture.scene(&value)?;
    let (code, report) = run(&scene, &report_path, 2, false, 2)?;
    assert_eq!(code, 2);
    assert_eq!(report["status"], "assertion_failed");
    assert!(
        report["assertions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["passed"] == false)
    );
    for bad in [
        json!({"version": 2, "shard": value["shard"], "bots": value["bots"]}),
        json!({"version": 1, "shard": value["shard"], "bots": []}),
        json!({"version": 1, "shard": value["shard"], "bots": value["bots"], "assertions": [{"pointer": "/bad~2", "op": "exists"}]}),
        json!({"version": 1, "shard": value["shard"], "bots": value["bots"], "objects": [{"alias": "bad", "room": "W1N1", "kind": "creep", "spec": {"pos": [50, 2]}}]}),
    ] {
        fixture.scene(&bad)?;
        let (code, report) = run(&scene, &report_path, 2, false, 2)?;
        assert_eq!(code, 1, "{report}");
        assert_eq!(report["status"], "setup_failed");
        assert_eq!(report["world_ticks_committed"], 0);
    }
    let original = std::fs::read(&scene)?;
    let (code, _) = run(&scene, &scene, 2, false, 2)?;
    assert_eq!(code, 1);
    assert_eq!(std::fs::read(&scene)?, original);
    let source = fixture.0.join("modules/main.js");
    let original = std::fs::read(&source)?;
    let (code, _) = run(&scene, &source, 2, false, 2)?;
    assert_eq!(code, 1);
    assert_eq!(std::fs::read(source)?, original);
    Ok(())
}

#[path = "sim_tests/mods.rs"]
mod mods;
#[path = "sim_tests/object_ops.rs"]
mod object_ops;

#[path = "sim_tests/composition.rs"]
mod composition;

#[path = "sim_tests/rules.rs"]
mod rules;

#[path = "sim_tests/building_fields.rs"]
mod building_fields;
#[path = "sim_tests/buildings.rs"]
mod buildings;
#[path = "sim_tests/checkpoint.rs"]
mod checkpoint;
#[path = "sim_tests/damage_policy.rs"]
mod damage_policy;
#[path = "sim_tests/heal_policy.rs"]
mod heal_policy;
#[path = "sim_tests/mass_policy.rs"]
mod mass_policy;

#[path = "sim_tests/combat_ranges.rs"]
mod combat_ranges;
#[path = "sim_tests/determinism.rs"]
mod determinism;
#[path = "sim_tests/incoming_policy.rs"]
mod incoming_policy;
#[path = "sim_tests/input_log.rs"]
mod input_log;
#[path = "sim_tests/mod_trace.rs"]
mod mod_trace;
#[path = "sim_tests/move_admission.rs"]
mod move_admission;
#[path = "sim_tests/move_arbitration.rs"]
mod move_arbitration;
#[path = "sim_tests/move_cost.rs"]
mod move_cost;
#[path = "sim_tests/move_obstacle.rs"]
mod move_obstacle;
#[path = "sim_tests/move_policy.rs"]
mod move_policy;
#[path = "sim_tests/move_precheck.rs"]
mod move_precheck;
#[path = "sim_tests/move_pull_registration.rs"]
mod move_pull_registration;
#[path = "sim_tests/retaliation_policy.rs"]
mod retaliation_policy;

#[path = "sim_tests/switch.rs"]
mod switch;
#[path = "sim_tests/tough_damage.rs"]
mod tough_damage;
