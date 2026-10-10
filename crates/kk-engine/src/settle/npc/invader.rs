//! 普通 Invader 预意图：攻击/追击/逃离、治疗与自动远程开火。
use super::invader_path::range;
use crate::{
    intent::{Action, IntentData, UserIntents},
    settle::{SettleError, Working},
};
use kk_schema::room::RoomObject;
use kk_world::{domain::npc::INVADER_OWNER, types::BodyKind};
mod attack;
mod healer;

pub(super) fn active(actor: &kk_schema::room::CreepObj, kind: BodyKind) -> bool {
    actor
        .body
        .iter()
        .any(|p| p.kind == kind.code() && p.hits > 0)
}

/// 普通 Invader 共用房间输入；每个执行者按稳定对象序更新私有记忆，
/// 先 findAttack/healer，再 shootAtWill（覆盖前者可能生成的远程建筑攻击）。
pub(super) fn intents(w: &mut Working) -> Result<UserIntents, SettleError> {
    let mut out = UserIntents::default();
    let hostiles: Vec<_> = w
        .room
        .objects
        .iter()
        .filter_map(|(id, o)| hostile_hits(o).map(|_| *id))
        .collect();
    let invaders: Vec<_> = w
        .room
        .objects
        .iter()
        .filter_map(|(id, o)| {
            let friendly = match o {
                RoomObject::Creep(c) => c.owner == INVADER_OWNER && !c.spawning,
                RoomObject::Generic(g) => g.data.identity().is_some_and(|identity| {
                    identity.type_name.as_deref() == Some("powerCreep")
                        && identity.owner.as_deref() == Some(INVADER_OWNER)
                }),
                _ => false,
            };
            friendly.then_some(*id)
        })
        .collect();
    let actors: Vec<_> = w
        .room
        .objects
        .iter()
        .filter_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.owner == INVADER_OWNER => Some((*id, c.clone())),
            _ => None,
        })
        .collect();
    let healers: Vec<_> = invaders
        .iter()
        .filter(|id| matches!(&w.room.objects[id],RoomObject::Creep(c) if active(c,BodyKind::Heal)))
        .copied()
        .collect();
    for (id, actor) in actors {
        let mut memory = w.room.invader_memory.get(&id).cloned().unwrap_or_default();
        let actions = out.objects.entry(id).or_default();
        if actor.body.iter().any(|p| p.kind == BodyKind::Heal.code()) {
            healer::intents(w, &actor, &invaders, &hostiles, actions, &mut memory)?;
        } else {
            attack::intents(w, &actor, &hostiles, &healers, actions, &mut memory)?;
        }
        if active(&actor, BodyKind::RangedAttack) {
            let target = hostiles
                .iter()
                .filter_map(|id| {
                    let o = &w.room.objects[id];
                    let hits = hostile_hits(o)??;
                    (range((actor.x, actor.y), o.pos()) <= 3).then_some((*id, hits))
                })
                .min_by_key(|(_, hits)| *hits);
            if let Some((target, _)) = target {
                actions.insert(Action::RangedAttack, IntentData::target(target));
            }
        }
        // 仅在产生真实移动缓存/lastMove 后创建保存项；已有记忆保留。
        if memory.move_cache.is_some() || memory.last_move.is_some() {
            w.room.invader_memory.insert(id, memory);
        }
    }
    Ok(out)
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
            // 前置字面量门（含 \u 转义藏匿兜底）：本臂只认
            // type=powerCreep 的 Generic，其余在解析前排除。
            if !kk_world::domain::literal_gate(&g.data, "\"powerCreep\"") {
                return None;
            }
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

pub(super) fn move_intent(direction: u8) -> IntentData {
    let mut data = IntentData::bare();
    data.direction = Some(direction);
    data
}
