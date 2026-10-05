//! 官方包走同一加载路径；报告身份与输入拒绝经真实 CLI 验证。
use super::*;

fn official_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mods/official")
}

#[test]
fn official_js_tick_and_player_api_work_across_rooms_and_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-tick/scenario.json")
        .canonicalize()?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("mod-{binary}.json")),
            50,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["world_ticks_committed"], 50);
        assert_eq!(report["final"]["objects"]["first"]["fields"]["value"], 100);
        assert_eq!(report["final"]["objects"]["second"]["fields"]["value"], 107);
        assert_eq!(report["final"]["mod_states"]["clockwork"]["visits"], 100);
        assert_eq!(report["final"]["players"]["p1"]["memory"]["value"], 98);
        assert_eq!(report["final"]["players"]["p2"]["memory"]["value"], 105);
        assert_eq!(report["mods"]["packages"]["clockwork"]["version"], "1.0.0");
        assert_eq!(report["plan_hash"].as_str().unwrap().len(), 64);
        assert_eq!(report["disabled_mods"], json!([]));
        reports.push(report);
    }
    assert_eq!(reports[0]["final"]["world"], reports[1]["final"]["world"]);
    assert_eq!(
        reports[0]["final"]["mod_states"],
        reports[1]["final"]["mod_states"]
    );
    assert_eq!(reports[0]["plan_hash"], reports[1]["plan_hash"]);
    assert_eq!(reports[0]["mods"], reports[1]["mods"]);
    Ok(())
}

#[test]
fn configuration_and_code_change_report_real_content_and_plan_identities() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop=()=>{};")?;
    let mut value = simple_scene();
    // 包移动到普通作者目录，验证官方路径和包 ID 没有额外装载语义。
    let package = fixture.0.join("mods/clockwork");
    std::fs::create_dir_all(&package)?;
    for name in ["mod.json", "server.js", "player.js"] {
        std::fs::copy(
            official_root().join("clockwork").join(name),
            package.join(name),
        )?;
    }
    value["mods"] = json!({"root":"mods","configs":{"clockwork":{"step":1}}});
    value["objects"] = json!([{"alias":"counter","room":"W1N1","kind":"mod","spec":{"typeKey":"clockwork:counter","fields":{"x":10,"y":12}}}]);
    let scene = fixture.scene(&value)?;
    let (code, first) = run(&scene, &fixture.0.join("first.json"), 3, false, 2)?;
    assert_eq!(code, 0, "{first}");
    value["mods"]["configs"]["clockwork"]["step"] = json!(2);
    fixture.scene(&value)?;
    let (code, configured) = run(&scene, &fixture.0.join("configured.json"), 3, false, 2)?;
    assert_eq!(code, 0, "{configured}");
    assert_eq!(first["final"]["objects"]["counter"]["fields"]["value"], 3);
    assert_eq!(
        configured["final"]["objects"]["counter"]["fields"]["value"],
        6
    );
    assert_ne!(first["plan_hash"], configured["plan_hash"]);
    assert_eq!(
        first["mods"]["packages"]["clockwork"]["code_sha256"],
        configured["mods"]["packages"]["clockwork"]["code_sha256"]
    );
    assert_ne!(
        first["mods"]["packages"]["clockwork"]["config_sha256"],
        configured["mods"]["packages"]["clockwork"]["config_sha256"]
    );
    let source = package.join("server.js");
    let mut bytes = std::fs::read(&source)?;
    bytes.extend_from_slice(b"\n// identity test\n");
    std::fs::write(&source, bytes)?;
    let (code, changed) = run(&scene, &fixture.0.join("changed.json"), 3, false, 2)?;
    assert_eq!(code, 0, "{changed}");
    assert_ne!(configured["plan_hash"], changed["plan_hash"]);
    assert_ne!(
        configured["mods"]["packages"]["clockwork"]["code_sha256"],
        changed["mods"]["packages"]["clockwork"]["code_sha256"]
    );
    assert_eq!(
        configured["mods"]["packages"]["clockwork"]["player_api_sha256"],
        changed["mods"]["packages"]["clockwork"]["player_api_sha256"]
    );
    Ok(())
}

#[test]
fn invalid_mod_loading_and_object_specs_fail_before_running() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop=()=>{};")?;
    let mut value = simple_scene();
    let report_path = fixture.0.join("invalid.json");
    for mods in [
        json!({"root":"absent"}),
        json!({"root":""}),
        json!({"root":official_root(),"configs":{"missing":{}}}),
        json!({"root":official_root(),"configs":{"clockwork":{"step":0}}}),
        json!({"root":official_root(),"unknown":true}),
    ] {
        value["mods"] = mods;
        let scene = fixture.scene(&value)?;
        let (code, report) = run(&scene, &report_path, 2, false, 2)?;
        assert_eq!(code, 1, "{report}");
        assert_eq!(report["status"], "setup_failed");
    }
    value["mods"] = json!({"root":official_root()});
    for spec in [
        json!({"typeKey":"missing:counter","fields":{"x":10,"y":12}}),
        json!({"typeKey":"clockwork:counter","fields":{"x":50,"y":12}}),
        json!({"typeKey":"clockwork:counter","fields":{"x":10,"y":12},"extra":true}),
    ] {
        value["objects"] = json!([{"alias":"bad","room":"W1N1","kind":"mod","spec":spec}]);
        let scene = fixture.scene(&value)?;
        let (code, report) = run(&scene, &report_path, 2, false, 2)?;
        assert_eq!(code, 1, "{report}");
        assert_eq!(report["world_ticks_committed"], 0);
    }
    Ok(())
}

#[test]
fn lifecycle_fault_report_preserves_committed_state_and_binding_identity() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    fixture.module("exports.loop=()=>{Memory.dirty=true;};")?;
    let package = fixture.0.join("mods/clockwork");
    std::fs::create_dir_all(&package)?;
    std::fs::copy(
        official_root().join("clockwork/mod.json"),
        package.join("mod.json"),
    )?;
    std::fs::copy(
        official_root().join("clockwork/player.js"),
        package.join("player.js"),
    )?;
    let source = std::fs::read_to_string(official_root().join("clockwork/server.js"))?;
    std::fs::write(
        package.join("server.js"),
        source.replace(
            "ctx.state = { visits:",
            "throw new Error('tick-fault'); ctx.state = { visits:",
        ),
    )?;
    let mut value = simple_scene();
    value["mods"] = json!({"root":"mods"});
    value["objects"] = json!([{"alias":"counter","room":"W1N1","kind":"mod","spec":{"typeKey":"clockwork:counter","fields":{"x":10,"y":12}}}]);
    let scene = fixture.scene(&value)?;
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("fault-{binary}.json")),
            3,
            binary,
            2,
        )?;
        assert_eq!(code, 2, "{report}");
        assert_eq!(report["world_ticks_committed"], 0);
        assert_eq!(report["initial"], report["final"]);
        assert_eq!(report["failure"]["stage"], "runtime");
        assert_eq!(report["mod_fault"]["room"], "W1N1");
        assert_eq!(report["mod_fault"]["target"], report["aliases"]["counter"]);
        assert_eq!(
            report["mod_fault"]["error"]["fault"]["slot"],
            "clockwork:counter.tick"
        );
        assert_eq!(
            report["mod_fault"]["error"]["fault"]["binding"],
            "clockwork:counter.tick.process"
        );
    }
    Ok(())
}
