//! 单房间结算：官方 `engine/src/processor.js` 阶段序的忠实子集（M1 范围）。
//!
//! 与官方的三处有意差异（方案 D4，登记于 compatibility.md 灰区表）：
//! 1. 对象迭代按 id 升序——id 由世界单调分配，升序即创建序；官方为 DB 插入序；
//! 2. 不攒 bulk 批量写，直接改工作副本（内存模型无写放大），eventLog 照常产出；
//! 3. 结算失败即整体回退：返回 Err 时调用方丢弃工作副本，房间保持上一 tick
//!    状态（官方是打日志继续）。多房间由 [`Engine::tick`] 两阶段提交保证原子性。
//!
//! M1 不结算：NPC、战斗全家、power、市场、跨房间落地、Generic 桶对象
//! （tower/rampart 等透传对象只保证可见，无规则行为）。
//! B1 起 hits 模型落地：近战 attack/repair/dismantle 已结算（damage.rs），
//! ranged/heal/boost 仍不在范围。
//! B2 起 controller 结算族落地（creep.rs 五执行器 + controller.rs 的
//! activateSafeMode + economy.rs 的 controllers/tick 补齐）。

pub mod controller;
pub mod creep;
pub mod damage;
pub mod economy;
pub mod movement;
pub mod room;
pub mod spawn;

use std::collections::{BTreeMap, BTreeSet};

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, RoomIndex, RoomState};
use serde_json::{Value, json};

use crate::intent::{Action, IntentBatch, IntentData};

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
///
/// `rooms` 是官方 `user.rooms` 等价物（driver/lib/index.js:542-547
/// addRoomToUser 的 addToSet 集合）：claimController 的 GCL 门读
/// `user.rooms.length`（claimController.js:34-35）。维护时序对齐官方——
/// addRoomToUser 的 bulk addToSet **不**改内存对象（bulk.js:101-107，仅入队
/// DB 操作，tick 末落盘），故结算期读到的永远是上一 tick 的定集；本引擎由
/// [`Engine::tick_with_intents`] 在全部房间结算完成后按世界真值重建（与官方
/// 每 tick controllers/tick.js:18 对已占房重加 + unclaim/降级路径移除的
/// 不动点一致），同 tick 的后序 claim 不会看到先序 claim 的新房——官方同形。
#[derive(Debug, Default, Clone)]
pub struct PlayerState {
    pub gcl: i64,
    pub rooms: BTreeSet<String>,
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
    /// 本 tick 对 creep 的待施加伤害（官方 `_damageToApply`，_damage.js:20——
    /// intent 期累加、creep 自己的 tick 结算；对象移除时一并清理）。
    pub damage_to_apply: BTreeMap<ObjId, i64>,
    /// 本 tick 对 creep 的待施加治疗（官方 `_healToApply`，heal.js:29——
    /// 伤害通道的对称面，tick.js:126-128 在死亡判定前施加）。
    pub heal_to_apply: BTreeMap<ObjId, i64>,
    /// 本 tick controller 的升级封锁钟（官方 `target._upgradeBlocked`，
    /// attackController.js:48 写、controllers/tick.js:20-23 持久化后清除）。
    /// 值为绝对 tick（gameTime + CONTROLLER_ATTACK_BLOCKED_UPGRADE）。
    pub upgrade_block_pending: BTreeMap<ObjId, u64>,
    /// 本 tick 的 safeMode 激活标记（官方 `object._safeModeActivated`，
    /// activateSafeMode.js:24 写、controllers/tick.js:25-31 在 controller tick
    /// 消费——两文件间的瞬态通道）。
    pub safe_mode_activated: BTreeSet<ObjId>,
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
        self.room
            .insert_with_id(id, object)
            .expect("结算 id 分配器不得复用已有对象");
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
        self.damage_to_apply.remove(&id);
        self.heal_to_apply.remove(&id);
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

