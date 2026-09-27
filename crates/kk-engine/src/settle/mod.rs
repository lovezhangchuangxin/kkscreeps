//! 单房间结算：官方 `engine/src/processor.js:16-520` 阶段序的忠实子集。
//!
//! 与官方的三处有意差异（登记灰区，方案 D4）：
//! 1. 对象迭代序 = BTreeMap id 升序（id 全局单调分配 ⇒ 创建序），官方为 DB 插入序；
//! 2. bulk 写入改为工作副本直接变更（内存模型），eventLog 照产；
//! 3. 失败即回退：`settle_room` 返回 Err 时调用方丢弃工作副本、房间保持上一 tick
//!    状态（官方出错打日志继续）。
//!
//! M1 范围外（结算忽略）：NPC、战斗全家、power、市场、跨房间落地、
//! Generic 桶对象（透传存放的 tower/rampart/…无规则行为）。

pub mod creep;
pub mod economy;
pub mod movement;
pub mod room;
pub mod spawn;

use std::collections::BTreeMap;

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, RoomIndex, RoomState};
use serde_json::{Value, json};

use crate::intent::IntentBatch;

/// RCL 配额查询（CONTROLLER_STRUCTURES，嵌套 JSON 常量解析一次）。
/// 返回该结构类型在给定 RCL 的允许数量（无条目 = 0）。
pub fn controller_allowed(structure_type: &str, rcl: i64) -> i64 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<serde_json::Value> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str(kk_core::constants::CONTROLLER_STRUCTURES)
            .expect("CONTROLLER_STRUCTURES 常量必为合法 JSON（kk-core 生成物测试锁定）")
    });
    table[structure_type][rcl.to_string()].as_i64().unwrap_or(0)
}

/// 结算失败（调用方丢弃工作副本）。
#[derive(Debug)]
pub struct SettleError(pub String);

impl std::fmt::Display for SettleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "结算失败: {}", self.0)
    }
}

/// 玩家状态（M1 仅 gcl；持久化 M2）。
#[derive(Debug, Default, Clone)]
pub struct PlayerState {
    pub gcl: i64,
}

/// 结算工作上下文：房间工作副本 + 索引 + per-tick 瞬态。
pub struct Working<'w> {
    pub room: &'w mut RoomState,
    pub index: RoomIndex,
    pub time: u64,
    pub next_id: &'w mut u64,
    pub players: &'w mut BTreeMap<String, PlayerState>,
    pub events: Vec<Value>,
    /// move 时刻的 fatigue（官方 _oldFatigue，canMove 判定用）。
    pub old_fatigue: BTreeMap<ObjId, u32>,
    /// 本 tick 疲劳累加器（官方 _fatigue；结算结束回写 creep.fatigue）。
    pub tick_fatigue: BTreeMap<ObjId, i64>,
    /// 本 tick controller 升级量（官方 _upgraded）。
    pub upgraded: BTreeMap<ObjId, i64>,
    pub movement: movement::Movement,
}

impl<'w> Working<'w> {
    pub fn creep(&self, id: ObjId) -> Option<&CreepObj> {
        match self.room.objects.get(&id) {
            Some(RoomObject::Creep(c)) => Some(c),
            _ => None,
        }
    }

    pub fn creep_mut(&mut self, id: ObjId) -> Option<&mut CreepObj> {
        match self.room.objects.get_mut(&id) {
            Some(RoomObject::Creep(c)) => Some(c),
            _ => None,
        }
    }

    pub fn controller(&self) -> Option<&kk_schema::room::ControllerObj> {
        self.room.objects.values().find_map(|o| match o {
            RoomObject::Controller(c) => Some(c),
            _ => None,
        })
    }

    /// 结算中创建新对象（build 落成/掉落/孵化）：全局 id + 索引登记。
    pub fn insert_new(&mut self, object: RoomObject) -> ObjId {
        let id = *self.next_id;
        *self.next_id = self.next_id.saturating_add(1);
        let (x, y) = object.pos();
        self.room.insert_with_id(id, object);
        self.index.insert(ObjId(id), (x, y));
        ObjId(id)
    }

