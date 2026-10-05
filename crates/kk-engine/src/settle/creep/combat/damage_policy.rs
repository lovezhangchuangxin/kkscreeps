//! 近战/单体远程攻击的策略伤害应用：只保护候选表示，不复用默认准入。
use super::CombatAction;
use crate::settle::{Working, damage};
use kk_core::constants as c;
use kk_mod_api::combat::{CombatBodyPart, CombatView};
use kk_schema::room::RoomObject;
use kk_world::ObjId;

/// 同房候选的脱离副本。不返回无耐久对象或 Mod 自有字段对象；跨房与
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
    Some(CombatView {
        id: id.as_str(),
        type_key: crate::settle::movement::type_name_of(object).into_owned(),
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
    damage::apply_damage_without_retaliation(w, id, target_id, amount, attack_type);
    Ok(())
}
