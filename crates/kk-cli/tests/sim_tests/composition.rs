//! 玩家可见结果与报告链序通过双房、双投影的真实结算验证。
use super::*;
#[test]
fn composition_report_matches_actual_custom_tick_trace_across_channels() -> anyhow::Result<()> {
    let f = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-composition/scenario.json")
        .canonicalize()?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, r) = run(
            &scene,
            &f.0.join(format!("composition-{binary}.json")),
            100,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{r}");
        assert_eq!(r["composition"]["plan_hash"], r["plan_hash"]);
        assert_eq!(r["composition"]["disabled_mods"], r["disabled_mods"]);
        for name in ["first", "second"] {
            assert_eq!(r["final"]["objects"][name]["fields"]["trace"], "ZA([C])az");
            assert_eq!(r["final"]["objects"][name]["fields"]["value"], 100);
        }
        for player in ["p1", "p2"] {
            assert_eq!(
                r["final"]["players"][player]["memory"]["trace"],
                "ZA([C])az"
            );
        }
        reports.push(r);
    }
    assert_eq!(reports[0]["composition"], reports[1]["composition"]);
    assert_eq!(
        reports[0]["final"]["mod_objects"],
        reports[1]["final"]["mod_objects"]
    );
    assert_eq!(
        reports[0]["final"]["mod_states"],
        reports[1]["final"]["mod_states"]
    );
    Ok(())
}
