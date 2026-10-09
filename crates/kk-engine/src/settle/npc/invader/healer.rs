//! 锁定官方 healer.js：缺血量评分、治疗/跟随、严重受伤逃离与无目标退场。
use super::super::{
    invader_path::{self, range},
    path::Options,
};
use super::active;
use crate::{
    intent::{Action, IntentData},
    settle::{SettleError, Working, movement},
};
use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, domain::invader_memory::InvaderMemory, types::BodyKind};
use std::collections::BTreeMap;

fn missing(o: &RoomObject) -> i64 {
    match o {
        RoomObject::Creep(c) => c.body.len() as i64 * 100 - i64::from(c.hits),
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            data["hitsMax"].as_i64().unwrap_or(0) - data["hits"].as_i64().unwrap_or(0)
        }
        _ => 0,
    }
}

pub(super) fn intents(
    w: &Working,
    actor: &CreepObj,
    invaders: &[ObjId],
    hostiles: &[ObjId],
    actions: &mut BTreeMap<Action, IntentData>,
    memory: &mut InvaderMemory,
) -> Result<(), SettleError> {
    let pos = (actor.x, actor.y);
    // stable sort 的首项：最大缺血量，平手保留第一个对象（包含自己/满血）。
    let mut heal_target = None;
    for id in invaders {
        if range(pos, w.room.objects[id].pos()) <= 3
            && heal_target
                .is_none_or(|old| missing(&w.room.objects[id]) > missing(&w.room.objects[&old]))
        {
            heal_target = Some(*id);
        }
    }
    if let Some(id) = heal_target {
        actions.insert(
            if range(pos, w.room.objects[&id].pos()) <= 1 {
                Action::Heal
            } else {
                Action::RangedHeal
            },
            IntentData::target(id),
        );
    }
    if u64::from(actor.hits) * 2 < actor.body.len() as u64 * 100 {
        if let Some(direction) = invader_path::flee(w, pos, hostiles, 4)? {
            actions.insert(Action::Move, super::move_intent(direction));
        } else {
            let helpers:Vec<_>=invaders.iter().filter(|id|id.0!=actor.id && matches!(&w.room.objects[id],RoomObject::Creep(c) if active(c,BodyKind::Heal))).copied().collect();
            if let Some(id) = invader_path::closest(w, pos, &helpers, Options::default())?
                && let Some(direction) = invader_path::move_to(
                    w,
                    pos,
                    w.room.objects[&id].pos(),
                    1,
                    Options::default(),
                    w.time,
                    memory,
                )?
            {
                actions.insert(Action::Move, super::move_intent(direction));
            }
        }
        return Ok(());
    }
    let injured: Vec<_> = invaders
        .iter()
        .filter(|id| missing(&w.room.objects[id]) > 0)
        .copied()
        .collect();
    let mut target = invader_path::closest(w, pos, &injured, Options::default())?;
    if target.is_none() {
        if let Some(direction) = invader_path::flee(w, pos, hostiles, 4)? {
            actions.insert(Action::Move, super::move_intent(direction));
            return Ok(());
        }
        let fighters:Vec<_>=invaders.iter().filter(|id|id.0!=actor.id && !matches!(&w.room.objects[id],RoomObject::Creep(c) if active(c,BodyKind::Heal))).copied().collect();
        target = invader_path::closest(w, pos, &fighters, Options::default())?;
    }
    let Some(id) = target else {
        actions.insert(Action::Suicide, IntentData::bare());
        return Ok(());
    };
    let dest = w.room.objects[&id].pos();
    let direction = if range(pos, dest) <= 1 {
        Some(movement::direction_from_delta(
            i32::from(dest.0) - i32::from(pos.0),
            i32::from(dest.1) - i32::from(pos.1),
        ))
        .filter(|d| *d != 0)
    } else {
        invader_path::move_to(w, pos, dest, 1, Options::default(), w.time, memory)?
    };
    if let Some(direction) = direction {
        actions.insert(Action::Move, super::move_intent(direction));
    }
    Ok(())
}