    /// 删除对象（含索引）。
    pub fn remove_obj(&mut self, id: ObjId) -> Option<RoomObject> {
        let obj = self.room.objects.remove(&id)?;
        let (x, y) = obj.pos();
        self.index.remove(id, (x, y));
        self.old_fatigue.remove(&id);
        self.tick_fatigue.remove(&id);
        Some(obj)
    }

    /// 对象移动（索引增量更新）。
    pub fn move_obj(&mut self, id: ObjId, to: (u8, u8)) {
        if let Some(obj) = self.room.objects.get_mut(&id) {
            let from = obj.pos();
            obj.set_pos(to.0, to.1);
            self.index.apply_move(id, from, to);
        }
    }

    /// 官方 _add-fatigue（无 pull 的 M1 形态）：负贡献先消自身疲劳，再入累加器；
    /// 持久值 = max(0, 累加器)。
    pub fn add_fatigue(&mut self, id: ObjId, mut d: i64) {
        // 官方 _add-fatigue.js:5：累加器首触达时以持久 fatigue 播种
        let mut acc = match self.tick_fatigue.get(&id) {
            Some(v) => *v,
            None => self.creep(id).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        if acc > 0 && d < 0 {
            let resting = acc.min(-d);
            acc -= resting;
            d += resting;
            self.tick_fatigue.insert(id, acc);
            if let Some(creep) = self.creep_mut(id) {
                creep.fatigue = acc.max(0) as u32;
            }
            if d == 0 {
                return;
            }
        }
        acc += d;
        self.tick_fatigue.insert(id, acc);
        if let Some(creep) = self.creep_mut(id) {
            creep.fatigue = acc.max(0) as u32;
        }
    }

    /// 掉落资源落地（官方 _create-energy：同格同资源叠加；M1 无 container）。
    pub fn create_energy(&mut self, x: u8, y: u8, amount: i64) {
        if amount <= 0 {
            return;
        }
        let existing = self.index.at(x as usize, y as usize).iter().find_map(|id| {
            match self.room.objects.get(id) {
                Some(RoomObject::Energy(e)) if e.resource_type == 0 => Some(*id),
                _ => None,
            }
        });
        match existing {
            Some(id) => {
                if let Some(RoomObject::Energy(e)) = self.room.objects.get_mut(&id) {
                    e.amount += amount as u32;
                }
            }
            None => {
                self.insert_new(RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x,
                    y,
                    resource_type: 0,
                    amount: amount as u32,
                }));
            }
        }
    }

    /// 阶段 6 入口：movement 仲裁（字段分裂借用——矩阵可变、世界只读）。
    pub fn check_movement(&mut self) {
        let safe_mode_user: Option<String> = self.controller().and_then(|ctl| {
            if ctl.safe_mode > self.time {
                ctl.owner.clone()
            } else {
                None
            }
        });
        self.movement.check_impl(
            self.room,
            &self.index,
            &self.old_fatigue,
            safe_mode_user.as_deref(),
        );
    }

    pub fn push_event(&mut self, event: i64, object_id: ObjId, data: Value) {
        self.events.push(json!({
            "event": event,
            "objectId": object_id.as_str(),
            "data": data,
        }));
    }
}

