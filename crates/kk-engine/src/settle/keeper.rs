//! keeper lair 的倒计时与 Source Keeper 候选创建，不运行 NPC 主动 AI。
use super::{SettleError, Working};
use kk_core::constants as c;
use kk_schema::room::{BodyPart, CreepObj, RoomObject, Store};
use kk_world::{ObjId, domain::keeper::OWNER, types::BodyKind};
use serde_json::{Value, json};

/// 无满血同名 keeper 时开始 300 tick 倒计时，截止前一拍替换旧 keeper。
/// 替换仅删除本体，不创建墓碑或死亡事件；新 creep 下一轮才初始化寿命。
/// 来源：engine/src/processor/intents/keeper-lairs/tick.js。
pub(super) fn tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(RoomObject::Generic(lair)) = w.room.objects.get(&id) else {
        return Ok(());
    };
    let Ok(mut data) = serde_json::from_str::<Value>(&lair.data) else {
        return Ok(());
    };
    let pos = (lair.x, lair.y);
    let name = format!("Keeper{}", id.as_str());
    let keeper = w.room.objects.iter().find_map(|(id, obj)| match obj {
        RoomObject::Creep(creep) if creep.owner == OWNER && creep.name == name => {
            Some((*id, creep.hits))
        }
        _ => None,
    });
    let mut next = data
        .get("nextSpawnTime")
        .and_then(Value::as_u64)
        .filter(|n| *n != 0);
    if next.is_none() && keeper.is_none_or(|(_, hits)| hits < 5000) {
        next = Some(
            kk_world::domain::keeper::deadline(w.time, c::ENERGY_REGEN_TIME as u64)
                .map_err(SettleError)?,
        );
        data["nextSpawnTime"] = json!(next);
    }
    if next.is_some_and(|t| w.time >= t.saturating_sub(1)) {
        if let Some((keeper_id, _)) = keeper {
            w.remove_obj(keeper_id);
        }
        let mut body = Vec::with_capacity(50);
        let part = |kind: BodyKind| BodyPart {
            kind: kind.code(),
            hits: 100,
            boost: None,
        };
        body.extend((0..17).map(|_| part(BodyKind::Tough)));
        body.extend((0..13).map(|_| part(BodyKind::Move)));
        for _ in 0..10 {
            body.push(part(BodyKind::Attack));
            body.push(part(BodyKind::RangedAttack));
        }
        if w.insert_new(RoomObject::Creep(CreepObj {
            id: 0,
            x: pos.0,
            y: pos.1,
            fatigue: 0,
            age_time: 0,
            hits: 5000,
            spawning: false,
            action_log: 0,
            name,
            owner: OWNER.into(),
            body,
            store: Store::default(),
            store_extra: vec![],
            say_message: None,
            say_public: None,
            notify_when_attacked: None,
        }))
        .is_err()
        {
            return Ok(());
        }
        data["nextSpawnTime"] = Value::Null;
    }
    if let Some(RoomObject::Generic(lair)) = w.room.objects.get_mut(&id) {
        lair.data = data.to_string().into();
    }
    Ok(())
}
