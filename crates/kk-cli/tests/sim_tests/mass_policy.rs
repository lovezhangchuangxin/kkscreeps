//! 两种独立 JS 群体公式和 native 对照，经真实 CLI 双通道及冷恢复验收。
use super::*;

fn scene(policy: &str) -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-mass-policy")
        .join(format!("scenario-{policy}.json"))
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
fn independent_mass_formulas_and_native_control_agree_across_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = Vec::new();
    for (policy, near, far, container) in [
        ("flat", 77, 77, 77),
        ("falloff", 80, 92, 86),
        ("plain", 90, 100, 96),
    ] {
        let path = scene(policy)?;
        let mut reports = Vec::new();
        for binary in [false, true] {
            let (code, report) = run(
                &path,
                &fixture.0.join(format!("{policy}-{binary}.json")),
                3,
                binary,
                1,
            )?;
            assert_eq!(code, 0, "{report}");
            let memory = &report["final"]["players"]["p1"]["memory"];
            assert_eq!(memory["code"], 0);
            assert_eq!(memory["nearHits"], near);
            assert_eq!(memory["farHits"], far);
            assert_eq!(memory["containerHits"], container);
            assert_eq!(memory["allyHits"], 100);
            assert_eq!(memory["rampartHits"], 1000);
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
fn cold_resume_preserves_all_batch_targets_namespace_state_and_composition() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    for policy in ["flat", "falloff"] {
        let path = scene(policy)?;
        for binary in [false, true] {
            let prefix = format!("{policy}-{binary}");
            let (code, expected) = run(
                &path,
                &fixture.0.join(format!("{prefix}-reference.json")),
                3,
                binary,
                1,
            )?;
            assert_eq!(code, 0, "{expected}");
            let checkpoint = fixture.0.join(format!("{prefix}-checkpoint.json"));
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
                invoke(
                    "1",
                    &fixture.0.join(format!("{prefix}-first.json")),
                    "--save-checkpoint"
                )?,
                2,
                "Memory 尚未观察结算后的伤害"
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
