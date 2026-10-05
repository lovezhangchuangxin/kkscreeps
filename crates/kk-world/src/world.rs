//! 房间状态、对象表和世界级 ID 分配。
//!
//! 房间与对象使用 BTreeMap，遍历顺序由键决定。World::insert 分配的 ID
//! 单调递增，所以普通插入的对象按创建顺序遍历；导入指定 ID 时则按 ID 排序。
//! World 的目录和投影缓存依靠 generation、terrain_epoch 失效，修改状态须维护版本。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use kk_core::Terrain;
use kk_schema::room::{RoomHeader, RoomObject, RoomSnapshot};

/// 世界内对象的数字 ID，玩家可见字符串为 kk 加 16 位小写十六进制数。
/// World 分配的 ID 跨房间唯一；手工导入由调用方保证跨房间不重复。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjId(pub u64);

impl ObjId {
    /// 格式化为玩家可见的不透明 ID。
    pub fn as_str(&self) -> String {
        format!("kk{:016x}", self.0)
    }

    /// 解析 kk 前缀后的十六进制数，不要求固定 16 位长度。
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
    /// 最近一次房间结算产生的事件，每项是一个 JSON 字符串。
    /// 结算整体替换列表，二进制房间快照同时保存它。
    pub events: Vec<String>,
    /// 对象扩展字段：对象 ID → 字段稳定键全名 → 值。
    /// 键带贡献命名空间，不能与内建字段冲突；值在写入入口按
    /// 字段 schema 校验（类型、限额、有限浮点）。随房间候选克隆，
    /// 与对象表同生命周期：删除对象时调用方须同步清理本表。
    /// 二进制房间快照（encode/decode_snapshot）当前不保存扩展；
    /// 持久化格式冻结前必须补入。
    pub extensions: BTreeMap<ObjId, BTreeMap<String, serde_json::Value>>,
}

impl RoomState {
    pub fn new(name: impl Into<String>, terrain: Terrain) -> Self {
        Self {
            name: name.into(),
            terrain,
            objects: BTreeMap::new(),
            events: Vec::new(),
            extensions: BTreeMap::new(),
        }
    }

    /// 按对象 ID 顺序返回第一个控制器，用作放置参数的上下文。
    pub fn controller(&self) -> Option<&kk_schema::room::ControllerObj> {
        self.objects.values().find_map(|o| match o {
            RoomObject::Controller(c) => Some(c),
            _ => None,
        })
    }

    /// 按指定 ID 导入对象，同时覆写对象自身的 ID；房内重复 ID 返回错误。
    /// 不检查坐标或跨房间 ID 唯一性，也不更新 World 的分配游标。
    pub fn insert_with_id(&mut self, id: u64, mut object: RoomObject) -> Result<(), String> {
        if self.objects.contains_key(&ObjId(id)) {
            return Err(format!("对象 id 重复: {}", ObjId(id).as_str()));
        }
        object.set_id(id);
        self.objects.insert(ObjId(id), object);
        Ok(())
    }

    /// 复制房间对象、地形和事件，附带调用方提供的世界时间、种子及分配游标。
    /// 房间名不编码在快照中。
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

    /// 从快照对象和地形重建房间，拒绝非法地形码、越界坐标与房内重复 ID。
    /// 房间名不在快照中，结果 name 为空；世界时间、种子和分配游标须由调用方恢复。
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

/// 标识读取时的房间名、世界时间和内容版本（ADR-013 决策 2）。
/// 只包含版本元数据，不持有房间数据，也不会阻止世界继续变化。
/// generation 是整个 World 的版本，其他房间变化也会改变此值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomVersionHandle {
    pub room: String,
    pub tick: u64,
    pub generation: u64,
}

/// 缓存对象 ID 到世界房间键的映射，供 get、get_mut 和 remove 定位。
/// generation 不匹配时遍历所有对象重建；每个唯一 ID 保存一份房名字符串。
/// 目录有效时查找使用 BTreeMap，复杂度为 O(log N)。
#[derive(Debug, Clone, Default)]
struct ObjDir {
    generation: u64,
    rooms: BTreeMap<ObjId, String>,
}

