//! 测试 factory produce 结算：配方消耗与产出、冷却、等级/效应/容量/组件
//! 各拒绝门。行为锚点：官方 processor/intents/factories/produce.js 与
//! screeps-ok 的 FACTORY-PRODUCE 用例族。

use super::*;
use serde_json::json;

fn place_generic(f: &mut Fixture, x: u8, y: u8, body: serde_json::Value) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: body.to_string(),
            }),
        )
        .unwrap()
}

/// 放置 factory：平面容量 50000，可选等级与预置效果。
fn factory(
    f: &mut Fixture,
    level: i64,
    store: serde_json::Value,
    cooldown_time: u64,
    effects: serde_json::Value,
) -> ObjId {
    let mut data = json!({
        "type": "structure", "structureType": "factory", "owner": "p1",
        "hits": 1000, "hitsMax": 1000,
        "level": level,
        "store": store, "storeCapacity": 50000,
    });
    if cooldown_time > 0 {
        data["cooldownTime"] = json!(cooldown_time);
    }
    if !effects.is_null() {
        data["effects"] = effects;
    }
    place_generic(f, 25, 25, data)
}

fn data_of(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Generic 不存在：{other:?}"),
    }
}

fn produce_intent(resource: &str) -> IntentData {
    let mut d = IntentData::bare();
    d.resource_type = Some(resource.into());
    d
}

/// produce.js：battery 配方 600 energy → 50 battery，冷却 REACTION cooldown=10。
#[test]
fn factory_produce_battery_recipe() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(7, Some("p1"));
    let id = factory(&mut f, 0, json!({"energy": 600}), 0, Value::Null);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", id, Action::Produce, produce_intent("battery"));
    f.settle(batch);
    let d = data_of(&f, id);
    assert_eq!(d.pointer("/store/energy"), Some(&json!(0)));
    assert_eq!(d.pointer("/store/battery"), Some(&json!(50)));
    assert_eq!(d["cooldownTime"], json!(10));
}

/// 双向配方：utrium_bar（U 500 + energy 200 → 100）。
#[test]
fn factory_produce_bar_recipe() {
    let mut f = Fixture::new(2, terrain_with(&[]));
    f.controller(7, Some("p1"));
    let id = factory(&mut f, 0, json!({"U": 500, "energy": 200}), 0, Value::Null);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", id, Action::Produce, produce_intent("utrium_bar"));
    f.settle(batch);
    let d = data_of(&f, id);
    assert_eq!(d.pointer("/store/U"), Some(&json!(0)));
    assert_eq!(d.pointer("/store/energy"), Some(&json!(0)));
    assert_eq!(d.pointer("/store/utrium_bar"), Some(&json!(100)));
    assert_eq!(d["cooldownTime"], json!(20));
}

/// 有级配方的效果门两向：endTime=1 在 time=0 有效（官方判据
/// `endTime >= gameTime`——`>=` 边界即正向锚）；推进到 time=50（冷却
/// 50 同时到期）后效果过期，静默 no-op。同房同主第二座 factory 会被
/// RCL7 配额 1 的距离清点拒（kk 无官方单件捷径，登记的既有偏差），
/// 故两向都在同一座上按时间推进实现。
#[test]
fn factory_produce_leveled_recipe_with_effect() {
    let mut f = Fixture::new(3, terrain_with(&[]));
    f.controller(7, Some("p1"));
    // 组件双倍：首轮消耗一半，次轮余量仍足——把「无产出」唯一归因到效果过期。
    let id = factory(
        &mut f,
        1,
        json!({"utrium_bar": 40, "zynthium_bar": 40, "energy": 40}),
        0,
        json!([{"power": 19, "level": 1, "endTime": 1}]),
    );
    let mut first = IntentBatch::default();
    first.add_object_intent("p1", id, Action::Produce, produce_intent("composite"));
    f.settle(first);
    let ok = data_of(&f, id);
    assert_eq!(
        ok.pointer("/store/composite"),
        Some(&json!(20)),
        "endTime=1 在 time=0 有效"
    );
    assert_eq!(ok.pointer("/store/utrium_bar"), Some(&json!(20)));
    assert_eq!(ok["cooldownTime"], json!(50));

    // 空批推进 49 轮到 time=50：cooldownTime=50 不再 > time，效果 1 < 50 过期。
    for _ in 0..49 {
        f.settle(IntentBatch::default());
    }
    let mut second = IntentBatch::default();
    second.add_object_intent("p1", id, Action::Produce, produce_intent("composite"));
    f.settle(second);
    let stale = data_of(&f, id);
    assert_eq!(
        stale.pointer("/store/composite"),
        Some(&json!(20)),
        "效果过期不产"
    );
    assert_eq!(
        stale.pointer("/store/utrium_bar"),
        Some(&json!(20)),
        "组件不消耗"
    );
    assert_eq!(stale["cooldownTime"], json!(50), "不重置冷却");
}

