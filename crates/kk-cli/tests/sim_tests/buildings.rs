//! N2b 建筑面经真实 CLI 验证：三场景双投影通道全等、组合报告与
//! 检查点冷恢复续跑（真实进程退出再恢复与连续运行等价）。
use super::*;

fn building_scene(name: &str) -> anyhow::Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-buildings")
        .join(name)
        .canonicalize()?)
}

/// 主场景双通道全等：建造/生产/搬运/受击维修/寻路语义在 JSON 与 KKPV
/// 两条投影通道下逐项一致。
#[test]
fn smelter_building_scenario_matches_across_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = building_scene("scenario.json")?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("smelter-{binary}.json")),
            320,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["world_ticks_committed"], 320);
        let mem = &report["final"]["players"]["p1"]["memory"];
        assert_eq!(mem["grid_foundry"], 255, "obstacle 建筑在代价矩阵中为 255");
        assert_eq!(mem["foundry_hits"], 5000, "受击 6 次后维修回满");
        assert_eq!(mem["foundry_owner"], "p1", "归属语义随完工落成");
        assert!(
            mem["storage_ingots"].as_i64().unwrap_or(0) >= 100,
            "产物经真实搬运回仓：{}",
            mem["storage_ingots"]
        );
        assert_eq!(mem["attacks"], 6);
        assert_eq!(mem["build_precheck_empty"], true, "空载预检拒绝分支");
        reports.push(report);
    }
    assert_eq!(
        reports[0]["final"]["world"], reports[1]["final"]["world"],
        "JSON 与 KKPV 最终世界全等"
    );
    assert_eq!(reports[0]["plan_hash"], reports[1]["plan_hash"]);
    Ok(())
}

/// 组合场景：两个规则不同的包并存——panel 单资源成本、road 寻路（矩阵 1）、
/// 免费生产；smelter 行为不回归。组合报告登记 buildings 目录与选择展开。
#[test]
fn combined_scenario_hosts_two_rule_variants_and_reports_buildings() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = building_scene("scenario-combined.json")?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("combined-{binary}.json")),
            320,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{report}");
        let mem = &report["final"]["players"]["p1"]["memory"];
        assert_eq!(mem["grid_foundry"], 255);
        assert_eq!(mem["grid_panel"], 1, "road 语义建筑按道路成本");
        assert_eq!(mem["panel_energy"], 200, "免费生产至容量后停止");
        assert_eq!(mem["panel_hits"], 300);
        assert!(
            mem["storage_ingots"].as_i64().unwrap_or(0) >= 100,
            "smelter 行为不回归：{}",
            mem["storage_ingots"]
        );
        reports.push(report);
    }
    assert_eq!(reports[0]["final"]["world"], reports[1]["final"]["world"]);
    let composition = &reports[0]["composition"];
    assert_eq!(composition["report_version"], 4);
    let buildings = composition["buildings"].as_array().unwrap();
    let keys: Vec<&str> = buildings
        .iter()
        .map(|b| b["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["panel:collector", "smelter:foundry"]);
    // 配方数据完整进入组合报告（诊断与内容身份可追溯）。
    let foundry = &buildings[1];
    assert_eq!(foundry["source"], "smelter");
    assert_eq!(foundry["hits_max"], 5000);
    assert_eq!(foundry["pathing"], "obstacle");
    assert_eq!(foundry["store_capacity"], 1000);
    assert_eq!(foundry["production"]["interval"], 12);
    assert_eq!(
        reports[0]["composition"]["selection"]["selected"],
        json!(["panel", "smelter"])
    );
    Ok(())
}

/// 对照场景：未安装任何包的世界拒绝 Mod 工地创建与资源搬运，无残留。
#[test]
fn plain_world_rejects_mod_building_surface() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = building_scene("scenario-plain.json")?;
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("plain-{binary}.json")),
            8,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{report}");
        let mem = &report["final"]["players"]["p1"]["memory"];
        assert_eq!(mem["site_rejected"], true);
        assert_eq!(mem["withdraw_rejected"], true);
        assert_eq!(mem["no_sites"], true);
        assert_eq!(mem["no_foundry"], true);
        assert_eq!(
            report["composition"]["buildings"]
                .as_array()
                .map(Vec::len)
                .unwrap_or(0),
            0,
            "零 Mod 世界无建筑目录"
        );
    }
    Ok(())
}

/// 真实进程保存、退出、恢复续跑：建筑、库存、生产与耐久状态在冷恢复后
/// 与未中断参考一致（完整持久状态不丢失）。
#[test]
fn cold_resume_matches_uninterrupted_building_world() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = building_scene("scenario.json")?;
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
    for binary in [false, true] {
        let reference = fixture.0.join(format!("ref-{binary}.json"));
        let checkpoint = fixture.0.join(format!("saved-{binary}.json"));
        let first = fixture.0.join(format!("first-{binary}.json"));
        let resumed = fixture.0.join(format!("resumed-{binary}.json"));
        let output = command(&scene, &reference, 320, binary).output()?;
        assert_eq!(output.status.code(), Some(0));
        let expected: Value = serde_json::from_slice(&std::fs::read(&reference)?)?;
        let output = command(&scene, &first, 170, binary)
            .arg("--save-checkpoint")
            .arg(&checkpoint)
            .output()?;
        assert_eq!(output.status.code(), Some(2), "中途断言未满足但状态合法");
        let output = command(&scene, &resumed, 150, binary)
            .arg("--load-checkpoint")
            .arg(&checkpoint)
            .output()?;
        assert_eq!(output.status.code(), Some(0));
        let actual: Value = serde_json::from_slice(&std::fs::read(&resumed)?)?;
        assert_eq!(actual["world_ticks_committed"], 150);
        // 完整持久状态等价：世界（含建筑、工地、库存、事件）逐项一致。
        let strip_cpu = |mut value: Value| {
            for player in value["final"]["players"]
                .as_object_mut()
                .unwrap()
                .values_mut()
            {
                player.as_object_mut().unwrap().remove("cpu_bucket");
            }
            value
        };
        let a = strip_cpu(actual);
        let b = strip_cpu(expected);
        assert_eq!(a["final"]["world"], b["final"]["world"]);
        assert_eq!(
            a["final"]["players"]["p1"]["memory"]["storage_ingots"],
            b["final"]["players"]["p1"]["memory"]["storage_ingots"]
        );
        assert_eq!(a["composition"], b["composition"]);
    }
    Ok(())
}
