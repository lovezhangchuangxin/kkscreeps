//! G09.c 结构受击槽（builtin:mod.receiveDamage）的托管挂起与排空单元
//! 测试；全链 JS 行为由 sim 三场景与 compat 夹具验收。
use super::*;
use crate::settle::contrib::{ContributionDispatch, SlotCall, SlotExecutor};
use crate::settle::{Plane, RoomCtx, TickState};
use kk_mod_api::{Contribution, HandlerDescriptor, HandlerKind, Key, Source};
use kk_schema::room::{CreepObj, ModObj, RoomObject};
use kk_world::{ObjId, RoomState};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

/// 把伤害减半的宿主执行器；`decline` 形态验证正常拒绝回退默认伤害。
struct GuardExecutor {
    decline: bool,
}

impl SlotExecutor for GuardExecutor {
    fn run(
        &self,
        _: &kk_mod_api::SlotPlan,
        _: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<kk_mod_api::SlotOutcome, String> {
        let SlotCall::ModIncoming { candidate, .. } = call else {
            panic!("结构受击槽只应以 ModIncoming 调用");
        };
        assert_eq!(candidate.input().damage, 50, "输入为当次伤害原值");
        let source = candidate
            .input()
            .source
            .as_ref()
            .expect("在场攻击者进入来源摘要");
        assert_eq!(source.source_id, ObjId(3).as_str());
        assert_eq!(source.attack_type, c::EVENT_ATTACK_TYPE_MELEE);
        if self.decline {
            return Ok(kk_mod_api::SlotOutcome::Declined(kk_mod_api::Decline {
                binding: Key::new("gvmod:guard").unwrap(),
                code: "skip".into(),
            }));
        }
        candidate.set_damage(25).expect("修改伤害量");
        Ok(kk_mod_api::SlotOutcome::Completed)
    }
}

/// 装配托管派发（Replace 覆写 builtin:mod.receiveDamage），放入攻击者
/// creep 与耐久 Mod 目标，并在意图时刻施加 50 点近战伤害。
fn hosted_after_damage(decline: bool) -> RoomCtx {
    hosted_ctx(
        decline,
        vec![],
        r#"{"x":10,"y":10,"hits":120,"hitsMax":300}"#,
    )
}

fn hosted_ctx(
    decline: bool,
    object_types: Vec<kk_mod_api::ObjectTypeDescriptor>,
    target_data: &str,
) -> RoomCtx {
    let contribution = Contribution {
        source: Source {
            namespace: "gvmod".into(),
            api_version: 1,
            revision: "1".into(),
            code_digest: [7; 32],
            config_digest: [0; 32],
            player_api_digest: [0; 32],
        },
        bindings: BTreeSet::from([Key::new("gvmod:guard").unwrap()])
            .into_iter()
            .chain(
                object_types
                    .iter()
                    .flat_map(|ty| ty.fields.iter().map(|field| field.binding.clone())),
            )
            .collect(),
        object_types,
        field_mounts: vec![],
        intents: vec![],
        rules: vec![],
        overrides: [].into_iter().collect(),
        constraints: BTreeSet::new(),
        slots: vec![],
        handlers: vec![HandlerDescriptor {
            key: Key::new("gvmod:guard.main").unwrap(),
            slot: Key::new(kk_mod_api::builtin::MOD_INCOMING_SLOT).unwrap(),
            kind: HandlerKind::Replace,
            binding: Key::new("gvmod:guard").unwrap(),
            depends_on: BTreeSet::new(),
        }],
        resources: vec![],
        buildings: vec![],
    };
    let plan: Arc<kk_mod_api::ContributionPlan> = Arc::new(
        kk_mod_api::freeze_with_builtin(
            kk_mod_api::builtin::link(),
            vec![contribution],
            kk_mod_api::FreezeOptions::default(),
        )
        .unwrap(),
    );
    let dispatch = ContributionDispatch::assemble_hosted(
        &plan,
        Rc::new(GuardExecutor { decline }),
        &BTreeSet::from([Key::new("gvmod:guard").unwrap()]),
    )
    .unwrap();
    assert!(dispatch.is_mod_incoming_hosted());
    let ctx: RoomCtx = Rc::new(RefCell::new(Working::new(
        RoomState::new("W1N1", kk_core::Terrain::plains()),
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            buildings: Default::default(),
            links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
            stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
            combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
            move_gate: Default::default(),
            quotas: Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
            contrib: Rc::new(dispatch),
            extensions: Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan,
        },
    )));
    ctx.borrow_mut().room.objects.insert(
        ObjId(3),
        RoomObject::Creep(CreepObj {
            id: 3,
            x: 10,
            y: 9,
            fatigue: 0,
            age_time: u64::MAX / 2,
            hits: 200,
            spawning: false,
            action_log: 0,
            name: "atk".into(),
            owner: "foe".into(),
            body: vec![kk_schema::room::BodyPart {
                kind: 4,
                hits: 100,
                boost: None,
            }],
            store: kk_schema::room::Store {
                energy: 0,
                ghodium: 0,
            },
            store_extra: Vec::new(),
            say_message: None,
            say_public: None,
            notify_when_attacked: None,
        }),
    );
    ctx.borrow_mut().room.objects.insert(
        ObjId(5),
        RoomObject::Mod(ModObj {
            id: 5,
            x: 10,
            y: 10,
            type_key: "gvmod:ward".into(),
            data: target_data.into(),
        }),
    );
    apply_damage(
        &mut ctx.borrow_mut(),
        ObjId(3),
        ObjId(5),
        50,
        c::EVENT_ATTACK_TYPE_MELEE,
    );
    ctx
}

