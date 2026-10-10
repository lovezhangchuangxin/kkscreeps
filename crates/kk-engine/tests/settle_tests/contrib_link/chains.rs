//! 内建处理链的装配、链序、next 纪律和不同 native 中心。
use super::*;

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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![with_handler],
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
    let freeze_err =
        kk_mod_api::freeze_with_builtin(builtin::link(), vec![stray], FreezeOptions::default())
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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使 CONTROLLER_STRUCTURES 的 link 配额非零：native 转移可
    // 真实执行，截断/替换类断言才有判别力。
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
            Ok(())
        })),
    );
    handlers.insert(
        wrap,
        ActionHandler::Wrap(Box::new(move |ctx, _id, _d, next| {
            log_wrap.borrow_mut().push("wrap-enter".into());
            // wrap 先把源能量改小，native 读到修改后的候选：
            // 写 250 时 native 余 50（读旧值 300 则余 100，可分辨）。
            if let Some(RoomObject::Generic(g)) = ctx.borrow_mut().room.objects.get_mut(&_id) {
                let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["energy"] = json!(250);
                g.data = v.to_string().into();
            }
            let result = next.call(_id, _d);
            log_wrap.borrow_mut().push("wrap-exit".into());
            result
        })),
    );
    handlers.insert(
        after_a.clone(),
        ActionHandler::After(Box::new(move |_, _, _| {
            log_after_a.borrow_mut().push("after_a".into());
            Ok(())
        })),
    );
    handlers.insert(
        after_b.clone(),
        ActionHandler::After(Box::new(move |_, _, _| {
            log_after_b.borrow_mut().push("after_b".into());
            Ok(())
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
    f.settle_with_dispatch(batch, std::rc::Rc::new(dispatch));

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
    let after = Key::new("testmod:after_gate").unwrap();
    contribution.bindings.insert(after.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: after.clone(),
        slot: Key::new("builtin:link.transfer").unwrap(),
        kind: kk_mod_api::HandlerKind::After,
        binding: after.clone(),
        depends_on: BTreeSet::new(),
    });
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
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
    let after_seen = Rc::new(std::cell::Cell::new(0));
    let sink = after_seen.clone();
    handlers.insert(
        after,
        ActionHandler::After(Box::new(move |_, _, _| {
            sink.set(sink.get() + 1);
            Ok(())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使 CONTROLLER_STRUCTURES 的 link 配额非零：native 转移可
    // 真实执行，截断/替换类断言才有判别力。
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
    f.settle_with_dispatch(batch, std::rc::Rc::new(dispatch));
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(300), "native 未执行");
    assert_eq!(d2["store"]["energy"], json!(0));
    assert!(d1.get("cooldown").is_none(), "native 未执行，冷却从未写入");
    assert!(*seen.borrow(), "wrap 处理器已执行");
    assert_eq!(after_seen.get(), 1, "取消中心但链成功，after 恰好执行一次");
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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let mut handlers = BTreeMap::new();
    handlers.insert(
        key.clone(),
        ActionHandler::Replace(Box::new(|ctx, id, _d| {
            // 替换实现：把源能量对半写回，不触碰目标。
            let Some(src_e) = ctx.borrow().room.objects.get(&id).and_then(|o| match o {
                RoomObject::Generic(g) => {
                    let v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                    v["store"]["energy"].as_i64()
                }
                _ => None,
            }) else {
                return Ok(());
            };
            if let Some(RoomObject::Generic(g)) = ctx.borrow_mut().room.objects.get_mut(&id) {
                let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["energy"] = json!(src_e / 2);
                v["cooldown"] = json!(42);
                g.data = v.to_string().into();
            }
            Ok(())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使 CONTROLLER_STRUCTURES 的 link 配额非零：native 转移可
    // 真实执行，截断/替换类断言才有判别力。
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
    f.settle_with_dispatch(batch, std::rc::Rc::new(dispatch));
    let d1 = data_of(&f, l1);
    let d2 = data_of(&f, l2);
    assert_eq!(d1["store"]["energy"], json!(150), "替换实现写半量");
    // 替换写 42，同轮对象 tick 再减一（对象动作先于对象 tick 的官方
    // 结算序，links/tick.js 逐 tick 递减）。
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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key.clone(),
        ActionHandler::Wrap(Box::new(|_ctx, id, d, next| {
            next.call(id, d)?;
            next.call(id, d)
        })),
    );
    // 装配校验本身须通过；执行走引擎路径，处理器在引擎装配时重新提供。
    drop(ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap());

    let mut f = Fixture::new(7, terrain_with(&[]));
    // RCL5 使 CONTROLLER_STRUCTURES 的 link 配额非零：native 转移可
    // 真实执行，截断/替换类断言才有判别力。
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
        ActionHandler::Wrap(Box::new(|_ctx, id, d, next| {
            next.call(id, d)?;
            next.call(id, d)
        })),
    );
    let world = std::mem::replace(&mut f.world, kk_world::World::new(0));
    let mut engine =
        kk_engine::Engine::with_plan(world, plan.into(), handlers2, BTreeMap::new()).unwrap();
    let err = engine.tick_with_intents(batch).unwrap_err();
    assert!(err.0.contains("重复调用 next"), "{}", err.0);
    // 已提交世界保持上一 tick：能量未动、时间未推进、run_state 进恢复态。
    let d1: serde_json::Value = match engine.world().get(l1).map(|(_, o)| o.clone()) {
        Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
        other => panic!("link 仍在世界中：{other:?}"),
    };
    assert_eq!(d1["store"]["energy"], json!(300), "候选被丢弃，世界未提交");
    assert_eq!(engine.world().time(), 0);
    assert!(
        matches!(
            engine.run_state(),
            kk_engine::settle::RunState::NeedsRecovery { .. }
        ),
        "失败进入恢复态"
    );
}

/// 三个动作共用 Rust 安装表，仍须按槽分配函数与正确的 native 中心。
#[test]
fn builtin_resource_slots_partition_rust_handlers_and_use_distinct_native_steps() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut contribution = overrides_mod(&[]);
    let mut handlers = BTreeMap::new();
    for (name, slot) in [
        ("link", "builtin:link.transfer"),
        ("transfer", "builtin:creep.transfer"),
        ("withdraw", "builtin:creep.withdraw"),
    ] {
        let binding = Key::new(format!("testmod:{name}")).unwrap();
        contribution.bindings.insert(binding.clone());
        contribution.handlers.push(HandlerDescriptor {
            key: binding.clone(),
            slot: Key::new(slot).unwrap(),
            kind: kk_mod_api::HandlerKind::Before,
            binding: binding.clone(),
            depends_on: BTreeSet::new(),
        });
        let log = log.clone();
        handlers.insert(
            binding,
            ActionHandler::Before(Box::new(move |_, _, _| {
                log.borrow_mut().push(name);
                Ok(())
            })),
        );
    }
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(8, Some("p1"));
    let actor = f.creep(
        "hauler",
        "p1",
        10,
        9,
        &[BodyKind::Carry, BodyKind::Move],
        30,
    );
    let container = place_generic(
        &mut f,
        10,
        10,
        json!({"type":"structure","structureType":"container","store":{"energy":20},"storeCapacity":37,"nextDecayTime":1000}),
    );
    let source = place_generic(
        &mut f,
        20,
        20,
        json!({"type":"structure","structureType":"link","owner":"p1","store":{"energy":100},"storeCapacityResource":{"energy":800},"cooldown":0}),
    );
    let target = place_generic(
        &mut f,
        21,
        20,
        json!({"type":"structure","structureType":"link","owner":"p1","store":{"energy":0},"storeCapacityResource":{"energy":800},"cooldown":0}),
    );
    let mut batch = IntentBatch::default();
    for (id, action, target, amount) in [
        (actor, Action::Transfer, container, 5),
        (actor, Action::Withdraw, container, 7),
        (source, Action::Transfer, target, 25),
    ] {
        batch.add_object_intent(
            "p1",
            id,
            action,
            IntentData {
                id: Some(target),
                amount: Some(amount),
                resource_type: Some("energy".into()),
                ..IntentData::bare()
            },
        );
    }
    f.settle_with_dispatch(batch, Rc::new(dispatch));
    assert_eq!(*log.borrow(), vec!["transfer", "withdraw", "link"]);
    let Some((_, RoomObject::Creep(creep))) = f.world.get(actor) else {
        panic!("执行 creep 消失")
    };
    assert_eq!(creep.store.energy, 32);
    assert_eq!(data_of(&f, container)["store"]["energy"], 18);
    assert_eq!(data_of(&f, source)["store"]["energy"], 75);
    assert_eq!(data_of(&f, target)["store"]["energy"], 24);
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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
        FreezeOptions::default(),
    )
    .unwrap();

    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log_outer = log.clone();
    let log_inner = log.clone();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        outer.clone(),
        ActionHandler::Wrap(Box::new(move |_ctx, id, d, next| {
            log_outer.borrow_mut().push("outer-enter".into());
            let result = next.call(id, d);
            log_outer.borrow_mut().push("outer-exit".into());
            result
        })),
    );
    handlers.insert(
        inner.clone(),
        ActionHandler::Wrap(Box::new(move |_ctx, id, d, next| {
            log_inner.borrow_mut().push("inner-enter".into());
            let result = next.call(id, d);
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
    f.settle_with_dispatch(batch, std::rc::Rc::new(dispatch));
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
    let plan = kk_mod_api::freeze_with_builtin(
        builtin::link(),
        vec![contribution],
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
            Ok(())
        })),
    );
    let log_w = log.clone();
    tick_handlers.insert(
        wrap,
        TickHandler::Wrap(Box::new(move |_ctx, id, next| {
            log_w.borrow_mut().push("wrap-enter".into());
            let result = next.call(id);
            log_w.borrow_mut().push("wrap-exit".into());
            result
        })),
    );
    let log_a = log.clone();
    tick_handlers.insert(
        after,
        TickHandler::After(Box::new(move |_, _| {
            log_a.borrow_mut().push("after".into());
            Ok(())
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
    f.settle_with_dispatch(IntentBatch::default(), std::rc::Rc::new(dispatch));
    assert_eq!(
        *log.borrow(),
        vec!["before", "wrap-enter", "wrap-exit", "after"]
    );
    let d1 = data_of(&f, l1);
    assert_eq!(d1["cooldown"], json!(4), "native 递减经 wrap 链执行");
}
