//! 锁定官方 findAttack.js 的多轮目标寻路、退场与路径首格建筑改向。
use super::super::{
    invader_path::{self, range},
    path::{self, Options},
};
use super::active;
use crate::{
    intent::{Action, IntentData},
    settle::{SettleError, Working, movement},
};
use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, domain::invader_memory::InvaderMemory, types::BodyKind};
use std::collections::BTreeMap;

pub(super) fn intents(
    w: &Working,
    actor: &CreepObj,
    hostiles: &[ObjId],
    healers: &[ObjId],
    actions: &mut BTreeMap<Action, IntentData>,
    memory: &mut InvaderMemory,
) -> Result<(), SettleError> {
    let pos = (actor.x, actor.y);
    let have_attack = active(actor, BodyKind::Attack);
    if !have_attack
        && active(actor, BodyKind::RangedAttack)
        && let Some(direction) = invader_path::flee(w, pos, hostiles, 3)?
    {
        actions.insert(Action::Move, super::move_intent(direction));
        return Ok(());
    }
    let mut target = None;
    let roads = Options {
        ignore_roads: true,
        ..Default::default()
    };
    if u64::from(actor.hits) * 2 < actor.body.len() as u64 * 100
        && !healers.is_empty()
        && !have_attack
    {
        target = invader_path::closest(w, pos, healers, roads)?;
        if let Some(id) = target {
            if let Some(direction) =
                invader_path::move_to(w, pos, w.room.objects[&id].pos(), 0, roads, w.time, memory)?
            {
                actions.insert(Action::Move, super::move_intent(direction));
            } else {
                target = None;
            }
        }
    }
    if have_attack
        && let Some(id) = hostiles
            .iter()
            .find(|id| range(pos, w.room.objects[id].pos()) <= 1)
    {
        actions.insert(Action::Attack, IntentData::target(*id));
    }
    for opts in [
        Options {
            ignore_creeps: true,
            ..roads
        },
        Options {
            ignore_ramparts: true,
            ..roads
        },
        Options {
            ignore_destructible: true,
            ..roads
        },
    ] {
        if target.is_some() {
            break;
        }
        target = invader_path::closest(w, pos, hostiles, opts)?;
        if let Some(id) = target
            && (have_attack || range(pos, w.room.objects[&id].pos()) > 3)
            && let Some(direction) =
                invader_path::move_to(w, pos, w.room.objects[&id].pos(), 0, opts, w.time, memory)?
        {
            actions.insert(Action::Move, super::move_intent(direction));
        }
    }
    let Some(target) = target else {
        let mut unreachable = None;
        for (id, o) in &w.room.objects {
            if !matches!(o, RoomObject::Spawn(_)) {
                continue;
            }
            let dest = o.pos();
            let route = path::search(
                w,
                kk_world::domain::npc::INVADER_OWNER,
                pos,
                &[(dest, 0)],
                Options::default(),
            )?;
            let end = route.last().map(|p| (p[0], p[1])).unwrap_or(pos);
            // 官方对 RoomPosition（非 goal record）补上相邻终点，incomplete 也补。
            if range(end, dest) != 1 && (route.is_empty() || end != dest) {
                unreachable = Some(*id);
                break;
            }
        }
        if unreachable.is_none()
            && w.room
                .controller()
                .is_some_and(|c| c.owner.as_deref().is_some_and(|u| !u.is_empty()))
        {
            actions.insert(Action::Suicide, IntentData::bare());
            return Ok(());
        }
        if let Some(id) = unreachable
            && let Some(direction) = invader_path::move_to(
                w,
                pos,
                w.room.objects[&id].pos(),
                0,
                Options {
                    ignore_destructible: true,
                    ..roads
                },
                w.time,
                memory,
            )?
        {
            actions.insert(Action::Move, super::move_intent(direction));
        }
        return Ok(());
    };
    actions.insert(Action::Attack, IntentData::target(target));
    if (have_attack || active(actor, BodyKind::Work))
        && let Some(first) = memory
            .move_cache
            .as_ref()
            .and_then(|cache| cache.path.first())
        && let Some((id, _)) = w.room.objects.iter().find(|(_, o)| {
            let kind = movement::type_name_of(o);
            kind != "spawn"
                && crate::settle::controller_allowed(&kind, 8) > 0
                && o.pos() == (first[0], first[1])
        })
    {
        if active(actor, BodyKind::RangedAttack) {
            actions.insert(Action::RangedAttack, IntentData::target(*id));
        }
        actions.insert(
            if active(actor, BodyKind::Work) {
                Action::Dismantle
            } else {
                Action::Attack
            },
            IntentData::target(*id),
        );
    }
    Ok(())
}
