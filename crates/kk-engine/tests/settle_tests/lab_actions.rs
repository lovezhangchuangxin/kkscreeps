//! 测试 lab 四动作结算：反应产出与消耗、冷却、boost 部件选择 quirk、
//! unboost 掉落与冷却。行为锚点：官方 processor/intents/labs/*.js 与
//! screeps-ok 的 LAB-RUN/LAB-REVERSE/BOOST-CREEP/UNBOOST 用例。

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

/// 放置 lab；minerals 形如 {"UH": 90}，默认绑定 3000 矿物槽。
fn lab(f: &mut Fixture, x: u8, y: u8, minerals: serde_json::Value, cooldown_time: u64) -> ObjId {
    let mut store = json!({"energy": 2000});
    let mut slots = json!({"energy": 2000});
    if let (Some(m), Some(s)) = (store.as_object_mut(), slots.as_object_mut()) {
        for (k, v) in minerals.as_object().into_iter().flatten() {
            m.insert(k.clone(), v.clone());
            s.insert(k.clone(), json!(3000));
        }
    }
    let mut data = json!({
        "type": "structure", "structureType": "lab", "owner": "p1",
        "hits": 500, "hitsMax": 500,
        "store": store, "storeCapacityResource": slots,
    });
    if cooldown_time > 0 {
        data["cooldownTime"] = json!(cooldown_time);
    }
    place_generic(f, x, y, data)
}

fn data_of(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Generic 不存在：{other:?}"),
    }
}

fn reaction_intent(lab1: ObjId, lab2: ObjId) -> IntentData {
    let mut d = IntentData::bare();
    d.lab1 = Some(lab1);
    d.lab2 = Some(lab2);
    d
}

/// run-reaction.js：H+O→OH——调用 lab 得 5 产物并绑定容量槽、两试剂 lab
/// 各扣 5、冷却 REACTION_TIME[OH]=20。
#[test]
fn lab_run_reaction_produces_consumes_and_cools() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let calling = lab(&mut f, 25, 25, json!({}), 0);
    let l1 = lab(&mut f, 25, 27, json!({"H": 100}), 0);
    let l2 = lab(&mut f, 27, 25, json!({"O": 100}), 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", calling, Action::RunReaction, reaction_intent(l1, l2));
    f.settle(batch);
    let d = data_of(&f, calling);
    assert_eq!(d.pointer("/store/OH"), Some(&json!(5)));
    assert_eq!(d.pointer("/storeCapacityResource/OH"), Some(&json!(3000)));
    assert_eq!(d["storeCapacity"], json!(null));
    assert_eq!(d["cooldownTime"], json!(20));
    assert_eq!(data_of(&f, l1).pointer("/store/H"), Some(&json!(95)));
    assert_eq!(data_of(&f, l2).pointer("/store/O"), Some(&json!(95)));
}