fn hits_of(w: &Working, id: ObjId) -> i64 {
    object_hits(w.room.objects.get(&id).expect("目标在场"))
}

fn main_attack_count(w: &Working) -> usize {
    w.events
        .iter()
        .filter(|event| {
            event["event"] == c::EVENT_ATTACK
                && event["objectId"] == json!(ObjId(3).as_str())
                && event["data"]["targetId"] == json!(ObjId(5).as_str())
                && event["data"]["damage"] == json!(50)
        })
        .count()
}

/// 按 room_loop 的排空顺序消费挂起队列并归还 Working。
fn drain(ctx: RoomCtx) -> Working {
    let pending = std::mem::take(&mut ctx.borrow_mut().mod_damage_pending);
    assert_eq!(pending.len(), 1, "托管伤害恰挂起一项");
    let contrib = ctx.borrow().plane.contrib.clone();
    let mut drained = Vec::new();
    for item in pending {
        // 与 room_loop 排空同款门：目标或攻击者已消失的条目整项丢弃。
        let gone = {
            let w = ctx.borrow();
            !matches!(w.room.objects.get(&item.target), Some(RoomObject::Mod(_)))
                || !w.room.objects.contains_key(&item.attacker)
        };
        if gone {
            continue;
        }
        let damage = contrib
            .run_mod_incoming(
                &ctx,
                item.target,
                item.damage,
                item.attacker,
                item.attack_type,
            )
            .unwrap();
        drained.push((item, damage));
    }
    for (item, damage) in drained {
        apply_mod_damage(&mut ctx.borrow_mut(), item, damage);
    }
    Rc::into_inner(ctx).unwrap().into_inner()
}

/// 托管派发：意图时刻只挂起（不扣减、主攻击事件照发），排空时刻经槽
/// 减半后落地；主攻击事件全程恰一次。
#[test]
fn hosted_slot_defers_then_applies_modified_damage() {
    let ctx = hosted_after_damage(false);
    {
        let w = ctx.borrow();
        assert_eq!(hits_of(&w, ObjId(5)), 120, "意图时刻不扣减");
        assert_eq!(main_attack_count(&w), 1, "主攻击事件在意图时刻发出");
    }
    let w = drain(ctx);
    assert_eq!(hits_of(&w, ObjId(5)), 95, "排空落地槽修改后的伤害");
    assert_eq!(main_attack_count(&w), 1, "排空不重复主攻击事件");
}

/// 正常拒绝（Declined）回退默认伤害：扣减按传入原值落地。
#[test]
fn declined_slot_falls_back_to_default_damage() {
    let w = drain(hosted_after_damage(true));
    assert_eq!(hits_of(&w, ObjId(5)), 70, "拒绝回退默认伤害");
    assert_eq!(main_attack_count(&w), 1);
}

