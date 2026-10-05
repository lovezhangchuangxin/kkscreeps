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
        // 玩家经公共玩家 API 读到规则驱动的已提交结果。tick 从 0 计且玩家
        // 先于同轮 Mod tick 执行：最后一轮读到前 99 轮的累计，偶数节拍
        // 0..=98 共 50 次，因此是 50 而不是本轮回合结束后的值。
        for player in ["p1", "p2"] {
            assert_eq!(r["final"]["players"][player]["memory"]["value"], 50);
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
