//! 建筑 RCL 配额规则（G11.a）：独立包经公共 SDK 覆盖内建配额族，
//! 玩家预检（工地创建）与结算读同一冻结值；原生对照锚定官方基线，
//! 双通道一致、组合共存、冷恢复保持配额身份。
use super::*;

// 场景期望：注入表读值、预检放行数、停止码、世界工地数由场景断言
// 核对；Rust 侧补双通道一致、组合身份与恢复语义。
const NAMES: [&str; 4] = ["native", "raise", "lower", "combined"];

fn scene_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-quota")
        .join(format!("scenario-{name}.json"))
        .canonicalize()?)
}

// 只读基准报告供两个测试共享；保存/恢复各起真实新进程。
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

#[test]
fn structure_quota_scenarios_hold_caps_across_channels() -> anyhow::Result<()> {
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
        // 双通道终态一致：配额折叠与工地结算不依赖投影通道。
        assert_eq!(channels[0]["final"]["world"], channels[1]["final"]["world"]);
        assert_eq!(
            channels[0]["final"]["players"]["p1"]["memory"],
            channels[1]["final"]["players"]["p1"]["memory"]
        );
    }
    // 内容选择冻结为不同计划身份：无 Mod 基线与三个覆盖组合互异。
    let mut hashes = std::collections::BTreeSet::new();
    for name in NAMES {
        hashes.insert(
            reference_reports()?[name][0]["plan_hash"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    assert_eq!(hashes.len(), 4, "覆盖组合分别冻结");
    Ok(())
}

#[test]
fn structure_quota_combined_scenario_resumes_with_same_identity() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = scene_path("combined")?;
    for binary in [false, true] {
        let expected = &reference_reports()?["combined"][usize::from(binary)];
        let checkpoint = fixture.0.join(format!("quota-{binary}-checkpoint.json"));
        let resumed = fixture.0.join(format!("quota-{binary}-resumed.json"));
        let invoke = |ticks: &str, report: &Path, option: &str| -> anyhow::Result<i32> {
            let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
            command
                .args(["sim", "run", "--scenario"])
                .arg(&scene)
                .args(["--ticks", ticks, "--report"])
                .arg(report)
                // 与共享基准同参数：单玩家下并发数无观察差异，只消除
                // 执行参数不对称。
                .args(["--concurrency", "1"])
                .arg(option)
                .arg(&checkpoint);
            if binary {
                command.arg("--kkpv");
            }
            Ok(command.output()?.status.code().unwrap())
        };
        // 首 tick 建站与配额读值已提交、补建与回读尚未发生，断言未齐。
        assert_eq!(
            invoke(
                "1",
                &fixture.0.join(format!("quota-{binary}-first.json")),
                "--save-checkpoint"
            )?,
            2,
            "补建与回读尚未发生"
        );
        // --ticks 按本次调用计：1 + 2 = 3 轮，与基准对齐。
        assert_eq!(invoke("2", &resumed, "--load-checkpoint")?, 0);
        let actual: Value = serde_json::from_slice(&std::fs::read(resumed)?)?;
        assert_eq!(actual["final"]["world"], expected["final"]["world"]);
        assert_eq!(
            actual["final"]["players"]["p1"]["memory"],
            expected["final"]["players"]["p1"]["memory"]
        );
        assert_eq!(actual["plan_hash"], expected["plan_hash"]);
    }
    Ok(())
}
