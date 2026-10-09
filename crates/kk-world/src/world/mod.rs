//! 房间状态、对象表和世界级 ID 分配。
//!
//! 房间与对象使用 BTreeMap，遍历顺序由键决定。World::insert 分配的 ID
//! 单调递增，所以普通插入的对象按创建顺序遍历；导入指定 ID 时则按 ID 排序。
//! World 的目录和投影缓存依靠 generation、terrain_epoch 失效，修改状态须维护版本。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod checkpoint;

mod mutation;
pub use mutation::RoomContents;

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
    pub(crate) terrain: Terrain,
    pub objects: BTreeMap<ObjId, RoomObject>,
    /// 最近一次房间结算产生的事件，每项是一个 JSON 字符串。
    /// 结算整体替换列表，二进制房间快照同时保存它。
    pub events: Vec<String>,
    /// 对象扩展字段：对象 ID → 字段稳定键全名 → 值。
    /// 键带贡献命名空间，不能与内建字段冲突；值在写入入口按
    /// 字段 schema 校验（类型、限额、有限浮点）。随房间候选克隆，
    /// 与对象表同生命周期：删除对象时调用方须同步清理本表。
    /// 二进制房间快照（encode/decode_snapshot）当前不保存扩展；
    /// 完整世界检查点通过独立 extensions 字段保存，不改变房间二进制布局。
    pub extensions: BTreeMap<ObjId, BTreeMap<String, serde_json::Value>>,
    /// 内建效果独立存储，随对象删除/迁移，随整轮候选提交。
    pub effects: BTreeMap<ObjId, Vec<crate::EffectEntry>>,
}