/// 试剂扣到 0 时解绑矿物槽并复位共享容量（官方 storeCapacity 复位分支）。
#[test]
fn lab_run_reaction_unbinds_drained_mineral_slot() {
    let mut f = Fixture::new(2, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let calling = lab(&mut f, 25, 25, json!({}), 0);
    let l1 = lab(&mut f, 25, 27, json!({"H": 5}), 0);
    let l2 = lab(&mut f, 27, 25, json!({"O": 5}), 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", calling, Action::RunReaction, reaction_intent(l1, l2));
    f.settle(batch);
    let d1 = data_of(&f, l1);
    assert_eq!(d1.pointer("/store/H"), Some(&json!(0)));
    assert!(d1.pointer("/storeCapacityResource/H").is_none());
    assert_eq!(d1["storeCapacity"], json!(5000));
    assert_eq!(data_of(&f, l2)["storeCapacity"], json!(5000));
}

/// run 的拒绝族：冷却、射程 >2、试剂不足、H+H 无产物、调用 lab 持非产物
/// 矿物、调用 lab 已满——全部静默 no-op（错误码在玩家预检层）。
#[test]
fn lab_run_reaction_silent_rejections() {
    for (case, calling_mineral, l2_pos, l2_mineral, cooldown) in [
        ("cooldown", json!({}), (27, 25), json!({"O": 100}), 5u64),
        ("range", json!({}), (28, 25), json!({"O": 100}), 0),
        ("not-enough", json!({}), (27, 25), json!({"O": 4}), 0),
        ("no-product", json!({}), (27, 25), json!({"H": 100}), 0),
        (
            "wrong-mineral",
            json!({"Z": 100}),
            (27, 25),
            json!({"O": 100}),
            0,
        ),
        ("full", json!({"OH": 3000}), (27, 25), json!({"O": 100}), 0),
    ] {
        let mut f = Fixture::new(3, terrain_with(&[]));
        f.controller(6, Some("p1"));
        let calling = lab(&mut f, 25, 25, calling_mineral.clone(), cooldown);
        let l1 = lab(&mut f, 25, 27, json!({"H": 100}), 0);
        let l2 = lab(&mut f, l2_pos.0, l2_pos.1, l2_mineral, 0);
        let mut batch = IntentBatch::default();
        batch.add_object_intent("p1", calling, Action::RunReaction, reaction_intent(l1, l2));
        f.settle(batch);
        let d = data_of(&f, calling);
        let oh_before = calling_mineral.get("OH").cloned().unwrap_or(json!(0));
        assert_eq!(
            d.pointer("/store/OH").cloned().unwrap_or(json!(0)),
            oh_before,
            "{case} 不得产出"
        );
        if cooldown == 0 {
            assert!(d.get("cooldownTime").is_none(), "{case} 不得进入冷却");
        } else {
            assert_eq!(d["cooldownTime"], json!(cooldown), "{case} 保留放置冷却");
        }
        assert_eq!(
            data_of(&f, l1).pointer("/store/H"),
            Some(&json!(100)),
            "{case} 不得消耗试剂"
        );
    }
}

/// 冷却中的第二次 run 被冷却门拦截；boost 无冷却门（官方 processor 无
/// 检查）——冷却期内仍可强化。
#[test]
fn lab_cooldown_blocks_second_run_but_not_boost() {
    let mut f = Fixture::new(4, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let calling = lab(&mut f, 25, 25, json!({}), 0);
    let l1 = lab(&mut f, 25, 27, json!({"H": 200}), 0);
    let l2 = lab(&mut f, 27, 25, json!({"O": 200}), 0);
    let mut first = IntentBatch::default();
    first.add_object_intent("p1", calling, Action::RunReaction, reaction_intent(l1, l2));
    f.settle(first);
    assert_eq!(data_of(&f, calling)["cooldownTime"], json!(20));
    let mut second = IntentBatch::default();
    second.add_object_intent("p1", calling, Action::RunReaction, reaction_intent(l1, l2));
    f.settle(second);
    assert_eq!(
        data_of(&f, calling).pointer("/store/OH"),
        Some(&json!(5)),
        "冷却期内不得重复反应"
    );

    // boost 不受冷却门：另置冷却中的 lab 仍完成强化并消耗。
    let boosting = lab(&mut f, 25, 28, json!({"UH": 60}), 20);
    let creep = f.creep("w1", "p1", 25, 27, &[BodyKind::Attack, BodyKind::Move], 0);
    let mut third = IntentBatch::default();
    third.add_object_intent(
        "p1",
        boosting,
        Action::BoostCreep,
        IntentData::target(creep),
    );
    f.settle(third);
    match f.world.get(creep).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => assert_eq!(c.body[0].boost.as_deref(), Some("UH")),
        other => panic!("creep 存活：{other:?}"),
    }
    assert_eq!(data_of(&f, boosting).pointer("/store/UH"), Some(&json!(30)));
}

/// reverse-reaction.js：OH 分解为 H+O 分发到两输出 lab（按内容相容方向），
/// 调用 lab 扣 5 并冷却 REACTION_TIME[OH]=20；same-lab 拒绝。
#[test]
fn lab_reverse_reaction_splits_compound() {
    let mut f = Fixture::new(5, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let calling = lab(&mut f, 25, 25, json!({"OH": 100}), 0);
    let l1 = lab(&mut f, 25, 27, json!({}), 0);
    let l2 = lab(&mut f, 27, 25, json!({}), 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        calling,
        Action::ReverseReaction,
        reaction_intent(l1, l2),
    );
    f.settle(batch);
    assert_eq!(data_of(&f, calling).pointer("/store/OH"), Some(&json!(95)));
    assert_eq!(data_of(&f, calling)["cooldownTime"], json!(20));
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    // 空输出 lab 按首个相容方向各得一种试剂；两 lab 合计 H=5、O=5。
    let h1 = d1.pointer("/store/H").and_then(|v| v.as_i64()).unwrap_or(0);
    let h2 = d2.pointer("/store/H").and_then(|v| v.as_i64()).unwrap_or(0);
    let o1 = d1.pointer("/store/O").and_then(|v| v.as_i64()).unwrap_or(0);
    let o2 = d2.pointer("/store/O").and_then(|v| v.as_i64()).unwrap_or(0);
    assert_eq!(h1 + h2, 5);
    assert_eq!(o1 + o2, 5);
    assert_ne!(h1 + o1, 10, "每个输出 lab 只得一种试剂");

    // same-lab 与 base 矿物（无试剂对）均静默 no-op。
    let mut again = IntentBatch::default();
    again.add_object_intent(
        "p1",
        calling,
        Action::ReverseReaction,
        reaction_intent(l1, l1),
    );
    f.settle(again);
    assert_eq!(data_of(&f, calling).pointer("/store/OH"), Some(&json!(95)));
    let base = lab(&mut f, 25, 28, json!({"H": 100}), 0);
    let l3 = lab(&mut f, 27, 28, json!({}), 0);
    let mut no_pair = IntentBatch::default();
    no_pair.add_object_intent("p1", base, Action::ReverseReaction, reaction_intent(l3, l3));
    f.settle(no_pair);
    assert_eq!(data_of(&f, base).pointer("/store/H"), Some(&json!(100)));
}

/// 输出 lab 已持矿物且加量越槽容量（满仓）时静默 no-op。
#[test]
fn lab_reverse_reaction_rejects_full_output() {
    let mut f = Fixture::new(6, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let calling = lab(&mut f, 25, 25, json!({"OH": 100}), 0);
    let l1 = lab(&mut f, 25, 27, json!({"H": 3000}), 0);
    let l2 = lab(&mut f, 27, 25, json!({}), 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        calling,
        Action::ReverseReaction,
        reaction_intent(l1, l2),
    );
    f.settle(batch);
    assert_eq!(data_of(&f, calling).pointer("/store/OH"), Some(&json!(100)));
    assert_eq!(data_of(&f, l1).pointer("/store/H"), Some(&json!(3000)));
    assert!(data_of(&f, l2).get("store").unwrap()["H"].is_null());
}

/// boost-creep.js：每部件耗 30 矿 + 20 能量；资源不足即停并解绑清零矿物。
#[test]
fn lab_boost_creep_consumes_per_part() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let boosting = lab(&mut f, 25, 25, json!({"UH": 60}), 0);
    let creep = f.creep("w1", "p1", 25, 26, &[BodyKind::Attack, BodyKind::Move], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        boosting,
        Action::BoostCreep,
        IntentData::target(creep),
    );
    f.settle(batch);
    match f.world.get(creep).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => {
            assert_eq!(c.body[0].boost.as_deref(), Some("UH"));
            assert_eq!(c.body[1].boost, None, "UH 不强化 MOVE");
            // 官方 quirk：目标写 healed 动作位。
            assert_ne!(c.action_log & (1 << 7), 0);
        }
        other => panic!("creep 存活：{other:?}"),
    }
    let d = data_of(&f, boosting);
    assert_eq!(d.pointer("/store/UH"), Some(&json!(30)));
    assert_eq!(d.pointer("/store/energy"), Some(&json!(1980)));
}

/// TOUGH-first quirk：首个候选非 TOUGH 时从身体尾部选取——
/// bodyPartsCount=1 强化的是最后一个 ATTACK；TOUGH 开头则从头部选取。
#[test]
fn lab_boost_creep_tough_first_order() {
    let mut f = Fixture::new(8, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let atk_lab = lab(&mut f, 25, 25, json!({"UH": 60}), 0);
    let tough_lab = lab(&mut f, 20, 20, json!({"GO": 60}), 0);
    let atk = f.creep("a1", "p1", 25, 26, &[BodyKind::Attack, BodyKind::Attack], 0);
    let tough = f.creep("t1", "p1", 20, 21, &[BodyKind::Tough, BodyKind::Tough], 0);
    let mut batch = IntentBatch::default();
    let mut limit_atk = IntentData::target(atk);
    limit_atk.body_parts_count = Some(1);
    batch.add_object_intent("p1", atk_lab, Action::BoostCreep, limit_atk);
    let mut limit_tough = IntentData::target(tough);
    limit_tough.body_parts_count = Some(1);
    batch.add_object_intent("p1", tough_lab, Action::BoostCreep, limit_tough);
    f.settle(batch);
    match (
        f.world.get(atk).map(|(_, o)| o),
        f.world.get(tough).map(|(_, o)| o),
    ) {
        (Some(RoomObject::Creep(a)), Some(RoomObject::Creep(t))) => {
            assert_eq!(a.body[0].boost, None, "非 TOUGH 开头 → 尾部优先");
            assert_eq!(a.body[1].boost.as_deref(), Some("UH"));
            assert_eq!(
                t.body[0].boost.as_deref(),
                Some("GO"),
                "TOUGH 开头 → 头部优先"
            );
            assert_eq!(t.body[1].boost, None);
        }
        other => panic!("creep 存活：{other:?}"),
    }
}

/// 资源不足即停：矿 30 只够一个部件；矿物清零解绑槽、容量复位。
#[test]
fn lab_boost_creep_stops_on_exhaustion() {
    let mut f = Fixture::new(9, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let boosting = lab(&mut f, 25, 25, json!({"UH": 30}), 0);
    let creep = f.creep("w1", "p1", 25, 26, &[BodyKind::Attack, BodyKind::Attack], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        boosting,
        Action::BoostCreep,
        IntentData::target(creep),
    );
    f.settle(batch);
    match f.world.get(creep).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => {
            let boosted = c.body.iter().filter(|p| p.boost.is_some()).count();
            assert_eq!(boosted, 1, "矿耗尽后停止");
        }
        other => panic!("creep 存活：{other:?}"),
    }
    let d = data_of(&f, boosting);
    assert_eq!(d.pointer("/store/UH"), Some(&json!(0)));
    assert!(d.pointer("/storeCapacityResource/UH").is_none());
    assert_eq!(d["storeCapacity"], json!(5000));
}

/// 无匹配部件（UH 对 CARRY）与超程（Chebyshev >1）均静默 no-op。
#[test]
fn lab_boost_creep_silent_rejections() {
    let mut f = Fixture::new(10, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let far_lab = lab(&mut f, 10, 10, json!({"UH": 60}), 0);
    let carry = f.creep("c1", "p1", 25, 26, &[BodyKind::Carry, BodyKind::Move], 0);
    let far = f.creep("c2", "p1", 10, 13, &[BodyKind::Attack, BodyKind::Move], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", far_lab, Action::BoostCreep, IntentData::target(carry));
    batch.add_object_intent("p1", far_lab, Action::BoostCreep, IntentData::target(far));
    f.settle(batch);
    for id in [carry, far] {
        match f.world.get(id).map(|(_, o)| o) {
            Some(RoomObject::Creep(c)) => {
                assert!(c.body.iter().all(|p| p.boost.is_none()))
            }
            other => panic!("creep 存活：{other:?}"),
        }
    }
    assert_eq!(data_of(&f, far_lab).pointer("/store/UH"), Some(&json!(60)));
}

/// unboost-creep.js：清强化、掉 15×部件 到 creep 格、冷却 Σ部件×链时长×15/5。
/// 掉落堆在本 tick 的资源衰减阶段再扣 ceil(30/1000)=1（观察值 29）。
#[test]
fn lab_unboost_creep_clears_drops_and_cools() {
    let mut f = Fixture::new(11, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let unboosting = lab(&mut f, 25, 25, json!({}), 0);
    let mut parts = super::body(&[BodyKind::Attack, BodyKind::Attack]);
    for p in parts.iter_mut() {
        p.boost = Some("UH".into());
    }
    let creep = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 25,
                y: 26,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "b1".into(),
                owner: "p1".into(),
                body: parts,
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        unboosting,
        Action::UnboostCreep,
        IntentData::target(creep),
    );
    f.settle(batch);
    match f.world.get(creep).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => {
            assert!(c.body.iter().all(|p| p.boost.is_none()));
        }
        other => panic!("creep 存活：{other:?}"),
    }
    // 2×UH 部件：冷却 = 2×10×15/5 = 60；掉落 30，同 tick 衰减 1。
    assert_eq!(data_of(&f, unboosting)["cooldownTime"], json!(60));
    let pile = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.resource.as_deref() == Some("UH") => Some(e.clone()),
            _ => None,
        })
        .expect("UH 掉落堆存在");
    assert_eq!((pile.x, pile.y), (25, 26));
    assert_eq!(pile.amount, 29);
}

/// 敌方 creep 不得被 unboost（官方 target.user 门），无强化部件同样 no-op。
#[test]
fn lab_unboost_creep_silent_rejections() {
    let mut f = Fixture::new(12, terrain_with(&[]));
    f.controller(6, Some("p1"));
    let unboosting = lab(&mut f, 25, 25, json!({}), 0);
    let mut enemy_parts = super::body(&[BodyKind::Attack]);
    enemy_parts[0].boost = Some("UH".into());
    let enemy = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 25,
                y: 26,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "e1".into(),
                owner: "p2".into(),
                body: enemy_parts,
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let plain = f.creep("p1c", "p1", 27, 25, &[BodyKind::Move], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        unboosting,
        Action::UnboostCreep,
        IntentData::target(enemy),
    );
    batch.add_object_intent(
        "p1",
        unboosting,
        Action::UnboostCreep,
        IntentData::target(plain),
    );
    f.settle(batch);
    match f.world.get(enemy).map(|(_, o)| o) {
        Some(RoomObject::Creep(c)) => {
            assert_eq!(c.body[0].boost.as_deref(), Some("UH"), "敌方目标不清强化")
        }
        other => panic!("enemy 存活：{other:?}"),
    }
    assert!(data_of(&f, unboosting).get("cooldownTime").is_none());
}

/// 意图名与官方载荷字段的往返：runReaction 四名解析、lab1/lab2 与
/// bodyPartsCount 的清洗及线形态。
#[test]
fn lab_intent_names_and_payload_roundtrip() {
    for (name, action) in [
        ("runReaction", Action::RunReaction),
        ("reverseReaction", Action::ReverseReaction),
        ("boostCreep", Action::BoostCreep),
        ("unboostCreep", Action::UnboostCreep),
    ] {
        assert_eq!(Action::parse(name), Some(action));
        assert_eq!(action.name(), name);
    }
    let raw = json!({
        "lab1": "kk0000000000000001",
        "lab2": "kk0000000000000002",
        "bodyPartsCount": 3,
    });
    let d = kk_engine::intent::sanitize_object_intent(Action::RunReaction, &raw);
    assert_eq!(
        d.lab1.map(|i| i.as_str().to_string()),
        Some("kk0000000000000001".into())
    );
    assert_eq!(
        d.lab2.map(|i| i.as_str().to_string()),
        Some("kk0000000000000002".into())
    );
    assert_eq!(d.body_parts_count, Some(3));
}
