//! 计划级墙地形通行声明（见 movement.md 静态通行声明）：声明包、原生对照、
//! 跨规则组合、双通道与检查点冷恢复。
use super::*;

fn stable_final(report: &Value) -> Value {
    let mut result = report["final"].clone();
    for player in result["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    result
}

#[test]
fn terrain_wall_passability_composes_across_channels_and_resume_state() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let mut hashes = std::collections::BTreeSet::new();
    for name in ["native", "ghost", "deepwalk", "combined"] {
        let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compat/fixtures/sim-mod-terrain-walls")
            .join(format!("scenario-{name}.json"))
            .canonicalize()?;
        let mut references = Vec::new();
        for binary in [false, true] {
            let prefix = format!("{name}-{binary}");
            let (code, expected) = run(
                &scene,
                &fixture.0.join(format!("{prefix}-reference.json")),
                7,
                binary,
                1,
            )?;
            assert_eq!(code, 0, "{expected}");
            // 声明端点行为断言（穿墙/阻挡/物理查询）由场景断言钉死；
            // 此处只核对两侧通道的最终业务状态一致。
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
                command
                    .arg("--input-log")
                    .arg(fixture.0.join(format!("{prefix}-{option}.jsonl")))
                    .arg("--verify-replay");
                if binary {
                    command.arg("--kkpv");
                }
                let output = command.output()?;
                let value: Value = serde_json::from_slice(&std::fs::read(report)?)?;
                assert_eq!(value["verify_replay"]["status"], "passed", "{value}");
                Ok(output.status.code().unwrap())
            };
            // 第 0 tick 完成首个 moveTo；位置观测尚未写入，保存退出码为 2。
            // 中途保存后从检查点增量续跑 6 tick（1+6=7 对齐基准），恢复的
            // 引擎按同一计划重建，通行声明与寻路地形同源生效。
            assert_eq!(
                invoke(
                    "1",
                    &fixture.0.join(format!("{prefix}-first.json")),
                    "--save-checkpoint"
                )?,
                2,
                "观测尚未更新"
            );
            assert_eq!(invoke("6", &resumed, "--load-checkpoint")?, 0);
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
    assert_eq!(hashes.len(), 4, "声明选择和组合身份分别冻结");
    Ok(())
}
