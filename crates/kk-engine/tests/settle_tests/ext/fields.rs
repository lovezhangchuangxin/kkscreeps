//! 扩展字段全链：声明 → 装配 → 处理器写入 → 候选提交 → 投影裁切，
//! 以及写入校验（未声明键、schema 限额）、删除清理、引用一致性、
//! 绕过写入 API 的直改在发布前被拒、失败轮保持已提交扩展。

use super::*;
use kk_engine::settle::Plane;
use kk_engine::settle::contrib::{ActionHandler, ContributionDispatch};
use kk_mod_api::extension::ExtensionCatalog;
use kk_mod_api::{
    Contribution, FieldDefault, FieldDescriptor, FieldMountDescriptor, FreezeOptions,
    HandlerDescriptor, Key, NumberDomain, Source, TypeRef, ValueKind, ValueSchema, Visibility,
};
use serde_json::json;

fn key(value: &str) -> Key {
    Key::new(value).unwrap()
}

fn ext_schema(kind: ValueKind) -> ValueSchema {
    ValueSchema {
        kind,
        nullable: false,
    }
}

/// 声明挂到 builtin:link 上的四个扩展字段：charge（Owner）、label（Public）、
/// secret（ServerOnly）、target（Public 引用）。
fn extension_mod() -> Contribution {
    let mut c = Contribution {
        source: Source {
            namespace: "extmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [7; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::new(),
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    };
    let mut mount = |short: &str, visibility: Visibility, schema: ValueSchema| {
        let bind = key(&format!("extmod:bind.{short}"));
        c.bindings.insert(bind.clone());
        c.field_mounts.push(FieldMountDescriptor {
            target: TypeRef {
                key: key("builtin:link"),
                schema_version: 1,
            },
            field: FieldDescriptor {
                key: key(&format!("extmod:{short}")),
                schema,
                default: FieldDefault::Missing,
                visibility,
                binding: bind,
            },
        });
    };
    mount(
        "charge",
        Visibility::Owner,
        ext_schema(ValueKind::Number(NumberDomain::Signed {
            min: 0,
            max: 1000,
        })),
    );
    mount(
        "label",
        Visibility::Public,
        ext_schema(ValueKind::String { max_bytes: 16 }),
    );
    mount(
        "secret",
        Visibility::ServerOnly,
        ext_schema(ValueKind::Bool),
    );
    mount(
        "target",
        Visibility::Public,
        ext_schema(ValueKind::ObjectRef(TypeRef {
            key: key("builtin:link"),
            schema_version: 1,
        })),
    );
    c
}

fn place_link(f: &mut Fixture, x: u8, y: u8, energy: i64) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: json!({
                    "type": "structure", "structureType": "link", "owner": "p1",
                    "hits": 1000, "hitsMax": 1000, "nextDecayTime": 1_000_000,
                    "store": {"energy": energy}, "storeCapacity": 800,
                    "storeCapacityResource": {"energy": 800},
                })
                .to_string(),
            }),
        )
        .unwrap()
}

fn transfer_batch(l1: ObjId, l2: ObjId) -> IntentBatch {
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
    batch
}

fn frozen_extension_plan(contrib: Contribution) -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contrib],
            FreezeOptions::default(),
        )
        .unwrap(),
    )
}

