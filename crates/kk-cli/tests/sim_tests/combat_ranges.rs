//! G07.a 战斗射程：reach/medic 独立规则包、原生对照、组合、双通道
//! 一致性与射程规则后的检查点冷恢复。语义见夹具 README。
use super::*;

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn combat_ranges_packages_compose_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    for name in ["plain", "reach", "medic", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-combat-ranges")
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
            // 第 2 tick 完成两轮攻击与射程门观察；time>=3 的记录值
            // （foeHits）与 reach/combined 的 foe 死亡断言尚未满足，
            // 保存退出码为 2。规则包无处理器与状态提交，无模块计数面。
            assert_eq!(
                invoke(
                    "2",
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
fn combat_ranges_overrides_gate_expected_reach() -> anyhow::Result<()> {
    // reach 只放宽 attack（近战 1→3）：距离 2 命中、3 轮打掉 90；
    // medic 只放宽治疗族（heal 1→3、rangedHeal 3→5）：距离 2/4 的门
    // 放行而攻击仍拒；组合两者同时生效；原生对照全部拒绝且存活。
    let fixture = Fixture::new()?;
    let expected = [
        ("plain", -9, -9, -9, "1,1,3", json!(100), false),
        ("reach", 0, -9, -9, "3,1,3", json!(10), true),
        ("medic", -9, 0, 0, "1,3,5", json!(100), false),
        ("combined", 0, 0, 0, "3,3,5", json!(10), true),
    ];
    for (name, attack, heal, ranged, ranges, foe_hits, foe_dead) in expected {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-combat-ranges")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("{name}-gates.json")),
            4,
            false,
            1,
        )?;
        assert_eq!(code, 0, "{report}");
        let memory = &report["final"]["players"]["p1"]["memory"];
        assert_eq!(memory["codes"]["attack"], attack, "场景 {name}：attack 门");
        assert_eq!(memory["codes"]["heal"], heal, "场景 {name}：heal 门");
        assert_eq!(
            memory["codes"]["rangedHeal"], ranged,
            "场景 {name}：rangedHeal 门"
        );
        assert_eq!(memory["ranges"], ranges, "场景 {name}：常量注入");
        assert_eq!(
            memory["record"]["foeHits"], foe_hits,
            "场景 {name}：3 轮结算后的 foe 血量"
        );
        // 击杀只发生在放宽 attack 的场景（第 4 轮结算后）。
        let foe_alive = !report["final"]["objects"]["foe"].is_null();
        assert_eq!(foe_alive, !foe_dead, "场景 {name}：foe 存活状态");
    }
    Ok(())
}
