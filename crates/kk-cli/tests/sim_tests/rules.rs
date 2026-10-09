//! 有效规则读取经真实 CLI 双投影验证：玩法结果由冻结有效值驱动。
use super::*;

#[test]
fn effective_rule_reads_drive_gameplay_across_channels() -> anyhow::Result<()> {
    let f = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-rules/scenario.json")
        .canonicalize()?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, r) = run(
            &scene,
            &f.0.join(format!("rules-{binary}.json")),
            100,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{r}");
        assert_eq!(r["world_ticks_committed"], 100);
        for name in ["first", "second"] {
            // 覆盖 period=2（默认 4）与 rate=0.25（默认 0.5）：若处理器读了
            // 注册闭包里的默认值，这里会是 25 与 50.0。
            assert_eq!(r["final"]["objects"][name]["fields"]["value"], 50);
            assert_eq!(
                r["final"]["objects"][name]["fields"]["pool"].as_f64(),
                Some(25.0)
            );
        }
        // 最后一轮提交的 namespace state 记录执行期实际读到的有效值。
        assert_eq!(r["final"]["mod_states"]["base"]["period"], 2);
        assert_eq!(r["final"]["mod_states"]["base"]["rate"], 0.25);
        assert_eq!(r["final"]["mod_states"]["base"]["capacity"], 700);
        assert_eq!(r["final"]["mod_states"]["base"]["ticks"], 200);
        // 玩家经公共玩家 API 读到规则驱动的已提交结果。玩家先于同轮
        // Mod tick 执行：最后一轮读到前 99 轮的累计；热身后对象 tick
        // 从 1 起，偶数节拍 2..=98 共 49 次（相位随绝对 tick 平移）。
        for player in ["p1", "p2"] {
            assert_eq!(r["final"]["players"][player]["memory"]["value"], 49);
        }
        // 组合诊断与执行计划来自同一冻结计划。
        assert_eq!(r["composition"]["plan_hash"], r["plan_hash"]);
        let rule = r["composition"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["key"] == "builtin:link.capacity")
            .unwrap();
        assert_eq!(rule["value"]["Signed"], 700);
        assert_eq!(rule["selected_source"], "tuner");
        reports.push(r);
    }
    assert_eq!(reports[0]["final"]["world"], reports[1]["final"]["world"]);
    assert_eq!(
        reports[0]["final"]["mod_objects"],
        reports[1]["final"]["mod_objects"]
    );
    assert_eq!(
        reports[0]["final"]["mod_states"],
        reports[1]["final"]["mod_states"]
    );
    assert_eq!(reports[0]["composition"], reports[1]["composition"]);
    Ok(())
}

#[test]
fn documented_rule_example_runs_with_its_declared_fields() -> anyhow::Result<()> {
    let f = Fixture::new()?;
    let document = include_str!("../../../../docs/architecture/mod-rules.md");
    let source = document
        .split_once("```js\n")
        .unwrap()
        .1
        .split_once("\n```")
        .unwrap()
        .0;
    let package = f.0.join("mods/base");
    std::fs::create_dir_all(&package)?;
    std::fs::write(
        package.join("mod.json"),
        r#"{"id":"base","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )?;
    std::fs::write(package.join("server.js"), source)?;
    std::fs::write(
        package.join("player.js"),
        "export default function install(api){class RulesMeter extends api.builtins.RoomObject{};api.objects.bind('base:meter',RulesMeter);}",
    )?;
    f.module("exports.loop = function() {};")?;
    let mut input = simple_scene();
    input["mods"] = json!({"root":"mods"});
    input["objects"] = json!([{
        "alias":"meter","room":"W1N1","kind":"mod",
        "spec":{"typeKey":"base:meter","fields":{"x":10,"y":10}}
    }]);
    let scene = f.scene(&input)?;
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &f.0.join(format!("doc-rules-{binary}.json")),
            9,
            binary,
            1,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["world_ticks_committed"], 9);
        // 默认 period=4，热身后对象 tick 从 1 起：4/8 两拍更新（相位随绝对
        // tick 平移）；value 由文档 schema 默认初始化。
        assert_eq!(report["final"]["objects"]["meter"]["fields"]["value"], 2);
    }
    Ok(())
}
