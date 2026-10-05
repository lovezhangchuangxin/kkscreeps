//! 独立 JS 原型经过 CLI 验证自主生成、更新、到期删除和玩家查询。
use super::*;

#[test]
fn official_nursery_creates_updates_and_removes_in_two_rooms_and_channels() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let scene = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/sim-mod-objects/scenario.json")
        .canonicalize()?;
    let mut reports = Vec::new();
    for binary in [false, true] {
        let (code, report) = run(
            &scene,
            &fixture.0.join(format!("nursery-{binary}.json")),
            53,
            binary,
            2,
        )?;
        assert_eq!(code, 0, "{report}");
        assert_eq!(report["world_ticks_committed"], 53);
        assert_eq!(
            report["final"]["mod_states"]["nursery"],
            json!({"born":22,"expired":20})
        );
        for player in ["p1", "p2"] {
            assert_eq!(report["final"]["players"][player]["memory"]["loops"], 53);
            assert_eq!(report["final"]["players"][player]["memory"]["emitted"], 11);
            assert_eq!(report["final"]["players"][player]["memory"]["sparks"], 1);
            assert_eq!(
                report["final"]["players"][player]["memory"]["ages"],
                json!([1])
            );
        }
        let sparks: Vec<_> = report["final"]["mod_objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|object| object["typeKey"] == "nursery:spark")
            .collect();
        assert_eq!(sparks.len(), 2);
        for spark in sparks {
            assert_eq!(spark["fields"]["age"], 2);
        }
        reports.push(report);
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
    assert_eq!(reports[0]["plan_hash"], reports[1]["plan_hash"]);
    Ok(())
}