/// 保存房间集合、时钟和对象分配状态；玩家沙箱与玩家账户由运行时管理。
#[derive(Debug, Clone)]
pub struct World {
    /// 当前绝对 tick，正常推进使用 advance_ticks，以同时维护内容版本。
    /// 构造测试场景可用 set_time_for_test，但它不会使缓存失效。
    pub(crate) time: u64,
    pub seed: u64,
    /// advance_ticks 累计推进的 tick 数；非零时 set_terrain 拒绝替换地形。
    pub player_ticks: u64,
    /// 按房名保存房间；键是查询和投影使用的房间标识。
    /// crate 内直接修改对象后须递增 generation，否则对象目录和最大 ID 缓存会陈旧。
    /// 外部整房替换使用 commit_room，并遵守其批量提交契约。
    pub(crate) rooms: BTreeMap<String, RoomState>,
    /// 全局 id 分配器（跨房间唯一；快照每房 header 存同一全局值）。
    next_obj_id: u64,
    /// 世界内容版本，用于查询和缓存失效。
    /// 插入、成功删除、注册新房、设置地形及推进时间会递增；可变访问保守递增，
    /// 即使没有找到对象或房间。commit_room 不递增，批量提交由 advance_ticks 收束。
    /// 引擎失败重试依靠时间和此版本判断世界是否仍与首次尝试相同。
    generation: u64,
    /// 地形缓存版本，注册新房或成功调用 set_terrain 时递增。
    /// 即使新旧地形相同，set_terrain 也递增；所有房间的旧缓存条目随之失效。
    terrain_epoch: u64,
    /// 房名 → (terrain_epoch, base64 载荷)。版本相符时复用已编码的地形。
    /// RefCell 允许只读投影更新缓存，使 World 不能被多线程共享读取（不实现 Sync）。
    /// Clone 复制缓存，reset 清空缓存，避免重置后的版本号与旧条目相撞。
    pub(crate) terrain_cache: RefCell<BTreeMap<String, (u64, Arc<str>)>>,
    /// 按 generation 失效的对象目录；RefCell 的共享访问限制同地形缓存。
    obj_dir: RefCell<ObjDir>,
    /// (generation, 最大对象 ID)，供分配器使用。
    /// 与对象目录分开维护：只需扫描数字键，不必为每个对象分配房名字符串。
    max_id_cache: RefCell<(u64, u64)>,
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
            obj_dir: RefCell::new(ObjDir::default()),
            max_id_cache: RefCell::new((0, 0)),
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
        // 仅在目录完整覆盖修改前的版本时增量加入新对象。
        // 陈旧目录不能只补本次插入就标为有效，否则会漏掉其他尚未吸收的修改。
        {
            let mut dir = self.obj_dir.borrow_mut();
            if dir.generation + 1 == self.generation {
                dir.rooms.insert(ObjId(id), room_name.to_string());
                dir.generation = self.generation;
            }
            // 分配前已刷新最大 ID 缓存；同样只把有效的上一版本推进到新版本。
            let mut m = self.max_id_cache.borrow_mut();
            if m.0 + 1 == self.generation {
                *m = (self.generation, m.1.max(id));
            }
        }
        Ok(ObjId(id))
    }

    /// 同时推进 time 和 player_ticks，并递增一次 generation。
    /// 即使 n=0 也递增版本；相对计时投影及批量写回的房间状态依靠它使缓存失效。
    pub fn advance_ticks(&mut self, n: u64) {
        self.time += n;
        self.player_ticks += n;
        self.generation += 1;
    }

    /// 返回分配游标与当前最大对象 ID 加一中的较大值。
    /// 最大 ID 按 generation 缓存。手工导入的最大对象被删除后，此候选值可能回落；
    /// 需要永久保留导入 ID 范围时，调用 bump_next_obj_id 抬高游标。
    pub fn next_obj_id(&self) -> u64 {
        let after_existing = self.max_obj_id().saturating_add(1).max(1);
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

    /// 清空世界，保留 seed；时钟、分配游标、版本号及内部缓存全部重置。
    /// 外部持有的版本标识不能跨 reset 判断内容是否相同，因为版本号可能重新出现。
    pub fn reset(&mut self) {
        *self = Self::new(self.seed);
    }

    /// 注册新房间；房名已存在时忽略传入地形，不修改任何版本。
    pub fn register_room(&mut self, name: impl Into<String>, terrain: Terrain) {
        let name = name.into();
        if !self.rooms.contains_key(&name) {
            self.rooms
                .insert(name.clone(), RoomState::new(name, terrain));
            self.generation += 1;
            self.terrain_epoch += 1;
        }
    }

    /// 在 player_ticks=0 时替换已有房间的地形，成功后使地形和内容缓存失效。
    /// 不自行规范化 Terrain；调用方须在构造时完成地形码校验及必要的边界规范化。
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

    /// 返回按房名排序的只读房间集合。
    pub fn rooms(&self) -> &BTreeMap<String, RoomState> {
        &self.rooms
    }

    /// 当前绝对 tick。
    pub fn time(&self) -> u64 {
        self.time
    }

    /// 批量提交中的整房替换，不自动递增 generation 或 terrain_epoch。
    /// 调用方须保证对象 ID 跨房间唯一、坐标合法，且不改变已缓存的地形。
    /// 写回所有房间后，必须在查询前调用 advance_ticks，使目录和内容缓存失效。
    /// 引擎只在所有结算检查通过后调用本方法；失败重试也依赖该版本更新顺序。
    pub fn commit_room(&mut self, name: String, state: RoomState) {
        self.rooms.insert(name, state);
    }

    /// 仅设置测试场景的绝对 time，不修改 player_ticks 或 generation。
    /// 应在建立相关缓存前使用；doc(hidden) 只隐藏文档，不限制外部调用。
    #[doc(hidden)]
    pub fn set_time_for_test(&mut self, t: u64) {
        self.time = t;
    }

    /// 返回可变房间引用，并保守递增 generation，房间不存在时也递增。
    /// 不递增 terrain_epoch，不能通过该引用修改 terrain，否则地形缓存会陈旧。
    /// 直接导入对象时，调用方还须检查坐标及跨房间 ID 唯一性。
    pub fn room_mut(&mut self, name: &str) -> Option<&mut RoomState> {
        self.generation += 1; // 保守失效：可变访问即视为内容可能变更
        self.rooms.get_mut(name)
    }

    /// 根据 ID 返回世界目录中的房名和对象。
    /// 内容版本变化后的首次目录访问会重建目录；有效目录下使用对数复杂度查找。
    pub fn get(&self, id: ObjId) -> Option<(&str, &RoomObject)> {
        let dir = self.obj_dir();
        let name = dir.rooms.get(&id)?;
        let (room_name, state) = self.rooms.get_key_value(name)?;
        state.objects.get(&id).map(|o| (room_name.as_str(), o))
    }

    /// 获取可变对象并保守使内容缓存失效；不能修改对象 ID，也须保持坐标合法。
    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut RoomObject> {
        // 先使用当前版本的目录定位，再使缓存失效。
        // 提前递增会强制本次定位重建目录；即使 ID 不存在，仍保守递增版本。
        let name = self.obj_dir().rooms.get(&id).cloned();
        self.generation += 1;
        let name = name?;
        self.rooms.get_mut(&name)?.objects.get_mut(&id)
    }

    pub fn remove(&mut self, id: ObjId) -> Option<RoomObject> {
        let name = self.obj_dir().rooms.get(&id).cloned()?;
        let removed = self.rooms.get_mut(&name)?.objects.remove(&id);
        if removed.is_some() {
            self.generation += 1;
        }
        removed
    }

    /// 当前内容版本；可变访问和时间推进也会改变它。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 版本变化后扫描对象键重算最大 ID，同一版本内直接返回缓存。
    fn max_obj_id(&self) -> u64 {
        {
            let mut m = self.max_id_cache.borrow_mut();
            if m.0 != self.generation {
                *m = (
                    self.generation,
                    self.rooms
                        .values()
                        .flat_map(|room| room.objects.keys())
                        .map(|id| id.0)
                        .max()
                        .unwrap_or(0),
                );
            }
        }
        self.max_id_cache.borrow().1
    }

    /// 版本变化后重建定位表，同一版本内复用。
    /// 跨房重复 ID 属于非法状态；此处取房名排序最靠前的对象，不进行全局校验。
    fn obj_dir(&self) -> std::cell::Ref<'_, ObjDir> {
        {
            let mut dir = self.obj_dir.borrow_mut();
            if dir.generation != self.generation {
                // 快照解码出的 state.name 可能为空，定位须使用世界集合的房名键。
                let mut rooms: BTreeMap<ObjId, String> = BTreeMap::new();
                for (key, state) in &self.rooms {
                    for &id in state.objects.keys() {
                        rooms.entry(id).or_insert_with(|| key.clone());
                    }
                }
                *dir = ObjDir {
                    generation: self.generation,
                    rooms,
                };
            }
        }
        self.obj_dir.borrow()
    }

    /// 当前地形缓存版本。
    pub fn terrain_epoch(&self) -> u64 {
        self.terrain_epoch
    }

    /// 显式使对象目录和内容缓存失效，不推进时钟或使地形缓存失效。
    /// 供绕过通常更新方法的内容修改使用。
    pub fn bump_generation(&mut self) {
        self.generation += 1;
    }

    /// 获取当前房间的版本标识；房间不存在时返回 None。
    pub fn room_version(&self, room: &str) -> Option<RoomVersionHandle> {
        self.rooms.get(room)?;
        Some(RoomVersionHandle {
            // 房名取集合的键，避免使用快照解码后尚未设置的 state.name。
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
        // 分别检查地形更新、删除和可变访问的版本变化。
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

    /// 手工导入高 ID 后，查询和分配器须刷新缓存；删除它后候选 ID 可回落。
    #[test]
    fn obj_dir_lookup_remove_and_next_id_equivalence() {
        let mut world = World::new(1);
        for name in ["W1N1", "W2N2", "E3S3"] {
            world.register_room(name, Terrain::plains());
        }
        let a = world.insert("W1N1", energy(10)).unwrap();
        let b = world.insert("W2N2", energy(20)).unwrap();
        // 手工导入（room_mut → 代际 bump → 目录重建吸收）
        world
            .room_mut("E3S3")
            .unwrap()
            .insert_with_id(900, energy(30))
            .unwrap();
        // 目录新鲜度断言：手工导入后未触达 → 下次触达重建
        for (id, room, amount) in [(a, "W1N1", 10), (b, "W2N2", 20), (ObjId(900), "E3S3", 30)] {
            let (name, obj) = world.get(id).unwrap();
            assert_eq!(name, room, "目录定位 {id:?}");
            match obj {
                RoomObject::Energy(e) => assert_eq!(e.amount, amount),
                _ => panic!("非 energy"),
            }
        }
        assert_eq!(world.next_obj_id(), 901, "手工导入高 id 抬升候选");
        // 移除后目录失效（remove bump）→ get None；max 重算不含已移除
        assert!(world.remove(ObjId(900)).is_some());
        assert!(world.get(ObjId(900)).is_none());
        // 游标只保留普通分配所用的范围；手工导入的 900 被删除后不再抬高候选。
        assert_eq!(
            world.next_obj_id(),
            3,
            "移除导入 max 后候选回落（扫描同构）"
        );
        // get_mut 经目录定位
        if let RoomObject::Energy(e) = world.get_mut(b).unwrap() {
            e.amount = 25;
        }
        match world.get(b).unwrap().1 {
            RoomObject::Energy(e) => assert_eq!(e.amount, 25),
            _ => panic!("非 energy"),
        }
    }

    /// 构造场景时设置 time 不改变版本或地形修改的限制条件。
    #[test]
    fn set_time_for_test_touches_only_time() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let g0 = world.generation();
        let pt0 = world.player_ticks;
        world.set_time_for_test(5);
        assert_eq!(world.time(), 5);
        assert_eq!(world.generation(), g0, "摆钟不 bump 代际");
        assert_eq!(world.player_ticks, pt0, "摆钟不动 player_ticks");
    }

    /// 批量写回后推进时间，目录及最大 ID 缓存须看到新对象。
    #[test]
    fn obj_dir_rebuilds_after_settle_style_commit() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let a = world.insert("W1N1", energy(1)).unwrap();
        // 模拟引擎先替换房间，再统一推进时间的提交顺序。
        let mut room = RoomState::new("W1N1", Terrain::plains());
        room.insert_with_id(a.0, energy(2)).unwrap();
        room.insert_with_id(777, energy(3)).unwrap();
        world.commit_room("W1N1".to_string(), room);
        world.advance_ticks(1);
        // bump 后首次触达重建 → 新对象可见
        assert!(world.get(ObjId(777)).is_some(), "提交尾新对象经目录可见");
        assert_eq!(world.next_obj_id(), 778);
    }

    /// 首次查询建立目录后，连续插入须增量维护目录和最大 ID 缓存。
    #[test]
    fn obj_dir_eager_maintenance_on_consecutive_inserts() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        for i in 0..50 {
            let id = world.insert("W1N1", energy(i)).unwrap();
            // max 缓存增量续档：每次 insert 后保持新鲜
            assert_eq!(
                *world.max_id_cache.borrow(),
                (world.generation, (i + 1) as u64),
                "第 {i} 次 insert 后 max 缓存新鲜"
            );
            if i == 0 {
                // 目录首次触达（读侧消费者出现）
                assert!(world.get(id).is_some());
            }
            // 目录增量续档：触达后保持新鲜
            assert_eq!(
                world.obj_dir.borrow().generation,
                world.generation,
                "第 {i} 次 insert 后目录新鲜"
            );
        }
        assert_eq!(world.obj_dir.borrow().rooms.len(), 50);
    }

    /// 陈旧目录不能只补新插入对象就标为有效。
    /// 删除或导入后立即插入，再检查定位与分配结果，防止丢失未吸收的变更。
    #[test]
    fn obj_dir_guard_skips_extension_when_stale() {
        // 形态一：remove 致陈旧 → insert → 触达
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let _ = world.next_obj_id(); // 暖缓存（稳态）
        let a = world.insert("W1N1", energy(1)).unwrap();
        assert!(world.remove(a).is_some()); // bump，两缓存陈旧
        let b = world.insert("W1N1", energy(2)).unwrap(); // 不得续档
        assert_eq!(b.0, 2, "游标分配");
        assert_eq!(
            world.next_obj_id(),
            3,
            "重建吸收 remove（不残留 a 的 max=1 作 2）"
        );
        assert!(world.get(a).is_none());
        assert!(world.get(b).is_some());

        // 形态二：room_mut 手工导入高 id 致陈旧 → insert → 触达
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let _ = world.next_obj_id(); // 暖缓存
        let c = world.insert("W1N1", energy(1)).unwrap();
        world
            .room_mut("W1N1")
            .unwrap()
            .insert_with_id(900, energy(9))
            .unwrap(); // bump，缓存陈旧
        let d = world.insert("W1N1", energy(2)).unwrap(); // 不得续档
        assert_eq!(d.0, 901, "分配必须看到导入的 900（重建吸收）");
        assert_eq!(world.next_obj_id(), 902);
        assert!(world.get(ObjId(900)).is_some());
        assert!(world.get(c).is_some());
        assert!(world.get(d).is_some());
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

    /// Mod 对象随二进制快照往返（新标签编码）：type_key 与字段 payload
    /// 字节保持，混合对象表内顺序不变；查询快照读面带类型键与字段。
    #[test]
    fn snapshot_roundtrip_carries_mod_objects() {
        let mut world = World::new(42);
        world.register_room("W1N1", Terrain::plains());
        world.insert("W1N1", energy(1)).unwrap();
        let mod_id = world
            .insert(
                "W1N1",
                RoomObject::Mod(kk_schema::room::ModObj {
                    id: 0,
                    x: 10,
                    y: 12,
                    type_key: "extmod:counter".into(),
                    data: r#"{"owner":"p1","x":10,"y":12,"value":3}"#.into(),
                }),
            )
            .unwrap();
        let snap = world
            .room("W1N1")
            .unwrap()
            .encode_snapshot(77, 42, world.next_obj_id());
        let back = RoomState::decode_snapshot(&snap).unwrap();
        assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
        let mod_obj = back
            .objects
            .values()
            .find_map(|o| match o {
                RoomObject::Mod(m) => Some(m),
                _ => None,
            })
            .unwrap();
        assert_eq!(mod_obj.type_key, "extmod:counter");
        assert_eq!(mod_obj.x, 10);
        assert!(mod_obj.data.contains("\"value\":3"));
        world.commit_room("W1N1".into(), back);
        let snapshot = world.object_snapshot_by_id(mod_id).unwrap();
        assert_eq!(snapshot["kind"], "mod");
        assert_eq!(snapshot["typeKey"], "extmod:counter");
        assert_eq!(snapshot["fields"]["value"], 3);
    }

    /// Mod 声明的命名空间资源随二进制快照往返：store_extra 的资源名与
    /// 数量、命名的掉落堆都按既有 ResourceStack/EnergyObj 编码保持，
    /// 查询快照读面带资源键（N2a 资源目录的快照面不引入新编码）。
    #[test]
    fn snapshot_roundtrip_carries_mod_named_resources() {
        let mut world = World::new(42);
        world.register_room("W1N1", Terrain::plains());
        let creep_id = world
            .insert(
                "W1N1",
                RoomObject::Creep(kk_schema::room::CreepObj {
                    id: 0,
                    x: 10,
                    y: 12,
                    fatigue: 0,
                    age_time: u64::MAX / 2,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: "hauler".into(),
                    owner: "p1".into(),
                    body: vec![kk_schema::room::BodyPart {
                        kind: crate::types::BodyKind::Carry.code(),
                        hits: 100,
                        boost: None,
                    }],
                    store: kk_schema::room::Store::default(),
                    store_extra: vec![kk_schema::room::ResourceStack {
                        resource: "resmod:crystal".into(),
                        amount: 12,
                    }],
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            )
            .unwrap();
        world
            .insert(
                "W1N1",
                RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x: 11,
                    y: 12,
                    resource_type: 0,
                    amount: 7,
                    resource: Some("resmod:dust".into()),
                }),
            )
            .unwrap();
        let snap = world
            .room("W1N1")
            .unwrap()
            .encode_snapshot(77, 42, world.next_obj_id());
        let back = RoomState::decode_snapshot(&snap).unwrap();
        assert_eq!(back.encode_snapshot(77, 42, world.next_obj_id()), snap);
        let creep = match back.objects.get(&ObjId(creep_id.0)).expect("id 保持") {
            RoomObject::Creep(c) => c,
            _ => panic!("对象族保持"),
        };
        assert_eq!(
            creep.store_extra,
            vec![kk_schema::room::ResourceStack {
                resource: "resmod:crystal".into(),
                amount: 12,
            }]
        );
        let pile = back
            .objects
            .values()
            .find_map(|o| match o {
                RoomObject::Energy(e) if e.resource.as_deref() == Some("resmod:dust") => Some(e),
                _ => None,
            })
            .unwrap();
        assert_eq!(pile.amount, 7);
        world.commit_room("W1N1".into(), back);
        let snapshot = world.object_snapshot_by_id(creep_id).unwrap();
        assert_eq!(snapshot["store"]["resmod:crystal"], 12);
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
    fn restored_room_queries_use_world_map_key() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let id = world.insert("W1N1", energy(1)).unwrap();
        let snapshot = world.room("W1N1").unwrap().encode_snapshot(0, 1, 2);
        let restored = RoomState::decode_snapshot(&snapshot).unwrap();
        assert!(restored.name.is_empty());
        world.commit_room("W1N1".into(), restored);
        world.advance_ticks(1);
        assert_eq!(world.get(id).unwrap().0, "W1N1");
        assert_eq!(
            world.object_json_by_id(id).unwrap()["pos"]["roomName"],
            "W1N1"
        );
        assert_eq!(
            world.object_snapshot_by_id(id).unwrap()["pos"]["roomName"],
            "W1N1"
        );
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
