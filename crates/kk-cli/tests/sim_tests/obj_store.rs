//! G09.a 自有对象库存：两独立包经公共 SDK 声明对象库存容量，creep
//! transfer/withdraw 预检与结算同源；场景断言核对信封读值、容量门、
//! 无容量对照拒绝与 mod 侧 tick 读值，Rust 侧补双通道一致、组合身份
//! 与冷恢复。
use super::*;

const NAMES: [&str; 3] = ["hauler", "vault", "combined"];

fn scene_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-objstore")
        .join(format!("scenario-{name}.json"))
        .canonicalize()?)
}

fn reference_reports()
-> anyhow::Result<&'static std::collections::BTreeMap<&'static str, [Value; 2]>> {
    type Reports = std::collections::BTreeMap<&'static str, [Value; 2]>;
    static REPORTS: std::sync::OnceLock<Result<Reports, String>> = std::sync::OnceLock::new();
    REPORTS
        .get_or_init(|| {
            let collect = || -> anyhow::Result<Reports> {
                let fixture = Fixture::new()?;
                let mut reports = Reports::new();
                for name in NAMES {
                    let scene = scene_path(name)?;
                    let mut channels = Vec::new();
                    for binary in [false, true] {
                        let (code, report) = run(
                            &scene,
                            &fixture.0.join(format!("objstore-{name}-{binary}.json")),
                            4,
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

#[test]
fn object_store_scenarios_hold_caps_across_channels() -> anyhow::Result<()> {
    for name in NAMES {
        let channels = &reference_reports()?[name];
        for (binary, report) in [false, true].iter().zip(channels) {
            anyhow::ensure!(
                report["assertions"]
                    .as_array()
                    .is_some_and(|list| list.iter().all(|a| a["passed"] == true)),
                "场景 {name}/{binary} 断言未全过：{}",
                report["assertions"]
            );
        }
        assert_eq!(channels[0]["final"]["world"], channels[1]["final"]["world"]);
        assert_eq!(
            channels[0]["final"]["players"]["p1"]["memory"],
            channels[1]["final"]["players"]["p1"]["memory"]
        );
    }
    // 对象 tick 经 ctx.target.fields 读到同一库存信封（结算侧读值入 state）。
    let hauler = &reference_reports()?["hauler"][0];
    assert_eq!(
        hauler["final"]["mod_states"]["objstore-hauler"]["energy"],
        250
    );
    let combined = &reference_reports()?["combined"][0];
    assert_eq!(
        combined["final"]["mod_states"]["objstore-hauler"]["energy"],
        250
    );
    assert_eq!(
        combined["final"]["mod_states"]["objstore-vault"]["energy"],
        100
    );
    // 内容选择冻结为不同计划身份。
    let mut hashes = std::collections::BTreeSet::new();
    for name in NAMES {
        hashes.insert(
            reference_reports()?[name][0]["plan_hash"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    assert_eq!(hashes.len(), 3, "覆盖组合分别冻结");
    Ok(())
}

#[test]
fn object_store_combined_scenario_resumes_with_same_identity() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = scene_path("combined")?;
    for binary in [false, true] {
        let expected = &reference_reports()?["combined"][usize::from(binary)];
        let checkpoint = fixture.0.join(format!("objstore-{binary}-checkpoint.json"));
        let resumed = fixture.0.join(format!("objstore-{binary}-resumed.json"));
        let invoke = |ticks: &str, report: &Path, option: &str| -> anyhow::Result<i32> {
            let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
            command
                .args(["sim", "run", "--scenario"])
                .arg(&scene)
                .args(["--ticks", ticks, "--report"])
                .arg(report)
                .args(["--concurrency", "1"])
                .arg(option)
                .arg(&checkpoint);
            if binary {
                command.arg("--kkpv");
            }
            Ok(command.output()?.status.code().unwrap())
        };
        // 首 tick 动作已提交、库存读值尚未回读，断言未齐。
        assert_eq!(
            invoke(
                "1",
                &fixture.0.join(format!("objstore-{binary}-first.json")),
                "--save-checkpoint"
            )?,
            2,
            "提交读值尚未发生"
        );
        // --ticks 按本次调用计：1 + 3 = 4 轮，与基准对齐。
        assert_eq!(invoke("3", &resumed, "--load-checkpoint")?, 0);
        let actual: Value = serde_json::from_slice(&std::fs::read(resumed)?)?;
        assert_eq!(actual["final"]["world"], expected["final"]["world"]);
        assert_eq!(
            actual["final"]["players"]["p1"]["memory"],
            expected["final"]["players"]["p1"]["memory"]
        );
        assert_eq!(
            actual["final"]["mod_states"],
            expected["final"]["mod_states"]
        );
        assert_eq!(actual["plan_hash"], expected["plan_hash"]);
    }
    Ok(())
}
