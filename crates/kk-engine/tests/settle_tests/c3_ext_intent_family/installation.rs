//! 计划安装门与发布前 Mod 对象/扩展字段校验。
use super::*;

/// 内容键控短路的回归钉：默认引擎（计划无 Mod 类型、扩展目录为空）下，
/// 未知 Mod 类型与未声明扩展在受控编辑入口拒绝，已提交世界保持。
#[test]
fn engine_validate_guards_plan_foreign_mod_objects_and_extensions() {
    // 形态一：计划无 Mod 类型，候选带 Mod 对象。
    let make = || {
        let mut engine = Engine::new(kk_world::World::new(3));
        engine
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        engine
    };
    let mut engine = make();
    let err = engine
        .insert_object(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 3,
                y: 4,
                type_key: "extmod:counter".into(),
                data: json!({ "owner": "p1", "x": 3, "y": 4, "value": 0 }).to_string(),
            }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("不在计划中"), "{err}");
    assert_eq!(engine.world().time(), 0, "编辑失败不推进");
    assert!(engine.world().room("W1N1").unwrap().extensions.is_empty());

    // 形态二：扩展目录为空，候选房间带直插扩展条目。
    let mut engine = make();
    let oid = engine
        .insert_object(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                amount: 10,
                resource: None,
            }),
        )
        .unwrap();
    let err = engine
        .edit_room("W1N1", |room| {
            room.extensions.insert(
                oid,
                BTreeMap::from([("extmod:rogue".to_string(), json!(1))]),
            );
            Ok(())
        })
        .unwrap_err();
    assert!(err.to_string().contains("未接入扩展平面的对象"), "{err}");
    assert_eq!(engine.world().time(), 0, "编辑失败不推进");
    assert!(engine.world().room("W1N1").unwrap().extensions.is_empty());
}

/// 受控编辑拒绝未声明字段与位置失同步，合法载荷仍可发布。
#[test]
fn engine_validate_rejects_tampered_mod_payload() {
    let plan = frozen(counter_contribution(&[]));
    let make = || {
        let world = kk_world::World::new(3);
        let mut engine = Engine::new(world);
        let mut processes = BTreeMap::new();
        processes.insert(
            Key::new("extmod:increment.process").unwrap(),
            Box::new(|_ctx: &kk_engine::settle::RoomCtx, _id: ObjId, _a: &Value| Ok(()))
                as ExtSimple,
        );
        engine
            .install_plan_with_ext(
                plan.clone(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                processes,
            )
            .unwrap();
        engine
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        engine
    };
    // 形态一：payload 带未声明字段。
    let mut engine = make();
    let err = engine
        .insert_object(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 3,
                y: 4,
                type_key: "extmod:counter".into(),
                data: json!({ "owner": "p1", "x": 3, "y": 4, "value": 0, "rogue": 1 }).to_string(),
            }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("未在类型中声明"), "{err}");
    assert_eq!(engine.world().time(), 0, "编辑失败不推进");
    assert!(engine.world().room("W1N1").unwrap().extensions.is_empty());
    // 形态二：位置字段与信封坐标失同步。
    let mut engine = make();
    let err = engine
        .insert_object(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 7,
                y: 4,
                type_key: "extmod:counter".into(),
                data: json!({ "owner": "p1", "x": 3, "y": 4, "value": 0 }).to_string(),
            }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("不同步"), "{err}");
    // 形态三（对照）：合法 payload 通过并推进。
    let mut engine = make();
    engine
        .insert_object(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x: 3,
                y: 4,
                type_key: "extmod:counter".into(),
                data: json!({ "owner": "p1", "x": 3, "y": 4, "value": 0 }).to_string(),
            }),
        )
        .unwrap();
    engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(engine.world().time(), 1);
}

