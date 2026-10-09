//! Invader 的无目标退场、相邻近战与自动远程攻击；追击/治疗另行接线。
use std::borrow::Cow;

use kk_schema::room::RoomObject;
use kk_world::{RoomState, domain::npc::INVADER_OWNER, types::BodyKind};

use crate::intent::{Action, IntentData, UserIntents};

/// 仅在能够证明无目标的子域生成退场指令：有主控制器、无 spawn、无敌方
/// 已出生 creep/powerCreep。存在目标或 spawn 时还需要官方三轮寻路判断，
/// 本片不猜测其可达性；治疗型（含损毁 HEAL）由独立 healer 分支负责。
/// 来源：invaders/pretick.js 的分类与 findAttack.js 的无目标 suicide 门。
fn retirement_intents(room: &RoomState) -> UserIntents {
    let mut intents = UserIntents::default();
    let retiring: Vec<_> = room
        .objects
        .iter()
        .filter_map(|(id, object)| {
            matches!(object, RoomObject::Creep(c)
            if c.owner == INVADER_OWNER && !c.spawning
            && !c.body.iter().any(|part| part.kind == BodyKind::Heal.code()))
            .then_some(*id)
        })
        .collect();
    if retiring.is_empty() {
        return intents;
    }
    let owned = room.objects.values().any(|o| {
        matches!(o,
        RoomObject::Controller(c) if c.owner.as_deref().is_some_and(|u| !u.is_empty()))
    });
    if !owned
        || room
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Spawn(_)))
    {
        return intents;
    }
    let hostile = room.objects.values().any(|o| {
        let owner = match o {
            RoomObject::Creep(c) if !c.spawning => Some(Cow::Borrowed(c.owner.as_str())),
            RoomObject::Generic(g)
                if kk_world::domain::generic::type_name(&g.data).as_deref()
                    == Some("powerCreep") =>
            {
                kk_world::domain::object_owner(o)
            }
            _ => return false,
        };
        owner.as_deref() != Some(INVADER_OWNER)
            && owner.as_deref() != Some(kk_world::domain::keeper::OWNER)
    });
    if hostile {
        return intents;
    }
    let has_healer = room.objects.values().any(|o| {
        matches!(o,
        RoomObject::Creep(c) if c.owner == INVADER_OWNER && !c.spawning
        && c.body.iter().any(|p| p.kind == BodyKind::Heal.code() && p.hits > 0))
    });
    for id in retiring {
        // 严重受伤且无有效 ATTACK 时，官方可能先寻路靠近友方 healer。
        // 本片不判断此路径可达性，有潜在治疗目标即保留，不误发自杀。
        if has_healer
            && let Some(RoomObject::Creep(c)) = room.objects.get(&id)
            && u64::from(c.hits) * 2 < c.body.len() as u64 * 100
            && !c
                .body
                .iter()
                .any(|p| p.kind == BodyKind::Attack.code() && p.hits > 0)
        {
            continue;
        }
        intents
            .objects
            .entry(id)
            .or_default()
            .insert(Action::Suicide, IntentData::bare());
    }
    intents
}

/// 官方 @screeps/engine 4.3.0 的 invaders/pretick.js：非治疗型零步
/// 相邻近战与 shootAtWill 远程独立；目标选择交由各自规则。
/// 不做寻路、移动或直接扣血，生成的动作走共用战斗链。
pub(super) fn intents(room: &RoomState) -> UserIntents {
    let mut out = retirement_intents(room);
    for (id, object) in &room.objects {
        let RoomObject::Creep(actor) = object else {
            continue;
        };
        if actor.owner != INVADER_OWNER {
            continue;
        }
        // 含损毁 HEAL 仍走官方 healer 分支，不执行 findAttack；远程独立。
        if !actor.body.iter().any(|p| p.kind == BodyKind::Heal.code())
            && actor
                .body
                .iter()
                .any(|p| p.kind == BodyKind::Attack.code() && p.hits > 0)
            && let Some(target) = adjacent_target(room, (actor.x, actor.y))
        {
            out.objects
                .entry(*id)
                .or_default()
                .insert(Action::Attack, IntentData::target(target));
        }
        if !actor
            .body
            .iter()
            .any(|p| p.kind == BodyKind::RangedAttack.code() && p.hits > 0)
        {
            continue;
        }
        let target = room
            .objects
            .iter()
            .filter_map(|(id, object)| {
                let hits = hostile_hits(object)??;
                let (x, y) = object.pos();
                (actor.x.abs_diff(x).max(actor.y.abs_diff(y)) <= 3).then_some((*id, hits))
            })
            .min_by_key(|(_, hits)| *hits);
        if let Some((target, _)) = target {
            out.objects
                .entry(*id)
                .or_default()
                .insert(Action::RangedAttack, IntentData::target(target));
        }
    }
    out
}

/// 孵化 creep 排除；powerCreep 不读 spawning。入桶不读取 hits，近战
/// 零步选择仍保留缺 hits 的目标，远程最低值选择单独跳过它。
fn hostile_hits(object: &RoomObject) -> Option<Option<u64>> {
    match object {
        RoomObject::Creep(c)
            if !c.spawning
                && c.owner != INVADER_OWNER
                && c.owner != kk_world::domain::keeper::OWNER =>
        {
            Some(Some(u64::from(c.hits)))
        }
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).ok()?;
            if data["type"] != "powerCreep"
                || data["owner"]
                    .as_str()
                    .is_some_and(kk_world::domain::npc::is_npc_owner)
            {
                return None;
            }
            Some(data["hits"].as_u64())
        }
        _ => None,
    }
}

/// findAttack.js 最后一次 attack 覆盖来自 findClosestByPath 的零步结果：
/// 同格优先首个，否则空路径扫描相邻桶取最后一个，不能按最低 hits。
/// 仅实现已相邻分支；moveTo 路径中的建筑改向/拆毁随追击片接线。
fn adjacent_target(room: &RoomState, actor: (u8, u8)) -> Option<kk_world::ObjId> {
    let mut adjacent = None;
    for (id, object) in &room.objects {
        if hostile_hits(object).is_none() {
            continue;
        }
        let pos = object.pos();
        if pos == actor {
            return Some(*id);
        }
        if actor.0.abs_diff(pos.0).max(actor.1.abs_diff(pos.1)) <= 1 {
            adjacent = Some(*id);
        }
    }
    adjacent
}
