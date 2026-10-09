//! NPC 预意图生成与合并；Keeper 巡守/战斗、Invader 无目标退场。
mod keeper;
mod path;
use std::borrow::Cow;

use kk_schema::room::RoomObject;
use kk_world::{RoomState, domain::npc::INVADER_OWNER, types::BodyKind};

use crate::intent::{Action, IntentBatch, IntentData, UserIntents};

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

/// NPC 与玩家共用动作派发/候选提交；显式同名动作覆盖自动指令，按官方
/// processor.js 的 assign(ii, existing) 合并。无 NPC 指令时不复制玩家 batch。
/// Keeper 记忆写入房间候选，重试/回放从同一房间输入确定性重建。
pub(super) fn with_npc_intents<'a>(
    w: &mut super::Working,
    batch: &'a IntentBatch,
) -> Result<Cow<'a, IntentBatch>, super::SettleError> {
    let mut all = vec![(INVADER_OWNER, retirement_intents(&w.room))];
    all.push((kk_world::domain::keeper::OWNER, keeper::intents(w)?));
    let mut batch = Cow::Borrowed(batch);
    for (owner, generated) in all {
        if !generated.objects.is_empty() {
            let user = batch.to_mut().users.entry(owner.into()).or_default();
            for (id, actions) in generated.objects {
                let existing = user.objects.entry(id).or_default();
                for (action, data) in actions {
                    existing.entry(action).or_insert(data);
                }
            }
        }
    }
    Ok(batch)
}
