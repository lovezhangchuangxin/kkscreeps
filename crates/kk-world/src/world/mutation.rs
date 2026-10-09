//! 已提交世界的编辑与整轮批量发布；错误发生在修改权威数据之前。
use super::*;
use std::collections::{BTreeMap, HashSet};

/// 地形与房名不属于房间内容编辑面。
pub struct RoomContents<'a> {
    pub objects: &'a mut BTreeMap<ObjId, RoomObject>,
    pub events: &'a mut Vec<String>,
    pub extensions: &'a mut BTreeMap<ObjId, BTreeMap<String, serde_json::Value>>,
    pub effects: &'a mut BTreeMap<ObjId, Vec<crate::EffectEntry>>,
    pub keeper_memory: &'a mut BTreeMap<ObjId, crate::domain::keeper_memory::KeeperMemory>,
    pub invader_memory: &'a mut BTreeMap<ObjId, crate::domain::invader_memory::InvaderMemory>,
    pub raid: &'a mut crate::domain::invader_raid::RaidRoomState,
}

impl World {
    /// 同一校验供管理员编辑、恢复和整轮候选消费。
    pub fn validate_room_objects<'a>(
        rooms: impl IntoIterator<Item = (&'a str, &'a RoomState)>,
        next_id: u64,
    ) -> Result<(), String> {
        if next_id == 0 {
            return Err("对象 ID 游标不得为零".into());
        }
        let mut ids = HashSet::new();
        for (name, room) in rooms {
            if room.name != name {
                return Err(format!("房间键与名称不一致：{name}"));
            }
            for (id, object) in &room.objects {
                // 二进制恢复或直接替房不得夹带第二份 effects 权威状态。
                if let RoomObject::Generic(g) = object
                    && let Ok(data) = serde_json::from_str::<serde_json::Value>(&g.data)
                {
                    if data.get("effects").is_some() {
                        return Err("Generic effects 须归一到房间效果表".into());
                    }
                    crate::domain::deposit::validate(&data)?;
                    crate::domain::keeper::validate(&data)?;
                    crate::domain::invader_core::validate(&data)?;
                }
                if let RoomObject::Storage(storage) = object
                    && !(0..=9_007_199_254_740_991).contains(&storage.store_capacity)
                {
                    return Err("storage 容量必须是非负安全整数".into());
                }
                let (x, y) = object.pos();
                if id.0 == 0
                    || id.0 >= next_id
                    || object.id() != id.0
                    || x >= 50
                    || y >= 50
                    || !ids.insert(*id)
                {
                    return Err(format!(
                        "房间 {name} 对象 {} 的 ID/坐标不变式无效",
                        id.as_str()
                    ));
                }
            }
            crate::domain::keeper_memory::validate(room, next_id)?;
            crate::domain::invader_memory::validate(room)?;
            for (id, entries) in &room.effects {
                if !room.objects.contains_key(id) {
                    return Err(format!("房间 {name} 含悬空效果"));
                }
                let mut powers = HashSet::new();
                for entry in entries {
                    entry.validate()?;
                    if !powers.insert(entry.power) {
                        return Err("同 power 效果重复".into());
                    }
                }
            }
            if room
                .extensions
                .keys()
                .any(|id| !room.objects.contains_key(id))
            {
                return Err(format!("房间 {name} 含悬空扩展字段"));
            }
        }
        Ok(())
    }

    /// 在副本上编辑内容；失败保留房间、版本、游标及缓存。
    pub fn edit_room<R>(
        &mut self,
        name: &str,
        edit: impl FnOnce(RoomContents<'_>) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut room = self
            .room(name)
            .ok_or_else(|| format!("房间 {name} 不存在"))?
            .clone();
        let result = edit(RoomContents {
            objects: &mut room.objects,
            events: &mut room.events,
            extensions: &mut room.extensions,
            effects: &mut room.effects,
            keeper_memory: &mut room.keeper_memory,
            invader_memory: &mut room.invader_memory,
            raid: &mut room.raid,
        })?;
        for (id, object) in &mut room.objects {
            if let Some(entries) = crate::effects::extract(object)? {
                room.effects.insert(*id, entries);
            }
        }
        self.replace_room(name.to_string(), room)?;
        Ok(result)
    }

    /// 单对象编辑不可更改 ID；坐标与房间全局 ID 校验在提交前完成。
    pub fn edit_object<R>(
        &mut self,
        id: ObjId,
        edit: impl FnOnce(&mut RoomObject) -> Result<R, String>,
    ) -> Result<R, String> {
        let name = self
            .get(id)
            .ok_or_else(|| format!("对象 {} 不存在", id.as_str()))?
            .0
            .to_string();
        self.edit_room(&name, |room| {
            edit(room.objects.get_mut(&id).expect("目录已确认对象存在"))
        })
    }

    /// 管理员整房替换立即失效缓存；仅替换已存在的同名、同地形房间。
    pub fn replace_room(&mut self, name: String, mut room: RoomState) -> Result<(), String> {
        if room.name.is_empty() {
            room.name.clone_from(&name);
        }
        let old = self
            .room(&name)
            .ok_or_else(|| format!("房间 {name} 不存在"))?;
        if room.terrain.as_slice() != old.terrain.as_slice() {
            return Err("地形须经 set_terrain 修改".into());
        }
        let max = room.objects.keys().map(|id| id.0).max().unwrap_or(0);
        let next = self
            .next_obj_id
            .max(max.checked_add(1).ok_or("对象 ID 已耗尽")?);
        Self::validate_room_objects(
            self.rooms
                .iter()
                .filter(|(key, _)| *key != &name)
                .map(|(key, room)| (key.as_str(), room))
                .chain(std::iter::once((name.as_str(), &room))),
            next,
        )?;
        let generation = self.next_generation()?;
        self.rooms.insert(name, room);
        self.next_obj_id = next;
        self.generation = generation;
        Ok(())
    }

    /// 发布完整房间候选；所有可能失败的操作均在写入前完成。
    pub fn commit_tick_rooms(
        &mut self,
        rooms: Vec<(String, RoomState)>,
        next_id: u64,
        count_player_tick: bool,
    ) -> Result<(), String> {
        if rooms.len() != self.rooms.len()
            || rooms
                .iter()
                .zip(self.rooms.iter())
                .any(|((name, room), (key, old))| {
                    name != key || room.terrain.as_slice() != old.terrain.as_slice()
                })
        {
            return Err("整轮房间覆盖、顺序或地形不一致".into());
        }
        if next_id < self.next_obj_id {
            return Err("对象 ID 游标不得回退".into());
        }
        Self::validate_room_objects(
            rooms.iter().map(|(name, room)| (name.as_str(), room)),
            next_id,
        )?;
        let time = self.time.checked_add(1).ok_or("世界时间已耗尽")?;
        // 玩家轮只计宿主发起的执行轮；热身拍（createShard 装配结算）推进
        // 世界时间但保持 setTerrain 前置门可用（官方 guardedTick 同理）。
        let player_ticks = if count_player_tick {
            self.player_ticks.checked_add(1).ok_or("玩家轮数已耗尽")?
        } else {
            self.player_ticks
        };
        let generation = self.next_generation()?;
        self.rooms = rooms.into_iter().collect();
        self.next_obj_id = next_id;
        self.time = time;
        self.player_ticks = player_ticks;
        self.generation = generation;
        Ok(())
    }

    /// 收养隔离编辑副本；外部重置副本也不能回退权威代际。
    pub fn adopt_edit(&mut self, mut draft: World) -> Result<(), String> {
        Self::validate_room_objects(
            draft.rooms.iter().map(|(name, room)| (name.as_str(), room)),
            draft.next_obj_id,
        )?;
        let terrain_changed = self.terrain_epoch != draft.terrain_epoch
            || self.rooms.len() != draft.rooms.len()
            || self.rooms.iter().any(|(name, room)| {
                draft
                    .rooms
                    .get(name)
                    .is_none_or(|other| room.terrain.as_slice() != other.terrain.as_slice())
            });
        draft.generation = self.next_generation()?;
        draft.terrain_epoch = if terrain_changed {
            self.terrain_epoch.checked_add(1).ok_or("地形版本已耗尽")?
        } else {
            self.terrain_epoch
        };
        draft.terrain_cache.borrow_mut().clear();
        draft.obj_dir.borrow_mut().generation = u64::MAX;
        *draft.max_id_cache.borrow_mut() = (u64::MAX, 0);
        draft.instance = self.instance.clone();
        *self = draft;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warmup_publishes_world_but_preserves_terrain_setup_window() {
        let mut world = World::default();
        world.register_room("W1N1", Terrain::plains()).unwrap();
        let rooms = world
            .rooms()
            .iter()
            .map(|(n, r)| (n.clone(), r.clone()))
            .collect();
        world
            .commit_tick_rooms(rooms, world.next_obj_id, false)
            .unwrap();
        assert_eq!((world.time, world.player_ticks), (1, 0));
        world.set_terrain("W1N1", Terrain::plains()).unwrap();
        let rooms = world
            .rooms()
            .iter()
            .map(|(n, r)| (n.clone(), r.clone()))
            .collect();
        world
            .commit_tick_rooms(rooms, world.next_obj_id, true)
            .unwrap();
        assert_eq!((world.time, world.player_ticks), (2, 1));
        assert!(world.set_terrain("W1N1", Terrain::plains()).is_err());
    }

    #[test]
    fn invalid_whole_tick_does_not_publish_a_prefix_or_advance_time() {
        let mut world = World::default();
        world.register_room("W1N1", Terrain::plains()).unwrap();
        world.register_room("W2N2", Terrain::plains()).unwrap();
        let mut rooms: Vec<_> = world
            .rooms()
            .iter()
            .map(|(name, room)| (name.clone(), room.clone()))
            .collect();
        let generation = world.generation();
        let object = RoomObject::Energy(kk_schema::room::EnergyObj {
            id: 0,
            x: 20,
            y: 20,
            resource_type: 0,
            amount: 10,
            resource: None,
        });
        rooms[0].1.insert_with_id(1, object.clone()).unwrap();
        rooms[1].1.insert_with_id(1, object).unwrap();
        assert!(world.commit_tick_rooms(rooms, 2, true).is_err());
        assert_eq!(world.time(), 0);
        assert_eq!(world.generation(), generation);
        assert!(world.rooms().values().all(|room| room.objects.is_empty()));
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;

    #[test]
    fn explicit_terrain_edit_preserves_epoch_invalidation_even_for_equal_bytes() {
        let mut world = World::default();
        world.register_room("W1N1", Terrain::plains()).unwrap();
        let epoch = world.terrain_epoch();
        let identity = world.instance.clone();
        let mut draft = world.clone();
        draft.set_terrain("W1N1", Terrain::plains()).unwrap();
        world.adopt_edit(draft).unwrap();
        assert_eq!(world.terrain_epoch(), epoch + 1);
        assert!(std::sync::Arc::ptr_eq(&identity, &world.instance));
    }

    #[test]
    fn exhausted_clock_and_generation_reject_before_any_write() {
        for field in ["time", "player_ticks", "generation", "generation_sentinel"] {
            let mut world = World::default();
            world.register_room("W1N1", Terrain::plains()).unwrap();
            match field {
                "time" => world.time = u64::MAX,
                "player_ticks" => world.player_ticks = u64::MAX,
                "generation" => world.generation = u64::MAX,
                _ => world.generation = u64::MAX - 1,
            }
            let before = (world.time, world.player_ticks, world.generation);
            assert!(world.advance_ticks(1).is_err());
            let rooms = world
                .rooms
                .iter()
                .map(|(n, r)| (n.clone(), r.clone()))
                .collect();
            assert!(
                world
                    .commit_tick_rooms(rooms, world.next_obj_id, true)
                    .is_err()
            );
            assert_eq!((world.time, world.player_ticks, world.generation), before);
            assert!(world.room("W1N1").unwrap().objects.is_empty());
        }
    }
}
