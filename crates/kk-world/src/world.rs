//! typed 世界：房间状态 + 对象表 + 全局 id 分配。
//!
//! 对象按 `BTreeMap<ObjId, _>` 存放：`World::insert` 单调分配 id ⇒ 迭代序 =
//! 创建序，与官方 roomObjects 的 DB 插入序近似一致且天然确定（game-loop.md
//! 关键设计二、方案 D4-1）。`insert_with_id`（快照解码/导入）乱序插入时迭代序
//! 为 id 升序而非插入序；`next_obj_id` 保证其后的普通插入不会与已导入对象撞 id。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

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
    /// 房间事件日志缓冲（B3）：最近一个 tick 的事件 JSON 数组——官方
    /// saveRoomEventLog 每 tick 整体覆写（processor.js:497 → driver/lib/
    /// index.js:583-585），settle 每 tick 重写本缓冲同口径；快照随房携带。
    pub events: Vec<String>,
}

impl RoomState {
    pub fn new(name: impl Into<String>, terrain: Terrain) -> Self {
        Self {
            name: name.into(),
            terrain,
            objects: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    /// 房间内 controller（place 归一化的上下文）。
    pub fn controller(&self) -> Option<&kk_schema::room::ControllerObj> {
        self.objects.values().find_map(|o| match o {
            RoomObject::Controller(c) => Some(c),
            _ => None,
        })
    }

    /// 以指定 id 放入（快照解码/导入用）；拒绝覆盖已有对象。
    pub fn insert_with_id(&mut self, id: u64, mut object: RoomObject) -> Result<(), String> {
        if self.objects.contains_key(&ObjId(id)) {
            return Err(format!("对象 id 重复: {}", ObjId(id).as_str()));
        }
        object.set_id(id);
        self.objects.insert(ObjId(id), object);
        Ok(())
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
            events: self.events.clone(),
        }
    }

    /// 解码快照，并校验会影响房间索引的对象字段。
    pub fn decode_snapshot(snapshot: &RoomSnapshot) -> Result<Self, String> {
        let terrain = Terrain::from_codes(&snapshot.header.terrain)
            .map_err(|e| format!("快照地形非法: {e}"))?;
        let mut room = Self::new("", terrain);
        for object in &snapshot.objects {
            let id = ObjId(object.id());
            let (x, y) = object.pos();
            if x as usize >= kk_core::ROOM_SIZE || y as usize >= kk_core::ROOM_SIZE {
                return Err(format!("快照对象 {} 坐标越界: ({x},{y})", id.as_str()));
            }
            if room.objects.insert(id, object.clone()).is_some() {
                return Err(format!("快照对象 id 重复: {}", id.as_str()));
            }
        }
        room.events = snapshot.events.clone();
        Ok(room)
    }
}

/// 已提交房间版本的不可变句柄（M2B 前置批 P2，ADR-013 决策 2）：
/// `tick`（世界时序）+ `generation`（内容代际）共同标识一个已提交版本；
/// 读取方在同一 tick 内绑定同一版本（snapshot 契约纪律——embed 的玩家执行
/// 循环以 debug 断言守护）。列式/零拷贝时代，此句柄外延为携带字节长度与
/// 共享存活句柄的 SnapshotHandle（snapshot 契约的完整形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomVersionHandle {
    pub room: String,
    pub tick: u64,
    pub generation: u64,
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
    /// 世界内容代际（M2B 前置批 P2，ADR-013 决策 2）：任何已提交内容变更
    /// 单调递增——投影缓存失效键与 [`RoomVersionHandle`] 的内容分量。
    /// bump 点 = 全部内容可变面（insert/remove/set_terrain/register_room/
    /// advance_ticks + room_mut/get_mut 保守覆盖）；kk-engine 提交尾直接
    /// 写回 `rooms` 字段后必经 advance_ticks，代际随之递增。结算失败路径
    /// 不 bump（世界未变）。
    generation: u64,
    /// 地形纪元：仅地形**实际替换**时递增（register_room 新房 / set_terrain）
    /// ——地形缓存的失效键（M2B 前置批 P3 消费）。地形首玩家 tick 后不可变，
    /// 正常运行恒定；纪元变更 ⇒ 全部房间的地形缓存作废。
    terrain_epoch: u64,
    /// 地形投影缓存（P3）：房名 → (纪元, b64 载荷)。键含纪元 ⇒ 纪元递增
    /// 后旧条目自然失配重编码（全房作废——事件一次性，方向保守安全）。
    /// `RefCell` 前提：World 单线程持用（embed 主线程投影；无跨线程读取面，
    /// P2 评审已核）。随 World Clone/reset 复制/重置——缓存键纪元防陈旧命中。
    pub(crate) terrain_cache: RefCell<BTreeMap<String, (u64, Arc<str>)>>,
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
            generation: 0,
            terrain_epoch: 0,
            terrain_cache: RefCell::new(BTreeMap::new()),
        }
    }

    /// 放入对象并分配全局唯一 id（对象 id 字段被覆写为分配值）。
    pub fn insert(&mut self, room_name: &str, object: RoomObject) -> Result<ObjId, String> {
        if !self.rooms.contains_key(room_name) {
            return Err(format!("房间 {room_name} 不存在"));
        }
        let (x, y) = object.pos();
        if x as usize >= kk_core::ROOM_SIZE || y as usize >= kk_core::ROOM_SIZE {
            return Err(format!("对象坐标越界: ({x},{y})"));
        }
        let id = self.next_available_obj_id()?;
        self.rooms
            .get_mut(room_name)
            .expect("房间已检查存在")
            .insert_with_id(id, object)?;
        self.next_obj_id = id + 1;
        self.generation += 1;
        Ok(ObjId(id))
    }

    /// 推进时间（player_ticks 的唯一维护点——setTerrain 守卫依赖它）。
    /// 同时递增内容代际：投影的时钟折叠面（ticksToRegeneration 等）读
    /// `time`，tick 推进即投影内容变化，缓存必须失效；kk-engine 的结算
    /// 提交尾（直接写回 rooms 后）以本方法收束，代际随之覆盖。
    pub fn advance_ticks(&mut self, n: u64) {
        self.time += n;
        self.player_ticks += n;
        self.generation += 1;
    }

    /// 下一个候选 id；直接导入房间对象后也不会回退到已有 id。
    pub fn next_obj_id(&self) -> u64 {
        let max_existing = self
            .rooms
            .values()
            .flat_map(|room| room.objects.keys())
            .map(|id| id.0)
            .max();
        let after_existing = max_existing.map_or(1, |id| id.saturating_add(1));
        self.next_obj_id.max(after_existing)
    }

    /// 分配前检查余量；保留 `u64::MAX` 作为耗尽标记。
    pub fn next_available_obj_id(&self) -> Result<u64, String> {
        let id = self.next_obj_id();
        id.checked_add(1)
            .ok_or_else(|| "对象 id 已耗尽".to_string())?;
        Ok(id)
    }

    /// 抬高 id 分配器（快照解码重建用；只升不降）。
    pub fn bump_next_obj_id(&mut self, v: u64) {
        self.next_obj_id = self.next_obj_id.max(v);
    }

    /// 整体重置（新世界——代际/纪元随之归零重数）。**约束（评审 1 P2-2
    /// 钉子）**：版本键缓存（generation/terrain_epoch 的消费方，P3/P5）必须
    /// 驻 World 内（随体重置、结构性免疫跨分片 (room, epoch) 陈旧命中）；
    /// 若未来缓存外移，须引入分片级基偏移防撞键。
    pub fn reset(&mut self) {
        *self = Self::new(self.seed);
    }

    pub fn register_room(&mut self, name: impl Into<String>, terrain: Terrain) {
        let name = name.into();
        if !self.rooms.contains_key(&name) {
            self.rooms
                .insert(name.clone(), RoomState::new(name, terrain));
            self.generation += 1;
            self.terrain_epoch += 1;
        }
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
        self.generation += 1;
        self.terrain_epoch += 1;
        Ok(())
    }

    pub fn room(&self, name: &str) -> Option<&RoomState> {
        self.rooms.get(name)
    }

    /// **禁写 `terrain` 字段**（P3 钉子，评审 2 P2-1）：纪元不随本方法的
    /// 可变访问递增——绕过 `set_terrain` 改地形会让投影的地形缓存供陈旧
    /// 载荷（当前生产调用面为零，测试亦不写地形）。
    pub fn room_mut(&mut self, name: &str) -> Option<&mut RoomState> {
        self.generation += 1; // 保守失效：可变访问即视为内容可能变更
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
        self.generation += 1; // 保守失效（同 room_mut）
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
                self.generation += 1;
                return Some(o);
            }
        }
        None
    }

    /// 当前世界内容代际（见字段注释）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 当前地形纪元（见字段注释）。
    pub fn terrain_epoch(&self) -> u64 {
        self.terrain_epoch
    }

    /// 显式递增代际（防御面：绕过 World 方法直接写 `rooms` 字段的调用位——
    /// 正常路径由各方法内置 bump 覆盖，本方法供此类调用方显式声明提交）。
    pub fn bump_generation(&mut self) {
        self.generation += 1;
    }

    /// 房间版本句柄（tick 定时序、generation 定内容——「同一 tick 内所有
    /// getter 绑定同一版本」纪律的绑定物）。
    pub fn room_version(&self, room: &str) -> Option<RoomVersionHandle> {
        self.rooms.get(room)?;
        Some(RoomVersionHandle {
            // 用 map key 而非 state.name：快照解码路径产出 name="" 的房
            //（RoomState::new("")），key 才是权威房间名（评审 F-1）
            room: room.to_string(),
            tick: self.time,
            generation: self.generation,
        })
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
            resource: None,
        })
    }

    #[test]
    fn generation_bumps_on_content_changes() {
        let mut world = World::new(1);
        assert_eq!((world.generation(), world.terrain_epoch()), (0, 0));
        world.register_room("W1N1", Terrain::plains());
        let after_room = world.generation();
        assert_eq!((after_room, world.terrain_epoch()), (1, 1));
        // 已存在房重注册：不替换地形，不 bump
        world.register_room("W1N1", Terrain::plains());
        assert_eq!((world.generation(), world.terrain_epoch()), (1, 1));
        world.insert("W1N1", energy(1)).unwrap();
        assert_eq!(world.generation(), 2);
        assert_eq!(world.terrain_epoch(), 1, "对象变更不动地形纪元");
        // tick 推进（结算提交位）：时钟折叠面变化 ⇒ 代际必须失效
        world.advance_ticks(1);
        let after_tick = world.generation();
        assert_eq!(after_tick, 3);
        // player_ticks>0 后 set_terrain 拒绝（不 bump）
        assert!(world.set_terrain("W1N1", Terrain::plains()).is_err());
        assert_eq!((world.generation(), world.terrain_epoch()), (after_tick, 1));
        // 句柄 = tick + generation 的快照
        let handle = world.room_version("W1N1").unwrap();
        assert_eq!(
            handle,
            RoomVersionHandle {
                room: "W1N1".into(),
                tick: 1,
                generation: 3
            }
        );
        assert!(world.room_version("nope").is_none());
        // set_terrain 成功路径 / remove / get_mut / room_mut 的 bump 面
        //（评审 1 P2-1 清偿：逐面钉死，删除任一 bump 即红）
        let mut fresh = World::new(2);
        fresh.register_room("E1S1", Terrain::plains());
        fresh.set_terrain("E1S1", Terrain::plains()).unwrap();
        assert_eq!((fresh.generation(), fresh.terrain_epoch()), (2, 2));
        let id = fresh.insert("E1S1", energy(1)).unwrap();
        fresh.remove(id);
        assert_eq!(fresh.generation(), 4, "insert→remove 净 +2");
        let _ = fresh.get_mut(id);
        assert_eq!(fresh.generation(), 5, "get_mut 保守 +1（对象已删仍计）");
        fresh.room_mut("E1S1");
        assert_eq!(fresh.generation(), 6, "room_mut 保守 +1");
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
    fn insert_rejects_invalid_position_and_exhausted_ids() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let mut invalid = energy(1);
        if let RoomObject::Energy(obj) = &mut invalid {
            obj.y = 50;
        }
        assert!(world.insert("W1N1", invalid).is_err());
        assert_eq!(world.next_obj_id(), 1);

        world.bump_next_obj_id(u64::MAX);
        assert!(world.insert("W1N1", energy(1)).is_err());
        assert!(world.room("W1N1").unwrap().objects.is_empty());
    }

    #[test]
    fn insert_after_manual_import_keeps_ids_unique_across_rooms() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        world.register_room("W2N2", Terrain::plains());
        world
            .room_mut("W1N1")
            .unwrap()
            .insert_with_id(1, energy(1))
            .unwrap();
        world
            .room_mut("W2N2")
            .unwrap()
            .insert_with_id(5, energy(2))
            .unwrap();

        assert_eq!(world.next_obj_id(), 6);
        assert_eq!(world.insert("W1N1", energy(3)).unwrap(), ObjId(6));
        assert_eq!(world.room("W1N1").unwrap().objects.len(), 2);
        assert_eq!(world.room("W2N2").unwrap().objects.len(), 1);
        assert!(
            world
                .room_mut("W1N1")
                .unwrap()
                .insert_with_id(1, energy(99))
                .is_err()
        );
        assert!(matches!(
            world.get(ObjId(1)),
            Some((_, RoomObject::Energy(EnergyObj { amount: 1, .. })))
        ));
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
    fn snapshot_restore_recovers_next_id_and_rejects_bad_terrain() {
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
                .insert_with_id(object.id(), object.clone())
                .unwrap();
        }
        let max_id = snap.objects.iter().map(|o| o.id()).max().unwrap_or(0);
        world2.bump_next_obj_id(snap.header.next_obj_id.max(max_id + 1));
        assert_eq!(world2.insert("W1N1", energy(3)).unwrap().0, 3);

        // 地形位域防御：快照字节含未知位 → 拒载
        let mut bad = world.room("W1N1").unwrap().encode_snapshot(0, 0, 3);
        bad.header.terrain[10] = 0xff;
        assert!(RoomState::decode_snapshot(&bad).is_err());
    }

    #[test]
    fn decode_rejects_duplicate_ids_and_out_of_bounds_positions() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        world.insert("W1N1", energy(1)).unwrap();
        let mut snapshot = world.room("W1N1").unwrap().encode_snapshot(0, 1, 2);
        snapshot.objects.push(snapshot.objects[0].clone());
        assert!(
            RoomState::decode_snapshot(&snapshot)
                .unwrap_err()
                .contains("id 重复")
        );

        snapshot.objects.pop();
        if let RoomObject::Energy(obj) = &mut snapshot.objects[0] {
            obj.x = 50;
        }
        assert!(
            RoomState::decode_snapshot(&snapshot)
                .unwrap_err()
                .contains("坐标越界")
        );
    }
}
