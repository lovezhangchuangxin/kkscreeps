//! 扩展意图的归属、处理链、领域写入及失败传播。
use super::*;

/// 自有槽执行链：before → process（字段写入）→ after（冻结时已逆序）；
/// 玩家校验矩阵——参数越界/接收者不符/非归属/未知键全部丢弃且结算成功。
#[test]
fn ext_intent_chain_and_player_gates() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let plan = frozen(counter_contribution(&[
        ("pre", HandlerKind::Before),
        ("post", HandlerKind::After),
    ]));
    let mut ext_handlers = BTreeMap::new();
    ext_handlers.insert(
        Key::new("extmod:handler.pre").unwrap(),
        ExtHandler::Before(observer(log.clone(), "before")),
    );
    ext_handlers.insert(
        Key::new("extmod:handler.post").unwrap(),
        ExtHandler::After(observer(log.clone(), "after")),
    );
    let mut ext_processes = BTreeMap::new();
    ext_processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        ext_handlers,
        ext_processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);

    let mut f = Fixture::new(11, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);

    // 合法链：顺序 before → process → after，value 5+3。
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 3 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(*log.borrow(), vec!["before", "process", "after"]);
    assert_eq!(counter_value(&f, id)["value"], json!(8));
    // 位置字段同步：process 未改坐标，信封与 payload 一致。
    match f.world.get(id).unwrap().1 {
        RoomObject::Mod(o) => assert_eq!((o.x, o.y), (10, 12)),
        _ => unreachable!(),
    }

    // 参数越界（amount=99 > 10）：丢弃，链不执行，结算成功。
    log.borrow_mut().clear();
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 99 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert!(log.borrow().is_empty(), "越界参数不进链");
    assert_eq!(counter_value(&f, id)["value"], json!(8));

    // 非归属玩家：丢弃。
    settle_quiet(
        &mut f,
        ext_batch("p2", id, json!({ "amount": 2 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(counter_value(&f, id)["value"], json!(8), "非归属丢弃");

    // 未知意图键：丢弃。
    let mut unknown = IntentBatch::default();
    unknown.add_ext_intent("p1", id, Key::new("extmod:ghost").unwrap(), json!({}));
    settle_quiet(&mut f, unknown, dispatch.clone(), plan.clone()).unwrap();
    assert_eq!(counter_value(&f, id)["value"], json!(8), "未知键丢弃");

    // 目标不是 Mod 对象（接收者不符的等价形态）：丢弃。
    let creep = f.creep("w1", "p1", 20, 20, &[BodyKind::Move], 0);
    settle_quiet(
        &mut f,
        ext_batch("p1", creep, json!({ "amount": 1 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(counter_value(&f, id)["value"], json!(8), "非 Mod 目标丢弃");
}

/// process 失败使候选失败；部分字段回写被拒（整值写入纪律）。
#[test]
fn ext_intent_failure_and_partial_write_rejected() {
    let plan = frozen(counter_contribution(&[]));
    // 失败 process：直接报错。
    let mut processes = BTreeMap::new();
    processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        Box::new(
            move |_ctx: &kk_engine::settle::RoomCtx, _id: ObjId, _args: &Value| {
                Err::<(), String>("process 拒绝".into())
            },
        ) as ExtSimple,
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(12, terrain_with(&[]));
    let id = place_counter(&mut f, 5, 5, "p1", 1);
    let err = settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 2 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap_err();
    assert!(err.contains("process 拒绝"), "{err}");
    assert_eq!(counter_value(&f, id)["value"], json!(1), "失败候选不落");

    // 部分回写：只返回 value 字段 → 整值校验拒绝。
    let partial: ExtSimple = Box::new(|ctx, id, _args| {
        ctx.borrow_mut()
            .update_mod_fields(id, &json!({ "value": 9 }))
    });
    let mut processes = BTreeMap::new();
    processes.insert(Key::new("extmod:increment.process").unwrap(), partial);
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let err = settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 2 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap_err();
    assert!(err.contains("缺少字段"), "部分回写被拒：{err}");
    assert_eq!(counter_value(&f, id)["value"], json!(1));
}

/// link 槽 before 失败：中止后续（中心与 after 不执行），Err 传播为
/// 候选失败；replace 失败同理。before/replace 可失败化的引擎级专测。
#[test]
fn link_slot_before_and_replace_failures_propagate() {
    // before 失败：中心不执行（link 能量不动）。
    let plan = frozen(link_handler_contribution("gate", HandlerKind::Before));
    let mut handlers: BTreeMap<Key, kk_engine::settle::contrib::ActionHandler> = BTreeMap::new();
    handlers.insert(
        Key::new("extmod:handler.gate").unwrap(),
        kk_engine::settle::contrib::ActionHandler::Before(Box::new(|_, _, _| {
            Err::<(), String>("before 拒绝".into())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
    let mut f = Fixture::new(13, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_generic(
        &mut f,
        10,
        10,
        json!({"type":"structure","structureType":"link","owner":"p1",
               "hits":500,"hitsMax":500,"store":{"energy":300},"storeCapacityResource":{"energy":800},"cooldown":0}),
    );
    let l2 = place_generic(
        &mut f,
        10,
        11,
        json!({"type":"structure","structureType":"link","owner":"p1",
               "hits":500,"hitsMax":500,"store":{"energy":0},"storeCapacityResource":{"energy":800},"cooldown":0}),
    );
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        kk_engine::intent::Action::Transfer,
        kk_engine::intent::IntentData {
            id: Some(l2),
            amount: Some(100),
            ..kk_engine::intent::IntentData::bare()
        },
    );
    let dispatch = std::rc::Rc::new(dispatch);
    let err = settle_quiet(&mut f, batch, dispatch, plan.clone()).unwrap_err();
    assert!(err.contains("before 拒绝"), "{err}");
    let d1 = serde_json::from_str::<Value>(&link_data(&f, l1)).unwrap();
    assert_eq!(
        d1["store"]["energy"],
        json!(300),
        "中心未执行（before 中止）"
    );

    // replace 失败：传播为候选失败。
    let plan = frozen(link_handler_contribution("noop", HandlerKind::Replace));
    let mut handlers = BTreeMap::new();
    handlers.insert(
        Key::new("extmod:handler.noop").unwrap(),
        kk_engine::settle::contrib::ActionHandler::Replace(Box::new(|_, _, _| {
            Err::<(), String>("replace 拒绝".into())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent(
        "p1",
        l1,
        kk_engine::intent::Action::Transfer,
        kk_engine::intent::IntentData {
            id: Some(l2),
            amount: Some(100),
            ..kk_engine::intent::IntentData::bare()
        },
    );
    let dispatch = std::rc::Rc::new(dispatch);
    let err = settle_quiet(&mut f, batch, dispatch, plan.clone()).unwrap_err();
    assert!(err.contains("replace 拒绝"), "{err}");
    let d1 = serde_json::from_str::<Value>(&link_data(&f, l1)).unwrap();
    assert_eq!(d1["store"]["energy"], json!(300), "失败候选不落");
}

/// 无 Mod 意图计划的默认引擎：builtin 计划可结算（Plane 默认值契约）。
#[test]
fn builtin_plan_keeps_vanilla_settle() {
    let plan = builtin_plan();
    assert!(
        plan.resolve_intent(&Key::new("builtin:link.transfer").unwrap())
            .is_some()
    );
    let mut f = Fixture::new(14, terrain_with(&[]));
    f.controller(1, None);
    let batch = IntentBatch::default();
    let dispatch = ContributionDispatch::default();
    let dispatch = std::rc::Rc::new(dispatch);
    settle_quiet(&mut f, batch, dispatch, plan.clone()).unwrap();
    assert_eq!(f.world.time(), 1);
}

/// 授权 fail-closed：类型未声明归属字段时，任何玩家（含字段值匹配的
/// 发射者）都不能执行其对象上的扩展意图。
#[test]
fn ext_intent_owner_absent_is_fail_closed() {
    let mut contribution = counter_contribution(&[]);
    let ty = contribution.object_types.first_mut().unwrap();
    ty.owner_field = None;
    // owner 字段保留（仍是合法声明字段），只是不再标记为归属字段。
    let plan = frozen(contribution);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut processes = BTreeMap::new();
    processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(15, terrain_with(&[]));
    let id = place_counter(&mut f, 5, 5, "p1", 2);
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 1 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert!(log.borrow().is_empty(), "未声明归属字段：任何玩家不可执行");
    assert_eq!(counter_value(&f, id)["value"], json!(2));
}

/// 接收者不匹配：意图声明的接收者类型与目标对象类型不同时丢弃
/// （两个 Mod 类型互发，不是"非 Mod 目标"的等价变形）。
#[test]
fn ext_intent_receiver_mismatch_between_mod_types() {
    let mut contribution = counter_contribution(&[]);
    // 第二类型 gauge：字段与 counter 同构。
    let gauge = {
        let mut ty = contribution.object_types[0].clone();
        ty.key = Key::new("extmod:gauge").unwrap();
        for field in &mut ty.fields {
            field.binding = Key::new(format!(
                "extmod:gauge.field.{}",
                kk_mod_api::validate::field_name(&field.key)
            ))
            .unwrap();
        }
        ty
    };
    for field in &gauge.fields {
        contribution.bindings.insert(field.binding.clone());
    }
    contribution.object_types.push(gauge);
    let plan = frozen(contribution);
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut processes = BTreeMap::new();
    processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(16, terrain_with(&[]));
    let counter = place_counter(&mut f, 5, 5, "p1", 2);
    let gauge = f
        .world
        .insert(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 6,
                y: 6,
                type_key: "extmod:gauge".into(),
                data: json!({ "owner": "p1", "x": 6, "y": 6, "value": 9 }).to_string(),
            }),
        )
        .unwrap();
    settle_quiet(
        &mut f,
        ext_batch("p1", gauge, json!({ "amount": 1 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert!(
        log.borrow().is_empty(),
        "接收者不匹配：counter 意图不能作用于 gauge 对象"
    );
    let gauge_fields: Value = serde_json::from_str(&match f.world.get(gauge).unwrap().1 {
        RoomObject::Mod(o) => o.data.clone(),
        _ => unreachable!(),
    })
    .unwrap();
    assert_eq!(gauge_fields["value"], json!(9), "gauge 值未被动过");
    let _ = counter;
}

/// process 改写声明位置字段：信封坐标与位置索引同步，发布前校验通过。
#[test]
fn ext_intent_position_write_syncs_envelope() {
    let plan = frozen(counter_contribution(&[]));
    let moved: ExtSimple = Box::new(|ctx, id, args| {
        let mut fields = ctx.borrow().mod_fields(id).ok_or("目标不是 Mod 对象")?;
        fields["value"] = json!(1);
        fields["x"] = json!(fields["x"].as_i64().unwrap() + args["amount"].as_i64().unwrap());
        ctx.borrow_mut().update_mod_fields(id, &fields)
    });
    let mut processes = BTreeMap::new();
    processes.insert(Key::new("extmod:increment.process").unwrap(), moved);
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(17, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 10, "p1", 0);
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 5 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    match f.world.get(id).unwrap().1 {
        RoomObject::Mod(o) => {
            assert_eq!(o.x, 15, "信封 x 随 payload 位置字段同步");
            assert_eq!(o.y, 10);
            let fields: Value = serde_json::from_str(&o.data).unwrap();
            assert_eq!(fields["x"], json!(15), "payload 与信封一致");
        }
        _ => unreachable!(),
    }
}

/// 扩展链 before 失败：中心与 after 不执行，Err 传播为候选失败
/// （与 link 槽 before-Err 同语义的独立代码路径）。
#[test]
fn ext_slot_before_failure_aborts_center_and_after() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let plan = frozen(counter_contribution(&[
        ("gate", HandlerKind::Before),
        ("post", HandlerKind::After),
    ]));
    let mut ext_handlers = BTreeMap::new();
    ext_handlers.insert(
        Key::new("extmod:handler.gate").unwrap(),
        ExtHandler::Before(Box::new(|_, _, _| {
            Err::<(), String>("ext before 拒绝".into())
        })),
    );
    ext_handlers.insert(
        Key::new("extmod:handler.post").unwrap(),
        ExtHandler::After(observer(log.clone(), "after")),
    );
    let mut ext_processes = BTreeMap::new();
    ext_processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        ext_handlers,
        ext_processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(18, terrain_with(&[]));
    let id = place_counter(&mut f, 5, 5, "p1", 1);
    let err = settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 1 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap_err();
    assert!(err.contains("ext before 拒绝"), "{err}");
    assert!(log.borrow().is_empty(), "中心与 after 均未执行");
    assert_eq!(counter_value(&f, id)["value"], json!(1), "失败候选不落");
}

/// 跨 Mod 替换动作处理中心：Replace 处理器占位后自身 process 不再执行，
/// 链以 Replace 为中心运行（自身 process 实现仍须提供且被允许残余）。
#[test]
fn ext_slot_replace_center_takes_over_process() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let plan = frozen(counter_contribution(&[("takeover", HandlerKind::Replace)]));
    let mut ext_handlers = BTreeMap::new();
    ext_handlers.insert(
        Key::new("extmod:handler.takeover").unwrap(),
        ExtHandler::Replace(observer(log.clone(), "replace")),
    );
    let mut ext_processes = BTreeMap::new();
    ext_processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = ContributionDispatch::assemble_with_ext(
        &plan,
        BTreeMap::new(),
        BTreeMap::new(),
        ext_handlers,
        ext_processes,
    )
    .unwrap();
    let dispatch = std::rc::Rc::new(dispatch);
    let mut f = Fixture::new(19, terrain_with(&[]));
    let id = place_counter(&mut f, 5, 5, "p1", 7);
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 1 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        vec!["replace"],
        "Replace 中心执行，原 process 不跑"
    );
    assert_eq!(
        counter_value(&f, id)["value"],
        json!(7),
        "字段未被原 process 改动"
    );
}

/// 扩展槽 wrap 链：外→内→中心→内出→外出，after 仅在内层成功后执行。
///
/// 该路径随 `'static` 链导出一起启用，装配门已不再拒绝扩展槽 wrap，
/// 因此需要用例把三个不变量锁死：wrap 拓扑序、next 至多一次、
/// after 只在内层成功后跑。
#[test]
fn ext_slot_wrap_chain_order_and_after_timing() {
    let log = Rc::new(RefCell::new(Vec::new()));
    // 冻结期按处理器键稳定排序，链上 wraps[0] 为最外层；这里用键序
    // （a-outer < b-inner）表达嵌套方向，不依赖声明顺序。
    let plan = frozen(counter_contribution(&[
        ("a-outer", HandlerKind::Wrap),
        ("b-inner", HandlerKind::Wrap),
        ("post", HandlerKind::After),
    ]));
    let mut ext_handlers = BTreeMap::new();
    for tag in ["a-outer", "b-inner"] {
        let sink = log.clone();
        ext_handlers.insert(
            Key::new(format!("extmod:handler.{tag}")).unwrap(),
            ExtHandler::Wrap(Box::new(move |ctx, id, args, next| {
                sink.borrow_mut().push(format!("{tag}-enter"));
                next.call(id)?;
                sink.borrow_mut().push(format!("{tag}-exit"));
                // next 之后的 ctx 仍可用：借用已在 next 内释放。
                let _ = ctx.borrow().mod_fields(id);
                let _ = args;
                Ok(())
            })),
        );
    }
    ext_handlers.insert(
        Key::new("extmod:handler.post").unwrap(),
        ExtHandler::After(observer(log.clone(), "after")),
    );
    let mut ext_processes = BTreeMap::new();
    ext_processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch = std::rc::Rc::new(
        ContributionDispatch::assemble_with_ext(
            &plan,
            BTreeMap::new(),
            BTreeMap::new(),
            ext_handlers,
            ext_processes,
        )
        .unwrap(),
    );

    let mut f = Fixture::new(11, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 3 })),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            "a-outer-enter",
            "b-inner-enter",
            "process",
            "b-inner-exit",
            "a-outer-exit",
            "after"
        ],
        "扩展槽 wrap 链须按外→内→中心→内出→外出执行，after 收尾"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(8));

    // 内层 wrap 截断：中心不执行；槽成功返回时仍执行已安装的 after。
    log.borrow_mut().clear();
    let plan2 = frozen(counter_contribution(&[("cut", HandlerKind::Wrap)]));
    let mut handlers2 = BTreeMap::new();
    let sink = log.clone();
    handlers2.insert(
        Key::new("extmod:handler.cut").unwrap(),
        ExtHandler::Wrap(Box::new(move |_ctx, _id, _args, _next| {
            sink.borrow_mut().push("cut".into());
            Ok(()) // 不调 next：取消内层
        })),
    );
    let mut processes2 = BTreeMap::new();
    processes2.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    let dispatch2 = std::rc::Rc::new(
        ContributionDispatch::assemble_with_ext(
            &plan2,
            BTreeMap::new(),
            BTreeMap::new(),
            handlers2,
            processes2,
        )
        .unwrap(),
    );
    settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 3 })),
        dispatch2,
        plan2.clone(),
    )
    .unwrap();
    assert_eq!(*log.borrow(), vec!["cut"], "截断后中心不执行");
    assert_eq!(counter_value(&f, id)["value"], json!(8), "截断后值不变");
}

/// 扩展槽 wrap 重复调用 next 必须使本轮候选失败。
#[test]
fn ext_slot_wrap_double_next_fails_candidate() {
    let plan = frozen(counter_contribution(&[("twice", HandlerKind::Wrap)]));
    let mut handlers = BTreeMap::new();
    handlers.insert(
        Key::new("extmod:handler.twice").unwrap(),
        ExtHandler::Wrap(Box::new(|_ctx, id, _args, next| {
            next.call(id)?;
            next.call(id)
        })),
    );
    let mut processes = BTreeMap::new();
    processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        Box::new(|_ctx: &kk_engine::settle::RoomCtx, _id: ObjId, _a: &Value| Ok(()))
            as kk_engine::settle::contrib::ExtSimple,
    );
    let dispatch = std::rc::Rc::new(
        ContributionDispatch::assemble_with_ext(
            &plan,
            BTreeMap::new(),
            BTreeMap::new(),
            handlers,
            processes,
        )
        .unwrap(),
    );

    let mut f = Fixture::new(11, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);
    let error = settle_quiet(
        &mut f,
        ext_batch("p1", id, json!({ "amount": 3 })),
        dispatch,
        plan,
    )
    .expect_err("重复 next 必须使本轮候选失败");
    assert!(
        error.contains("重复调用 next"),
        "错误应指明重复调用，实得：{error}"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(5), "失败轮不提交");
}

/// 处理器吞掉 next 的 Err 不能使候选成功：违规与内层故障在执行器留存。
#[test]
fn ext_caught_next_error_still_aborts_candidate() {
    for duplicate in [false, true] {
        let plan = frozen(counter_contribution(&[
            ("guard", HandlerKind::Wrap),
            ("post", HandlerKind::After),
        ]));
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut handlers = BTreeMap::new();
        handlers.insert(
            Key::new("extmod:handler.guard").unwrap(),
            ExtHandler::Wrap(Box::new(move |_, id, _, next| {
                let _ = next.call(id);
                if duplicate {
                    let _ = next.call(id);
                }
                Ok(())
            })),
        );
        handlers.insert(
            Key::new("extmod:handler.post").unwrap(),
            ExtHandler::After(observer(log.clone(), "after")),
        );
        let mut processes: BTreeMap<Key, ExtSimple> = BTreeMap::new();
        processes.insert(
            Key::new("extmod:increment.process").unwrap(),
            Box::new(move |ctx, id, _| {
                let mut fields = ctx.borrow().mod_fields(id).unwrap();
                fields["value"] = json!(8);
                ctx.borrow_mut().update_mod_fields(id, &fields)?;
                if duplicate {
                    Ok(())
                } else {
                    Err("process 故障".into())
                }
            }),
        );
        let dispatch = Rc::new(
            ContributionDispatch::assemble_with_ext(
                &plan,
                BTreeMap::new(),
                BTreeMap::new(),
                handlers,
                processes,
            )
            .unwrap(),
        );
        let mut f = Fixture::new(11, terrain_with(&[]));
        let id = place_counter(&mut f, 10, 12, "p1", 5);
        let error = settle_quiet(
            &mut f,
            ext_batch("p1", id, json!({"amount":3})),
            dispatch,
            plan,
        )
        .expect_err("捕获 next 错误不能提交");
        assert!(error.contains(if duplicate {
            "重复调用"
        } else {
            "process 故障"
        }));
        assert_eq!(counter_value(&f, id)["value"], json!(5));
        assert!(log.borrow().is_empty(), "失败链不得运行 after");
    }
}