/// 拒绝族：非配方资源、等级不匹配、无效应的同级配方、冷却、RCL 配额、
/// 组件不足、平面容量越限——全部静默 no-op（错误码在玩家预检层）。
#[test]
fn factory_produce_silent_rejections() {
    for (case, level, store, resource, cooldown) in [
        (
            "not-commodity",
            0i64,
            json!({"energy": 1000}),
            "power",
            0u64,
        ),
        (
            "level-mismatch",
            0,
            json!({"utrium_bar": 20, "zynthium_bar": 20, "energy": 20}),
            "composite",
            0,
        ),
        (
            "no-effect",
            1,
            json!({"utrium_bar": 20, "zynthium_bar": 20, "energy": 20}),
            "composite",
            0,
        ),
        ("cooldown", 0, json!({"energy": 600}), "battery", 5),
        ("not-enough", 0, json!({"energy": 100}), "battery", 0),
        (
            "full",
            0,
            json!({"battery": 50, "energy": 49551}),
            "energy",
            0,
        ),
    ] {
        let mut f = Fixture::new(4, terrain_with(&[]));
        f.controller(7, Some("p1"));
        let store_before = store.clone();
        let id = factory(&mut f, level, store, cooldown, Value::Null);
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", id, Action::Produce, produce_intent(resource));
        f.settle(batch);
        let d = data_of(&f, id);
        assert_eq!(d["store"], store_before, "{case} 不得改动 store");
        assert!(d.get("cooldownTime").is_none() || d["cooldownTime"] == json!(cooldown));
    }
}

/// RCL 配额门：RCL 6 房 factory 配额为 0，produce 静默 no-op
/// （checkStructureAgainstController 拒绝）。
#[test]
fn factory_produce_rejected_by_rcl_quota() {
    let mut f = Fixture::new(5, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let id = factory(&mut f, 0, json!({"energy": 600}), 0, Value::Null);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", id, Action::Produce, produce_intent("battery"));
    f.settle(batch);
    let d = data_of(&f, id);
    assert_eq!(d.pointer("/store/energy"), Some(&json!(600)));
    assert!(d.get("cooldownTime").is_none());
}

/// 冷却中的第二次 produce 被冷却门拦截（官方 processor 同形）。
#[test]
fn factory_cooldown_blocks_second_produce() {
    let mut f = Fixture::new(6, terrain_with(&[]));
    f.controller(7, Some("p1"));
    let id = factory(&mut f, 0, json!({"energy": 1200}), 0, Value::Null);
    let mut first = IntentBatch::default();
    first.add_object_intent("p1", id, Action::Produce, produce_intent("battery"));
    f.settle(first);
    assert_eq!(data_of(&f, id)["cooldownTime"], json!(10));
    let mut second = IntentBatch::default();
    second.add_object_intent("p1", id, Action::Produce, produce_intent("battery"));
    f.settle(second);
    assert_eq!(
        data_of(&f, id).pointer("/store/battery"),
        Some(&json!(50)),
        "冷却期内不得重复生产"
    );
}

/// 意图名与载荷字段：produce 解析、resourceType 走既有字段清洗。
#[test]
fn factory_intent_name_and_payload() {
    assert_eq!(Action::parse("produce"), Some(Action::Produce));
    assert_eq!(Action::Produce.name(), "produce");
    let raw = json!({"resourceType": "battery"});
    let d = kk_engine::intent::sanitize_object_intent(Action::Produce, &raw);
    assert_eq!(d.resource_type.as_deref(), Some("battery"));
}
