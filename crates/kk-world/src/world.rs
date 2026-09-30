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

/// 对象目录（O1，双房批）：id → 房名定位，按内容代际懒重建（同
/// terrain_cache 的版本键纪律）。服务 [`World::get`]/[`World::get_mut`]/
/// [`World::remove`] 的房名定位：每代际首次触达全量重建一次（N 次
/// BTreeMap 插入 + **每唯一 id 一次**房名 String 分配——分配按对象数
/// 计非房数，评审 P2 勘正），之后本代际内 O(log N) 查找；
/// insert 增量续档。**收益量级勘定（评审 3 F4 实测）**：小规模首房命中
/// 旧逐房扫描常数更低（4 房 1000 对象命中 1.17-1.24× 慢）；40 房命中
/// 4.3× 快、miss 面 23× 快——收益随房数/未命中增长，现状 2-4 房为形状
/// 先行。[`World::next_obj_id`] 的 max 走分体的 `max_id_cache`（零分配
/// 键扫描——结算每 tick 的 next_available 路径，与原扫描同价），不经
/// 本目录。正确性前提 = generation 的既有不变量（全部内容可变面 bump；
/// 结算提交尾直接写 rooms 后必经 advance_ticks——见 generation 字段
/// 注释）。
#[derive(Debug, Clone, Default)]
struct ObjDir {
    generation: u64,
    rooms: BTreeMap<ObjId, String>,
}

