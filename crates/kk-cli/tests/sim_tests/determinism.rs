//! G22.b 确定性玩法时间/RNG 的场景级验收：场景种子生效、黄金抽值双
//! 场景钉死（异种子相异）、检查点冷恢复续跑与直通运行逐位等值。
use super::*;

fn dice_scene(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-determinism")
        .join(name)
}

struct Case(PathBuf);
impl Case {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-dice-{}-{}-{}",
            tag,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn report(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn command(scene: &Path, report: &Path, ticks: u32) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string());
    command
}

fn result(command: &mut Command, report: &Path) -> anyhow::Result<(i32, Value)> {
    let output = command.output()?;
    let value: Value = serde_json::from_slice(&std::fs::read(report).unwrap_or_default())
        .unwrap_or_else(|_| json!({"stderr": String::from_utf8_lossy(&output.stderr).to_string()}));
    Ok((output.status.code().unwrap(), value))
}

/// 两个种子场景各自通过（黄金抽值已在场景内钉死），且两场景的抽值
/// 序列相异——种子是场景可复现身份的一部分。
#[test]
fn seed_scenarios_pin_distinct_golden_draws() -> anyhow::Result<()> {
    for name in ["scenario-dice.json", "scenario-dice-alt.json"] {
        let case = Case::new("golden");
        let (code, report) = result(
            command(&dice_scene(name), &case.report("r.json"), 4).arg("--kkpv"),
            &case.report("r.json"),
        )?;
        assert_eq!(code, 0, "场景 {name} 黄金断言须通过: {report}");
    }
    // 相异由两份黄金值显式成立（12345 vs 999 的 draws/0 不同）；此处
    // 再以完整状态对照兜底：异种子不得复用同一条流。
    let a = Case::new("cmp-a");
    let b = Case::new("cmp-b");
    let (_, ra) = result(
        &mut command(&dice_scene("scenario-dice.json"), &a.report("r.json"), 2),
        &a.report("r.json"),
    )?;
    let (_, rb) = result(
        &mut command(
            &dice_scene("scenario-dice-alt.json"),
            &b.report("r.json"),
            2,
        ),
        &b.report("r.json"),
    )?;
    assert_ne!(
        ra["final"]["mod_states"]["dice"]["draws"],
        rb["final"]["mod_states"]["dice"]["draws"]
    );
    Ok(())
}

/// 冷恢复续跑与直通运行逐位等值：检查点保存的种子驱动恢复分片的随机
/// 流，恢复后的 tick 取与直通相同的抽值；init 抽值重装不变。
#[test]
fn checkpoint_cold_resume_matches_straight_run() -> anyhow::Result<()> {
    let straight = Case::new("straight");
    let (code, full) = result(
        &mut command(
            &dice_scene("scenario-dice.json"),
            &straight.report("r.json"),
            6,
        ),
        &straight.report("r.json"),
    )?;
    assert_eq!(code, 0, "{full}");

    let phased = Case::new("phased");
    let checkpoint = phased.report("saved.kkscreeps-checkpoint");
    let (code, first) = result(
        command(
            &dice_scene("scenario-dice.json"),
            &phased.report("r1.json"),
            3,
        )
        .arg("--save-checkpoint")
        .arg(&checkpoint),
        &phased.report("r1.json"),
    )?;
    assert_eq!(code, 0, "{first}");
    let (code, second) = result(
        command(
            &dice_scene("scenario-dice.json"),
            &phased.report("r2.json"),
            3,
        )
        .arg("--load-checkpoint")
        .arg(&checkpoint),
        &phased.report("r2.json"),
    )?;
    assert_eq!(code, 0, "{second}");
    assert_eq!(second["seed"], json!(12345), "恢复分片使用载荷种子");

    let a = full["final"]["mod_states"]["dice"].clone();
    let b = second["final"]["mod_states"]["dice"].clone();
    assert_eq!(a, b, "冷恢复续跑的抽值序列必须与直通运行逐位相等");
    assert_eq!(second["final"]["time"], full["final"]["time"]);
    Ok(())
}

#[test]
fn utc_date_is_identical_across_process_timezones_and_cold_restore() -> anyhow::Result<()> {
    let scene = dice_scene("scenario-dice.json");
    let case = Case::new("tz");
    let mut states = Vec::new();
    for (index, tz) in ["UTC", "Asia/Shanghai", "America/Los_Angeles"]
        .iter()
        .enumerate()
    {
        let report = case.report(&format!("tz{index}.json"));
        let (code, value) = result(command(&scene, &report, 4).env("TZ", tz), &report)?;
        assert_eq!(code, 0, "{value}");
        let state = value["final"]["mod_states"]["dice"].clone();
        assert_eq!(state["date"]["parsed"], 0);
        assert_eq!(state["date"]["constructed"], 0);
        assert_eq!(state["date"]["coercedFunction"], 0);
        assert_eq!(state["date"]["rejectedObject"], true);
        assert_eq!(state["date"]["set"], 9 * 3600 * 1000);
        assert_eq!(state["date"]["offset"], 0);
        assert_eq!(state["date"]["roundtrip"], 0);
        states.push(state);
    }
    assert!(states.windows(2).all(|pair| pair[0] == pair[1]));
    let cp = case.report("cp.json");
    let report = case.report("first.json");
    let (code, value) = result(
        command(&scene, &report, 2)
            .env("TZ", "UTC")
            .arg("--save-checkpoint")
            .arg(&cp),
        &report,
    )?;
    assert_eq!(code, 0, "{value}");
    let report = case.report("restored.json");
    let (code, value) = result(
        command(&scene, &report, 2)
            .env("TZ", "Asia/Shanghai")
            .arg("--load-checkpoint")
            .arg(&cp),
        &report,
    )?;
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["final"]["mod_states"]["dice"], states[0]);
    Ok(())
}
