//! B.2 link 默认贡献：有效规则贯穿放置、建造、结算与配额，
//! 处理链（before/wrap/replace/after）的执行序与 next 语义，
//! 以及装配对缺失实现的拒绝。

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use kk_engine::settle::contrib::{ActionHandler, ContributionDispatch, TickHandler};
use kk_mod_api::builtin::{self, LinkRules};
use kk_mod_api::{Contribution, FreezeOptions, HandlerDescriptor, Key, RuleOverride, Source};
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

fn tower(f: &mut Fixture, x: u8, y: u8, energy: i64) -> ObjId {
    place_generic(
        f,
        x,
        y,
        json!({
            "type": "structure", "structureType": "tower", "owner": "p1",
            "hits": 3000, "hitsMax": 3000, "nextDecayTime": 1_000_000,
            "store": {"energy": energy}, "storeCapacity": 1000,
            "storeCapacityResource": {"energy": 1000},
        }),
    )
}

fn data_of(f: &Fixture, id: ObjId) -> serde_json::Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("Generic 不存在：{other:?}"),
    }
}

/// 构造只带规则覆盖的测试贡献；namespace 须为小写合法命名空间。
fn overrides_mod(rules: &[(&str, kk_mod_api::Number)]) -> Contribution {
    let mut c = Contribution {
        source: Source {
            namespace: "testmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [9; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
    };
    for (rule, value) in rules {
        c.overrides.push(RuleOverride {
            rule: Key::new(format!("builtin:{rule}")).unwrap(),
            value: value.clone(),
        });
    }
    c
}

/// 冻结 builtin link + 覆盖，并提取有效规则。
fn frozen_plan(rules: &[(&str, kk_mod_api::Number)]) -> kk_mod_api::ContributionPlan {
    kk_mod_api::freeze(
        vec![builtin::link(), overrides_mod(rules)],
        FreezeOptions::default(),
    )
    .unwrap()
}

fn rules_of(plan: &kk_mod_api::ContributionPlan) -> LinkRules {
    LinkRules::from_plan(plan.rules()).unwrap()
}

/// 显式规则的放置上下文；控制器可为 None。
fn place_ctx(links: LinkRules) -> kk_world::place::PlaceCtx<'static> {
    kk_world::place::PlaceCtx {
        time: 50,
        links,
        controller: None,
        name_seq: 0,
    }
}

