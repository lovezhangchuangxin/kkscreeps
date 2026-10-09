//! Source Keeper 的资源巡守和攻击预意图；目标按稳定对象顺序选择。
use kk_schema::room::RoomObject;
use kk_world::{
    ObjId,
    domain::{keeper::OWNER, npc::is_npc_owner},
};

use crate::intent::{Action, IntentData, UserIntents};
use crate::settle::{SettleError, Working};

use super::path;

fn range(a: (u8, u8), b: (u8, u8)) -> u8 {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}

/// 玩家 creep（包括孵化雏形）与 powerCreep 都是 Keeper 的敌方；NPC 用户排除。
/// 来源：keepers/pretick.js（与 Invader 的 !spawning 分类不同）。
/// 官方入桶不读 hits：正常对局 hits 恒为整数，手工检查点注入缺失 hits 的
/// powerCreep 时仍计入全部敌桶（mass 照加、最低 hits 选择忽略它）。
fn hostile_hits(o: &RoomObject) -> Option<Option<u64>> {
    match o {
        RoomObject::Creep(c) if !is_npc_owner(&c.owner) => Some(Some(u64::from(c.hits))),
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).ok()?;
            if data["type"] != "powerCreep" || data["owner"].as_str().is_some_and(is_npc_owner) {
                return None;
            }
            Some(data["hits"].as_u64())
        }
        _ => None,
    }
}

/// 先记忆资源/生成移动，再按最低 hits 发近战与远程；mass 收益用固定
/// [10,10,4,1] 估计且严格 >13，与实际身体效能及 rampart 门分离。
/// 指令交给现有压制/战斗链执行，不在 AI 中直接扣血或移动。
pub(super) fn intents(w: &mut Working) -> Result<UserIntents, SettleError> {
    let mut out = UserIntents::default();
    let keepers: Vec<_> = w
        .room
        .objects
        .iter()
        .filter_map(|(id, o)| matches!(o,RoomObject::Creep(c) if c.owner==OWNER).then_some(*id))
        .collect();
    for id in keepers {
        let actor = w.room.objects[&id].pos();
        let mut memory = w.room.keeper_memory.get(&id).cloned().unwrap_or_default();
        let target = memory
            .source_id
            .map(ObjId)
            .filter(|target| w.room.objects.contains_key(target))
            .or_else(|| {
                w.room.objects.iter().find_map(|(oid, o)| {
                    (matches!(o, RoomObject::Source(_) | RoomObject::Mineral(_))
                        && range(actor, o.pos()) <= 5)
                        .then_some(*oid)
                })
            });
        let actions = out.objects.entry(id).or_default();
        if let Some(target) = target {
            memory.source_id = Some(target.0);
            let position = w.room.objects[&target].pos();
            if range(actor, position) > 1 {
                let direction = path::direction(&w.room, actor, position, w.time, &mut memory)?;
                if let Some(direction) = direction {
                    let mut data = IntentData::bare();
                    data.direction = Some(direction);
                    actions.insert(Action::Move, data);
                } else {
                    memory.move_cache = None;
                }
            }
        }
        w.room.keeper_memory.insert(id, memory);
        let targets: Vec<_> = w
            .room
            .objects
            .iter()
            .filter_map(|(oid, o)| {
                let hits = hostile_hits(o)?;
                let distance = range(actor, o.pos());
                (distance <= 3).then_some((*oid, hits, distance))
            })
            .collect();
        // 缺失 hits（仅手工检查点可达）仍留在桶内参与 mass 估计；最低
        // hits 选择跳过它——官方 _.min 跳过 undefined，全部缺失时不发。
        if let Some((target, _, _)) = targets
            .iter()
            .filter(|(_, hits, r)| hits.is_some() && *r <= 1)
            .min_by_key(|(_, hits, _)| hits.unwrap_or(u64::MAX))
        {
            actions.insert(Action::Attack, IntentData::target(*target));
        }
        if targets.is_empty() {
            continue;
        }
        let mass: u64 = targets
            .iter()
            .map(|(_, _, r)| [10, 10, 4, 1][*r as usize])
            .sum();
        if mass > 13 {
            actions.insert(Action::RangedMassAttack, IntentData::bare());
        } else if let Some((target, _, _)) = targets
            .iter()
            .filter(|(_, hits, _)| hits.is_some())
            .min_by_key(|(_, hits, _)| hits.unwrap_or(u64::MAX))
        {
            actions.insert(Action::RangedAttack, IntentData::target(*target));
        }
    }
    Ok(out)
}