    /// 官方 _add-fatigue（_add-fatigue.js:24-27 的拉拽归链）：负贡献先消自身
    /// 疲劳，剩余部分沿被拉链（`object._pulled`）上溯到链首拉者施加——被拉
    /// creep 的移动疲劳记在拉者头上（拉拽官方语义）；持久值 = max(0, 累加器)。
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
        // 拉拽归链（官方 while 循环）：链上拉者消亡即断链（!!roomObjects 守卫）
        let head = self.movement.head_puller(self.room, id);
        let mut acc = match self.tick_fatigue.get(&head) {
            Some(v) => *v,
            None => self.creep(head).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        acc += d;
        self.tick_fatigue.insert(head, acc);
        if let Some(creep) = self.creep_mut(head) {
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
                    // 饱和合并：放置面可造 u32::MAX 堆（官方 JS 数值无此界），
                    // 裸加法在 debug 溢出 panic、release 回绕后衰减清堆丢数据
                    e.amount = e
                        .amount
                        .saturating_add(u32::try_from(amount).unwrap_or(u32::MAX));
                }
            }
            None => {
                self.insert_new(RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x,
                    y,
                    resource_type: 0,
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
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

/// 对象归属（官方 processor.js:248 的 `object.user` 面）：typed 变体直读；
/// Generic 透传解析 data.owner。与 world_json 的 object_owner 同语义（那侧
/// 是投影私有，此处结算侧单源）。
fn object_owner_of(obj: &RoomObject) -> Option<std::borrow::Cow<'_, str>> {
    match obj {
        RoomObject::Creep(o) => Some(o.owner.as_str().into()),
        RoomObject::Spawn(o) => Some(o.owner.as_str().into()),
        RoomObject::Extension(o) => Some(o.owner.as_str().into()),
        RoomObject::Storage(o) => Some(o.owner.as_str().into()),
        RoomObject::Controller(o) => o.owner.as_deref().map(Into::into),
        RoomObject::ConstructionSite(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Generic(o) => serde_json::from_str::<Value>(&o.data).ok().and_then(|v| {
            v.get("owner")
                .and_then(Value::as_str)
                .map(str::to_string)
                .map(Into::into)
        }),
        // wall/road/source/energy/flag 等无主（官方 user undefined——通用段
        // 的所有权门不拦，中性结构可设标志）
        _ => None,
    }
}

/// 官方 processor.js:313-314 的 notifyWhenAttacked 通用段：intent 存在且对象
/// 是 CONSTRUCTION_COST 类型（或 creep/powerCreep——后者 M4）→ 落库
/// `notifyWhenAttacked: !!enabled`。:248 的所有权门（他人对象跳过）内联。
/// 消费面是伤害通知（_damage.js:78 / towers/attack.js:63——M2 通知面未接线，
/// 本批只持久化标志，通知发送灰区登记）。
fn notify_when_attacked(w: &mut Working, player: &str, id: ObjId, data: &IntentData) {
    let Some(obj) = w.room.objects.get(&id) else {
        return;
    };
    if object_owner_of(obj).is_some_and(|owner| owner != player) {
        return; // 官方 :248：他人对象（无主不拦——中性墙/路可设）
    }
    // 官方 :313 门：CONSTRUCTION_COST[object.type] || creep || powerCreep
    //（controller/source/工地/旗等不在表内——API 面也只在结构类暴露，此处
    // 是清洗层允许伪造 intent 后的兜底）
    let type_gated = matches!(obj, RoomObject::Creep(_))
        || kk_world::types::construction_cost(&movement::type_name_of(obj)).is_some();
    if !type_gated {
        return;
    }
    let enabled = data.enabled.unwrap_or(false); // 官方 !!intent.enabled
    match w.room.objects.get_mut(&id) {
        Some(RoomObject::Spawn(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extension(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Storage(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::ConstructedWall(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Road(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Generic(o)) => {
            // 透传桶：标志写进 data JSON（投影层原样还原，prelude 的
            // rawData(id).notifyWhenAttacked 读到）
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or_default();
            v["notifyWhenAttacked"] = Value::Bool(enabled);
            o.data = v.to_string();
        }
        // creep：官方经 bulk.update 同样落库（出生即 true，create-creep.js:93）
        // ——CreepObj 无该字段且 API 面不暴露（Creep 无 notifyWhenAttacked
        // 方法），通知面未接线前不可观测，灰区登记
        _ => {}
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
        damage_to_apply: BTreeMap::new(),
        heal_to_apply: BTreeMap::new(),
        upgrade_block_pending: BTreeMap::new(),
        safe_mode_activated: BTreeSet::new(),
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
            // notifyWhenAttacked 通用段（官方 processor.js:313-314——各类型
            // intent 处理器之外的对象级通用段，对任何对象类型生效；纯标志写
            // 与类型处理器无交互，前置执行等价）
            if let Some(data) = actions.get(&Action::NotifyWhenAttacked) {
                notify_when_attacked(&mut w, player, *obj_id, data);
            }
            // 官方 :248：对象不存在或所有权不符 → 跳过；按对象类型分派
            match w.room.objects.get(obj_id) {
                Some(RoomObject::Creep(o)) if o.owner == *player => {
                    creep::process_creep_intents(&mut w, *obj_id, actions);
                }
                Some(RoomObject::Spawn(o)) if o.owner == *player => {
                    spawn::process_spawn_intents(&mut w, *obj_id, actions);
                }
                // controller 对象：官方 :248 的所有权门是 `object.user &&
                // object.user != userId`——他人 controller 的 intent 跳过；
                // 无主 controller 不拦（activateSafeMode 处理器自身 :8
                // `!object.user || !object.level` 再拒）， :306-311 分派
                // unclaim/activateSafeMode
                Some(RoomObject::Controller(o))
                    if o.owner.as_deref().is_none_or(|u| u == player.as_str()) =>
                {
                    controller::process_controller_intents(&mut w, *obj_id, actions);
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
    /// 测试钩子：命中名房的结算注入 Err，验证两阶段回退（仅 cfg(test) 可见）。
    #[cfg(test)]
    fail_room: Option<String>,
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
            #[cfg(test)]
            fail_room: None,
        }
    }

    /// 推进一个 tick：收集 intents（玩家名序）→ 逐房间（名字序）结算。
    ///
    /// 两阶段提交（§6.5）：全部房间先在克隆副本上结算，任一房间 Err 即整体
    /// 放弃——世界、玩家（gcl）、id 分配器、time 全部保持上一 tick 状态；全部
    /// 成功才统一写回并推进 time（逐房即写会让先房在下 tick 按同一 time 重放
    /// 结算——双倍采集）。
    pub fn tick(&mut self) -> Result<(), SettleError> {
        let mut batch = IntentBatch::default();
        {
            let view = crate::sandbox::WorldView { world: &self.world };
            for (player, sandbox) in &mut self.sandboxes {
                let part = sandbox.collect_intents(&view, player);
                for (p, intents) in part.users {
                    // 身份边界：intent 只归本沙箱注册的玩家（结算按玩家键判所有权，
                    // 冒名键直接丢弃——真沙箱接入后这是防跨玩家操作的硬不变量）
                    if p != *player {
                        continue;
                    }
                    let entry = batch.users.entry(p).or_default();
                    entry.objects.extend(intents.objects);
                    entry.room.extend(intents.room);
                }
            }
        }
        self.tick_with_intents(batch)
    }

    /// 以外部构造的 intent 批推进一个 tick——**embed 真沙箱通道的结算入口**
    /// （M2A-A2：batch 由 [`crate::intent::IntentBatch::from_json`] 从沙箱
    /// intentsList 清洗而来；假沙箱 [`Engine::tick`] 与本入口共用同一条
    /// settle_room 链——tick 契约场景 1 的"同一结算入口"）。身份边界由调用方
    /// 保证（EmbedEngine 按注册 handle 逐玩家构造，冒名面结构上不存在）。
    pub fn tick_with_intents(&mut self, batch: IntentBatch) -> Result<(), SettleError> {
        let mut events = Vec::new();
        let mut settled: Vec<(String, RoomState)> = Vec::new();
        let mut next = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let mut players = self.players.clone();
        let mut batch = batch;
        // 门④可信侧兜底（官方 API 层 structures.js:223-225 的等价物——"玩家
        // 名下任一 controller safeMode 生效中 → 新 activateSafeMode 全拒"）。
        // 官方 game 层跑在 isolated-vm 可信 realm，处理器无需复核；我方
        // prelude 与玩家共享 realm，原型迭代器污染可令门④扫描失明（A4-iii
        // 评审 2 必修）——结算侧唯一无兜底的门，在此按世界真值补齐：玩家
        // 已有生效中的 safeMode → 该玩家全部 ActivateSafeMode intent 丢弃
        //（官方 BUSY 拒 = intent 不记；检查时点同官方 API 调用时读上一 tick
        // 结束态，本 tick 新激活的时钟此刻尚未落库，跨房多开被结构性封死）
        let time = self.world.time;
        let mut safemode_holders: Vec<String> = self
            .world
            .rooms
            .values()
            .flat_map(|r| r.objects.values())
            .filter_map(|o| match o {
                RoomObject::Controller(c) if c.safe_mode > time => c.owner.clone(),
                _ => None,
            })
            .collect();
        safemode_holders.sort_unstable();
        safemode_holders.dedup();
        if !safemode_holders.is_empty() {
            for holder in &safemode_holders {
                if let Some(intents) = batch.users.get_mut(holder) {
                    for actions in intents.objects.values_mut() {
                        actions.remove(&crate::intent::Action::ActivateSafeMode);
                    }
                }
            }
        }
        let room_names: Vec<String> = self.world.rooms.keys().cloned().collect();
        for name in room_names {
            #[cfg(test)]
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let mut room = self.world.rooms.get(&name).expect("房间名来自键集").clone();
            let mut room_events =
                settle_room(&mut room, self.world.time, &mut next, &mut players, &batch)?;
            settled.push((name, room));
            events.append(&mut room_events);
        }
        for (name, room) in settled {
            self.world.rooms.insert(name, room);
        }
        // user.rooms 重建（官方不动点，见 PlayerState 文档）：claimController
        // 的 addRoomToUser（claimController.js:55）与每 tick 已占房重加
        // （controllers/tick.js:18）经 bulk addToSet 在 tick 末落盘——等价于
        // 结算完成后按世界真值（各房 controller.owner）重建集合；unclaim/降级
        // 到 0 的 removeRoomFromUser（unclaim.js:15 / tick.js:53）同被覆盖。
        for state in players.values_mut() {
            state.rooms.clear();
        }
        for room in self.world.rooms.values() {
            if let Some(owner) = room.objects.values().find_map(|o| match o {
                RoomObject::Controller(c) => c.owner.clone(),
                _ => None,
            }) && let Some(state) = players.get_mut(&owner)
            {
                state.rooms.insert(room.name.clone());
            }
        }
        self.players = players;
        self.next_id = next;
        self.world.bump_next_obj_id(next);
        self.last_events = events;
        self.world.advance_ticks(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两阶段回退（§6.5）：后房 Err 时先房的结算不得写回——time/能量/重生钟
    /// 全部保持上一 tick；故障解除后的首个成功 tick 才统一推进（否则先房会在
    /// 同一 time 上被重放结算——双倍采集）。
    #[test]
    fn engine_tick_rolls_back_all_rooms_on_failure() {
        let mut world = kk_world::World::new(1);
        for name in ["W1N1", "W2N2"] {
            world.register_room(name, kk_core::Terrain::plains());
            world
                .insert(
                    name,
                    RoomObject::Source(kk_schema::room::SourceObj {
                        id: 0,
                        x: 5,
                        y: 5,
                        energy: 1000,
                        energy_capacity: 3000,
                        next_regeneration_time: None,
                    }),
                )
                .unwrap();
        }
        let mut engine = Engine::new(world);
        engine.fail_room = Some("W2N2".into());
        assert!(engine.tick().is_err(), "注入房 Err ⇒ 整体 Err");
        assert_eq!(engine.world.time, 0, "time 不推进");
        for name in ["W1N1", "W2N2"] {
            match engine.world.room(name).unwrap().objects.values().next() {
                Some(RoomObject::Source(s)) => {
                    assert_eq!(s.energy, 1000, "{name}：能量未被动过");
                    assert_eq!(s.next_regeneration_time, None, "{name}：重生钟未挂");
                }
                _ => panic!("{name}：非 source"),
            }
        }
        // 故障解除后正常推进：两房同一 tick 挂钟（time=0 → 300）
        engine.fail_room = None;
        engine.tick().unwrap();
        assert_eq!(engine.world.time, 1);
        for name in ["W1N1", "W2N2"] {
            match engine.world.room(name).unwrap().objects.values().next() {
                Some(RoomObject::Source(s)) => {
                    assert_eq!(s.next_regeneration_time, Some(300), "{name}：挂钟")
                }
                _ => panic!("{name}：非 source"),
            }
        }
    }
}
