//! 独立 seniority/heaviest 包及原生对照，验证组合、双通道和竞争后的冷恢复。
use super::*;

// 两个独立测试只共享同一场景的只读基准报告；保存/恢复仍各起真实新进程。
// OnceLock 的作用域仅是当前测试进程，不跨构建缓存身份或世界状态。
fn reference_reports()
-> anyhow::Result<&'static std::collections::BTreeMap<&'static str, [Value; 2]>> {
    type Reports = std::collections::BTreeMap<&'static str, [Value; 2]>;
    static REPORTS: std::sync::OnceLock<Result<Reports, String>> = std::sync::OnceLock::new();
    REPORTS
        .get_or_init(|| {
            let collect = || -> anyhow::Result<Reports> {
                let fixture = Fixture::new()?;
                let mut reports = Reports::new();
                for name in ["plain", "seniority", "heaviest", "combined"] {
                    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../compat/fixtures/sim-mod-move-policy")
                        .join(format!("scenario-{name}.json"))
                        .canonicalize()?;
                    let mut channels = Vec::new();
                    for binary in [false, true] {
                        let (code, report) = run(
                            &scene,
                            &fixture.0.join(format!("{name}-{binary}.json")),
                            3,
                            binary,
                            1,
                        )?;
                        anyhow::ensure!(code == 0, "基准场景 {name}/{binary} 失败：{report}");
                        channels.push(report);
                    }
                    reports.insert(name, channels.try_into().expect("恰好两个投影通道"));
                }
                Ok(reports)
            };
            collect().map_err(|error| format!("{error:#}"))
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn move_contest_packages_compose_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    for name in ["plain", "seniority", "heaviest", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-move-policy")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let mut references = Vec::new();
        for binary in [false, true] {
            let prefix = format!("{name}-{binary}");
            let expected = &reference_reports()?[name][usize::from(binary)];
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
            // 第 0 tick 完成竞争并位移、namespace 计数恰一次；观察值
            // （winner/positions）尚未写入，保存退出码为 2。
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
            assert_eq!(stable_final(&actual), stable_final(expected));
            assert_eq!(actual["plan_hash"], expected["plan_hash"]);
            assert_eq!(actual["composition"], expected["composition"]);
            references.push(expected);
        }
        assert_eq!(stable_final(references[0]), stable_final(references[1]));
        assert_eq!(references[0]["plan_hash"], references[1]["plan_hash"]);
        hashes.insert(references[0]["plan_hash"].as_str().unwrap().to_string());
    }
    assert_eq!(hashes.len(), 4, "内容选择和组合身份分别冻结");
    Ok(())
}

#[test]
fn move_contest_strategy_picks_expected_winners() -> anyhow::Result<()> {
    // 同夹具不同场景：三种策略产生三种胜者，组合由 after 层压回最重者。
    let winners = [
        ("plain", "light"),
        ("seniority", "mid"),
        ("heaviest", "heavy"),
        ("combined", "heavy"),
    ];
    for (name, winner) in winners {
        let report = &reference_reports()?[name][0];
        assert_eq!(
            report["final"]["players"]["p1"]["memory"]["winner"], winner,
            "场景 {name}"
        );
    }
    Ok(())
}
