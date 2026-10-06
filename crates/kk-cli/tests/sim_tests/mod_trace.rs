//! `sim run --mod-trace`：拒绝/故障/回退事件进报告、与 inspect 活动链
//! 一致、默认关闭时无 trace 字段载荷。
use super::*;

fn trace_scene(name: &str) -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-trace")
        .join(name)
        .canonicalize()?)
}

/// 组合报告里指定槽的活动链绑定全集（before/after/wraps/native）。
fn active_bindings(report: &Value, slot: &str) -> Vec<String> {
    report["composition"]["slots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == slot)
        .map(|entry| {
            let active = &entry["active"];
            let mut bindings: Vec<String> = Vec::new();
            for phase in ["before", "after", "wraps"] {
                for binding in active[phase].as_array().unwrap_or(&Vec::new()) {
                    bindings.push(binding.as_str().unwrap().to_string());
                }
            }
            if let Some(center) = active["center"].as_str() {
                bindings.push(center.to_string());
            }
            bindings
        })
        .unwrap_or_default()
}

#[test]
fn trace_records_decline_and_completion_events_consistent_with_composition() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = trace_scene("scenario-gatekeeper.json")?;
    let report = fixture.0.join("gate.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .args(["--ticks", "2", "--report"])
        .arg(&report)
        .arg("--mod-trace");
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(0));
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["status"], "passed");
    let trace = &report["mod_trace"];
    assert!(trace["note"].as_str().unwrap().contains("非确定性回放日志"));
    let entries = trace["entries"].as_array().unwrap();
    assert_eq!(trace["dropped"], 0);
    // 拒绝事件带绑定与 code；pass 完成。绑定/槽须能在组合活动链中
    // 找到——与 mod inspect 的活动链同源。
    let declined: Vec<_> = entries
        .iter()
        .filter(|entry| entry["event"]["SlotDeclined"].is_object())
        .collect();
    assert_eq!(declined.len(), 1, "{entries:?}");
    let decline = &declined[0];
    assert_eq!(decline["event"]["SlotDeclined"]["code"], "gate-closed");
    let binding = decline["event"]["SlotDeclined"]["binding"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        active_bindings(&report, "gatekeeper:hold.slot").contains(&binding),
        "拒绝绑定须在活动链内：{binding}"
    );
    assert_eq!(decline["slot"], "gatekeeper:hold.slot");
    assert_eq!(decline["stage"], "object_action");
    assert_eq!(decline["room"], "W1N1");
    assert_eq!(decline["tick"], 0, "槽在提交前的世界时间执行");
    assert!(decline["target"].as_str().is_some_and(|id| !id.is_empty()));
    // 单元变体序列化为字符串字面量；struct 变体保持对象形态。
    assert!(
        entries
            .iter()
            .any(|entry| entry["event"] == json!("SlotCompleted")
                && entry["slot"] == "gatekeeper:pass.slot")
    );
    // 默认关闭：另一份不带旗标的运行没有 trace 载荷。
    let plain = fixture.0.join("gate-plain.json");
    let (code, plain) = run(&scene, &plain, 2, false, 1)?;
    assert_eq!(code, 0, "{plain}");
    assert!(
        plain["mod_trace"].is_null(),
        "默认关闭时报告不得携带 trace 载荷"
    );
    assert_eq!(plain["final"]["world"], report["final"]["world"]);
    Ok(())
}

#[test]
fn trace_records_fault_and_settlement_retry_for_failing_package() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = trace_scene("scenario-breaker.json")?;
    let report = fixture.0.join("break.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .args(["--ticks", "2", "--concurrency", "1", "--report"])
        .arg(&report)
        .arg("--mod-trace");
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(2), "故障轮未提交，退出 2");
    let report: Value = serde_json::from_slice(&std::fs::read(&report)?)?;
    assert_eq!(report["status"], "run_failed");
    // 失败归因是结算故障（bot 已被守卫静默），不是玩家错误。
    assert_eq!(report["failure"]["stage"], "runtime");
    assert_eq!(report["world_ticks_committed"], 0);
    let entries = report["mod_trace"]["entries"].as_array().unwrap();
    let fault: Vec<_> = entries
        .iter()
        .filter(|entry| entry["event"]["SlotFault"].is_object())
        .collect();
    assert!(!fault.is_empty(), "{entries:?}");
    let fault = &fault[0];
    assert_eq!(fault["slot"], "builtin:link.tick");
    assert_eq!(fault["stage"], "object_tick");
    let binding = fault["event"]["SlotFault"]["binding"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        active_bindings(&report, "builtin:link.tick").contains(&binding),
        "故障绑定须在活动链内（与 inspect 同源）：{binding}"
    );
    assert!(
        fault["event"]["SlotFault"]["error"]
            .as_str()
            .unwrap()
            .contains("breaker-boom")
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["event"]["SettlementRetry"].is_object()),
        "整轮失败须记录缓存重试事件：{entries:?}"
    );
    // 复现材料在报告内：场景身份与计划指纹可据此重跑。
    assert!(
        report["scenario_sha256"]
            .as_str()
            .is_some_and(|v| v.len() == 64)
    );
    assert!(report["plan_hash"].as_str().is_some_and(|v| !v.is_empty()));
    Ok(())
}

#[test]
fn trace_flag_survives_checkpoint_resume() -> anyhow::Result<()> {
    // 评审驱动的回归：enable 在分片创建前挂起；检查点恢复用替代引擎
    // 整体替换 self，挂起标志必须转移——否则续跑静默失去 trace。
    let fixture = Fixture::new()?;
    let scene = trace_scene("scenario-gatekeeper.json")?;
    let checkpoint = fixture.0.join("resume-checkpoint.json");
    let saved = fixture.0.join("resume-saved.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .args(["--ticks", "2", "--concurrency", "1", "--report"])
        .arg(&saved)
        .arg("--save-checkpoint")
        .arg(&checkpoint);
    let output = command.output()?;
    assert_eq!(output.status.code(), Some(0));
    let resumed = fixture.0.join("resume-resumed.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_kkscreeps"));
    command
        .args(["sim", "run", "--scenario"])
        .arg(&scene)
        .args(["--ticks", "1", "--concurrency", "1", "--report"])
        .arg(&resumed)
        .arg("--load-checkpoint")
        .arg(&checkpoint)
        .arg("--mod-trace");
    let output = command.output()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&std::fs::read(&resumed)?)?;
    assert_eq!(report["status"], "passed");
    let trace = &report["mod_trace"];
    let entries = trace["entries"].as_array().unwrap();
    assert!(
        !entries.is_empty(),
        "恢复分片的续跑必须继续记录 trace：{trace}"
    );
    assert!(
        entries.iter().all(|entry| entry["tick"] == 2),
        "续跑事件从恢复后的世界时间（2）开始：{entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry["event"] == json!("SlotCompleted")
                && entry["slot"] == "gatekeeper:pass.slot"),
        "续跑轮的第 3 次通行产生完成事件：{entries:?}"
    );
    assert!(
        !trace["note"].as_str().unwrap().contains("未装配"),
        "有 Mod 分片时 note 不得误报未装配：{trace}"
    );
    Ok(())
}
