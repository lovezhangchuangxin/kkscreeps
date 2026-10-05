//! 两种 JS 治疗公式与原生对照，验证双通道、身体耐久与进程冷恢复。
use super::*;

fn scene(policy: &str, action: &str) -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-heal-policy")
        .join(format!("scenario-{policy}-{action}.json"))
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
fn independent_heal_formulas_and_native_control_agree_across_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = Vec::new();
    for (policy, melee, ranged) in [
        ("flat", 163, 163),
        ("scaled", 170, 170),
        ("plain", 152, 144),
    ] {
        let mut action_hashes = Vec::new();
        for (action, expected) in [("heal", melee), ("rangedHeal", ranged)] {
            let path = scene(policy, action)?;
            let mut reports = Vec::new();
            for binary in [false, true] {
                let (code, report) = run(
                    &path,
                    &fixture.0.join(format!("{policy}-{action}-{binary}.json")),
                    4,
                    binary,
                    1,
                )?;
                assert_eq!(code, 0, "{report}");
                let memory = &report["final"]["players"]["p1"]["memory"];
                assert_eq!(memory["attackCode"], 0);
                assert_eq!(memory["healCode"], 0);
                assert_eq!(memory["targetHits"], expected);
                assert_eq!(memory["targetBodyHits"], json!([expected - 100, 100]));
                reports.push(report);
            }
            assert_eq!(stable_final(&reports[0]), stable_final(&reports[1]));
            assert_eq!(reports[0]["plan_hash"], reports[1]["plan_hash"]);
            action_hashes.push(reports[0]["plan_hash"].clone());
        }
        assert_eq!(
            action_hashes[0], action_hashes[1],
            "场景动作不改变规则包身份"
        );
        hashes.push(action_hashes[0].clone());
    }
    assert_ne!(hashes[0], hashes[1]);
    assert_ne!(hashes[0], hashes[2]);
    Ok(())
}

#[test]
fn cold_resume_before_and_after_heal_preserves_body_state_and_composition() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    for policy in ["flat", "scaled"] {
        for action in ["heal", "rangedHeal"] {
            let path = scene(policy, action)?;
            for binary in [false, true] {
                let prefix = format!("{policy}-{action}-{binary}");
                let (code, expected) = run(
                    &path,
                    &fixture.0.join(format!("{prefix}-reference.json")),
                    4,
                    binary,
                    1,
                )?;
                assert_eq!(code, 0, "{expected}");
                // split=1 保留受伤身体再执行治疗；split=2 保留治疗及 Mod state，
                // 续跑不得再次调用公式。每次 invoke 都是真实独立 CLI 进程。
                for split in [1, 2] {
                    let checkpoint = fixture.0.join(format!("{prefix}-{split}-checkpoint.json"));
                    let resumed = fixture.0.join(format!("{prefix}-{split}-resumed.json"));
                    let invoke = |ticks: u32, report: &Path, option: &str| -> anyhow::Result<i32> {
                        let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
                        command
                            .args(["sim", "run", "--scenario"])
                            .arg(&path)
                            .arg("--ticks")
                            .arg(ticks.to_string())
                            .arg("--report")
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
                            split,
                            &fixture.0.join(format!("{prefix}-{split}-first.json")),
                            "--save-checkpoint"
                        )?,
                        2,
                        "Memory 尚未观察最终治疗"
                    );
                    assert_eq!(invoke(4 - split, &resumed, "--load-checkpoint")?, 0);
                    let actual: Value = serde_json::from_slice(&std::fs::read(resumed)?)?;
                    assert_eq!(stable_final(&actual), stable_final(&expected));
                    assert_eq!(actual["plan_hash"], expected["plan_hash"]);
                    assert_eq!(actual["composition"], expected["composition"]);
                }
            }
        }
    }
    Ok(())
}
