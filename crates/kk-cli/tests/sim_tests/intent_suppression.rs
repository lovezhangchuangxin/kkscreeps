//! G07.b ext 意图压制：suppress/free 独立包、四场景期望表、双通道
//! 一致性与压制声明后的检查点冷恢复。语义见夹具 README。
use super::*;

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn intent_suppression_packages_compose_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    // both/alone/invalid 共用 suppress 包（同一计划身份）；free 单选与
    // suppress+free 组合各自独立身份——共三个计划身份。
    for name in ["both", "alone", "invalid", "free", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-intent-suppression")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let mut references = Vec::new();
        for binary in [false, true] {
            let prefix = format!("{name}-{binary}");
            let (code, expected) = run(
                &scene,
                &fixture.0.join(format!("{prefix}-reference.json")),
                4,
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
                    .arg(&scene)
                    .args(["--ticks", ticks, "--report"])
                    .arg(report)
                    .arg(option)
                    .arg(&checkpoint);
                if binary {
                    command.arg("--kkpv");
                }
                Ok(command.output()?.status.code().unwrap())
            };
            // 期望值每 tick 结算后稳定：2 tick 保存退出 0，续跑 2 tick
            // 与直跑 4 tick 等值——恢复后压制语义仍在（描述符在计划身份内）。
            assert_eq!(
                invoke(
                    "2",
                    &fixture.0.join(format!("{prefix}-first.json")),
                    "--save-checkpoint"
                )?,
                0,
                "压制期望值自首 tick 起稳定"
            );
            assert_eq!(invoke("2", &resumed, "--load-checkpoint")?, 0);
            let actual: Value = serde_json::from_slice(&std::fs::read(resumed)?)?;
            assert_eq!(stable_final(&actual), stable_final(&expected));
            assert_eq!(actual["plan_hash"], expected["plan_hash"]);
            assert_eq!(actual["composition"], expected["composition"]);
            references.push(expected);
        }
        assert_eq!(stable_final(&references[0]), stable_final(&references[1]));
        assert_eq!(references[0]["plan_hash"], references[1]["plan_hash"]);
        hashes.insert(references[0]["plan_hash"].as_str().unwrap().to_string());
    }
    assert_eq!(
        hashes.len(),
        3,
        "suppress 三场景同一身份，free 单选与组合各独立"
    );
    Ok(())
}

#[test]
fn intent_suppression_expected_values_per_scenario() -> anyhow::Result<()> {
    // both=3（mark 被 bump 压制）；alone=7（压制者缺席则执行）；
    // invalid=0（bump 越界被丢弃但键存在，压制只看存在性）；
    // free=7（无压制：键序先置 3 后置 7）。期望值结算后每 tick 稳定。
    let fixture = Fixture::new()?;
    let expected = [
        ("both", "counter", 3),
        ("alone", "counter", 7),
        ("invalid", "counter", 0),
        ("free", "counter", 7),
        // 组合选择：两类型各一对象，压制按包内声明各自生效。
        ("combined", "counterA", 3),
        ("combined", "counterB", 7),
    ];
    for (name, alias, value) in expected {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-intent-suppression")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("{name}-values.json")),
            2,
            false,
            1,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(
            report["final"]["objects"][alias]["fields"]["value"], value,
            "场景 {name} 对象 {alias} 的压制语义期望"
        );
    }
    Ok(())
}
