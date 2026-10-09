//! observer 的 observeRoom 结算侧门与标记生命周期（意图直注，玩家
//! 预检之外的防线）。玩家链行为由 kk-runtime 的 mod_chain_tests 覆盖。

use super::*;

fn observer(f: &mut Fixture) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "observer", "owner": "p1"
                })
                .to_string(),
            }),
        )
        .unwrap()
}

fn marker(f: &mut Fixture, id: ObjId) -> Option<String> {
    match f.obj(id) {
        RoomObject::Generic(g) => {
            let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            v.get("observeRoom")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        }
        _ => panic!("非 observer"),
    }
}

/// 有效观察写标记；下一拍无新意图时预意图清零（官方 processor.js
/// :116-117 与 :284-287 的标记生命周期——观察视野恰一拍）。
#[test]
fn observer_marker_write_then_clear() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let o = observer(&mut f);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("W0N1".into()); // W1N1→W0N1 差 1，射程内
    b.add_object_intent("p1", o, Action::ObserveRoom, d);
    f.settle(b);
    assert_eq!(marker(&mut f, o).as_deref(), Some("W0N1"), "射程内写标记");
    f.settle(IntentBatch::default());
    assert_eq!(marker(&mut f, o), None, "无新意图的下一拍清零");

    // 差 10 边界（W1N1→W11N1）：引擎侧放行——JS 预检与结算门是两份
    // 独立实现，此断言防结算侧 off-by-one 静默漂移。
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("W11N1".into());
    b.add_object_intent("p1", o, Action::ObserveRoom, d);
    f.settle(b);
    assert_eq!(
        marker(&mut f, o).as_deref(),
        Some("W11N1"),
        "差 10 边界放行"
    );

    // 非法房名直注：不写标记（is_valid_room_name 门）。
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("not_a_room".into());
    b.add_object_intent("p1", o, Action::ObserveRoom, d);
    f.settle(b);
    assert_eq!(marker(&mut f, o), None, "非法房名直注拒绝");
}

/// 直注意图的越界防线：结算侧复验射程（玩家预检被绕过时标记不写）；
/// RCL 不足（7 级无 observer 配额）同样拒绝。
#[test]
fn observer_settle_rejects_injected_violations() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let o = observer(&mut f);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("W12N1".into()); // 差 11 超距
    b.add_object_intent("p1", o, Action::ObserveRoom, d);
    f.settle(b);
    assert_eq!(marker(&mut f, o), None, "超距直注不写标记");

    let mut f2 = Fixture::new(2, terrain_with(&[]));
    f2.controller(7, Some("p1"));
    let o2 = observer(&mut f2);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("W0N1".into());
    b.add_object_intent("p1", o2, Action::ObserveRoom, d);
    f2.settle(b);
    assert_eq!(marker(&mut f2, o2), None, "RCL7 配额 0 拒绝");

    // 无主 observer 直注：结算门放行（dispatcher 无主放行 + 配额检查视
    // 无主有效），与玩家面 NOT_OWNER 是官方同形分歧——钉住防两侧行为
    // 被后人"修复"成漂移。
    let mut f3 = Fixture::new(3, terrain_with(&[]));
    f3.controller(8, Some("p1"));
    let o3 = f3
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "observer"
                })
                .to_string(),
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.target_room_name = Some("W0N1".into());
    b.add_object_intent("p1", o3, Action::ObserveRoom, d);
    f3.settle(b);
    assert_eq!(
        marker(&mut f3, o3).as_deref(),
        Some("W0N1"),
        "无主直注写标记（官方同形分歧）"
    );
}
