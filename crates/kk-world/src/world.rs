//! typed 世界：房间状态 + 对象表 + id 分配。
//!
//! 对象按 `BTreeMap<ObjId, _>` 存放——id 经 `World::insert` 全局单调分配 ⇒
//! 迭代序 = 创建序（与官方 roomObjects 的 DB 插入序近似一致，且天然确定，
//! game-loop.md 关键设计二）。`insert_with_id`（解码/导入）乱序插入时迭代序
//! 为 id 升序而非插入序。

use std::collections::BTreeMap;

use kk_core::Terrain;
use kk_schema::room::{RoomHeader, RoomObject, RoomSnapshot};

/// 对象 id（**全世界**单调递增、跨房间唯一——分配器在 World 层；
/// 玩家可见的字符串形态为 `kk{:016x}`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjId(pub u64);

impl ObjId {
    /// 玩家可见 id（M0 契约：字符串、不透明）。
    pub fn as_str(&self) -> String {
        format!("kk{:016x}", self.0)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix("kk")?;
        Some(Self(u64::from_str_radix(hex, 16).ok()?))
    }
}

/// 单个房间的状态。id 分配在 [`World`] 层（跨房间唯一）。
#[derive(Debug, Clone)]
pub struct RoomState {
    pub name: String,
    pub terrain: Terrain,
    pub objects: BTreeMap<ObjId, RoomObject>,
}

impl RoomState {
    pub fn new(name: impl Into<String>, terrain: Terrain) -> Self {
        Self {
            name: name.into(),
            terrain,
            objects: BTreeMap::new(),
        }
    }

    /// 房间内 controller（place 归一化的上下文）。
    pub fn controller(&self) -> Option<&kk_schema::room::ControllerObj> {
        self.objects.values().find_map(|o| match o {
            RoomObject::Controller(c) => Some(c),
            _ => None,
        })
    }

    /// 以指定 id 放入（快照解码/导入用）。
    pub fn insert_with_id(&mut self, id: u64, mut object: RoomObject) {
        set_object_id(&mut object, id);
        self.objects.insert(ObjId(id), object);
    }

    pub fn encode_snapshot(&self, time: u64, seed: u64, next_obj_id: u64) -> RoomSnapshot {
        RoomSnapshot {
            header: RoomHeader {
                time,
                seed,
                next_obj_id,
                terrain: *self.terrain.as_slice(),
            },
            objects: self.objects.values().cloned().collect(),
        }
    }

    /// 解码快照（值域防御：地形位与 next_obj_id 一致性）。
    pub fn decode_snapshot(snapshot: &RoomSnapshot) -> Result<Self, String> {
        let terrain = Terrain::from_codes(&snapshot.header.terrain)
            .map_err(|e| format!("快照地形非法: {e}"))?;
        let mut room = Self::new("", terrain);
        for object in &snapshot.objects {
            room.objects.insert(ObjId(object.id()), object.clone());
        }
        Ok(room)
    }
}

fn set_object_id(object: &mut RoomObject, id: u64) {
    match object {
        RoomObject::Creep(o) => o.id = id,
        RoomObject::Source(o) => o.id = id,
        RoomObject::Controller(o) => o.id = id,
        RoomObject::Spawn(o) => o.id = id,
        RoomObject::Extension(o) => o.id = id,
        RoomObject::ConstructedWall(o) => o.id = id,
        RoomObject::ConstructionSite(o) => o.id = id,
        RoomObject::Energy(o) => o.id = id,
        RoomObject::Flag(o) => o.id = id,
        RoomObject::Road(o) => o.id = id,
        RoomObject::Storage(o) => o.id = id,
        RoomObject::Generic(o) => o.id = id,
    }
}

/// 世界（M1：房间集合 + 全局时间；玩家分片语义在 embed 层）。
#[derive(Debug, Clone)]
pub struct World {
    pub time: u64,
    pub seed: u64,
    /// 已经过玩家 tick（setTerrain 只允许在 0 时调用——screeps-ok 契约）。
    pub player_ticks: u64,
    pub rooms: BTreeMap<String, RoomState>,
    /// 全局 id 分配器（跨房间唯一；快照每房 header 存同一全局值）。
    next_obj_id: u64,
}

impl Default for World {
    fn default() -> Self {
        Self::new(0)
    }
}

impl World {
    pub fn new(seed: u64) -> Self {
        Self {
            time: 0,
            seed,
            player_ticks: 0,
            rooms: BTreeMap::new(),
            next_obj_id: 1,
        }
    }

    /// 放入对象并分配全局唯一 id（对象 id 字段被覆写为分配值）。
    pub fn insert(&mut self, room_name: &str, object: RoomObject) -> Result<ObjId, String> {
        let room = self
            .rooms
            .get_mut(room_name)
            .ok_or_else(|| format!("房间 {room_name} 不存在"))?;
        let id = self.next_obj_id;
        self.next_obj_id = self.next_obj_id.saturating_add(1);
        room.insert_with_id(id, object);
        Ok(ObjId(id))
    }