/// 规则覆盖贯穿放置默认值：容量、hits 与工地总进度读取同一有效规则，
/// 不再各自直读 kk-core 常量。
#[test]
fn rule_overrides_flow_into_placement_defaults() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.hits", kk_mod_api::Number::Signed(500)),
        ("link.hitsMax", kk_mod_api::Number::Signed(600)),
        ("link.buildCost", kk_mod_api::Number::Signed(120)),
    ]);
    let links = rules_of(&plan);

    let obj = kk_world::place::normalize(
        "structure",
        &json!({"structureType": "link", "pos": [10, 11], "owner": "p1"}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::Generic(g) = obj else {
        panic!("link 放置为 Generic");
    };
    let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(data["storeCapacity"], json!(400), "平面容量应取覆盖值");
    assert_eq!(
        data["storeCapacityResource"]["energy"],
        json!(400),
        "按资源容量应取覆盖值"
    );
    assert_eq!(data["hits"], json!(500));
    assert_eq!(data["hitsMax"], json!(600));
    // 显式放置字段不被默认值覆盖。
    let explicit = kk_world::place::normalize(
        "structure",
        &json!({"structureType": "link", "pos": [12, 11], "owner": "p1",
                "storeCapacity": 111, "storeCapacityResource": {"energy": 222}}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::Generic(eg) = explicit else {
        panic!();
    };
    let ed: serde_json::Value = serde_json::from_str(&eg.data).unwrap();
    assert_eq!(ed["storeCapacity"], json!(111));
    assert_eq!(ed["storeCapacityResource"]["energy"], json!(222));

    // 工地总进度来自同一 buildCost 规则。
    let site = kk_world::place::normalize(
        "constructionSite",
        &json!({"structureType": "link", "pos": [13, 11]}),
        &place_ctx(links),
    )
    .unwrap();
    let RoomObject::ConstructionSite(s) = site else {
        panic!();
    };
    assert_eq!(s.progress_total, 120);
}

/// 规则覆盖贯穿结算：转移损耗、冷却系数与目标容量钳都读有效规则。
#[test]
fn rule_overrides_flow_into_transfer_settlement() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.lossRatio", kk_mod_api::Number::Float(0.5)),
        ("link.cooldown", kk_mod_api::Number::Signed(3)),
    ]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    // link 为 RCL5 结构：rcl3 配额为 0 会让转移被配额门拒绝。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let l2 = place_generic(
        &mut f,
        13,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, links);

    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    // 损耗 ceil(200*0.5)=100：源扣 200，目标进 100。
    assert_eq!(d1["store"]["energy"], json!(100));
    assert_eq!(d2["store"]["energy"], json!(100));
    // 冷却 = 3 × 切比雪夫距离 3 = 9，同轮对象 tick 再减一（官方先加后减序）。
    assert_eq!(d1["cooldown"], json!(8));

    // 目标余量钳走权威字段 storeCapacityResource（放置时已按覆盖规则落值）；
    // 此处验证字段链路，规则到字段的贯穿由放置测试锁定。
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l2,
        Action::Transfer,
        IntentData {
            id: Some(l1),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    // 清两轮冷却并把 l1 预置到余量 50 的位置。
    for id in [l1, l2] {
        if let Some(RoomObject::Generic(g)) = f.world.get_mut(id) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            v["cooldown"] = json!(0);
            if id == l1 {
                v["store"]["energy"] = json!(350);
            }
            g.data = v.to_string();
        }
    }
    f.settle_with_rules(batch, links);
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(375), "只进钳后 50−损耗 25");
    assert_eq!(d2["store"]["energy"], json!(50));
}

/// RCL 配额覆盖生效：配额为 0 时转移被控制器检查拒绝，能量不动。
#[test]
fn rule_override_rcl_quota_gates_transfer() {
    // 覆盖 rcl5 为 0：官方表 rcl5=2，与覆盖值形成分歧窗口；
    // 若配额绕回官方表，转移会错误通过。
    let plan = frozen_plan(&[
        ("link.rcl5", kk_mod_api::Number::Signed(0)),
        ("link.rcl8", kk_mod_api::Number::Signed(0)),
    ]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, links);
    let d1 = data_of(&f, l1);
    assert_eq!(d1["store"]["energy"], json!(300), "配额为 0 时转移被拒");
}

