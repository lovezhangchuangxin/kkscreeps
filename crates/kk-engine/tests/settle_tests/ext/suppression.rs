//! ext 意图压制（G07.b）：同接收者同 tick 的存在性判定——齐发只执行
//! 未压制者、压制者缺席则执行、压制只看键存在不看压制者自身成败
//!（参数越界被丢弃仍压制）、跨对象意图表隔离。
use super::*;

/// 在 counter 贡献上加同接收者第二意图 `extmod:zero`（把 value 置为
/// mark），并令 `extmod:increment` 声明被 zero 压制。
fn suppression_contribution() -> Contribution {
    let mut c = counter_contribution(&[]);
    let zero = Key::new("extmod:zero").unwrap();
    let process = Key::new("extmod:zero.process").unwrap();
    let slot = Key::new("extmod:zero.slot").unwrap();
    c.intents[0].suppressed_by = BTreeSet::from([zero.clone()]);
    c.intents.push({
        let mut descriptor = c.intents[0].clone();
        descriptor.key = zero;
        descriptor.suppressed_by = BTreeSet::new();
        descriptor.arguments = ValueSchema {
            kind: ValueKind::Record {
                fields: BTreeMap::from([(
                    "mark".into(),
                    ValueSchema {
                        kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 9 }),
                        nullable: false,
                    },
                )]),
                optional: BTreeSet::new(),
            },
            nullable: false,
        };
        descriptor.slot = slot.clone();
        descriptor
    });
    c.slots.push(SlotDescriptor {
        key: slot,
        stage: kk_mod_api::Stage::ObjectAction,
        native_binding: process.clone(),
    });
    c.bindings.insert(process);
    c
}

