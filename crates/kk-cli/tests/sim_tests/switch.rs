//! 场景声明的中途包切换（mods_switch）：已提交边界换装执行层、玩家
//! 可见常量按新计划生效、切换后检查点以切换后包集合身份冷恢复续跑；
//! 被拒绝的切换不使运行失败，其检查点按初始集合续跑。
use super::*;

fn switch_scene(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-switch")
        .join(name)
}

/// 读写都在私有目录内（pid+序号唯一），陈旧报告不可能跨运行残留；
/// 读不到报告时把 stderr 带进断言信息而非裸 `?`。
struct Case(PathBuf);
impl Case {
    fn new(tag: &str, binary: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-switch-{}-{}-{}-{}",
            tag,
            binary,
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

fn command(scene: &Path, report: &Path, ticks: u32, binary: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(scene)
        .arg("--report")
        .arg(report)
        .arg("--ticks")
        .arg(ticks.to_string());
    if binary {
        command.arg("--kkpv");
    }
    command
}

fn result(command: &mut Command, report: &Path) -> anyhow::Result<(i32, Value)> {
    let output = command.output()?;
    let value: Value = serde_json::from_slice(&std::fs::read(report).unwrap_or_default())
        .unwrap_or_else(|_| json!({"stderr": String::from_utf8_lossy(&output.stderr).to_string()}));
    Ok((output.status.code().unwrap(), value))
}

fn without_timing_cpu(mut value: Value) -> Value {
    for player in value["players"].as_object_mut().unwrap().values_mut() {
        player.as_object_mut().unwrap().remove("cpu_bucket");
    }
    value
}

/// 增包与减包：切换记录 applied 且差集/身份正确；减包后 z-gate 的
/// LINK_CAPACITY 覆盖随包移除，玩家常量回落原生值；保留包的 state
/// 计数与对象跨切换连续。双通道。
#[test]
fn scenario_switch_adds_and_removes_packages_at_committed_boundary() -> anyhow::Result<()> {
    for binary in [false, true] {
        let case = Case::new("addremove", binary);
        let report = case.report("add.json");
        let (code, value) = result(
            &mut command(&switch_scene("scenario-add.json"), &report, 6, binary),
            &report,
        )?;
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["report_version"], json!(8));
        let record = &value["mods_switches"][0];
        assert_eq!(record["status"], json!("applied"), "{record}");
        assert_eq!(record["after_tick"], json!(2));
        assert_eq!(record["added"], json!(["clockwork"]));
        assert_eq!(record["removed"], json!([]));
        assert_ne!(record["plan_hash_before"], record["plan_hash_after"]);
        assert_ne!(
            record["execution_strategy_before"],
            record["execution_strategy_after"]
        );
        assert_eq!(
            value["composition"]["selection"]["selected"],
            json!(["clockwork", "nursery"])
        );

        let report = case.report("remove.json");
        let (code, value) = result(
            &mut command(&switch_scene("scenario-remove.json"), &report, 6, binary),
            &report,
        )?;
        assert_eq!(code, 0, "{value}");
        let record = &value["mods_switches"][0];
        assert_eq!(record["status"], json!("applied"), "{record}");
        assert_eq!(record["removed"], json!(["a-audit", "dependent", "z-gate"]));
        assert_eq!(record["added"], json!([]));
        assert_eq!(
            value["composition"]["selection"]["selected"],
            json!(["base"])
        );
        // 场景断言已钉 /players/p1/memory/linkCap == 800（z-gate 移除后
        // 玩家常量按新计划回落原生值），此处再对报告复核一次。
        assert_eq!(
            value["final"]["players"]["p1"]["memory"]["linkCap"],
            json!(800)
        );
    }
    Ok(())
}

/// 冷恢复闭环：第 2 轮后切换并保存的检查点，续跑装载切换后包集合
/// （身份门全绿），不重放切换；终态与直跑等价、执行策略为切换后身份。
/// 边界前（第 1 轮）保存的检查点按初始集合续跑，切换在续跑循环内
/// 跨过边界时照常生效。
#[test]
fn switch_checkpoint_cold_resumes_with_post_switch_packages() -> anyhow::Result<()> {
    let scene = switch_scene("scenario-add.json");
    for binary in [false, true] {
        let case = Case::new("cold", binary);
        let reference = case.report("ref.json");
        let checkpoint = case.report("saved.kkscreeps-checkpoint");
        let first = case.report("first.json");
        let resumed = case.report("resumed.json");
        let (code, expected) = result(&mut command(&scene, &reference, 6, binary), &reference)?;
        assert_eq!(code, 0, "{expected}");
        // 保存 2 轮：切换恰在提交完第 2 轮后、保存之前发生。断言按
        // 6 轮口径写，2 轮必不满足——退出码 2 但检查点照常保存。
        let (code, first_report) = result(
            command(&scene, &first, 2, binary)
                .arg("--save-checkpoint")
                .arg(&checkpoint),
            &first,
        )?;
        assert_eq!(code, 2, "{first_report}");
        assert_eq!(first_report["mods_switches"][0]["status"], json!("applied"));
        assert_eq!(first_report["checkpoint_saved"], json!(checkpoint));
        let (code, actual) = result(
            command(&scene, &resumed, 4, binary)
                .arg("--load-checkpoint")
                .arg(&checkpoint),
            &resumed,
        )?;
        assert_eq!(code, 0, "{actual}");
        assert_eq!(actual["start_time"], json!(2));
        assert_eq!(actual["end_time"], json!(6));
        assert_eq!(actual["mods_switches"], json!([]), "续跑不重放已生效的切换");
        assert_eq!(actual["aliases"], expected["aliases"]);
        assert_eq!(actual["composition"], expected["composition"]);
        assert_eq!(
            actual["execution_strategy"], expected["execution_strategy"],
            "续跑携带切换后执行策略身份"
        );
        assert_eq!(actual["initial"], expected["initial"], "断言起点沿用原始");
        assert_eq!(
            without_timing_cpu(actual["final"].clone()),
            without_timing_cpu(expected["final"].clone())
        );

        // 边界前保存：第 1 轮的检查点携带初始集合身份，续跑装载初始
        // 集合，切换在循环内自然生效（第 2 轮提交后）。
        let early_checkpoint = case.report("early.kkscreeps-checkpoint");
        let early = case.report("early.json");
        let continued = case.report("continued.json");
        let (code, early_report) = result(
            command(&scene, &early, 1, binary)
                .arg("--save-checkpoint")
                .arg(&early_checkpoint),
            &early,
        )?;
        assert_eq!(code, 2, "{early_report}");
        assert_eq!(early_report["mods_switches"], json!([]), "第 1 轮未到边界");
        let (code, continued) = result(
            command(&scene, &continued, 5, binary)
                .arg("--load-checkpoint")
                .arg(&early_checkpoint),
            &continued,
        )?;
        assert_eq!(code, 0, "{continued}");
        assert_eq!(continued["start_time"], json!(1));
        assert_eq!(continued["end_time"], json!(6));
        assert_eq!(
            continued["mods_switches"][0]["status"],
            json!("applied"),
            "续跑循环内跨过边界时切换照常生效"
        );
        assert_eq!(
            continued["execution_strategy"],
            expected["execution_strategy"]
        );
        assert_eq!(
            without_timing_cpu(continued["final"].clone()),
            without_timing_cpu(expected["final"].clone())
        );
    }
    Ok(())
}

/// 被拒绝的切换（clockwork 对象在世，减包即残留）：运行不失败、旧计划
/// 继续结算、报告记 rejected 与错误；该检查点越过边界但携带初始身份
/// （applied_switches 为空），续跑按初始集合装载且不再重试切换。
#[test]
fn rejected_switch_records_error_and_checkpoint_resumes_with_initial_set() -> anyhow::Result<()> {
    let scene = switch_scene("scenario-reject.json");
    for binary in [false, true] {
        let case = Case::new("reject", binary);
        let checkpoint = case.report("saved.kkscreeps-checkpoint");
        let first = case.report("first.json");
        let resumed = case.report("resumed.json");
        let (code, value) = result(
            command(&scene, &first, 3, binary)
                .arg("--save-checkpoint")
                .arg(&checkpoint),
            &first,
        )?;
        assert_eq!(code, 0, "拒绝不使运行失败：{value}");
        let record = &value["mods_switches"][0];
        assert_eq!(record["status"], json!("rejected"), "{record}");
        assert!(
            record["error"]
                .as_str()
                .is_some_and(|error| error.contains("残留")),
            "拒绝原因要指路残留清理/G23：{}",
            record["error"]
        );
        assert_eq!(
            value["composition"]["selection"]["selected"],
            json!(["clockwork", "nursery"]),
            "旧计划继续生效"
        );
        assert_eq!(value["checkpoint_saved"], json!(checkpoint));

        let (code, actual) = result(
            command(&scene, &resumed, 3, binary)
                .arg("--load-checkpoint")
                .arg(&checkpoint),
            &resumed,
        )?;
        assert_eq!(code, 0, "越界但未生效的检查点按初始集合续跑：{actual}");
        assert_eq!(actual["start_time"], json!(3));
        assert_eq!(actual["end_time"], json!(6));
        assert_eq!(actual["mods_switches"], json!([]), "时间已过边界不再重试");
        assert_eq!(
            actual["composition"]["selection"]["selected"],
            json!(["clockwork", "nursery"])
        );
    }
    Ok(())
}
