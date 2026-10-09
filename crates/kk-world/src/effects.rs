//! 对象效果的权威状态、有效期和对外投影；不解释具体玩法公式。
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// 同 power 替换，不同 power 共存；end_time 为绝对 tick。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectEntry {
    /// 效果标识；官方 PWR_SHIELD 专用分支只写 {power, level, endTime}
    /// 不写 effect（usePower.js:248-253），玩家面读 undefined——None 表
    /// 该形态；通用路径 effect==power 恒写。
    pub effect: Option<i64>,
    pub power: i64,
    pub level: i64,
    pub end_time: u64,
}

impl EffectEntry {
    pub fn validate(&self) -> Result<(), String> {
        if self.power <= 0
            || !(1..=5).contains(&self.level)
            || self.power > 9_007_199_254_740_991
            || self.end_time > 9_007_199_254_740_991
            || self
                .effect
                .is_some_and(|e| e <= 0 || e > 9_007_199_254_740_991)
        {
            return Err("效果标识、等级或截止时间非法".into());
        }
        Ok(())
    }
}

/// 保持官方严格 endTime > gameTime；过期条目不影响玩法，无需逐拍改写。
pub fn active_power(
    entries: Option<&Vec<EffectEntry>>,
    power: i64,
    time: u64,
) -> Option<&EffectEntry> {
    entries?
        .iter()
        .find(|e| e.power == power && e.end_time > time)
}

/// 玩家面与查询快照共用剩余时间，绝对截止时间不暴露。
pub(crate) fn project(record: &mut Value, entries: Option<&Vec<EffectEntry>>, time: u64) {
    if let Some(entries) = entries {
        record["effects"] = Value::Array(
            entries
                .iter()
                .filter(|e| e.end_time > time)
                .map(|e| {
                    let mut m = serde_json::Map::new();
                    if let Some(effect) = e.effect {
                        m.insert("effect".into(), json!(effect));
                    }
                    m.insert("power".into(), json!(e.power));
                    m.insert("level".into(), json!(e.level));
                    m.insert("ticksRemaining".into(), json!(e.end_time - time));
                    Value::Object(m)
                })
                .collect(),
        );
    }
}

/// 沙箱内部预检需要绝对 endTime（factory 的 ≥ 边界）；RoomObject 装配
/// 仅拷贝公开四键，管理查询快照仍使用 project，不泄露内部截止时间。
pub(crate) fn project_raw(record: &mut Value, entries: Option<&Vec<EffectEntry>>, time: u64) {
    if let Some(entries) = entries {
        record["effects"] = Value::Array(
            entries
                .iter()
                .map(|e| {
                    let mut m = serde_json::Map::new();
                    if let Some(effect) = e.effect {
                        m.insert("effect".into(), json!(effect));
                    }
                    m.insert("power".into(), json!(e.power));
                    m.insert("level".into(), json!(e.level));
                    m.insert("endTime".into(), json!(e.end_time));
                    m.insert(
                        "ticksRemaining".into(),
                        json!(e.end_time.saturating_sub(time)),
                    );
                    Value::Object(m)
                })
                .collect(),
        );
    }
}