/// value 置为 args.mark 的 process 中心；执行记入 log 以断言链序。
fn zero_process(log: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> ExtSimple {
    Box::new(move |ctx, id, args| {
        log.lock().unwrap().push("zero".into());
        let mut fields = ctx.borrow_mut().mod_fields(id).ok_or("目标不是 Mod 对象")?;
        let mark = args
            .get("mark")
            .and_then(Value::as_i64)
            .ok_or("缺少 mark")?;
        fields["value"] = json!(mark);
        ctx.borrow_mut().update_mod_fields(id, &fields)
    })
}

fn dispatches(
    plan: &std::sync::Arc<kk_mod_api::ContributionPlan>,
    log: &std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> std::sync::Arc<ContributionDispatch> {
    let mut processes = BTreeMap::new();
    processes.insert(
        Key::new("extmod:increment.process").unwrap(),
        counter_process(log.clone()),
    );
    processes.insert(
        Key::new("extmod:zero.process").unwrap(),
        zero_process(log.clone()),
    );
    std::sync::Arc::new(
        ContributionDispatch::assemble_with_ext(
            plan,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            processes,
        )
        .unwrap(),
    )
}

fn ext_intents(player: &str, id: ObjId, pairs: &[(Key, Value)]) -> IntentBatch {
    let mut batch = IntentBatch::default();
    for (key, args) in pairs {
        batch.add_ext_intent(player, id, key.clone(), args.clone());
    }
    batch
}

/// 齐发只执行压制者：increment 被 zero 压制，同 tick 同对象两键齐发
/// 时 increment 不进链（log 无 process），value=mark。
#[test]
fn suppressor_present_skips_suppressed_intent() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let plan = frozen(suppression_contribution());
    let dispatch = dispatches(&plan, &log);
    let mut f = Fixture::new(13, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);

    settle_quiet(
        &mut f,
        ext_intents(
            "p1",
            id,
            &[
                (
                    Key::new("extmod:increment").unwrap(),
                    json!({ "amount": 3 }),
                ),
                (Key::new("extmod:zero").unwrap(), json!({ "mark": 7 })),
            ],
        ),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        vec!["zero"],
        "被压制键不进链，仅压制者执行"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(7), "压制者效果落地");
}

/// 压制者缺席则执行：同对象只发 increment，正常累加。
#[test]
fn suppressor_absent_executes_suppressed_intent() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let plan = frozen(suppression_contribution());
    let dispatch = dispatches(&plan, &log);
    let mut f = Fixture::new(14, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);

    settle_quiet(
        &mut f,
        ext_intents(
            "p1",
            id,
            &[(
                Key::new("extmod:increment").unwrap(),
                json!({ "amount": 3 }),
            )],
        ),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(*log.lock().unwrap(), vec!["process"], "无压制者时照常执行");
    assert_eq!(counter_value(&f, id)["value"], json!(8));
}

/// 压制只看键存在不看压制者自身成败（vanilla Action::executes 同语
/// 义）：zero 携带越界参数被 schema 丢弃，increment 仍被压制。
#[test]
fn suppression_is_presence_based_not_success_based() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let plan = frozen(suppression_contribution());
    let dispatch = dispatches(&plan, &log);
    let mut f = Fixture::new(15, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);

    settle_quiet(
        &mut f,
        ext_intents(
            "p1",
            id,
            &[
                (
                    Key::new("extmod:increment").unwrap(),
                    json!({ "amount": 3 }),
                ),
                // mark=99 越界（0..=9）：zero 在参数校验处被丢弃，
                // 但键存在于意图表——increment 仍被压制，value 不变。
                (Key::new("extmod:zero").unwrap(), json!({ "mark": 99 })),
            ],
        ),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert!(log.lock().unwrap().is_empty(), "两条都不进链");
    assert_eq!(
        counter_value(&f, id)["value"],
        json!(5),
        "压制者参数越界被丢弃，被压制键仍不执行"
    );
}

/// 跨对象隔离：压制关系只作用于同接收者的意图表——zero 发往另一对
/// 象时不压制本对象的 increment，两个效果各自落地。
#[test]
fn suppression_is_scoped_to_same_receiver() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let plan = frozen(suppression_contribution());
    let dispatch = dispatches(&plan, &log);
    let mut f = Fixture::new(16, terrain_with(&[]));
    let a = place_counter(&mut f, 10, 12, "p1", 5);
    let b = place_counter(&mut f, 20, 20, "p1", 6);

    let mut batch = IntentBatch::default();
    batch.add_ext_intent(
        "p1",
        a,
        Key::new("extmod:increment").unwrap(),
        json!({ "amount": 3 }),
    );
    batch.add_ext_intent(
        "p1",
        b,
        Key::new("extmod:zero").unwrap(),
        json!({ "mark": 2 }),
    );
    settle_quiet(&mut f, batch, dispatch.clone(), plan.clone()).unwrap();
    assert_eq!(
        counter_value(&f, a)["value"],
        json!(8),
        "另一对象上的压制者不压制本对象"
    );
    assert_eq!(counter_value(&f, b)["value"], json!(2));
}