/// Native 派发（默认引擎）零开销内联：无挂起、意图时刻直接扣减。
#[test]
fn native_dispatch_applies_inline_without_pending() {
    let ctx = hosted_after_damage(false);
    // 用默认派发重放同一伤害：不经过挂起队列。
    let plan = ctx.borrow().plane.plan.clone();
    let w = Rc::into_inner(ctx).unwrap().into_inner();
    let mut w = Working::new(
        w.room,
        0,
        0,
        w.state.clone(),
        Plane {
            buildings: Default::default(),
            links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
            stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
            combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
            move_gate: Default::default(),
            quotas: Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
            contrib: Rc::new(ContributionDispatch::default()),
            extensions: Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan,
        },
    );
    assert!(!w.plane.contrib.is_mod_incoming_hosted());
    apply_damage(&mut w, ObjId(3), ObjId(5), 50, c::EVENT_ATTACK_TYPE_MELEE);
    assert!(w.mod_damage_pending.is_empty(), "Native 不挂起");
    assert_eq!(hits_of(&w, ObjId(5)), 70, "意图时刻内联扣减");
    assert_eq!(main_attack_count(&w), 1);
}

/// 声明反击基数是原始伤害（combat「按减伤前量」契约）：托管槽把 50 减
/// 半为 25 落地扣减，反击仍按 50×500/1000=25 计——Native/Hosted 同值。
#[test]
fn hosted_retaliation_base_stays_raw_damage() {
    let xy = |name: &str| kk_mod_api::FieldDescriptor {
        key: Key::new(format!("gvmod:{name}")).unwrap(),
        schema: kk_mod_api::ValueSchema {
            kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                min: 0,
                max: 49,
            }),
            nullable: false,
        },
        default: kk_mod_api::FieldDefault::Missing,
        visibility: kk_mod_api::Visibility::Public,
        binding: Key::new(format!("gvmod:ward.field.{name}")).unwrap(),
    };
    let ward = kk_mod_api::ObjectTypeDescriptor {
        key: Key::new("gvmod:ward").unwrap(),
        schema_version: 1,
        owner_field: None,
        position_fields: Some([Key::new("gvmod:x").unwrap(), Key::new("gvmod:y").unwrap()]),
        fields: vec![xy("x"), xy("y")],
        capabilities: Vec::new(),
        lifecycle_slots: BTreeSet::new(),
        visibility: kk_mod_api::ObjectVisibility::RoomVisible,
        player_class: "Ward".into(),
        store_capacity: None,
        hits_max: Some(300),
        retaliation_milli: Some(500),
    };
    let ctx = hosted_ctx(
        false,
        vec![ward],
        r#"{"x":10,"y":10,"hits":300,"hitsMax":300}"#,
    );
    let w = drain(ctx);
    assert_eq!(hits_of(&w, ObjId(5)), 275, "扣减用槽修改值 25");
    assert_eq!(
        w.damage_to_apply.get(&ObjId(3)),
        Some(&25),
        "反击按原始基数 50×0.5"
    );
    let hit_backs: Vec<_> = w
        .events
        .iter()
        .filter(|e| e["data"]["attackType"] == json!(c::EVENT_ATTACK_TYPE_HIT_BACK))
        .collect();
    assert_eq!(hit_backs.len(), 1);
    assert_eq!(hit_backs[0]["data"]["damage"], json!(25));
}

/// 排空前攻击者已消失：条目整项丢弃（不跑槽、不扣减、不反击、无 panic）。
#[test]
fn pending_item_with_vanished_attacker_is_dropped() {
    let ctx = hosted_after_damage(false);
    ctx.borrow_mut().room.objects.remove(&ObjId(3));
    let w = drain(ctx);
    assert_eq!(hits_of(&w, ObjId(5)), 120, "伤害无处归因，整项丢弃");
    assert!(!w.damage_to_apply.contains_key(&ObjId(3)));
    assert_eq!(main_attack_count(&w), 1, "意图时刻的主攻击事件保留");
}
