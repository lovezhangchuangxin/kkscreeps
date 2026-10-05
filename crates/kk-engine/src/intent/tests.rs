use super::*;
use serde_json::json;

#[test]
fn extend_from_preserves_other_actions() {
    let id = ObjId(1);
    let mut first = IntentBatch::default();
    first.add_object_intent("p1", id, Action::Move, IntentData::bare());
    first.add_object_intent("p1", id, Action::Harvest, IntentData::target(ObjId(2)));
    let mut second = IntentBatch::default();
    second.add_object_intent("p1", id, Action::Move, IntentData::target(ObjId(3)));
    second.add_object_intent("p1", id, Action::Say, IntentData::bare());
    first.extend_from(second);
    let actions = &first.users["p1"].objects[&id];
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[&Action::Move].id, Some(ObjId(3)));
    assert_eq!(actions[&Action::Harvest].id, Some(ObjId(2)));
    assert!(actions.contains_key(&Action::Say));
}

#[test]
fn priority_graph_matches_official() {
    // heal 的存在足以压制 harvest；其余压制关系定义于 suppressed_by。
    let mut present = BTreeSet::new();
    present.insert(Action::Harvest);
    assert!(Action::executes(&present, Action::Harvest));
    present.insert(Action::Heal);
    assert!(
        !Action::executes(&present, Action::Harvest),
        "heal 压制 harvest"
    );
    assert!(Action::executes(&present, Action::Heal));
    // repair 压制 build，build 压制 attack；压制不取决于执行成功与否。
    let p2 = BTreeSet::from([Action::Build, Action::Repair]);
    assert!(!Action::executes(&p2, Action::Build));
    assert!(Action::executes(&p2, Action::Repair));
    let p3 = BTreeSet::from([Action::Attack, Action::Build]);
    assert!(
        !Action::executes(&p3, Action::Attack),
        "build 在 attack 的压制表里"
    );
    assert!(Action::executes(&p3, Action::Build));
    // 执行顺序：drop 最先、move 在 harvest 后
    assert_eq!(EXECUTION_ORDER[0], Action::Drop);
    assert!(
        EXECUTION_ORDER.iter().position(|a| *a == Action::Harvest)
            < EXECUTION_ORDER.iter().position(|a| *a == Action::Move)
    );
}

#[test]
fn sanitize_translates_official_transforms() {
    let raw = json!({
        "id": "kk0000000000000001",
        "amount": 12.7,
        "resourceType": "energy",
        "unknown": "dropped",
    });
    let d = sanitize_object_intent(Action::Transfer, &raw);
    assert_eq!(d.id, Some(ObjId(1)));
    assert_eq!(d.amount, Some(12), "parseInt 截断");
    assert_eq!(d.resource_type.as_deref(), Some("energy"));
    // bodypart[] 过滤
    let raw2 = json!({ "body": ["work", "fly", "move"] });
    let d2 = sanitize_object_intent(Action::Suicide, &raw2);
    assert_eq!(d2.body, vec![BodyKind::Work, BodyKind::Move]);
    assert_eq!(creep_cost(&d2.body), 150);
    // userString 截 100
    let long = "x".repeat(150);
    let raw3 = json!({ "name": long });
    let d3 = sanitize_object_intent(Action::Say, &raw3);
    assert_eq!(d3.name.unwrap().chars().count(), 100);
    // directions：parseInt 截断（2.5→2）+ 保序去重；越界值原样传递——
    // 整单拒绝发生在 createCreep（1..=8 校验），无法 parseInt 的元素以
    // i64::MIN 哨兵传递（天然落入 1..=8 之外）
    let raw4 = json!({ "directions": [1, 9, 3, 3, 2.5] });
    let d4 = sanitize_object_intent(Action::Suicide, &raw4);
    assert_eq!(d4.directions, vec![1, 9, 3, 2]);
    let raw5 = json!({ "directions": [1, "abc"] });
    let d5 = sanitize_object_intent(Action::Suicide, &raw5);
    assert_eq!(d5.directions, vec![1, i64::MIN]);
    let raw6 = json!({ "directions": [1, 2, 1, "3tail", "0x2"] });
    let d6 = sanitize_object_intent(Action::CreateCreep, &raw6);
    assert_eq!(d6.directions, vec![1, 2, 3]);
    assert_eq!(to_int(Some(&json!("1e2"))), Some(1));
    assert_eq!(to_int(Some(&json!(-12.7))), Some(-12));
    assert_eq!(to_int(Some(&json!("-9223372036854775808"))), Some(i64::MIN));
    assert_eq!(to_int(Some(&json!(1e-6))), Some(0));
    assert_eq!(to_int(Some(&json!(1e-7))), Some(1));
    assert_eq!(to_int(Some(&json!(-2e-7))), Some(-2));
    assert_eq!(to_int(Some(&json!(1e21))), Some(1));
    assert_eq!(
        sanitize_object_intent(Action::CreateCreep, &json!({ "directions": [1e-6] })).directions,
        vec![0]
    );
}

/// 战斗槽键在 intent 派发、能力面与权威目录三处平行枚举，漂移会让
/// 新增槽的 Hosted 处理器静默不可达；本钉子要求三处集合等价。
#[test]
fn combat_slot_keys_stay_consistent_across_enumerations() {
    let mut engine_side: Vec<&str> = EXECUTION_ORDER
        .iter()
        .filter_map(|action| action.combat_slot_key())
        .collect();
    engine_side.sort_unstable();
    let mut authority: Vec<&str> = kk_mod_api::builtin::COMBAT_ACTION_SLOTS.to_vec();
    authority.sort_unstable();
    assert_eq!(engine_side, authority);
    for key in kk_mod_api::builtin::COMBAT_ACTION_SLOTS {
        assert!(kk_engine_probe_from_slot_key(key), "能力面未认识槽键 {key}");
    }
}

/// 建造族槽键与战斗族同型：intent 派发、能力面与权威目录三处等价。
#[test]
fn build_slot_keys_stay_consistent_across_enumerations() {
    let mut engine_side: Vec<&str> = EXECUTION_ORDER
        .iter()
        .filter_map(|action| action.build_slot_key())
        .collect();
    engine_side.sort_unstable();
    let mut authority: Vec<&str> = kk_mod_api::builtin::BUILD_ACTION_SLOTS.to_vec();
    authority.sort_unstable();
    assert_eq!(engine_side, authority);
    for key in kk_mod_api::builtin::BUILD_ACTION_SLOTS {
        assert!(
            crate::settle::creep::BuildAction::from_slot_key(key).is_some(),
            "能力面未认识槽键 {key}"
        );
    }
}

/// from_slot_key 定义域探测的桥；定义在测试内避免能力面类型出模块。
fn kk_engine_probe_from_slot_key(key: &str) -> bool {
    crate::settle::creep::CombatAction::from_slot_key(key).is_some()
}