/// 管理员放置/编辑的 Generic 原始字段归一到独立表；数据对象不再持有第二份。
pub(crate) fn extract(
    object: &mut kk_schema::room::RoomObject,
) -> Result<Option<Vec<EffectEntry>>, String> {
    let kk_schema::room::RoomObject::Generic(g) = object else {
        return Ok(None);
    };
    let Ok(mut data) = serde_json::from_str::<Value>(&g.data) else {
        return Ok(None);
    };
    let Some(entries) = data.as_object_mut().and_then(|m| m.remove("effects")) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for e in entries.as_array().ok_or("effects 须为数组")? {
        let power = e
            .get("power")
            .and_then(Value::as_i64)
            .ok_or("effect 缺 power")?;
        let entry = EffectEntry {
            effect: match e.get("effect") {
                Some(value) => Some(value.as_i64().ok_or("effect 标识须为整数")?),
                // 管理放置默认按通用形 effect=power；官方 SHIELD 的无 effect
                // 形态只由引擎施放写入产生（usePower.js:248-253）。
                None => Some(power),
            },
            power,
            level: e
                .get("level")
                .and_then(Value::as_i64)
                .ok_or("effect 缺 level")?,
            end_time: e
                .get("endTime")
                .and_then(Value::as_u64)
                .ok_or("effect 缺 endTime")?,
        };
        entry.validate()?;
        if out.iter().any(|old: &EffectEntry| old.power == entry.power) {
            return Err("同 power 效果重复".into());
        }
        out.push(entry);
    }
    g.data = data.to_string();
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ObjId, World};
    use kk_schema::room::{GenericObj, RoomObject};

    fn object(entries: Value) -> RoomObject {
        RoomObject::Generic(GenericObj {
            id: 0,
            x: 20,
            y: 20,
            data: json!({"type":"structure","structureType":"tower","effects":entries}).to_string(),
        })
    }

    #[test]
    fn ingress_owns_effects_and_edit_remove_checkpoint_keep_one_authority() {
        let mut world = World::default();
        world
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        let entries = json!([{"power":3,"level":2,"endTime":10}]);
        let id = world.insert("W1N1", object(entries)).unwrap();
        let RoomObject::Generic(g) = world.get(id).unwrap().1 else {
            panic!()
        };
        assert!(
            serde_json::from_str::<Value>(&g.data)
                .unwrap()
                .get("effects")
                .is_none()
        );
        assert_eq!(
            world.object_snapshot_by_id(id).unwrap()["effects"],
            json!([{"effect":3,"power":3,"level":2,"ticksRemaining":10}])
        );
        world
            .edit_object(id, |obj| {
                let RoomObject::Generic(g) = obj else {
                    panic!()
                };
                let mut data: Value = serde_json::from_str(&g.data).unwrap();
                data["effects"] = json!([{"power":3,"level":5,"endTime":20}]);
                g.data = data.to_string();
                Ok(())
            })
            .unwrap();
        let mut restored = World::from_checkpoint(world.checkpoint()).unwrap();
        assert_eq!(restored.room("W1N1").unwrap().effects[&id][0].level, 5);
        restored.remove(id).unwrap();
        assert!(restored.room("W1N1").unwrap().effects.is_empty());
        assert!(World::from_checkpoint(restored.checkpoint()).is_ok());
    }

    #[test]
    fn invalid_ingress_or_edit_is_atomic_and_expiration_has_exact_boundary() {
        let mut world = World::default();
        world
            .register_room("W1N1", kk_core::Terrain::plains())
            .unwrap();
        for entries in [
            json!({}),
            json!([{"power":3,"level":0,"endTime":9}]),
            json!([{"power":3,"level":1,"endTime":-1}]),
            json!([{"power":3,"level":1,"endTime":9},{"power":3,"level":2,"endTime":10}]),
        ] {
            let before = serde_json::to_value(world.checkpoint()).unwrap();
            assert!(world.insert("W1N1", object(entries)).is_err());
            assert_eq!(serde_json::to_value(world.checkpoint()).unwrap(), before);
        }
        let id = world
            .insert("W1N1", object(json!([{"power":3,"level":1,"endTime":2}])))
            .unwrap();
        assert!(
            world
                .edit_room("W1N1", |room| {
                    room.effects.insert(ObjId(999), vec![]);
                    Ok(())
                })
                .is_err()
        );
        world.advance_ticks(1).unwrap();
        assert_eq!(
            world.object_snapshot_by_id(id).unwrap()["effects"][0]["ticksRemaining"],
            1
        );
        world.advance_ticks(1).unwrap();
        assert_eq!(
            world.object_snapshot_by_id(id).unwrap()["effects"],
            json!([])
        );
        assert!(active_power(world.room("W1N1").unwrap().effects.get(&id), 3, 2).is_none());
    }
}

#[cfg(test)]
mod persisted_authority_tests {
    use crate::{ObjId, RoomState, World};
    use kk_schema::room::{GenericObj, RoomObject};
    #[test]
    fn snapshot_cannot_hide_embedded_effects_in_an_escaped_key() {
        let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
        room.objects.insert(
            ObjId(1),
            RoomObject::Generic(GenericObj {
                id: 1,
                x: 20,
                y: 20,
                data: r#"{"type":"structure","\u0065ffects":[]}"#.into(),
            }),
        );
        assert!(World::validate_room_objects([("W1N1", &room)], 2).is_err());
    }
}