    /// 推进时间（player_ticks 的唯一维护点——setTerrain 守卫依赖它）。
    pub fn advance_ticks(&mut self, n: u64) {
        self.time += n;
        self.player_ticks += n;
    }

    /// 当前全局 id 分配器值（快照编码用）。
    pub fn next_obj_id(&self) -> u64 {
        self.next_obj_id
    }

    /// 抬高 id 分配器（快照解码重建用；只升不降）。
    pub fn bump_next_obj_id(&mut self, v: u64) {
        self.next_obj_id = self.next_obj_id.max(v);
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.seed);
    }

    pub fn register_room(&mut self, name: impl Into<String>, terrain: Terrain) {
        let name = name.into();
        self.rooms
            .entry(name.clone())
            .or_insert_with(|| RoomState::new(name, terrain));
    }

    /// 设置地形（校验与四角墙由 Terrain 构造保证；player_ticks>0 拒绝）。
    pub fn set_terrain(&mut self, room: &str, terrain: Terrain) -> Result<(), String> {
        if self.player_ticks > 0 {
            return Err("setTerrain 只允许在第一个玩家 tick 之前调用".into());
        }
        let state = self
            .rooms
            .get_mut(room)
            .ok_or_else(|| format!("房间 {room} 不存在"))?;
        state.terrain = terrain;
        Ok(())
    }

    pub fn room(&self, name: &str) -> Option<&RoomState> {
        self.rooms.get(name)
    }

    pub fn room_mut(&mut self, name: &str) -> Option<&mut RoomState> {
        self.rooms.get_mut(name)
    }

    /// 跨房间取对象（M1 主要服务于 embed getObject）。
    pub fn get(&self, id: ObjId) -> Option<(&str, &RoomObject)> {
        for state in self.rooms.values() {
            if let Some(o) = state.objects.get(&id) {
                return Some((&state.name, o));
            }
        }
        None
    }

    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut RoomObject> {
        for state in self.rooms.values_mut() {
            if let Some(o) = state.objects.get_mut(&id) {
                return Some(o);
            }
        }
        None
    }

    pub fn remove(&mut self, id: ObjId) -> Option<RoomObject> {
        for state in self.rooms.values_mut() {
            if let Some(o) = state.objects.remove(&id) {
                return Some(o);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::room::EnergyObj;

    fn energy(amount: u32) -> RoomObject {
        RoomObject::Energy(EnergyObj {
            id: 0,
            x: 1,
            y: 1,
            resource_type: 0,
            amount,
        })
    }

    #[test]
    fn obj_id_str_roundtrip() {
        let id = ObjId(255);
        assert_eq!(ObjId::parse(&id.as_str()), Some(id));
        assert_eq!(ObjId::parse("zz"), None);
    }

    #[test]
    fn ids_are_globally_unique_across_rooms() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        world.register_room("E2S3", Terrain::plains());
        let a = world.insert("W1N1", energy(1)).unwrap();
        let b = world.insert("E2S3", energy(2)).unwrap();
        assert_ne!(a, b, "跨房间 id 必须唯一");
        assert_eq!(world.get(a).unwrap().0, "W1N1");
        assert_eq!(world.get(b).unwrap().0, "E2S3");
    }

    #[test]
    fn insert_unknown_room_errs() {
        let mut world = World::new(1);
        assert!(world.insert("nowhere", energy(1)).is_err());
    }

    #[test]
    fn snapshot_roundtrip_preserves_order_and_ids() {
        let mut world = World::new(42);
        world.register_room("W1N1", Terrain::plains());
        world.insert("W1N1", energy(1)).unwrap();
        world.insert("W1N1", energy(2)).unwrap();
        let snap = world
            .room("W1N1")
            .unwrap()
            .encode_snapshot(77, 42, world.next_obj_id());
        let back = RoomState::decode_snapshot(&snap).unwrap();
        assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
    }

    #[test]
    fn decode_defends_against_stale_next_obj_id_and_bad_terrain() {
        // header.next_obj_id 落后于对象表：World 重建侧用 bump 抬高后不复用
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        world.insert("W1N1", energy(1)).unwrap();
        world.insert("W1N1", energy(2)).unwrap();
        let snap = world.room("W1N1").unwrap().encode_snapshot(0, 0, 1);
        let mut world2 = World::new(1);
        world2.register_room("W1N1", Terrain::plains());
        for object in &snap.objects {
            world2
                .room_mut("W1N1")
                .unwrap()
                .insert_with_id(object.id(), object.clone());
        }
        let max_id = snap.objects.iter().map(|o| o.id()).max().unwrap_or(0);
        world2.bump_next_obj_id(snap.header.next_obj_id.max(max_id + 1));
        assert_eq!(world2.insert("W1N1", energy(3)).unwrap().0, 3);

        // 地形位域防御：快照字节含未知位 → 拒载
        let mut bad = world.room("W1N1").unwrap().encode_snapshot(0, 0, 3);
        bad.header.terrain[10] = 0xff;
        assert!(RoomState::decode_snapshot(&bad).is_err());
    }
}
