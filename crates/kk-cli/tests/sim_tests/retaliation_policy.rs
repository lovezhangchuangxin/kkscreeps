//! 两种独立 JS 反击公式与原生对照，经真实 CLI 双通道和冷恢复验收。
use super::*;

fn scene(name: &str) -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-retaliation-policy")
        .join(name)
        .canonicalize()?)
}

fn stable_final(report: &Value) -> Value {
    let mut value = report["final"].clone();
    for player in value["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    value
}

#[test]
fn independent_js_retaliation_formulas_keep_native_primary_damage_across_channels()
-> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = Vec::new();
    for (name, expected_target, expected_actor) in [
        ("scenario-flat.json", 170, 193),
        ("scenario-thorns.json", 170, 194),
        ("scenario-plain.json", 170, 170),
    ] {
        let path = scene(name)?;
        let mut reports = Vec::new();
        for binary in [false, true] {
            let (code, report) = run(
                &path,
                &fixture.0.join(format!("{name}-{binary}.json")),
                3,
                binary,
                1,
            )?;
            assert_eq!(code, 0, "{report}");
            let memory = &report["final"]["players"]["p1"]["memory"];
            assert_eq!(memory["code"], 0);
            assert_eq!(memory["target_hits"], expected_target);
            assert_eq!(memory["actor_hits"], expected_actor);
            reports.push(report);
        }
        assert_eq!(stable_final(&reports[0]), stable_final(&reports[1]));
        assert_eq!(reports[0]["plan_hash"], reports[1]["plan_hash"]);
        hashes.push(reports[0]["plan_hash"].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
    Ok(())
}

#[test]
fn cold_resume_preserves_retaliation_damage_and_namespace_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    for name in ["scenario-flat.json", "scenario-thorns.json"] {
        let path = scene(name)?;
        for binary in [false, true] {
            let prefix = format!("{name}-{binary}.json");
            let (code, expected) = run(
                &path,
                &fixture.0.join(format!("{prefix}-reference.json")),
                3,
                binary,
                1,
            )?;
            assert_eq!(code, 0, "{expected}");
            let checkpoint = fixture.0.join(format!("{prefix}-checkpoint.json"));
            let first = fixture.0.join(format!("{prefix}-first.json"));
            let resumed = fixture.0.join(format!("{prefix}-resumed.json"));
            let invoke = |ticks: &str, report: &Path, option: &str| -> anyhow::Result<i32> {
                let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
                command
                    .args(["sim", "run", "--scenario"])
                    .arg(&path)
                    .args(["--ticks", ticks, "--report"])
                    .arg(report)
                    .arg(option)
                    .arg(&checkpoint);
                if binary {
                    command.arg("--kkpv");
                }
                Ok(command.output()?.status.code().unwrap())
            };
            assert_eq!(
                invoke("1", &first, "--save-checkpoint")?,
                2,
                "观测 Memory 尚未反映结算"
            );
            assert_eq!(invoke("2", &resumed, "--load-checkpoint")?, 0);
            let actual: Value = serde_json::from_slice(&std::fs::read(resumed)?)?;
            assert_eq!(stable_final(&actual), stable_final(&expected));
            assert_eq!(actual["plan_hash"], expected["plan_hash"]);
            assert_eq!(actual["composition"], expected["composition"]);
        }
    }
    Ok(())
}
