//! G03.b 拉拽登记策略：anchor/leash 独立包、原生对照、组合、双通道
//! 一致性与登记槽后检查点冷恢复。位置语义见夹具 README。
use super::*;

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn move_pull_packages_compose_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    for name in ["plain", "anchor", "leash", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-move-pull-registration")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let mut references = Vec::new();
        for binary in [false, true] {
            let prefix = format!("{name}-{binary}");
            let (code, expected) = run(
                &scene,
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
            // 第 0 tick 完成拉拽登记否决与位移、两 namespace 恰计一次；
            // 位置观察值尚未写入，保存退出码为 2。
            assert_eq!(
                invoke(
                    "1",
                    &fixture.0.join(format!("{prefix}-first.json")),
                    "--save-checkpoint"
                )?,
                2,
                "观测尚未更新"
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
    assert_eq!(hashes.len(), 4, "内容选择和组合身份分别冻结");
    Ok(())
}

#[test]
fn move_pull_registration_gates_expected_pulled_movers() -> anyhow::Result<()> {
    // anchor 全量否决（cart/victim 原地）；leash 只否决跨主人配对
    // （cart 跟走、victim 原地）；组合两者并集；原生对照两组都跟走。
    let fixture = Fixture::new()?;
    let expected = [
        ("plain", "cart", json!([10, 10])),
        ("plain", "victim", json!([11, 10])),
        ("anchor", "cart", json!([10, 11])),
        ("anchor", "victim", json!([12, 10])),
        ("leash", "cart", json!([10, 10])),
        ("leash", "victim", json!([12, 10])),
        ("combined", "cart", json!([10, 11])),
        ("combined", "victim", json!([12, 10])),
    ];
    for (name, alias, value) in expected {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-move-pull-registration")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("{name}-gates.json")),
            3,
            false,
            1,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(
            report["final"]["players"]["p1"]["memory"]["positions"][alias], value,
            "场景 {name}"
        );
        // 拉者移动不受否决影响。
        assert_eq!(
            report["final"]["players"]["p1"]["memory"]["positions"]["hauler"],
            json!([10, 9]),
            "场景 {name}"
        );
        assert_eq!(
            report["final"]["players"]["p1"]["memory"]["positions"]["thief"],
            json!([12, 9]),
            "场景 {name}"
        );
        // 组合场景双 namespace 恰计一次，其余场景单计数；未选包投影为 null。
        let states = report["final"]["mod_states"]
            .as_object()
            .map(|m| m.values().filter(|v| !v.is_null()).count());
        assert_eq!(
            states.unwrap_or(0),
            if name == "combined" {
                2
            } else if name == "plain" {
                0
            } else {
                1
            },
            "场景 {name} 的 namespace 计数"
        );
    }
    Ok(())
}
