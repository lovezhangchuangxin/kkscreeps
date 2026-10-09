//! 近战/单体远程攻击的策略伤害应用：只保护候选表示，不复用默认准入。
use super::CombatAction;
use crate::settle::{Working, damage};
use kk_core::constants as c;
use kk_mod_api::combat::{CombatBodyPart, CombatView};
use kk_schema::room::RoomObject;
use kk_world::ObjId;

/// 同房候选的脱离副本。不返回无耐久对象（含未声明耐久的 Mod 对象）；
/// 耐久声明的 Mod 对象按结构语义返回（hitsMax 经信封键读出）；跨房与
/// 缺失目标同样为 None，不扩大写能力的访问域。
pub fn slot_combat_get(w: &Working, id: ObjId) -> Option<CombatView> {
    let object = w.room.objects.get(&id)?;
    let (hits_max, body) = match object {
        RoomObject::Creep(creep) => (
            creep.body.len() as i64 * 100,
            Some(
                creep
                    .body
                    .iter()
                    .map(|part| {
                        Some(CombatBodyPart {
                            kind: kk_world::types::BodyKind::from_code(part.kind)?
                                .name()
                                .into(),
                            hits: part.hits,
                            boost: part.boost.clone(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            ),
        ),
        _ => (damage::structure_hits_max(w, object)?, None),
    };
    // Mod 对象（G09.b）的类型标识为类型键：type_name_of 对 Mod 返回
    // 空串（结构规则不认 Mod），战斗读面需要非空标识与事件 type 同源。
    let type_key = match object {
        RoomObject::Mod(m) => m.type_key.clone(),
        _ => crate::settle::movement::type_name_of(object).into_owned(),
    };
    Some(CombatView {
        id: id.as_str(),
        type_key,
        owner: kk_world::domain::object_owner(object).map(|owner| owner.into_owned()),
        x: object.pos().0,
        y: object.pos().1,
        hits: damage::object_hits(object),
        hits_max,
        body,
        spawning: match object {
            RoomObject::Creep(creep) => Some(creep.spawning),
            _ => None,
        },
    })
}

/// 施加 JS 计算的伤害。调用方先记首触日志；目标限定同房存活 creep 或
/// 可受伤结构。不检查距离/阵营/安全模式、部件功率或 rampart 改向，
/// 不追加原生反击；这些都是策略而非坐标/生命值表示不变量。
pub fn slot_combat_apply_damage(
    w: &mut Working,
    id: ObjId,
    target_id: ObjId,
    amount: i64,
    action: CombatAction,
) -> Result<(), String> {
    slot_combat_apply_damage_with_retaliation(w, id, target_id, amount, action, None)
}

pub fn slot_combat_apply_damage_with_retaliation(
    w: &mut Working,
    id: ObjId,
    target_id: ObjId,
    amount: i64,
    action: CombatAction,
    policy: Option<&super::RetaliationPolicy>,
) -> Result<(), String> {
    if !(0..=kk_mod_api::combat::MAX_DAMAGE).contains(&amount) {
        return Err("策略伤害须为 0..=1e9 的整数".into());
    }
    let attack_type = match action {
        CombatAction::Attack => c::EVENT_ATTACK_TYPE_MELEE,
        CombatAction::RangedAttack => c::EVENT_ATTACK_TYPE_RANGED,
        _ => return Err("策略伤害仅在 attack/rangedAttack 槽开放".into()),
    };
    let actor = w.creep(id).ok_or("策略执行者不是有效 creep")?;
    if actor.spawning {
        return Err("孵化中的 creep 不能施加策略伤害".into());
    }
    validate_damage_target(w, target_id)?;
    super::retaliation_policy::preflight(w, id, policy, if target_id == id { amount } else { 0 })?;
    w.damage_to_apply
        .get(&target_id)
        .copied()
        .unwrap_or(0)
        .checked_add(amount)
        .ok_or("待伤害累计量超出表示域")?;
    super::retaliation_policy::apply_damage(w, id, target_id, amount, attack_type, policy, false);
    Ok(())
}

/// 单体与批量策略共享目标表示校验；不包含原生距离、阵营或护墙策略。
pub(super) fn validate_damage_target(w: &Working, target_id: ObjId) -> Result<(), String> {
    let target = w
        .room
        .objects
        .get(&target_id)
        .ok_or("策略目标不存在于本房")?;
    let attackable = match target {
        RoomObject::Creep(creep) => !creep.spawning,
        _ => damage::structure_hits_max(w, target).is_some(),
    };
    if !attackable || damage::object_hits(target) <= 0 {
        return Err("策略目标须为存活 creep 或有耐久的结构".into());
    }
    Ok(())
}

#[cfg(test)]
mod mod_view_tests {
    use super::*;
    use crate::settle::Plane;
    use crate::settle::{TickState, Working};
    use kk_schema::room::{ModObj, RoomObject};
    use kk_world::RoomState;

    /// G09.b：耐久声明的 Mod 对象进入战斗读面与策略伤害目标校验，
    /// CombatView 类型标识为类型键（type_name_of 对 Mod 恒空串）。
    #[test]
    fn combat_view_and_damage_target_accept_durability_mod() {
        let contribution = kk_mod_api_test_type();
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![contribution],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        let room = RoomState::new("W1N1", kk_core::Terrain::plains());
        let mut w = Working::new(
            room,
            0,
            0,
            TickState::new(std::collections::BTreeMap::new(), 100),
            Plane {
                buildings: Default::default(),
                links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
                stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
                combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
                move_gate: Default::default(),
                quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
                static_passability: Default::default(),
                resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(
                    &plan,
                )),
                contrib: std::rc::Rc::new(crate::settle::contrib::ContributionDispatch::default()),
                extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
                plan,
            },
        );
        let armored = kk_world::ObjId(1);
        w.room.objects.insert(
            armored,
            RoomObject::Mod(ModObj {
                id: 1,
                x: 10,
                y: 10,
                type_key: "cvmod:bunker".into(),
                data: r#"{"x":10,"y":10,"hits":120,"hitsMax":300}"#.into(),
            }),
        );
        let view = slot_combat_get(&w, armored).expect("耐久对象进入战斗读面");
        assert_eq!(view.type_key, "cvmod:bunker");
        assert_eq!(view.hits, 120);
        assert_eq!(view.hits_max, 300);
        assert!(
            view.body.is_none() && view.spawning.is_none(),
            "结构语义无身体"
        );
        validate_damage_target(&w, armored).expect("策略伤害目标接纳");
        // 缺信封（无耐久）对象被双面拒绝。
        let bare = kk_world::ObjId(2);
        w.room.objects.insert(
            bare,
            RoomObject::Mod(ModObj {
                id: 2,
                x: 11,
                y: 10,
                type_key: "cvmod:bunker".into(),
                data: r#"{"x":11,"y":10}"#.into(),
            }),
        );
        assert!(slot_combat_get(&w, bare).is_none());
        assert!(validate_damage_target(&w, bare).is_err());
    }

    /// G09.d：显式反击策略整体替换声明式反击（combat.md「空表可禁用
    /// 反击」契约）；未列目标得 0 反击，列表目标只得策略量。
    #[test]
    fn explicit_policy_replaces_declared_retaliation() {
        let contribution = kk_mod_api_test_type();
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![contribution],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        let room = RoomState::new("W1N1", kk_core::Terrain::plains());
        let mut w = Working::new(
            room,
            0,
            0,
            TickState::new(std::collections::BTreeMap::new(), 100),
            Plane {
                buildings: Default::default(),
                links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
                stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
                combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
                move_gate: Default::default(),
                quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
                static_passability: Default::default(),
                resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(
                    &plan,
                )),
                contrib: std::rc::Rc::new(crate::settle::contrib::ContributionDispatch::default()),
                extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
                plan,
            },
        );
        let spike = kk_world::ObjId(1);
        w.room.objects.insert(
            spike,
            RoomObject::Mod(ModObj {
                id: 1,
                x: 10,
                y: 10,
                type_key: "cvmod:spike".into(),
                data: r#"{"x":10,"y":10,"hits":300,"hitsMax":300}"#.into(),
            }),
        );
        // 攻击者是敌对 creep（200 血，免疫治疗差异无关）。
        let attacker = kk_world::ObjId(2);
        w.room.objects.insert(
            attacker,
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 2,
                x: 10,
                y: 9,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "atk".into(),
                owner: "p2".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: 4,
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: 0,
                        hits: 100,
                        boost: None,
                    },
                ],
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
        // 策略表列出 spike：只得策略量 7，不吃声明反击 15。
        crate::settle::damage::apply_damage_with_retaliation(
            &mut w,
            attacker,
            spike,
            30,
            kk_core::constants::EVENT_ATTACK_TYPE_MELEE,
            7,
        );
        let events = std::mem::take(&mut w.events);
        let hit_backs: Vec<_> = events
            .iter()
            .filter(|e| {
                e["data"]["attackType"]
                    == serde_json::json!(kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK)
            })
            .collect();
        assert_eq!(hit_backs.len(), 1, "恰一条 HIT_BACK：{events:?}");
        assert_eq!(hit_backs[0]["data"]["damage"], serde_json::json!(7));
        assert_eq!(w.damage_to_apply.get(&attacker), Some(&7), "不叠加声明反击");
    }

    fn kk_mod_api_test_type() -> kk_mod_api::Contribution {
        let field = |name: &str| kk_mod_api::FieldDescriptor {
            key: kk_mod_api::Key::new(format!("cvmod:{name}")).unwrap(),
            schema: kk_mod_api::ValueSchema {
                kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                    min: 0,
                    max: 49,
                }),
                nullable: false,
            },
            default: kk_mod_api::FieldDefault::Missing,
            visibility: kk_mod_api::Visibility::Public,
            binding: kk_mod_api::Key::new(format!("cvmod:bunker.field.{name}")).unwrap(),
        };
        kk_mod_api::Contribution {
            source: kk_mod_api::Source {
                namespace: "cvmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [13; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: std::collections::BTreeSet::from([
                kk_mod_api::Key::new("cvmod:bunker.field.x").unwrap(),
                kk_mod_api::Key::new("cvmod:bunker.field.y").unwrap(),
                kk_mod_api::Key::new("cvmod:spike.field.x").unwrap(),
                kk_mod_api::Key::new("cvmod:spike.field.y").unwrap(),
            ]),
            field_mounts: Vec::new(),
            object_types: vec![
                kk_mod_api::ObjectTypeDescriptor {
                    key: kk_mod_api::Key::new("cvmod:spike").unwrap(),
                    schema_version: 1,
                    owner_field: None,
                    position_fields: Some([
                        kk_mod_api::Key::new("cvmod:x").unwrap(),
                        kk_mod_api::Key::new("cvmod:y").unwrap(),
                    ]),
                    fields: vec![field("x"), field("y")],
                    capabilities: Vec::new(),
                    lifecycle_slots: std::collections::BTreeSet::new(),
                    visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                    player_class: "Spike".into(),
                    store_capacity: None,
                    hits_max: Some(300),
                    retaliation_milli: Some(500),
                },
                kk_mod_api::ObjectTypeDescriptor {
                    key: kk_mod_api::Key::new("cvmod:bunker").unwrap(),
                    schema_version: 1,
                    owner_field: None,
                    position_fields: Some([
                        kk_mod_api::Key::new("cvmod:x").unwrap(),
                        kk_mod_api::Key::new("cvmod:y").unwrap(),
                    ]),
                    fields: vec![field("x"), field("y")],
                    capabilities: Vec::new(),
                    lifecycle_slots: std::collections::BTreeSet::new(),
                    visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                    player_class: "Bunker".into(),
                    store_capacity: None,
                    hits_max: Some(300),
                    retaliation_milli: None,
                },
            ],
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: std::collections::BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        }
    }
}