/// 四类权限的投影隔离：owner 看 charge/label/target，非 owner 只看
/// label/target，ServerOnly 不进入任何玩家投影。
#[test]
fn extension_projection_applies_visibility() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_link(&mut f, 10, 10, 300);
    let l2 = place_link(&mut f, 12, 10, 0);
    // p2 的 creep 使 W1N1 对 p2 可见：非 owner 玩家能看到 link 对象，
    // Public/Owner 裁切才有判别力（否则对象不出现，断言恒真）。
    f.creep("observer", "p2", 40, 40, &[BodyKind::Move], 0);

    let mut contribution = extension_mod();
    let handler = key("extmod:hook");
    contribution.bindings.insert(handler.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: handler.clone(),
        slot: key("builtin:link.transfer"),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: handler,
        depends_on: BTreeSet::new(),
    });
    let plan = frozen_extension_plan(contribution);
    let catalog = ExtensionCatalog::from_plan(&plan).unwrap();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key("extmod:hook"),
        ActionHandler::Wrap(Box::new(move |ctx, id, d, next| {
            let target = d.id.expect("转移意图带目标");
            ctx.borrow_mut()
                .set_extension(id, "extmod:charge", json!(64))?;
            ctx.borrow_mut()
                .set_extension(id, "extmod:label", json!("charged"))?;
            ctx.borrow_mut()
                .set_extension(id, "extmod:secret", json!(true))?;
            ctx.borrow_mut()
                .set_extension(id, "extmod:target", json!(target.as_str()))?;
            // 目标对象只写对非 owner 全不可见的字段：非 owner 视角
            // 对它不得出现 ext 键（空守卫——无条件插入 ext:{} 会泄露
            // "存在隐藏字段"的元事实）。
            ctx.borrow_mut()
                .set_extension(target, "extmod:charge", json!(32))?;
            ctx.borrow_mut()
                .set_extension(target, "extmod:secret", json!(false))?;
            next.call(id, d)
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
    f.settle_with_plane(
        transfer_batch(l1, l2),
        Plane {
            links: kk_world::place::builtin_link_rules(),
            stores: kk_world::place::builtin_store_rules(),
            combat: kk_world::place::builtin_combat_ranges(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            buildings: Default::default(),
            resources: Default::default(),
            contrib: std::rc::Rc::new(dispatch),
            extensions: std::rc::Rc::new(catalog.clone()),
            plan: plan.clone(),
        },
    );

    // 扩展值已提交到权威状态。
    let room = f.world.room("W1N1").unwrap();
    let stored = room.extensions.get(&l1).unwrap();
    assert_eq!(stored["extmod:charge"], json!(64));

    // owner 投影：charge/label/target 可见，secret 不进玩家块。
    let owner_view = f
        .world
        .world_json_for_viewer_with_extensions("p1", 0, &catalog, None);
    let obj = owner_view["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == json!(l1.as_str()))
        .unwrap();
    assert_eq!(obj["ext"]["extmod:charge"], json!(64));
    assert_eq!(obj["ext"]["extmod:label"], json!("charged"));
    assert_eq!(obj["ext"]["extmod:target"], json!(l2.as_str()));
    assert!(
        obj["ext"].get("extmod:secret").is_none(),
        "ServerOnly 字段不进玩家投影"
    );

    // 非 owner 投影：对象可见，但只见 Public 字段。
    let other_view = f
        .world
        .world_json_for_viewer_with_extensions("p2", 0, &catalog, None);
    let obj = other_view["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == json!(l1.as_str()))
        .expect("p2 有视野，能看到 link 对象");
    assert!(
        obj.get("ext")
            .is_none_or(|e| e.get("extmod:charge").is_none()),
        "Owner 字段对非 owner 不可见"
    );
    assert_eq!(
        obj["ext"]["extmod:label"],
        json!("charged"),
        "Public 字段可见"
    );
    assert_eq!(obj["ext"]["extmod:target"], json!(l2.as_str()));
    assert!(
        obj["ext"].get("extmod:secret").is_none(),
        "ServerOnly 字段对任何玩家都不可见"
    );
    // 非 owner 对"字段全不可见"的对象：不出现 ext 键本身。
    let hidden = other_view["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == json!(l2.as_str()))
        .expect("p2 有视野，目标 link 可见");
    assert!(hidden.get("ext").is_none(), "字段全被裁掉时不输出 ext 键");

    // 目录为空时输出与无扩展投影相同（不带 ext 键）。
    let plain = f.world.world_json_for_viewer("p1", 0);
    let obj = plain["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == json!(l1.as_str()))
        .unwrap();
    assert!(obj.get("ext").is_none());
}

/// 写入校验：未声明字段拒绝；schema 限额与取值域拒绝。
#[test]
fn extension_write_validates_against_catalog() {
    // 借 wrap 处理器在结算里探测写入门，逐值断言错误信息。
    let probe = |field: &'static str, value: serde_json::Value| -> String {
        // 直接构造 Working 太重；经 replace 处理器触发写入并回传错误。
        // 这里在测试主体用同一 plane 重跑，错误沿候选失败传出。
        let mut f2 = Fixture::new(7, terrain_with(&[]));
        f2.controller(5, Some("p1"));
        let a = place_link(&mut f2, 10, 10, 300);
        let b = place_link(&mut f2, 12, 10, 0);
        let mut contribution = extension_mod();
        let handler = key("extmod:probe");
        contribution.bindings.insert(handler.clone());
        contribution.handlers.push(HandlerDescriptor {
            key: handler.clone(),
            slot: key("builtin:link.transfer"),
            kind: kk_mod_api::HandlerKind::Wrap,
            binding: handler,
            depends_on: BTreeSet::new(),
        });
        let plan = frozen_extension_plan(contribution);
        let catalog = ExtensionCatalog::from_plan(&plan).unwrap();
        let mut handlers = BTreeMap::new();
        handlers.insert(
            key("extmod:probe"),
            ActionHandler::Wrap(Box::new(move |ctx, id, d, next| {
                let result = ctx.borrow_mut().set_extension(id, field, value.clone());
                let out = next.call(id, d);
                result?;
                out
            })),
        );
        let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
        // 结算返回扩展写入的错误字符串。
        let state =
            kk_engine::TickState::new(std::mem::take(&mut f2.players), f2.world.next_obj_id());
        let room = f2.world.room("W1N1").unwrap().clone();
        let result = settle_room(
            room,
            f2.world.time(),
            1_700_000_000_000,
            &state,
            &transfer_batch(a, b),
            &Plane {
                buildings: Default::default(),
                links: kk_world::place::builtin_link_rules(),
                stores: kk_world::place::builtin_store_rules(),
                combat: kk_world::place::builtin_combat_ranges(),
                move_gate: Default::default(),
                quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
                terrain_passability: Default::default(),
                resources: Default::default(),
                contrib: std::rc::Rc::new(dispatch),
                extensions: std::rc::Rc::new(catalog.clone()),
                plan: plan.clone(),
            },
        );
        result.err().map(|e| e.0).unwrap_or_default()
    };

    assert!(
        probe("extmod:unknown", json!(1)).contains("未声明"),
        "{}",
        probe("extmod:unknown", json!(1))
    );
    assert!(
        probe("extmod:charge", json!(1001)).contains("schema"),
        "取值域上限拒绝"
    );
    assert!(
        probe("extmod:label", json!("0123456789abcdefg")).contains("schema"),
        "字符串长度限额拒绝"
    );
    assert!(
        probe("extmod:charge", json!("64")).contains("schema"),
        "类型不匹配拒绝"
    );
    assert!(
        probe("extmod:secret", json!(serde_json::Value::Null)).contains("schema"),
        "null 拒绝（字段非 nullable）"
    );
}

/// 引用收敛：悬空引用在发布前清除（非 nullable 删字段），tick 仍可
/// 提交——被引用对象销毁不会卡住世界推进；本轮写入悬空值的候选
/// 同样收敛而非失败。
#[test]
fn dangling_extension_reference_reconciles_on_publish() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_link(&mut f, 10, 10, 300);
    let l2 = place_link(&mut f, 12, 10, 0);

    let mut contribution = extension_mod();
    let handler = key("extmod:hook");
    contribution.bindings.insert(handler.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: handler.clone(),
        slot: key("builtin:link.transfer"),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: handler,
        depends_on: BTreeSet::new(),
    });
    let plan = frozen_extension_plan(contribution);
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key("extmod:hook"),
        ActionHandler::Wrap(Box::new(|ctx, id, d, next| {
            let bad = kk_world::ObjId(9_999_999);
            ctx.borrow_mut()
                .set_extension(id, "extmod:target", json!(bad.as_str()))?;
            next.call(id, d)
        })),
    );
    let world = std::mem::replace(&mut f.world, kk_world::World::new(0));
    let mut engine = kk_engine::Engine::with_plan(world, plan, handlers, BTreeMap::new()).unwrap();
    engine
        .tick_with_intents(transfer_batch(l1, l2))
        .expect("悬空引用收敛而非失败");
    // 收敛后 tick 正常提交：时间推进，悬空的 target 字段被删除；
    // 它是该对象唯一的扩展字段，行随之清空。
    assert_eq!(engine.world().time(), 1);
    let room = engine.world().room("W1N1").unwrap();
    assert!(
        room.extensions
            .get(&l1)
            .is_none_or(|fields| fields.get("extmod:target").is_none()),
        "非 nullable 悬空引用整项删除"
    );

    // 场景 B：先一轮成功提交指向 l2 的合法引用；下一轮处理器销毁 l2，
    // 已提交引用变悬空——发布前收敛，tick 仍可提交（销毁不卡世界）。
    let mut f2 = Fixture::new(7, terrain_with(&[]));
    f2.controller(5, Some("p1"));
    let a = place_link(&mut f2, 10, 10, 300);
    let b = place_link(&mut f2, 12, 10, 0);

    let mut contribution = extension_mod();
    let handler = key("extmod:lifecycle");
    contribution.bindings.insert(handler.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: handler.clone(),
        slot: key("builtin:link.transfer"),
        kind: kk_mod_api::HandlerKind::Wrap,
        binding: handler,
        depends_on: BTreeSet::new(),
    });
    let plan = frozen_extension_plan(contribution);
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key("extmod:lifecycle"),
        ActionHandler::Wrap(Box::new(move |ctx, id, d, next| {
            if ctx.borrow().time == 0 {
                // 第一轮：写指向 b 的合法引用并执行默认转移。
                ctx.borrow_mut()
                    .set_extension(id, "extmod:target", json!(b.as_str()))?;
                next.call(id, d)
            } else {
                // 第二轮：销毁 b，使已提交引用悬空。
                ctx.borrow_mut().remove_obj(b);
                Ok(())
            }
        })),
    );
    let world = std::mem::replace(&mut f2.world, kk_world::World::new(0));
    let mut engine = kk_engine::Engine::with_plan(world, plan, handlers, BTreeMap::new()).unwrap();
    engine
        .tick_with_intents(transfer_batch(a, b))
        .expect("第一轮合法引用提交");
    let stored = engine.world().room("W1N1").unwrap();
    assert_eq!(
        stored.extensions[&a]["extmod:target"],
        json!(b.as_str()),
        "第一轮引用已提交"
    );
    engine
        .tick_with_intents(transfer_batch(a, b))
        .expect("销毁轮收敛悬空引用后提交");
    let room = engine.world().room("W1N1").unwrap();
    assert!(!room.objects.contains_key(&b), "目标对象已销毁");
    assert!(
        room.extensions
            .get(&a)
            .is_none_or(|fields| fields.get("extmod:target").is_none()),
        "已提交的悬空引用在发布前删除"
    );
    assert_eq!(engine.world().time(), 2);
}

/// 删除对象清理扩展：不留孤儿字段；直改扩展表塞未声明键在发布前被拒。
#[test]
fn object_removal_clears_extensions_and_raw_writes_are_rejected() {
    let mut f = Fixture::new(7, terrain_with(&[]));
    f.controller(5, Some("p1"));
    let l1 = place_link(&mut f, 10, 10, 300);
    let l2 = place_link(&mut f, 12, 10, 0);

    // replace 处理器：删除源对象（连同扩展）后不执行默认转移。
    let mut contribution = extension_mod();
    let handler = key("extmod:delete");
    contribution.bindings.insert(handler.clone());
    contribution.handlers.push(HandlerDescriptor {
        key: handler.clone(),
        slot: key("builtin:link.transfer"),
        kind: kk_mod_api::HandlerKind::Replace,
        binding: handler,
        depends_on: BTreeSet::new(),
    });
    let plan = frozen_extension_plan(contribution);
    let catalog = ExtensionCatalog::from_plan(&plan).unwrap();
    let mut handlers = BTreeMap::new();
    handlers.insert(
        key("extmod:delete"),
        ActionHandler::Replace(Box::new(|ctx, id, _d| {
            ctx.borrow_mut()
                .set_extension(id, "extmod:charge", json!(7))
                .unwrap();
            ctx.borrow_mut().remove_obj(id);
            Ok(())
        })),
    );
    let dispatch = ContributionDispatch::assemble(&plan, handlers, BTreeMap::new()).unwrap();
    f.settle_with_plane(
        transfer_batch(l1, l2),
        Plane {
            buildings: Default::default(),
            links: kk_world::place::builtin_link_rules(),
            stores: kk_world::place::builtin_store_rules(),
            combat: kk_world::place::builtin_combat_ranges(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: Default::default(),
            contrib: std::rc::Rc::new(dispatch),
            extensions: std::rc::Rc::new(catalog.clone()),
            plan: plan.clone(),
        },
    );
    let room = f.world.room("W1N1").unwrap();
    assert!(!room.extensions.contains_key(&l1), "删除对象后扩展表无残留");
    assert!(!room.objects.contains_key(&l1));

    // 直改扩展表（绕过写入门）塞未声明键：发布前被 validate 拒绝。
    let mut f2 = Fixture::new(7, terrain_with(&[]));
    f2.controller(5, Some("p1"));
    let a = place_link(&mut f2, 10, 10, 300);
    let b = place_link(&mut f2, 12, 10, 0);
    let mut raw = extension_mod();
    let handler = key("extmod:raw");
    raw.bindings.insert(handler.clone());
    raw.handlers.push(HandlerDescriptor {
        key: handler.clone(),
        slot: key("builtin:link.transfer"),
        kind: kk_mod_api::HandlerKind::Replace,
        binding: handler,
        depends_on: BTreeSet::new(),
    });
    let raw_plan = frozen_extension_plan(raw);
    let mut raw_handlers = BTreeMap::new();
    raw_handlers.insert(
        key("extmod:raw"),
        ActionHandler::Replace(Box::new(|ctx, id, _d| {
            ctx.borrow_mut()
                .room
                .extensions
                .entry(id)
                .or_default()
                .insert("extmod:not-declared".to_string(), json!(1));
            Ok(())
        })),
    );
    let world = std::mem::replace(&mut f2.world, kk_world::World::new(0));
    let mut engine =
        kk_engine::Engine::with_plan(world, raw_plan, raw_handlers, BTreeMap::new()).unwrap();
    let _ = (a, b);
    let err = engine
        .tick_with_intents(transfer_batch(a, b))
        .err()
        .map(|e| e.0)
        .unwrap_or_default();
    assert!(err.contains("未声明字段"), "{err}");
}