/// 世界（M1：房间集合 + 全局时间；玩家分片语义在 embed 层）。
#[derive(Debug, Clone)]
pub struct World {
    /// 世界时间（M2B 扩面 P0 收敛：pub(crate)——外部读取经 [`World::time`]，
    /// 推进经 [`World::advance_ticks`]，跨 crate 测试/夹具摆钟经
    /// [`World::set_time_for_test`]；crate 内测试仍直写（pub(crate) 放行，
    /// 构造期无害口径）。直写不 bump 代际的旧纪律不变，跨 crate 面由
    /// 可见性收口强制。
    pub(crate) time: u64,
    pub seed: u64,
    /// 已经过玩家 tick（setTerrain 只允许在 0 时调用——screeps-ok 契约）。
    pub player_ticks: u64,
    /// **硬纪律（O1 后果升级，评审 P2-8 挑战登记口径）**：直写本字段而不
    /// bump 代际，对象目录/`max_id_cache` 将供陈旧定位——get 误报 None、
    /// remove 静默失败（较投影缓存陈旧更重）。**M2B 扩面 P0 已收敛
    /// pub(crate)**（双房批 P2-8/O1 双登记的兑现）：外部读经
    /// [`World::rooms`]/[`World::room`]，整房写回经 [`World::commit_room`]
    ///（代际纪律——调用方必经 advance_ticks——见 [`World::commit_room`]）。
    pub(crate) rooms: BTreeMap<String, RoomState>,
    /// 全局 id 分配器（跨房间唯一；快照每房 header 存同一全局值）。
    next_obj_id: u64,
    /// 世界内容代际（M2B 前置批 P2，ADR-013 决策 2）：任何已提交内容变更
    /// 单调递增——投影缓存失效键、对象目录/`max_id_cache` 的失效键
    ///（O1，双房批）与 [`RoomVersionHandle`] 的内容分量。
    /// bump 点 = 全部内容可变面（insert/remove/set_terrain/register_room/
    /// advance_ticks + room_mut/get_mut 保守覆盖）；kk-engine 提交尾经
    /// [`World::commit_room`] 整房写回后必经 advance_ticks，代际随之递增
    ///（commit_room 自身不 bump——「一次成功 tick ⇒ 恰 +1」由 advance_ticks
    /// 单点承载）。结算失败路径不 bump（世界未变）。
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
    /// 对象目录缓存（O1）：见 [`ObjDir`]。RefCell 前提同 terrain_cache
    ///（World 单线程持用）；随 Clone 复制、new/reset 归零——代际键防陈旧
    /// 命中。
    obj_dir: RefCell<ObjDir>,
    /// max id 游标缓存（O1）：代际键 (generation, max)——next_obj_id 消费。
    /// 与目录分体：重建=纯键扫描零分配（与原全房扫描同价），不付目录
    /// map 的分配成本（结算每 tick 的 next_available 在此路径上——实测
    /// 每代际全量建目录令 settle 相 +6%，分体后复原）。
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
        // 目录增量续档（O1）：目录在上一代际仍新鲜时直接带入本代际
        //（连续 insert 每次免全量重建——夹具摆 N 对象 O(N²) → O(N log N)）；
        // 目录已陈旧则留给下次触达懒重建
        {
            let mut dir = self.obj_dir.borrow_mut();
            if dir.generation + 1 == self.generation {
                dir.rooms.insert(ObjId(id), room_name.to_string());
                dir.generation = self.generation;
            }
            // max 侧守卫当前恒真（评审 P2-3 实测注记）：入口
            // next_available_obj_id→max_obj_id 已把 m.0 刷到本代际——
            // 守卫是对未来调用序变化的防御；「陈旧则跳过」的真纪律在
            // 目录侧守卫
            let mut m = self.max_id_cache.borrow_mut();
            if m.0 + 1 == self.generation {
                *m = (self.generation, m.1.max(id));
            }
        }
        Ok(ObjId(id))
    }

    /// 推进时间（player_ticks 的唯一维护点——setTerrain 守卫依赖它）。
    /// 同时递增内容代际：投影的时钟折叠面（ticksToRegeneration 等）读
    /// `time`，tick 推进即投影内容变化，缓存必须失效；kk-engine 的结算
    /// 提交尾（经 commit_room 整房写回后）以本方法收束，代际随之覆盖。
    pub fn advance_ticks(&mut self, n: u64) {
        self.time += n;
        self.player_ticks += n;
        self.generation += 1;
    }

    /// 下一个候选 id；直接导入房间对象后也不会回退到已有 id。max 取自
    /// `max_id_cache` 游标缓存（O1，双房批；原每次调用全房扫描——insert
    /// 路径每摆一对象扫一次全房，storage-bench T3 夹具实测 55× 提速）。
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

    /// 只读全房视图（P0 收敛读取面：keys/values/迭代消费走此——
    /// `rooms` 字段 pub(crate) 后的外部访问口）。
    pub fn rooms(&self) -> &BTreeMap<String, RoomState> {
        &self.rooms
    }

    /// 当前世界时间（P0 收敛读取面；推进经 [`Self::advance_ticks`]）。
    pub fn time(&self) -> u64 {
        self.time
    }

    /// 结算提交尾的整房写回口（P0 收敛：外部唯一整房替换写路径——
    /// 就地内容变更走 room_mut，自带保守 bump）。
    /// **代际不在此 bump**——调用方（结算提交尾）必经
    /// [`Self::advance_ticks`] 收束，「一次成功 tick ⇒ generation 恰 +1」
    /// 的既有不变量（engine 级测试锁定）由 advance_ticks 单点承载；
    /// 失败回退路径不调用本方法（世界未变）。
    pub fn commit_room(&mut self, name: String, state: RoomState) {
        self.rooms.insert(name, state);
    }

    /// 测试/夹具的构造期摆钟（P0 收敛：语义与旧 pub 直写逐点一致——不动
    /// player_ticks/generation，构造期无害系 P2 前置批登记口径）。
    /// #[doc(hidden)] 测试旋钮先例：`set_fail_room_for_test` 等。
    #[doc(hidden)]
    pub fn set_time_for_test(&mut self, t: u64) {
        self.time = t;
    }

    /// **禁写 `terrain` 字段**（P3 钉子，评审 2 P2-1）：纪元不随本方法的
    /// 可变访问递增——绕过 `set_terrain` 改地形会让投影的地形缓存供陈旧
    /// 载荷（当前生产调用面为零，测试亦不写地形）。
    pub fn room_mut(&mut self, name: &str) -> Option<&mut RoomState> {
        self.generation += 1; // 保守失效：可变访问即视为内容可能变更
        self.rooms.get_mut(name)
    }

    /// 跨房间取对象（M1 主要服务于 embed getObject）。经对象目录三次
    /// O(log N) BTreeMap 查找（id→房名→房态→对象；O1，双房批——原逐房线性扫描；外部
    /// 复审 P2 勘正：非 O(1)，暖缓存后 BTreeMap 双跳对数查找）。
    pub fn get(&self, id: ObjId) -> Option<(&str, &RoomObject)> {
        let dir = self.obj_dir();
        let name = dir.rooms.get(&id)?;
        let state = self.rooms.get(name)?;
        state.objects.get(&id).map(|o| (state.name.as_str(), o))
    }

    pub fn get_mut(&mut self, id: ObjId) -> Option<&mut RoomObject> {
        // 先经目录定位房名（只读、Ref 语句内结束借用）**再** bump——若
        // 先 bump 则目录必为新一代际首触达、每次全量重建，O(1) 落空
        //（评审 2 P2-2 清偿）；bump 语义不变：id 不存在仍计 +1（保守失效
        // 同 room_mut，generation_bumps 测试锁定）
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

    /// 当前世界内容代际（见字段注释）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// max id 触达（懒重建——代际键失效；重建=纯键扫描零分配，与原
    /// next_obj_id 的扫描同价；本代际内后续 O(1)）。
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

    /// 对象目录触达（懒重建——代际键失效；重建 O(N log N)+**每唯一 id
    /// 一次房名 String 分配**（外部复审 P2 勘正：非「每房一次」——分配
    /// 探针实测单房 1000 对象首触达 1000 次房名分配/共 1167 次；同代际
    /// 第二次查询零分配），每代际至多一次；之后本代际内查找 O(log N)。
    /// get/get_mut/remove 消费——结算热路径（next_available）不经此
    ///（走分体 max 缓存的零分配扫描）。
    fn obj_dir(&self) -> std::cell::Ref<'_, ObjDir> {
        {
            let mut dir = self.obj_dir.borrow_mut();
            if dir.generation != self.generation {
                // 值用 map key（F-1 key 权威裁决：快照解码可产出
                // name="" 的房——state.name 非权威）；同 id 跨房并存
                //（违反全局唯一不变量）时首房胜——与原 values() 扫描
                // 同构（评审 1 P2-1/P2-2 清偿）
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

    /// 当前地形纪元（见字段注释）。
    pub fn terrain_epoch(&self) -> u64 {
        self.terrain_epoch
    }

    /// 显式递增代际（防御面：绕过 World 方法写 rooms 内容的调用位——
    /// P0 pub(crate) 收敛后跨 crate 已结构性封死，仅剩 crate 内直写；正常
    /// 路径由各方法内置 bump 覆盖，本方法供此类调用方显式声明提交）。
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

    /// O1 目录：get/remove/next_obj_id 经目录 O(1) 且与原全房扫描语义
    /// 等价（混合来源：insert 分配 + room_mut 手工导入高 id）。
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
        // 移除 max 对象后候选回落——与原全房扫描语义逐位一致（游标只
        // 保证其分配过的 id 不复用；手工导入的 900 已不存在）
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

    /// set_time_for_test 负向语义（M2B 扩面 P0 评 3 P2-1 清偿）：与旧 pub
    /// 直写逐点等价——不动 generation/player_ticks（若有人令其顺带 bump 或
    /// 拨 player_ticks 翻转 set_terrain 守卫，此测试红）。
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

    /// O1 目录：结算式提交（经 commit_room 整房写回 + advance_ticks 收束）后
    /// 目录经代际 bump 懒重建吸收新对象——generation 不变量的消费面。
    #[test]
    fn obj_dir_rebuilds_after_settle_style_commit() {
        let mut world = World::new(1);
        world.register_room("W1N1", Terrain::plains());
        let a = world.insert("W1N1", energy(1)).unwrap();
        // 模拟结算提交尾：commit_room 整房写回（自身不 bump——与真实提交尾
        // 同语义），代际由 advance_ticks 承载
        let mut room = RoomState::new("W1N1", Terrain::plains());
        room.insert_with_id(a.0, energy(2)).unwrap();
        room.insert_with_id(777, energy(3)).unwrap();
        world.commit_room("W1N1".to_string(), room);
        world.advance_ticks(1);
        // bump 后首次触达重建 → 新对象可见
        assert!(world.get(ObjId(777)).is_some(), "提交尾新对象经目录可见");
        assert_eq!(world.next_obj_id(), 778);
    }

    /// O1 增量续档（分体双属性）：max 缓存在连续 insert 下始终增量保持
    ///（next_obj_id 路径零重建零分配）；目录（id→房名 map）在首次
    /// get/remove 触达后增量保持（纯 insert 序列不触达目录——其惰性重建
    /// 留给首个读侧消费者，一次付清）。
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

    /// O1 续档守卫杀伤线（评审 2 P1-1 清偿）：致陈旧操作（remove/手工
    /// 导入）与 insert 之间**无刷新读**——insert 见陈旧必须跳过续档，
    /// 下次触达懒重建同时吸收两侧；守卫若被弱化（无条件续档），缓存带
    /// 缺失增量标记新鲜 → next_obj_id 偏低 → 撞 id，本测试即红
    ///（变异实验：删守卫全套件仍绿——零覆盖缺口由本测试封堵）。
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