/// 链式压制（A 压 B、B 压 C，freeze 允许——拒绝所有压制环）：A 缺席时
/// B 执行且压 C；A 在场时 B 被压、其压制效力仍在（C 被存在性判定跳
/// 过——压制只看 B 的键存在，不看 B 是否执行）。
#[test]
fn chained_suppression_each_link_presence_based() {
    let mut c = counter_contribution(&[]);
    // 三意图同接收者：low 被 mid 压、mid 被 high 压；process 分别写
    // 独立标记值（value=1/2/4），效果可区分。
    let mut descriptors = Vec::new();
    let mut slots = Vec::new();
    for (name, _mark, suppressed) in [
        ("high", 4, None),
        ("mid", 2, Some("high")),
        ("low", 1, Some("mid")),
    ] {
        let mut d = c.intents[0].clone();
        d.key = Key::new(format!("extmod:{name}")).unwrap();
        d.slot = Key::new(format!("extmod:{name}.slot")).unwrap();
        d.arguments = ValueSchema {
            kind: ValueKind::Record {
                fields: BTreeMap::from([(
                    "mark".into(),
                    ValueSchema {
                        kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 9 }),
                        nullable: false,
                    },
                )]),
                optional: BTreeSet::new(),
            },
            nullable: false,
        };
        d.suppressed_by = suppressed
            .map(|sup| BTreeSet::from([Key::new(format!("extmod:{sup}")).unwrap()]))
            .unwrap_or_default();
        let process = Key::new(format!("extmod:{name}.process")).unwrap();
        slots.push(SlotDescriptor {
            key: d.slot.clone(),
            stage: kk_mod_api::Stage::ObjectAction,
            native_binding: process.clone(),
        });
        c.bindings.insert(process);
        descriptors.push(d);
    }
    c.intents = descriptors;
    c.slots = slots;
    let plan = frozen(c);
    let log_all: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut processes = BTreeMap::new();
    for name in ["high", "mid", "low"] {
        let log_one = log_all.clone();
        processes.insert(
            Key::new(format!("extmod:{name}.process")).unwrap(),
            Box::new(
                move |ctx: &kk_engine::settle::RoomCtx, id: ObjId, args: &Value| {
                    log_one.lock().unwrap().push(name.to_string());
                    let mut fields = ctx.borrow_mut().mod_fields(id).ok_or("目标不是 Mod 对象")?;
                    let mark = args
                        .get("mark")
                        .and_then(Value::as_i64)
                        .ok_or("缺少 mark")?;
                    fields["value"] = json!(mark);
                    ctx.borrow_mut().update_mod_fields(id, &fields)
                },
            ) as ExtSimple,
        );
    }
    let dispatch = std::sync::Arc::new(
        ContributionDispatch::assemble_with_ext(
            &plan,
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            processes,
        )
        .unwrap(),
    );
    let mut f = Fixture::new(17, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);
    let key = |name: &str| Key::new(format!("extmod:{name}")).unwrap();

    // 三键齐发：只执行 high（mid 被 high 压、low 被 mid 的存在压）。
    log_all.lock().unwrap().clear();
    settle_quiet(
        &mut f,
        ext_intents(
            "p1",
            id,
            &[
                (key("low"), json!({ "mark": 1 })),
                (key("mid"), json!({ "mark": 2 })),
                (key("high"), json!({ "mark": 4 })),
            ],
        ),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(
        *log_all.lock().unwrap(),
        vec!["high"],
        "链上只有未压制者执行"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(4));

    // high 缺席：mid 执行且压 low（mid 的键存在即压制）。
    log_all.lock().unwrap().clear();
    settle_quiet(
        &mut f,
        ext_intents(
            "p1",
            id,
            &[
                (key("low"), json!({ "mark": 1 })),
                (key("mid"), json!({ "mark": 2 })),
            ],
        ),
        dispatch.clone(),
        plan.clone(),
    )
    .unwrap();
    assert_eq!(
        *log_all.lock().unwrap(),
        vec!["mid"],
        "mid 执行且其键存在压制 low"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(2));
}

/// 跨玩家隔离（G07.b 清偿）：ext 表按玩家分组——p2 的表里出现压制者
/// 键不影响 p1 的意图执行（p2 的发射在归属校验处被丢弃，本就不进链）。
#[test]
fn suppression_is_isolated_per_player() {
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let plan = frozen(suppression_contribution());
    let dispatch = dispatches(&plan, &log);
    let mut f = Fixture::new(18, terrain_with(&[]));
    let id = place_counter(&mut f, 10, 12, "p1", 5);

    // p1 发 increment；p2 对同一对象发压制者键 zero——p1 的表不含 zero，
    // increment 照常执行；p2 的 zero 被归属校验丢弃。
    let mut batch = IntentBatch::default();
    batch.add_ext_intent(
        "p1",
        id,
        Key::new("extmod:increment").unwrap(),
        json!({ "amount": 3 }),
    );
    batch.add_ext_intent(
        "p2",
        id,
        Key::new("extmod:zero").unwrap(),
        json!({ "mark": 7 }),
    );
    settle_quiet(&mut f, batch, dispatch.clone(), plan.clone()).unwrap();
    assert_eq!(
        *log.lock().unwrap(),
        vec!["process"],
        "跨玩家的压制者键不压制本玩家"
    );
    assert_eq!(counter_value(&f, id)["value"], json!(8));
}