impl RoomState {
    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    pub fn new(name: impl Into<String>, terrain: Terrain) -> Self {
        Self {
            name: name.into(),
            terrain,
            objects: BTreeMap::new(),
            events: Vec::new(),
            extensions: BTreeMap::new(),
            effects: BTreeMap::new(),
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
        if id == 0 || id == u64::MAX {
            return Err("对象 ID 不得为零或耗尽标记".into());
        }
        if self.objects.contains_key(&ObjId(id)) {
            return Err(format!("对象 id 重复: {}", ObjId(id).as_str()));
        }
        if let RoomObject::Generic(g) = &object
            && let Ok(data) = serde_json::from_str::<serde_json::Value>(&g.data)
        {
            crate::domain::deposit::validate(&data)?;
        }
        let effects = crate::effects::extract(&mut object)?;
        object.set_id(id);
        if let Some(effects) = effects {
            self.effects.insert(ObjId(id), effects);
        }
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
#[derive(Debug)]
pub struct World {
    instance: Arc<()>,
    /// 当前绝对 tick，正常推进使用 advance_ticks，以同时维护内容版本。
    /// 构造测试场景可用 set_time_for_test，它也会推进版本使投影失效。
    pub(crate) time: u64,
    pub(crate) seed: u64,
    /// advance_ticks 累计推进的 tick 数；非零时 set_terrain 拒绝替换地形。
    pub(crate) player_ticks: u64,
    /// 按房名保存房间；键是查询和投影使用的房间标识。
    /// crate 内直接修改对象后须递增 generation，否则对象目录和最大 ID 缓存会陈旧。
    /// 外部编辑与发布使用受控编辑入口或 commit_tick_rooms。
    pub(crate) rooms: BTreeMap<String, RoomState>,
    /// 全局 id 分配器（跨房间唯一；快照每房 header 存同一全局值）。
    next_obj_id: u64,
    /// 世界内容版本，用于查询和缓存失效。
    /// 插入、成功删除、注册新房、设置地形及推进时间会递增；可变访问保守递增，
    /// 旧内部测试的可变访问即使未找到对象也递增；生产整轮提交只推进一次。
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

// 独立副本拥有新身份；受控编辑发布时 adopt_edit 保留原世界身份。
impl Clone for World {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::new(()),
            time: self.time,
            seed: self.seed,
            player_ticks: self.player_ticks,
            rooms: self.rooms.clone(),
            next_obj_id: self.next_obj_id,
            generation: self.generation,
            terrain_epoch: self.terrain_epoch,
            terrain_cache: self.terrain_cache.clone(),
            obj_dir: self.obj_dir.clone(),
            max_id_cache: self.max_id_cache.clone(),
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new(0)
    }
}

impl World {
    /// 观察限时视野：查看者名下 observer 的 observeRoom 标记（结算相写
    /// 入、预意图相清零）指向的目标房集合。标记在观察者所在房，目标是
    /// 任意房——须世界级扫描；目标房未注册时跳过（官方 data.js 的房间
    /// 查询自然落空，Game.rooms 不出现该房）。
    /// 来源：driver/lib/runtime/data.js:99-127 的 userObjects 房间计算。
    pub(crate) fn observed_rooms_of(&self, user: &str) -> std::collections::BTreeSet<String> {
        let mut observed = std::collections::BTreeSet::new();
        for state in self.rooms.values() {
            for object in state.objects.values() {
                let RoomObject::Generic(g) = object else {
                    continue;
                };
                if !g.data.contains("\"observeRoom\"")
                    || crate::domain::object_owner(object).as_deref() != Some(user)
                {
                    continue;
                }
                let Ok(data) = serde_json::from_str::<serde_json::Value>(&g.data) else {
                    continue;
                };
                if data
                    .get("structureType")
                    .and_then(serde_json::Value::as_str)
                    != Some("observer")
                {
                    continue;
                }
                if let Some(target) = data.get("observeRoom").and_then(serde_json::Value::as_str)
                    && self.rooms.contains_key(target)
                {
                    observed.insert(target.to_string());
                }
            }
        }
        observed
    }

    pub fn instance(&self) -> Arc<()> {
        self.instance.clone()
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn player_ticks(&self) -> u64 {
        self.player_ticks
    }

    pub fn new(seed: u64) -> Self {
        Self {
            instance: Arc::new(()),
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
        let generation = self.next_generation()?;
        let id = self.next_available_obj_id()?;
        self.rooms
            .get_mut(room_name)
            .expect("房间已检查存在")
            .insert_with_id(id, object)?;
        self.next_obj_id = id + 1;
        self.generation = generation;
        // 仅在目录完整覆盖修改前的版本时增量加入新对象。
        // 陈旧目录不能只补本次插入就标为有效，否则会漏掉其他尚未吸收的修改。
        // 恢复自检查点的世界把缓存代际置为 u64::MAX 陈旧哨兵，比较须用
        // 减法（generation 已在上面递增，恒 ≥1），加法会令哨兵 +1 溢出。
        {
            let mut dir = self.obj_dir.borrow_mut();
            if dir.generation == self.generation - 1 {
                dir.rooms.insert(ObjId(id), room_name.to_string());
                dir.generation = self.generation;
            }
            // 分配前已刷新最大 ID 缓存；同样只把有效的上一版本推进到新版本。
            let mut m = self.max_id_cache.borrow_mut();
            if m.0 == self.generation - 1 {
                *m = (self.generation, m.1.max(id));
            }
        }
        Ok(ObjId(id))
    }

    /// 同时推进 time 和 player_ticks，并递增一次 generation。
    /// 即使 n=0 也递增版本；相对计时投影及批量写回的房间状态依靠它使缓存失效。
    pub fn advance_ticks(&mut self, n: u64) -> Result<(), String> {
        let time = self.time.checked_add(n).ok_or("世界时间已耗尽")?;
        let player_ticks = self
            .player_ticks
            .checked_add(n)
            .ok_or("玩家 tick 时钟已耗尽")?;
        let generation = self.next_generation()?;
        self.time = time;
        self.player_ticks = player_ticks;
        self.generation = generation;
        Ok(())
    }

    /// 返回分配游标与当前最大对象 ID 加一中的较大值。
    /// 最大 ID 按 generation 缓存。手工导入的最大对象被删除后，此候选值可能回落；
    /// 需要永久保留导入 ID 范围时，调用 bump_next_obj_id 抬高游标。
    pub fn next_obj_id(&self) -> u64 {
        // MAX 只表示耗尽；实际分配由 next_available_obj_id 的 checked_add 拒绝。
        let max_id = self.max_obj_id();
        if max_id == u64::MAX {
            return u64::MAX;
        }
        self.next_obj_id.max(max_id + 1)
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
    pub fn register_room(
        &mut self,
        name: impl Into<String>,
        terrain: Terrain,
    ) -> Result<(), String> {
        let name = name.into();
        if !self.rooms.contains_key(&name) {
            let generation = self.next_generation()?;
            let terrain_epoch = self.terrain_epoch.checked_add(1).ok_or("地形版本已耗尽")?;
            self.rooms
                .insert(name.clone(), RoomState::new(name, terrain));
            self.generation = generation;
            self.terrain_epoch = terrain_epoch;
        }
        Ok(())
    }

    /// 在 player_ticks=0 时替换已有房间的地形，成功后使地形和内容缓存失效。
    /// 不自行规范化 Terrain；调用方须在构造时完成地形码校验及必要的边界规范化。
    pub fn set_terrain(&mut self, room: &str, terrain: Terrain) -> Result<(), String> {
        if self.player_ticks > 0 {
            return Err("setTerrain 只允许在第一个玩家 tick 之前调用".into());
        }
        let generation = self.next_generation()?;
        let terrain_epoch = self.terrain_epoch.checked_add(1).ok_or("地形版本已耗尽")?;
        let state = self
            .rooms
            .get_mut(room)
            .ok_or_else(|| format!("房间 {room} 不存在"))?;
        state.terrain = terrain;
        self.generation = generation;
        self.terrain_epoch = terrain_epoch;
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
    #[cfg(test)]
    pub(crate) fn commit_room(&mut self, name: String, state: RoomState) {
        self.rooms.insert(name, state);
    }

    /// 设置测试场景的绝对 time 并推进 generation；不修改 player_ticks。
    /// 版本耗尽时保留原状态；通过 Engine 调用仍受开放轮次约束。
    #[doc(hidden)]
    pub fn set_time_for_test(&mut self, t: u64) -> Result<(), String> {
        let generation = self.next_generation()?;
        self.time = t;
        self.generation = generation;
        Ok(())
    }

    /// 返回可变房间引用，并保守递增 generation，房间不存在时也递增。
    /// 不递增 terrain_epoch，不能通过该引用修改 terrain，否则地形缓存会陈旧。
    /// 直接导入对象时，调用方还须检查坐标及跨房间 ID 唯一性。
    #[cfg(test)]
    pub(crate) fn room_mut(&mut self, name: &str) -> Option<&mut RoomState> {
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
    #[cfg(test)]
    pub(crate) fn get_mut(&mut self, id: ObjId) -> Option<&mut RoomObject> {
        // 先使用当前版本的目录定位，再使缓存失效。
        // 提前递增会强制本次定位重建目录；即使 ID 不存在，仍保守递增版本。
        let name = self.obj_dir().rooms.get(&id).cloned();
        self.generation += 1;
        let name = name?;
        self.rooms.get_mut(&name)?.objects.get_mut(&id)
    }

    pub fn remove(&mut self, id: ObjId) -> Result<Option<RoomObject>, String> {
        let Some(name) = self.obj_dir().rooms.get(&id).cloned() else {
            return Ok(None);
        };
        let generation = self.next_generation()?;
        let room = self.rooms.get_mut(&name).expect("目录来自房间");
        let removed = room.objects.remove(&id);
        room.effects.remove(&id);
        room.extensions.remove(&id);
        if removed.is_some() {
            self.generation = generation;
        }
        Ok(removed)
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
    /// 账户等不属于房间的状态提交使用；版本耗尽时失败。
    // MAX 是目录/最大 ID 缓存的失效哨兵，不能成为有效内容版本。
    fn next_generation(&self) -> Result<u64, String> {
        self.generation
            .checked_add(1)
            .filter(|value| *value != u64::MAX)
            .ok_or_else(|| "世界版本已耗尽".into())
    }

    pub fn bump_generation(&mut self) -> Result<(), String> {
        self.generation = self.next_generation()?;
        Ok(())
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
mod tests;