/// 装配门：队列合并与非对象域动作在装配期显式拒绝（fail-loud，
/// 不静默不执行）。
#[test]
fn install_gates_unsupported_intent_shapes() {
    // 队列合并。
    let mut contribution = counter_contribution(&[]);
    contribution.intents[0].merge = Merge::Queue { max_len: 4 };
    let plan = frozen(contribution);
    let mut engine = Engine::new(kk_world::World::new(2));
    let err = engine
        .install_plan_with_ext(
            plan,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap_err();
    assert!(
        err.contains("队列合并") && err.contains("extmod:increment"),
        "{err}"
    );
    // 房间域。
    let mut contribution = counter_contribution(&[]);
    contribution.intents[0].scope = IntentScope::Room;
    contribution.intents[0].receiver = None;
    let plan = frozen(contribution);
    let mut engine = Engine::new(kk_world::World::new(2));
    let err = engine
        .install_plan_with_ext(
            plan,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
        )
        .unwrap_err();
    assert!(
        err.contains("房间") && err.contains("extmod:increment"),
        "{err}"
    );
}

/// 装配门矩阵：非严格转换、非玩家授权、压制关系、builtin 槽劫持、
/// 长键、阶段不符、无意图槽全部显式拒绝。
#[test]
fn install_gate_matrix_rejects_unwired_shapes() {
    let install = |c: Contribution| {
        let plan = frozen(c);
        Engine::new(kk_world::World::new(2))
            .install_plan_with_ext(
                plan,
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .map(|_| ())
    };
    let expect_reject = |c: Contribution, needle: &str| {
        let err = install(c).unwrap_err();
        assert!(err.contains(needle), "期望含 {needle:?}，实得 {err}");
    };
    let mut c = counter_contribution(&[]);
    c.intents[0].conversion =
        ArgumentConversion::Native(Key::new("builtin:link.sanitize").unwrap());
    expect_reject(c, "非严格参数转换");
    let mut c = counter_contribution(&[]);
    c.intents[0].authority = IntentAuthority::ServerOnly;
    expect_reject(c, "非玩家授权");
    let mut c = counter_contribution(&[]);
    // 压制动作须同接收者：再造一个同接收者意图。
    c.intents[0].suppressed_by = BTreeSet::from([Key::new("extmod:other").unwrap()]);
    c.intents.push({
        let mut other = c.intents[0].clone();
        other.key = Key::new("extmod:other").unwrap();
        other.slot = Key::new("extmod:other.slot").unwrap();
        other.suppressed_by = BTreeSet::new();
        other
    });
    c.slots.push(SlotDescriptor {
        key: Key::new("extmod:other.slot").unwrap(),
        stage: kk_mod_api::Stage::ObjectAction,
        native_binding: Key::new("extmod:other.process").unwrap(),
    });
    c.bindings.insert(Key::new("extmod:other.process").unwrap());
    expect_reject(c, "压制关系");
    // 内建资源槽只能挂处理器，不能把自有意图混接到类型化派发域；
    // 已提供 Replace 同样不能绕过门禁。
    for slot in builtin::RESOURCE_ACTION_SLOTS {
        let mut c = counter_contribution(&[]);
        c.intents[0].slot = Key::new(slot).unwrap();
        c.slots.clear();
        expect_reject(c.clone(), "中心不是自身 process");
        let binding = Key::new("extmod:replace-builtin").unwrap();
        c.bindings.insert(binding.clone());
        c.handlers.push(HandlerDescriptor {
            key: binding.clone(),
            slot: Key::new(slot).unwrap(),
            kind: HandlerKind::Replace,
            binding: binding.clone(),
            depends_on: BTreeSet::new(),
        });
        let error = Engine::new(kk_world::World::new(2))
            .install_plan_with_ext(
                frozen(c),
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::from([(binding, ExtHandler::Replace(Box::new(|_, _, _| Ok(()))))]),
                BTreeMap::new(),
            )
            .unwrap_err();
        assert!(error.contains("扩展意图不能挂载内建派发槽"), "{error}");
    }
    // 长意图键：派生 process 键超 128 字节上限 → Err 不是 panic。
    let mut c = counter_contribution(&[]);
    // 意图键本身合法（总长恰 128），派生 process 键 136 超上限。
    let long_name = "i".repeat(121);
    c.intents[0].key = Key::new(format!("extmod:{long_name}")).unwrap();
    let err = install(c).unwrap_err();
    assert!(err.contains("键过长"), "{err}");
    // 阶段不符：对象域意图挂 Market 阶段槽。
    let mut c = counter_contribution(&[]);
    c.slots[0].stage = kk_mod_api::Stage::Market;
    expect_reject(c, "对象动作阶段");
    // 无意图槽：声明不被任何意图引用的槽。
    let mut c = counter_contribution(&[]);
    c.slots.push(SlotDescriptor {
        key: Key::new("extmod:orphan.slot").unwrap(),
        stage: kk_mod_api::Stage::ObjectAction,
        native_binding: Key::new("extmod:orphan.process").unwrap(),
    });
    c.bindings
        .insert(Key::new("extmod:orphan.process").unwrap());
    expect_reject(c, "没有挂载任何动作");
}
