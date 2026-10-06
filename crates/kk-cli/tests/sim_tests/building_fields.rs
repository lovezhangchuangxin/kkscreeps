//! G08.a 建筑字段与生命周期：gauge/beacon 独立策略包、原生对照、组合、
//! 双投影通道一致性与字段终态后的检查点冷恢复。语义见夹具 README。
use super::*;

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn building_fields_packages_compose_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    for name in ["plain", "gauge", "beacon", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-building-fields")
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
            // 第 2 tick：time≥3 记录门未过，memory 为空，指针缺失
            // 使 eq 断言失败（退出码 2）。字段终值随房间快照进检查点，
            // 恢复续跑后与连续运行等价。
            assert_eq!(
                invoke(
                    "2",
                    &fixture.0.join(format!("{prefix}-first.json")),
                    "--save-checkpoint"
                )?,
                2,
                "断言尚未满足"
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
fn building_fields_surface_and_world_terminal_values() -> anyhow::Result<()> {
    // gauge：末轮玩家读到 3 轮结算（heat/cycles=3、energy=15、绑定类名），
    // 世界终态含第 4 轮；beacon：3 轮翻转 signal=1；组合两者同时成立；
    // 原生对照无 mod 结构。
    let fixture = Fixture::new()?;
    for name in ["plain", "gauge", "beacon", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-building-fields")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("{name}-surface.json")),
            4,
            false,
            1,
        )?;
        assert_eq!(code, 0, "{report}");
        let memory = &report["final"]["players"]["p1"]["memory"];
        let objects = &report["final"]["objects"];
        if name == "plain" {
            assert_eq!(memory["hasGauge"], false, "{name}");
            assert_eq!(memory["hasBeacon"], false, "{name}");
            continue;
        }
        if name != "beacon" {
            assert_eq!(memory["gauge"]["heat"], 3, "{name}：玩家末轮 3 轮结算");
            assert_eq!(memory["gauge"]["cycles"], 3, "{name}");
            assert_eq!(memory["gauge"]["energy"], 15, "{name}：先产后 tick");
            assert_eq!(memory["gauge"]["cls"], "Gauge", "{name}：玩家类绑定");
            assert_eq!(objects["gauge"]["bl:heat"], 4, "{name}：世界终态第 4 轮");
            assert_eq!(objects["gauge"]["store"]["energy"], 20, "{name}");
        }
        if name != "gauge" {
            assert_eq!(memory["beacon"]["signal"], 1, "{name}：3 轮翻转");
            assert_eq!(memory["beacon"]["flashes"], 3, "{name}");
            assert_eq!(memory["beacon"]["cls"], "Beacon", "{name}：玩家类绑定");
            assert_eq!(objects["beacon"]["bk:flashes"], 4, "{name}：世界终态");
        }
    }
    Ok(())
}
