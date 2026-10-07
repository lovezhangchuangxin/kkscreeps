//! 全世界检查点数据：房间二进制信封之外显式保存扩展字段和世界游标。
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCheckpoint {
    pub time: u64,
    pub seed: u64,
    pub player_ticks: u64,
    pub next_obj_id: u64,
    pub generation: u64,
    pub terrain_epoch: u64,
    pub rooms: BTreeMap<String, RoomCheckpoint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomCheckpoint {
    pub snapshot: Vec<u8>,
    pub extensions: BTreeMap<u64, BTreeMap<String, serde_json::Value>>,
}

impl World {
    pub fn checkpoint(&self) -> WorldCheckpoint {
        WorldCheckpoint {
            time: self.time,
            seed: self.seed,
            player_ticks: self.player_ticks,
            next_obj_id: self.next_obj_id(),
            generation: self.generation,
            terrain_epoch: self.terrain_epoch,
            rooms: self
                .rooms
                .iter()
                .map(|(name, room)| {
                    (
                        name.clone(),
                        RoomCheckpoint {
                            snapshot: kk_schema::room::encode_snapshot(&room.encode_snapshot(
                                self.time,
                                self.seed,
                                self.next_obj_id(),
                            )),
                            extensions: room
                                .extensions
                                .iter()
                                .map(|(id, fields)| (id.0, fields.clone()))
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// 完整解码后重建派生目录；不从各房挑选不同版本，不修补非法 ID。
    pub fn from_checkpoint(saved: WorldCheckpoint) -> Result<Self, String> {
        if saved.next_obj_id == 0
            || saved.time == u64::MAX
            || saved.generation == u64::MAX
            || saved.player_ticks == u64::MAX
            || saved.terrain_epoch == u64::MAX
        {
            return Err("检查点时钟、版本或 ID 游标非法".into());
        }
        let mut world = Self::new(saved.seed);
        let mut ids = BTreeSet::new();
        for (name, data) in saved.rooms {
            let parsed = kk_core::RoomName::parse(&name).map_err(|e| e.to_string())?;
            if !(-128..=127).contains(&parsed.xx) || !(-128..=127).contains(&parsed.yy) {
                return Err(format!("检查点房间 {name} 超出玩家坐标范围"));
            }
            let snapshot =
                kk_schema::room::decode_snapshot(&data.snapshot).map_err(|e| e.to_string())?;
            let header = &snapshot.header;
            if header.time != saved.time
                || header.seed != saved.seed
                || header.next_obj_id != saved.next_obj_id
            {
                return Err(format!("检查点房间 {name} 与世界版本不一致"));
            }
            let mut room = RoomState::decode_snapshot(&snapshot)?;
            room.name = name.clone();
            for id in room.objects.keys() {
                if id.0 == 0 || id.0 >= saved.next_obj_id || !ids.insert(*id) {
                    return Err(format!("检查点全局对象 ID 非法或重复：{}", id.as_str()));
                }
            }
            for (id, fields) in data.extensions {
                if !room.objects.contains_key(&ObjId(id)) {
                    return Err(format!("检查点扩展字段引用不存在的对象 {id}"));
                }
                room.extensions.insert(ObjId(id), fields);
            }
            world.rooms.insert(name, room);
        }
        world.time = saved.time;
        world.player_ticks = saved.player_ticks;
        world.next_obj_id = saved.next_obj_id;
        world.generation = saved.generation;
        world.terrain_epoch = saved.terrain_epoch;
        // 恢复数据已有对象；空目录绝不能被误认作当前代际的有效目录。
        world.obj_dir.borrow_mut().generation = u64::MAX;
        *world.max_id_cache.borrow_mut() = (u64::MAX, 0);
        Ok(world)
    }
}

#[cfg(test)]
mod tests;
