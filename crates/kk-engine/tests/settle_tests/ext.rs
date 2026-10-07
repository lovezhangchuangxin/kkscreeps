//! 扩展意图执行链与 Mod 对象存储的引擎级用例：自有槽的
//! process 中心 + before/after 链序、玩家校验矩阵（参数/接收者/归属/
//! 未知键丢弃）、payload 写入与发布前校验（直改封堵）、装配门与
//! link 槽 before/replace 失败传播的引擎级专测。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::*;
use kk_engine::intent::IntentBatch;
use kk_engine::settle::contrib::{ContributionDispatch, ExtHandler, ExtSimple};
use kk_engine::settle::{Engine, Plane, TickState, builtin_plan, settle_room};
use kk_mod_api::builtin;
use kk_mod_api::{
    ArgumentConversion, Contribution, FieldDefault, FieldDescriptor, FreezeOptions,
    HandlerDescriptor, HandlerKind, IntentAuthority, IntentDescriptor, IntentScope, Key, Merge,
    NumberDomain, ObjectTypeDescriptor, ObjectVisibility, SlotDescriptor, Source, TypeRef,
    ValueKind, ValueSchema, Visibility,
};
use kk_schema::room::{ModObj, RoomObject};
use serde_json::{Value, json};

/// 构造 counter 型 Mod 贡献：类型（owner/x/y/value）+ increment 动作
/// （对象域、严格转换、末次覆盖）+ 可选的槽贡献处理器。
fn counter_contribution(handlers: &[(&str, HandlerKind)]) -> Contribution {
    let ns = "extmod";
    let field = |name: &str, schema: ValueSchema, default: FieldDefault| FieldDescriptor {
        key: Key::new(format!("{ns}:{name}")).unwrap(),
        schema,
        default,
        visibility: Visibility::Public,
        binding: Key::new(format!("{ns}:counter.field.{name}")).unwrap(),
    };
    let u49 = ValueSchema {
        kind: ValueKind::Number(NumberDomain::Unsigned { min: 0, max: 49 }),
        nullable: false,
    };
    let mut bindings = BTreeSet::new();
    let mut fields = vec![
        field(
            "owner",
            ValueSchema {
                nullable: true,
                ..ValueSchema {
                    kind: ValueKind::String { max_bytes: 128 },
                    nullable: false,
                }
            },
            FieldDefault::Missing,
        ),
        field("x", u49.clone(), FieldDefault::Missing),
        field("y", u49, FieldDefault::Missing),
        field(
            "value",
            ValueSchema {
                kind: ValueKind::Number(NumberDomain::Signed {
                    min: i32::MIN.into(),
                    max: i32::MAX.into(),
                }),
                nullable: false,
            },
            FieldDefault::Value(json!(0)),
        ),
    ];
    fields.sort_by(|a, b| a.key.cmp(&b.key));
    for f in &fields {
        bindings.insert(f.binding.clone());
    }
    let intent_key = Key::new("extmod:increment").unwrap();
    let process = Key::new("extmod:increment.process").unwrap();
    let slot = Key::new("extmod:increment.slot").unwrap();
    bindings.insert(process.clone());
    let mut contribution = Contribution {
        source: Source {
            namespace: ns.into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [3; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings,
        field_mounts: Vec::new(),
        object_types: vec![ObjectTypeDescriptor {
            key: Key::new("extmod:counter").unwrap(),
            schema_version: 1,
            owner_field: Some(Key::new("extmod:owner").unwrap()),
            position_fields: Some([Key::new("extmod:x").unwrap(), Key::new("extmod:y").unwrap()]),
            fields,
            capabilities: Vec::new(),
            lifecycle_slots: BTreeSet::new(),
            visibility: ObjectVisibility::RoomVisible,
            player_class: "Counter".into(),
            store_capacity: None,
        }],
        intents: vec![IntentDescriptor {
            key: intent_key.clone(),
            scope: IntentScope::Object,
            receiver: Some(TypeRef {
                key: Key::new("extmod:counter").unwrap(),
                schema_version: 1,
            }),
            arguments: ValueSchema {
                kind: ValueKind::Record {
                    fields: BTreeMap::from([(
                        "amount".into(),
                        ValueSchema {
                            kind: ValueKind::Number(NumberDomain::Signed { min: 1, max: 10 }),
                            nullable: false,
                        },
                    )]),
                    optional: BTreeSet::new(),
                },
                nullable: false,
            },
            conversion: ArgumentConversion::Strict,
            authority: IntentAuthority::Player,
            merge: Merge::Last,
            slot: slot.clone(),
            suppressed_by: BTreeSet::new(),
        }],
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: vec![SlotDescriptor {
            key: slot,
            stage: kk_mod_api::Stage::ObjectAction,
            native_binding: process,
        }],
        handlers: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
    };
    for (id, kind) in handlers {
        let key = Key::new(format!("extmod:handler.{id}")).unwrap();
        contribution.bindings.insert(key.clone());
        contribution.handlers.push(HandlerDescriptor {
            key: key.clone(),
            slot: Key::new("extmod:increment.slot").unwrap(),
            kind: *kind,
            binding: key,
            depends_on: BTreeSet::new(),
        });
    }
    contribution
}

/// 只含 link 槽贡献处理器的贡献（不带类型与意图）。
fn link_handler_contribution(id: &str, kind: HandlerKind) -> Contribution {
    let ns = "extmod";
    let key = Key::new(format!("extmod:handler.{id}")).unwrap();
    let mut bindings = BTreeSet::new();
    bindings.insert(key.clone());
    Contribution {
        source: Source {
            namespace: ns.into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [3; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings,
        field_mounts: Vec::new(),
        object_types: Vec::new(),
        intents: Vec::new(),
        rules: Vec::new(),
        overrides: Vec::new(),
        constraints: BTreeSet::new(),
        slots: Vec::new(),
        resources: Vec::new(),
        buildings: Vec::new(),
        handlers: vec![HandlerDescriptor {
            key: key.clone(),
            slot: Key::new("builtin:link.transfer").unwrap(),
            kind,
            binding: key,
            depends_on: BTreeSet::new(),
        }],
    }
}

fn frozen(contribution: Contribution) -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    std::sync::Arc::new(
        kk_mod_api::freeze_with_builtin(
            builtin::link(),
            vec![contribution],
            FreezeOptions::default(),
        )
        .unwrap(),
    )
}

/// 记录链序并把 args.amount 累进 value 的 process 中心。
fn counter_process(log: Rc<RefCell<Vec<String>>>) -> ExtSimple {
    Box::new(move |ctx, id, args| {
        log.borrow_mut().push("process".into());
        let mut fields = ctx.borrow().mod_fields(id).ok_or("目标不是 Mod 对象")?;
        let amount = args
            .get("amount")
            .and_then(Value::as_i64)
            .ok_or("缺少 amount")?;
        fields["value"] = json!(fields["value"].as_i64().unwrap_or(0) + amount);
        ctx.borrow_mut().update_mod_fields(id, &fields)
    })
}

fn observer(log: Rc<RefCell<Vec<String>>>, tag: &'static str) -> ExtSimple {
    Box::new(move |_, _, _| {
        log.borrow_mut().push(tag.into());
        Ok(())
    })
}

/// 以给定执行表结算一个批；Err 原样返回（失败传播类用例）。
fn settle_quiet(
    f: &mut Fixture,
    batch: IntentBatch,
    dispatch: std::rc::Rc<ContributionDispatch>,
    plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
) -> Result<(), String> {
    let state = TickState::new(std::mem::take(&mut f.players), f.world.next_obj_id());
    let mut room = f.world.room("W1N1").unwrap().clone();
    room.name = "W1N1".into();
    let plane = Plane {
        links: kk_world::place::builtin_link_rules(),
        stores: kk_world::place::builtin_store_rules(),
        combat: kk_world::place::builtin_combat_ranges(),
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        terrain_passability: Default::default(),
        buildings: Default::default(),
        resources: Default::default(),
        contrib: dispatch,
        extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
        plan,
    };
    let settled = settle_room(
        room,
        f.world.time(),
        1_700_000_000_000,
        &state,
        &batch,
        &plane,
    );
    let result = settled.as_ref().map(|_| ()).map_err(|e| e.0.clone());
    if let Ok((room, _events, _moves)) = settled {
        let (players, next, _) = state.into_parts().unwrap();
        f.players = players;
        let name = room.name.clone();
        f.world.replace_room(name, room).unwrap();
        f.world.bump_next_obj_id(next);
        f.world.advance_ticks(1).unwrap();
    }
    result
}

fn place_counter(f: &mut Fixture, x: u8, y: u8, owner: &str, value: i64) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Mod(ModObj {
                id: 0,
                x,
                y,
                type_key: "extmod:counter".into(),
                data: json!({ "owner": owner, "x": x, "y": y, "value": value }).to_string(),
            }),
        )
        .unwrap()
}

fn counter_value(f: &Fixture, id: ObjId) -> Value {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Mod(o)) => serde_json::from_str(&o.data).unwrap(),
        other => panic!("Mod 对象不存在：{other:?}"),
    }
}

fn place_generic(f: &mut Fixture, x: u8, y: u8, body: Value) -> ObjId {
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

fn link_data(f: &Fixture, id: ObjId) -> String {
    match f.world.get(id).map(|(_, o)| o) {
        Some(RoomObject::Generic(g)) => g.data.clone(),
        other => panic!("link 不存在：{other:?}"),
    }
}

fn ext_batch(player: &str, id: ObjId, args: Value) -> IntentBatch {
    let mut batch = IntentBatch::default();
    batch.add_ext_intent(player, id, Key::new("extmod:increment").unwrap(), args);
    batch
}

mod fields;
mod installation;

mod execution;
mod object_ops;
mod object_tick;

mod suppression;