/// 装配拒绝：处理器声明存在但实现缺失；声明了未安装的绑定。
#[test]
fn assembly_rejects_missing_handlers_and_unwired_bindings() {
    // 带一个 before 处理器的贡献。
    let mut with_handler = overrides_mod(&[]);
    let handler_key = Key::new("testmod:hook").unwrap();
    with_handler.bindings.insert(handler_key.clone());
    with_handler.handlers.push(HandlerDescriptor {
        key: handler_key.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Before,
        binding: handler_key,
        depends_on: BTreeSet::new(),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), with_handler],
        FreezeOptions::default(),
    )
    .unwrap();
    let err = kk_engine::Engine::with_plan(
        kk_world::World::new(1),
        plan.clone().into(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .err()
    .expect("缺处理器装配失败");
    assert!(err.contains("testmod:hook"), "{err}");

    // 外部贡献不能占用 builtin 命名空间声明键；freeze 先行拒绝，
    // 未安装绑定的拒收在引擎装配层仍是第二道防线。
    let mut stray = overrides_mod(&[]);
    stray.bindings.insert(Key::new("testmod:extra").unwrap());
    stray.source.namespace = "builtin".into();
    let freeze_err = kk_mod_api::freeze(vec![builtin::link(), stray], FreezeOptions::default())
        .expect_err("builtin 命名空间占用被拒绝");
    assert!(freeze_err.to_string().contains("builtin"), "{freeze_err}");
}

/// 处理链执行序：before → wrap(外) → native → wrap(内返回) → after（冻结时已逆序）。
/// wrap 内可修改候选，修改对 native 可见。
#[test]
fn chained_handlers_execute_in_frozen_order() {
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let mut contribution = overrides_mod(&[]);
    let mut hook = |name: &str| {
        let key = Key::new(format!("testmod:{name}")).unwrap();
        contribution.bindings.insert(key.clone());
        key
    };
    let before = hook("before1");
    let wrap = hook("wrap1");
    let after_a = hook("after_a");
    let after_b = hook("after_b");
    let descriptor = |key: &Key, kind| HandlerDescriptor {
        key: key.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind,
        binding: key.clone(),
        depends_on: BTreeSet::new(),
    };
    contribution
        .handlers
        .push(descriptor(&before, kk_mod_api::HandlerKind::Before));
    contribution
        .handlers
        .push(descriptor(&wrap, kk_mod_api::HandlerKind::Wrap));
    contribution
        .handlers
        .push(descriptor(&after_a, kk_mod_api::HandlerKind::After));
    // after_b 依赖 after_a：拓扑保证 after_a 先注册，冻结输出已逆序（after_b 先执行）。
    contribution.handlers.push(HandlerDescriptor {
        key: after_b.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::After,
        binding: after_b.clone(),
        depends_on: BTreeSet::from([after_a.clone()]),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使官方配额非零：native 转移可真实执行，截断/替换断言才有判别力。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );

    let log_before = log.clone();
    let log_wrap = log.clone();
    let log_after_a = log.clone();
    let log_after_b = log.clone();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        before.clone(),
        ActionHandler::Before(Box::new(move |_, _, _| {
            log_before.borrow_mut().push("before1".into());
        })),
    );
    handlers.insert(
        wrap,
        ActionHandler::Wrap(Box::new(move |w, _id, _d, next| {
            log_wrap.borrow_mut().push("wrap-enter".into());
            // wrap 先把源能量改小，native 读到修改后的候选：
            // 写 250 时 native 余 50（读旧值 300 则余 100，可分辨）。
            if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&_id) {
                let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["energy"] = json!(250);
                g.data = v.to_string();
            }
            let result = next.call(w, _id, _d);
            log_wrap.borrow_mut().push("wrap-exit".into());
            result
        })),
    );
    handlers.insert(
        after_a.clone(),
        ActionHandler::After(Box::new(move |_, _, _| {
            log_after_a.borrow_mut().push("after_a".into());
        })),
    );
    handlers.insert(
        after_b.clone(),
        ActionHandler::After(Box::new(move |_, _, _| {
            log_after_b.borrow_mut().push("after_b".into());
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_dispatch(batch, &dispatch);

    assert_eq!(
        *log.borrow(),
        vec!["before1", "wrap-enter", "wrap-exit", "after_b", "after_a"],
        "after 依赖序在冻结时逆序，执行器顺序调用"
    );
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    // wrap 改源为 250 后 native 转 200：源余 50，目标进 200−ceil(6)=194。
    assert_eq!(
        d1["store"]["energy"],
        json!(50),
        "native 读到 wrap 修改后的候选"
    );
    assert_eq!(d2["store"]["energy"], json!(194));
}

/// wrap 不调用 next：native 默认动作被截止，链仍成功，after 照常执行。
#[test]
fn wrap_without_next_suppresses_default_but_not_after() {
    let mut contribution = overrides_mod(&[]);
    let key = Key::new("testmod:gate").unwrap();
    contribution.bindings.insert(key.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: key.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: key.clone(),
        depends_on: BTreeSet::new(),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let seen = Rc::new(RefCell::new(false));
    let seen_flag = seen.clone();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key.clone(),
        ActionHandler::Wrap(Box::new(move |_, _, _, _next| {
            *seen_flag.borrow_mut() = true;
            Ok(())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使官方配额非零：native 转移可真实执行，截断/替换断言才有判别力。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_dispatch(batch, &dispatch);
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(300), "native 未执行");
    assert_eq!(d2["store"]["energy"], json!(0));
    assert!(d1.get("cooldown").is_none(), "native 未执行，冷却从未写入");
    assert!(*seen.borrow(), "wrap 处理器已执行");
}

/// replace 替换中心：默认动作不执行，替换实现全权处理。
#[test]
fn replace_supplants_native_transfer() {
    let mut contribution = overrides_mod(&[]);
    let key = Key::new("testmod:instead").unwrap();
    contribution.bindings.insert(key.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: key.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Replace,
        binding: key.clone(),
        depends_on: BTreeSet::new(),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let mut handlers = BTreeMap::new();
    handlers.insert(
        key.clone(),
        ActionHandler::Replace(Box::new(|w, id, _d| {
            // 替换实现：把源能量对半写回，不触碰目标。
            let Some(src_e) = w.room.objects.get(&id).and_then(|o| match o {
                RoomObject::Generic(g) => {
                    let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                    v["store"]["energy"].as_i64()
                }
                _ => None,
            }) else {
                return;
            };
            if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
                let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["energy"] = json!(src_e / 2);
                v["cooldown"] = json!(42);
                g.data = v.to_string();
            }
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使官方配额非零：native 转移可真实执行，截断/替换断言才有判别力。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_dispatch(batch, &dispatch);
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(150), "替换实现写半量");
    // 替换写 42，同轮对象 tick 递减一（先加后减的官方序）。
    assert_eq!(d1["cooldown"], json!(41));
    assert_eq!(d2["store"]["energy"], json!(0), "目标不被默认动作写入");
}

/// wrap 重复调用 next：本轮结算候选失败，世界保持上一 tick。
#[test]
fn double_next_fails_the_round_and_keeps_old_state() {
    let mut contribution = overrides_mod(&[]);
    let key = Key::new("testmod:twice").unwrap();
    contribution.bindings.insert(key.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: key.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: key.clone(),
        depends_on: BTreeSet::new(),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key.clone(),
        ActionHandler::Wrap(Box::new(|w, id, d, next| {
            next.call(w, id, d)?;
            next.call(w, id, d)
        })),
    );
    // 装配校验本身须通过；执行走引擎路径（处理器在下方重新提供）。
    drop(ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap());

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使官方配额非零：native 转移可真实执行，截断/替换断言才有判别力。
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );

    // 经引擎整轮驱动：处理器错误使候选丢弃，已提交世界保持上一 tick。
    // 处理器 map 已被 dispatch 装配消耗，为引擎重新构造一份。
    let mut handlers2 = BTreeMap::new();
    handlers2.insert(
        key.clone(),
        ActionHandler::Wrap(Box::new(|w, id, d, next| {
            next.call(w, id, d)?;
            next.call(w, id, d)
        })),
    );
    let world = std::mem::replace(&mut f.world, kk_world::World::new(0));
    let mut engine =
        kk_engine::Engine::with_plan(world, plan.into(), handlers2, BTreeMap::new()).unwrap();
    let err = engine.tick_with_intents(batch).unwrap_err();
    assert!(err.0.contains("重复调用 next"), "{}", err.0);
    // 已提交世界保持上一 tick：能量未动、时间未推进、run_state 进恢复态。
    let d1: serde_json::Value = match engine.world.get(l1).map(|(_, o)| o.clone()) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("link 仍在世界中：{other:?}"),
    };
    assert_eq!(d1["store"]["energy"], json!(300), "候选被丢弃，世界未提交");
    assert_eq!(engine.world.time(), 0);
    assert!(
        matches!(
            engine.run_state(),
            kk_engine::settle::RunState::NeedsRecovery { .. }
        ),
        "失败进入恢复态"
    );
}

/// 覆盖规则下的自转移：只扣 ceil(amount*loss_ratio)，冷却按距离 0 不增。
#[test]
fn rule_overrides_flow_into_self_transfer() {
    let plan = frozen_plan(&[
        ("link.capacity", kk_mod_api::Number::Signed(400)),
        ("link.lossRatio", kk_mod_api::Number::Float(0.5)),
        ("link.cooldown", kk_mod_api::Number::Signed(3)),
    ]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 400,
            "storeCapacityResource": {"energy": 400},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l1),
            amount: Some(200),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, links);
    let d1 = data_of(&f, l1);
    // 自转移先过自身余量钳（300+200>400 → amount=100），再扣损耗
    // ceil(100*0.5)=50：300−50=250；距离 0，冷却增加 3×0=0。
    assert_eq!(d1["store"]["energy"], json!(250));
    assert_eq!(d1["cooldown"], json!(0));
}

/// 玩家工地的 RCL 配额门与转移门同源：覆盖 rcl5=0 后工地创建被拒。
#[test]
fn rule_override_rcl_quota_gates_player_site_creation() {
    // 官方 rcl5=2；覆盖为 0 形成分歧窗口（绕回官方表则工地照建）。
    let plan = frozen_plan(&[
        ("link.rcl5", kk_mod_api::Number::Signed(0)),
        ("link.rcl8", kk_mod_api::Number::Signed(0)),
    ]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateConstructionSite {
            room: "W1N1".into(),
            x: 20,
            y: 20,
            structure_type: "link".into(),
            name: None,
        },
    );
    f.settle_with_rules(batch, links);
    let sites = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter(|o| matches!(o, RoomObject::ConstructionSite(s) if s.structure_type == "link"))
        .count();
    assert_eq!(sites, 0, "配额覆盖为 0 时玩家 link 工地被拒");
}

/// 双层 wrap：每层 Next 计数独立，执行序为外入→内入→中心→内出→外出；
/// 内层截断（不调 next）时中心不执行，错误沿链向外传播。
#[test]
fn nested_wraps_execute_layer_by_layer() {
    let mut contribution = overrides_mod(&[]);
    let mut hook = |name: &str| {
        let key = Key::new(format!("testmod:{name}")).unwrap();
        contribution.bindings.insert(key.clone());
        key
    };
    let outer = hook("outer");
    let inner = hook("inner");
    // inner 依赖 outer：wrap 拓扑依赖在前为外层，outer 包裹 inner。
    contribution.handlers.push(HandlerDescriptor {
        key: outer.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: outer.clone(),
        depends_on: BTreeSet::new(),
    });
    contribution.handlers.push(HandlerDescriptor {
        key: inner.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: inner.clone(),
        depends_on: BTreeSet::from([outer.clone()]),
    });
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log_outer = log.clone();
    let log_inner = log.clone();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        outer.clone(),
        ActionHandler::Wrap(Box::new(move |w, id, d, next| {
            log_outer.borrow_mut().push("outer-enter".into());
            let result = next.call(w, id, d);
            log_outer.borrow_mut().push("outer-exit".into());
            result
        })),
    );
    handlers.insert(
        inner.clone(),
        ActionHandler::Wrap(Box::new(move |w, id, d, next| {
            log_inner.borrow_mut().push("inner-enter".into());
            let result = next.call(w, id, d);
            log_inner.borrow_mut().push("inner-exit".into());
            result
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 300}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let l2 = place_generic(
        &mut f,
        12,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        Action::Transfer,
        IntentData {
            id: Some(l2),
            amount: Some(100),
            resource_type: Some("energy".into()),
            ..IntentData::bare()
        },
    );
    f.settle_with_dispatch(batch, &dispatch);
    assert_eq!(
        *log.borrow(),
        vec!["outer-enter", "inner-enter", "inner-exit", "outer-exit"],
        "双层 wrap 由外到内包裹、由内到外退出"
    );
    let d2 = data_of(&f, l2);
    assert_eq!(
        d2["store"]["energy"],
        json!(97),
        "中心经两层 next 执行：100−ceil(3%)=97"
    );
}

/// tick 槽的 Chained 链：wrap 调 next 时 native 冷却递减仍执行，
/// before/after 按冻结序运行。
#[test]
fn tick_slot_chained_wraps_native_cooldown() {
    let mut contribution = overrides_mod(&[]);
    let mut hook = |name: &str| {
        let key = Key::new(format!("testmod:{name}")).unwrap();
        contribution.bindings.insert(key.clone());
        key
    };
    let before = hook("tick_before");
    let wrap = hook("tick_wrap");
    let after = hook("tick_after");
    for (key, kind) in [
        (&before, kk_mod_api::HandlerKind::Before),
        (&wrap, kk_mod_api::HandlerKind::Wrap),
        (&after, kk_mod_api::HandlerKind::After),
    ] {
        contribution.handlers.push(HandlerDescriptor {
            key: key.clone(),
            slot: Key::new("builtin:link.tick").unwrap(),
            kind,
            binding: key.clone(),
            depends_on: BTreeSet::new(),
        });
    }
    let plan = kk_mod_api::freeze(
        vec![builtin::link(), contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let mut tick_handlers = BTreeMap::new();
    let log_b = log.clone();
    tick_handlers.insert(
        before,
        TickHandler::Before(Box::new(move |_, _| {
            log_b.borrow_mut().push("before".into());
        })),
    );
    let log_w = log.clone();
    tick_handlers.insert(
        wrap,
        TickHandler::Wrap(Box::new(move |w, id, next| {
            log_w.borrow_mut().push("wrap-enter".into());
            let result = next.call(w, id);
            log_w.borrow_mut().push("wrap-exit".into());
            result
        })),
    );
    let log_a = log.clone();
    tick_handlers.insert(
        after,
        TickHandler::After(Box::new(move |_, _| {
            log_a.borrow_mut().push("after".into());
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, BTreeMap::new(), tick_handlers).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
            "cooldown": 5,
        }),
    );
    f.settle_with_dispatch(IntentBatch::default(), &dispatch);
    assert_eq!(
        *log.borrow(),
        vec!["before", "wrap-enter", "wrap-exit", "after"]
    );
    let d1 = data_of(&f, l1);
    assert_eq!(d1["cooldown"], json!(4), "native 递减经 wrap 链执行");
}

/// 玩家 createConstructionSite 的 link 工地总进度跟随 buildCost 覆盖，
/// 与放置夹具路径（normalize_site）读同一规则。
#[test]
fn rule_override_build_cost_flows_into_player_site() {
    let plan = frozen_plan(&[("link.buildCost", kk_mod_api::Number::Signed(120))]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateConstructionSite {
            room: "W1N1".into(),
            x: 20,
            y: 20,
            structure_type: "link".into(),
            name: None,
        },
    );
    f.settle_with_rules(batch, links);
    let site = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructionSite(s) if s.structure_type == "link" => Some(s.progress_total),
            _ => None,
        })
        .expect("玩家 link 工地已创建");
    assert_eq!(site, 120, "工地总进度取覆盖值而非官方表 5000");
}

/// hitsMax 规则覆盖钳住修复：tower 修理不越过覆盖上限。
#[test]
fn rule_override_hits_max_caps_tower_repair() {
    let plan = frozen_plan(&[
        ("link.hitsMax", kk_mod_api::Number::Signed(600)),
        ("link.hits", kk_mod_api::Number::Signed(100)),
    ]);
    let links = rules_of(&plan);

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使官方配额非零：native 转移可真实执行，截断/替换断言才有判别力。
    f.controller(5, Some("p1"));
    let tower = tower(&mut f, 5, 5, 1000);
    let link = place_generic(
        &mut f,
        8,
        8,
        json!({
            "type": "structure", "structureType": "link", "owner": "p1",
            "hits": 500, "hitsMax": 600, "nextDecayTime": 1_000_000,
            "store": {"energy": 0}, "storeCapacity": 800,
            "storeCapacityResource": {"energy": 800},
        }),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        tower,
        Action::Repair,
        IntentData {
            id: Some(link),
            ..IntentData::bare()
        },
    );
    f.settle_with_rules(batch, links);
    let d = data_of(&f, link);
    assert_eq!(d["hits"], json!(600), "修复钳到覆盖的 hitsMax=600");
}