/// 结算一个房间（processor.js processRoom 的 M1 子集）。
///
/// 工作副本由调用方 clone（失败即回退的载体）；`batch` 只取指向本房间的部分。
pub fn settle_room(
    room: &mut RoomState,
    time: u64,
    next_id: &mut u64,
    players: &mut BTreeMap<String, PlayerState>,
    batch: &IntentBatch,
) -> Result<Vec<Value>, SettleError> {
    let index = RoomIndex::build(&room.objects);
    let mut w = Working {
        room,
        index,
        time,
        next_id,
        players,
        events: Vec::new(),
        old_fatigue: BTreeMap::new(),
        tick_fatigue: BTreeMap::new(),
        upgraded: BTreeMap::new(),
        movement: movement::Movement::new(),
    };

    // 阶段 1 预扫描（processor.js:43-171 的 M1 子集）：actionLog 每 tick 重置
    for creep in w.room.objects.values_mut() {
        if let RoomObject::Creep(c) = creep {
            c.action_log = 0;
        }
    }

    // 阶段 3：_calc_spawns（processor.js:221-223）——off 标记与 energyAvailable 初算
    spawn::calc_spawns(&mut w);

    // 阶段 5：玩家 intents（官方 :227-322；玩家序 = BTreeMap 名字序）
    for (player, intents) in &batch.users {
        room::process_room_intents(&mut w, player, &intents.room);
        for (obj_id, actions) in &intents.objects {
            // 官方 :248：对象不存在或所有权不符 → 跳过；按对象类型分派
            match w.room.objects.get(obj_id) {
                Some(RoomObject::Creep(o)) if o.owner == *player => {
                    creep::process_creep_intents(&mut w, *obj_id, actions);
                }
                Some(RoomObject::Spawn(o)) if o.owner == *player => {
                    spawn::process_spawn_intents(&mut w, *obj_id, actions);
                }
                _ => continue,
            }
        }
    }

    // 阶段 6：movement.check（processor.js:324）
    w.check_movement();

    // 阶段 7：energyAvailable 重算（processor.js:326——spawn/extension 非 off 的存量）
    let _energy_available = spawn::energy_available(&w);

    // 阶段 8：逐对象 tick（官方 :342-483 的 M1 类型集；新插入对象不入本轮——
    // 官方 lodash forEach 键快照同语义）
    let ids: Vec<ObjId> = w.room.objects.keys().copied().collect();
    for id in ids {
        match w.room.objects.get(&id) {
            Some(RoomObject::Energy(_)) => economy::energy_tick(&mut w, id),
            Some(RoomObject::Source(_)) => economy::source_tick(&mut w, id),
            Some(RoomObject::Creep(_)) => creep::creep_tick(&mut w, id)?,
            Some(RoomObject::Spawn(_)) => spawn::spawn_tick(&mut w, id, _energy_available),
            Some(RoomObject::Controller(_)) => economy::controller_tick(&mut w, id),
            _ => {}
        }
    }

    Ok(w.events)
}

/// 引擎：世界 + 玩家状态 + 每玩家沙箱，按固定序推进 tick。
pub struct Engine {
    pub world: kk_world::World,
    pub players: BTreeMap<String, PlayerState>,
    pub sandboxes: BTreeMap<String, Box<dyn crate::sandbox::Sandbox>>,
    next_id: u64,
    /// 最近一次 tick 的房间事件日志（按房间结算序拼接；M1 测试消费，M2 持久化）。
    pub last_events: Vec<serde_json::Value>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(kk_world::World::new(0))
    }
}

impl Engine {
    pub fn new(world: kk_world::World) -> Self {
        let next_id = world.next_obj_id();
        Self {
            world,
            players: BTreeMap::new(),
            sandboxes: BTreeMap::new(),
            next_id,
            last_events: Vec::new(),
        }
    }

    /// 推进一个 tick：收集 intents（玩家名序）→ 逐房间（名字序）结算。
    /// 每房间 clone→结算→写回（失败即回退，ADR-008 第 3 条）；任一房间失败
    /// 即整体 Err 且世界保持一致（已写回房间保留、当前房间回退、time 不推进）。
    pub fn tick(&mut self) -> Result<(), SettleError> {
        let mut batch = IntentBatch::default();
        {
            let view = crate::sandbox::WorldView { world: &self.world };
            for (player, sandbox) in &mut self.sandboxes {
                let part = sandbox.collect_intents(&view, player);
                for (p, intents) in part.users {
                    let entry = batch.users.entry(p).or_default();
                    entry.objects.extend(intents.objects);
                    entry.room.extend(intents.room);
                }
            }
        }
        let mut events = Vec::new();
        let room_names: Vec<String> = self.world.rooms.keys().cloned().collect();
        for name in room_names {
            let mut room = self.world.rooms.get(&name).expect("房间名来自键集").clone();
            let mut next = self.next_id.max(self.world.next_obj_id());
            let mut room_events = settle_room(
                &mut room,
                self.world.time,
                &mut next,
                &mut self.players,
                &batch,
            )?;
            self.world.rooms.insert(name, room);
            self.next_id = next;
            self.world.bump_next_obj_id(next);
            events.append(&mut room_events);
        }
        self.last_events = events;
        self.world.advance_ticks(1);
        Ok(())
    }
}
