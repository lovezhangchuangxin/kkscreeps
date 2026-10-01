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
//! B3 起 death/decay 族落地（creep.rs 的 _die 全量——墓碑承接 + 死亡事件；
//! damage.rs 的 _destroy 全量——废墟承接；economy.rs 的 tombstones/ruins
//! tick；room.rs 的 destroyStructure；mod.rs 的 _create-energy 容器承接）。

pub mod controller;
pub mod creep;
pub mod damage;
pub mod economy;
pub mod market;
pub mod movement;
pub mod room;
pub mod spawn;
mod stages;
pub mod structures;

pub use stages::{RunState, TickStage};

use std::collections::{BTreeMap, BTreeSet};

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, RoomIndex, RoomState};
use serde_json::{Value, json};

use crate::intent::{Action, IntentBatch, IntentData};

/// 官方 utils.js:458-465 前奏（checkStructureAgainstController 的头三段，
/// M2B 扩面 P5 抽取共用）：调用方先做「无主恒 active」短路，本函数返回
/// None = controller 缺失 / level<1 / 异主（inactive）；Some = 通过的
/// controller（供配额段使用）。towers/links 全量版（structures.rs）与
/// extractor 特化（creep/harvest.rs）共用单源。
pub(crate) fn structure_rcl_prologue<'a>(
    w: &'a Working,
    owner: &str,
) -> Option<&'a kk_schema::room::ControllerObj> {
    let ctl = w.controller()?;
    if ctl.level < 1 || ctl.owner.as_deref() != Some(owner) {
        return None;
    }
    Some(ctl)
}

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
    /// 玩家持久 RawMemory（B0-5 统一提交点：玩家执行产出候选、世界结算成功
    /// 后在此提交；下一 tick 注入沙箱——官方每 tick 从存档恢复同构）。
    pub raw_memory: String,
    /// Game.notify 落库（M2B 扩面 P3）：官方 persist 到 users.notifications
    /// 集合（邮件通知，游戏内不可观测）；有界保留最近 100 条（官方集合
    /// 自然增长，登记差异）
    pub notifications: Vec<NotifyRecord>,
    /// RawMemory segments 全量存储（M2B 扩面 P6：官方 users.memorySegments
    /// 的 100 槽服务端存储；窗口写回在此合并）。有界：槽数 ≤100（id 0..99）、
    /// 每槽 ≤100KB（写回校验在 prelude tick 尾——官方 runtime.js:250-269 同位）
    pub segments: BTreeMap<u8, String>,
    /// 最近一次 setActiveSegments 请求（跨 tick 保持。官方闭包每 tick 重置、
    /// 持久在 users.activeSegments——本字段即官方 db 侧的等价物；空集=已请求
    /// 清空窗口，下一 tick 解算为空。评审 2 P2-2 勘正：非「官方闭包同形」）
    pub active_request: Vec<u8>,
    /// setPublicSegments（空列表官方 truthy-check 丢弃——no-op quirk 保持）
    pub public_ids: Vec<u8>,
    /// setDefaultPublicSegment（None = 未设/已清）
    pub default_public: Option<u8>,
    /// setActiveForeignSegment 请求（None = 未设/已清；Some((username, id?))
    /// ——id 缺省走发布者 defaultPublic；每 tick 重新解析授权——撤回即时生效）
    pub foreign_request: Option<(String, Option<u8>)>,
    /// 玩家 credits（2B 批）：**整数毫 credits**（官方 users.money 形态——
    /// `Game.market.credits` getter ÷1000，market.js:196-201）。种子 10M
    /// credits=10_000_000_000 毫（vanilla adapter index.ts:950 同款——
    /// 套件 INITIAL_CREDITS 锚）。
    pub money: i64,
    /// intershard 资源持有（2B 批）：官方 users.resources（INTERSHARD_RESOURCES
    /// 划转面——directDeals 结算消费，global-intents/market.js:420-435）。
    /// 缺省空：token/pixel 等来源面（generatePixel/unlock）归 MMO 订阅批，
    /// 本批只承载 deal 划转，不伪造来源。
    pub resources: BTreeMap<String, i64>,
}

/// 一条已落账的玩家通知（tick = 落账时的世界时间）。
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyRecord {
    pub message: String,
    pub group_interval: Option<i64>,
    pub tick: u64,
}

/// PlayerState.notifications 的容量（官方 users.notifications 集合自然
/// 增长，我方有界——登记差异）。
const NOTIFY_RECORD_CAP: usize = 100;

/// 一轮结算的非确定性输入（ADR-013 决策 5）：墙钟等外部输入显式注入——
/// 首次执行取实际值（[`TickInput::now`]，经 [`Engine::tick_with_intents`]
/// 的默认路径），重放用记录值（[`Engine::tick_with_intents_with_input`]）。
/// signController 的 `datetime` 落盘面由此获得重放确定性；玩家可见的墙钟
/// 语义不变（官方 signController.js:23 落一次 Date.now() 毫秒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickInput {
    /// Unix 毫秒墙钟。
    pub now_ms: u64,
}

impl TickInput {
    /// 首次执行的默认构造：取当前系统墙钟（时钟回拨异常时取 0，与原
    /// `SystemTime::now` 内联形态同口径）。
    pub fn now() -> Self {
        Self {
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

/// 结算工作上下文：房间工作副本 + 索引 + per-tick 瞬态。
pub struct Working<'w> {
    pub room: &'w mut RoomState,
    pub index: RoomIndex,
    pub time: u64,
    /// 本轮结算的墙钟输入（signController datetime 落盘面；ADR-013）。
    pub now_ms: u64,
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
    /// 本 tick 的提取器冷却瞬态（官方 `extractor._cooldown`，harvest.js:108
    /// 写、extractors/tick.js:21-25 同 tick 持久化为 cooldown 计数器——
    /// B4 的两文件瞬态通道，与 upgrade_block_pending 同族）。
    pub extractor_cooldown_pending: BTreeMap<ObjId, u64>,
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

    /// 掉落资源落地（官方 _create-energy：同格同资源叠加；容器承接 B3 补齐）。
    pub fn create_energy(&mut self, x: u8, y: u8, amount: i64) {
        self.create_energy_typed(x, y, amount, 0);
    }

    /// 资源参数化落地（resource_type 编码 0=energy/1=power/2=G——GPT 复审
    /// P1-1：官方 _create-energy 按资源类型键同格叠加，同格异资源不并堆）。
    pub fn create_energy_typed(&mut self, x: u8, y: u8, amount: i64, res_code: u8) {
        let name = match res_code {
            1 => "power",
            2 => kk_core::constants::RESOURCE_GHODIUM,
            _ => "energy",
        };
        self.create_resource(x, y, amount, name);
    }

    /// 官方 _create-energy.js（B3 全量形态）：任意资源名落地——同格 container
    /// （hits>0）先承接（容量 = CONTAINER_CAPACITY − 全槽合计，:17-27），余量
    /// 同格同资源并堆（:29-42）。tombstone/ruin 衰减掉落的 boost 矿物等经
    /// 命名槽承载（EnergyObj.resource）。
    pub fn create_resource(&mut self, x: u8, y: u8, amount: i64, resource: &str) {
        if amount <= 0 {
            return;
        }
        let amount = self.deposit_into_container(x, y, resource, amount);
        if amount <= 0 {
            return;
        }
        let existing = self.index.at(x as usize, y as usize).iter().find_map(|id| {
            match self.room.objects.get(id) {
                Some(RoomObject::Energy(e)) if energy_resource_name(e) == resource => Some(*id),
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
                let (res_code, named) = match resource {
                    kk_core::constants::RESOURCE_GHODIUM => (2u8, None),
                    other => (0u8, Some(other.to_string())),
                };
                self.insert_new(RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x,
                    y,
                    resource_type: res_code,
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
                    resource: named,
                }));
            }
        }
    }

    /// 官方 _create-energy.js:17-27 的容器承接分支：同格 container（hits>0）
    /// 先收，容量余量 = CONTAINER_CAPACITY − 全槽合计（calcResources）；返回
    /// 未承接的余量。container 在 typed 世界是 Generic 透传（结算范围外类型），
    /// store 读写走 data JSON。
    fn deposit_into_container(&mut self, x: u8, y: u8, resource: &str, amount: i64) -> i64 {
        let container = self
            .index
            .at(x as usize, y as usize)
            .iter()
            .find(|id| {
                self.room.objects.get(id).is_some_and(|o| {
                    matches!(o, RoomObject::Generic(_))
                        && movement::type_name_of(o) == "container"
                        && damage::object_hits(o) > 0
                })
            })
            .copied();
        let Some(cid) = container else {
            return amount;
        };
        let Some(RoomObject::Generic(g)) = self.room.objects.get_mut(&cid) else {
            return amount;
        };
        let mut data: Value = serde_json::from_str(&g.data).unwrap_or_default();
        // 读侧折零与 filter_map 跳过在和式中恒等（本行是口径统一）——真正的
        // 行为修复在下方写入侧：taken 已扣减而不入槽也不退回 = 资源静默灭失
        //（B3 评审 2 P3-2 + 清偿复审 P3-1 归因勘误）
        let total_before = data
            .pointer("/store")
            .and_then(Value::as_object)
            .map(|s| s.values().map(|v| v.as_i64().unwrap_or(0)).sum::<i64>())
            .unwrap_or(0);
        let headroom = kk_core::constants::CONTAINER_CAPACITY - total_before;
        let taken = amount.min(headroom);
        if taken <= 0 {
            return amount;
        }
        // 官方 `container.store = container.store || {}`（:19-20）：缺 store
        // 键的 container 现场建空对象后承接
        let Some(obj) = data.as_object_mut() else {
            return amount;
        };
        let store = obj.entry("store".to_string()).or_insert_with(|| json!({}));
        let Some(store) = store.as_object_mut() else {
            return amount;
        };
        let slot = store.entry(resource.to_string()).or_insert(json!(0));
        let v = slot.as_i64().unwrap_or(0);
        *slot = json!(v.saturating_add(taken));
        g.data = data.to_string();
        amount - taken
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

/// 官方 processor.js:313-314 的 notifyWhenAttacked 通用段：intent 存在且对象
/// 是 CONSTRUCTION_COST 类型（或 creep/powerCreep——后者 M4）→ 落库
/// `notifyWhenAttacked: !!enabled`。:248 的所有权门（他人对象跳过）内联。
/// 消费面是伤害通知（_damage.js:76 / towers/attack.js:63——M2 通知面未接线，
/// 本批只持久化标志，通知发送灰区登记）。
fn notify_when_attacked(w: &mut Working, player: &str, id: ObjId, data: &IntentData) {
    let Some(obj) = w.room.objects.get(&id) else {
        return;
    };
    if kk_world::domain::object_owner(obj).is_some_and(|owner| owner != player) {
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
        // M2B 扩面 P1：creep 同门（官方 :313 类型门显式收录 creep——processor
        // 通用段先于类型分派，对所有对象类型生效）
        Some(RoomObject::Creep(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Spawn(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extension(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Storage(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::ConstructedWall(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Road(o)) => o.notify_when_attacked = Some(enabled),
        // B4：extractor 在 CONSTRUCTION_COST（5）——官方 :313 类型门收录
        Some(RoomObject::Extractor(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Generic(o)) => {
            // 透传桶：标志写进 data JSON（投影层原样还原，prelude 的
            // rawData(id).notifyWhenAttacked 读到）
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or_default();
            v["notifyWhenAttacked"] = Value::Bool(enabled);
            o.data = v.to_string();
        }
        // creep 臂已在上方（M2B 扩面 P1——字段+API 面齐）；其余类型（工地/
        // 旗/controller/source 等）不在官方 :313 类型门内
        _ => {}
    }
}

/// EnergyObj 的资源名（code 0/energy、1/power、2/G 之外经命名槽——与
/// kk-world 投影侧的 world_json::energy_resource_name 同语义；跨 crate 不能
/// 复用 pub(crate)，此处为结算侧单源）。pub：settle_tests 锁值断言消费。
pub fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> &str {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => name.as_str(),
        (None, 1) => "power",
        (None, 2) => kk_core::constants::RESOURCE_GHODIUM,
        (None, _) => "energy",
    }
}

/// 结算一个房间（processor.js processRoom 的 M1 子集）。
///
/// 工作副本由调用方 clone（失败即回退的载体）；`batch` 只取指向本房间的部分；
/// `now_ms` 是本轮结算的墙钟输入（[`TickInput`]）。
pub fn settle_room(
    room: &mut RoomState,
    time: u64,
    now_ms: u64,
    next_id: &mut u64,
    players: &mut BTreeMap<String, PlayerState>,
    batch: &IntentBatch,
) -> Result<Vec<Value>, SettleError> {
    let index = RoomIndex::build(&room.objects);
    let mut w = Working {
        room,
        index,
        time,
        now_ms,
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
        extractor_cooldown_pending: BTreeMap::new(),
        movement: movement::Movement::new(),
    };

    // 阶段 1 预扫描（processor.js:43-171 的 M1 子集）：actionLog 每 tick 重置
    //（M2B 扩面 P1 起含 say 文本载荷两字段——官方 actionLog 整对象每 tick 重建）
    for creep in w.room.objects.values_mut() {
        if let RoomObject::Creep(c) = creep {
            c.action_log = 0;
            c.say_message = None;
            c.say_public = None;
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
                // Generic 结构对象（tower/link/rampart 等，M2B 扩面 P5）：
                // 官方 :248 所有权门 `object.user && object.user != userId`
                // ——有主不符跳过，**无主不跳**（与 controller 臂同款判定）。
                // 官方按 `object.type` 路由到 towers/links/ramparts 的
                // intents.js（:251-269）；我方 Generic 的 `type` 恒
                // "structure"，真实类型在 `structureType` 键——门与路由都在
                // 处理器内做（单次 data 解析）
                Some(RoomObject::Generic(_)) => {
                    structures::process_structure_intents(&mut w, *obj_id, player, actions);
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
    // 官方 lodash forEach 键快照同语义）。B3 补 tombstone/ruin（官方
    // processor.js:379-382 的两行分派——衰减到点掉落全量 store 并移除）
    let ids: Vec<ObjId> = w.room.objects.keys().copied().collect();
    for id in ids {
        match w.room.objects.get(&id) {
            Some(RoomObject::Energy(_)) => economy::energy_tick(&mut w, id),
            Some(RoomObject::Source(_)) => economy::source_tick(&mut w, id),
            Some(RoomObject::Creep(_)) => creep::creep_tick(&mut w, id)?,
            Some(RoomObject::Spawn(_)) => spawn::spawn_tick(&mut w, id, _energy_available),
            Some(RoomObject::Controller(_)) => economy::controller_tick(&mut w, id),
            Some(RoomObject::Tombstone(_)) => economy::tombstone_tick(&mut w, id),
            Some(RoomObject::Ruin(_)) => economy::ruin_tick(&mut w, id),
            // B4：minerals/tick.js（processor.js:356-357）与 extractors/tick.js
            //（processor.js:380-381）的 phase 8 分派
            Some(RoomObject::Mineral(_)) => economy::mineral_tick(&mut w, id),
            Some(RoomObject::Extractor(_)) => economy::extractor_tick(&mut w, id),
            // M2B 扩面 P4：结构衰减三件（官方 processor.js:365 rampart/
            // :369 road/:389 container 的 phase 8 分派；rampart 与 container
            // 走 Generic 透传桶）+ link 冷却计数器递减（:378 links/tick.js）。
            // 官方按 `object.type == '<t>'` 分派——我方 Generic 的 `type` 恒
            // "structure"，结构类型在 `structureType` 键（投影约定），故按后者取。
            Some(RoomObject::Road(_)) => economy::road_decay_tick(&mut w, id),
            Some(RoomObject::Generic(g)) => match generic_structure_type(&g.data).as_deref() {
                Some("container" | "rampart") => economy::generic_decay_tick(&mut w, id),
                Some("link") => economy::link_cooldown_tick(&mut w, id),
                // terminal/tick.js（2B 批 P1）：storeCapacity 按 RCL 归一
                Some("terminal") => economy::terminal_capacity_tick(&mut w, id),
                _ => {}
            },
            _ => {}
        }
    }

    // 官方 saveRoomEventLog（processor.js:497 → driver/lib/index.js:583-585 的
    // hset 整体覆写）：本 tick 事件数组随房持久化（快照 events 缓冲——玩家
    // 下一 tick 的 room.getEventLog 读到本 tick 全量）
    w.room.events = w.events.iter().map(Value::to_string).collect();

    Ok(w.events)
}

/// Generic 透传桶的 `structureType`（phase 8 分派用；缺键/畸形 JSON → None）。
/// 官方 DB 的结构 `type` 即结构类型，我方 Generic 的 `type` 恒 "structure"、
/// 真实类型在 `structureType` 键（与 world_json 投影、place/建造两入口同约定）。
fn generic_structure_type(data: &str) -> Option<String> {
    serde_json::from_str::<Value>(data)
        .ok()?
        .get("structureType")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 引擎：世界 + 玩家状态 + 每玩家沙箱，按固定序推进 tick。
pub struct Engine {
    pub world: kk_world::World,
    pub players: BTreeMap<String, PlayerState>,
    pub sandboxes: BTreeMap<String, Box<dyn crate::sandbox::Sandbox>>,
    /// 市场域态（2B 批 P0）：订单簿 + 交易流水——官方 market.orders/
    /// transactions 两 DB 集合的引擎态等价物（跨房世界级但非 kk-world
    /// 世界态，快照契约不动）。
    pub market: market::MarketState,
    next_id: u64,
    /// 最近一次 tick 的房间事件日志（按房间结算序拼接；M1 测试消费，M2 持久化）。
    pub last_events: Vec<serde_json::Value>,
    /// 测试钩子：命中名房的结算注入 Err，验证两阶段回退（双房批起对
    /// embed 集成测试开放——经 set_fail_room_for_test；生产不得触碰）。
    fail_room: Option<String>,
    /// 运行状态机（A1 批）：Ready/Committed=净态；NeedsRecovery=段失败后
    /// 待重试/放弃（世界保持上一 tick，期间拒绝外部变异后的推进）。
    run_state: stages::RunState,
    /// 测试钩子（A1）：指定段的**入口**注入 Err（持续语义，同 fail_room——
    /// 调用方管理清零；embed 级一次性包装 fail_next_settle_stage_for_test）。
    fail_stage: Option<TickStage>,
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
            market: market::MarketState::default(),
            next_id,
            last_events: Vec::new(),
            fail_room: None,
            run_state: stages::RunState::Ready,
            fail_stage: None,
        }
    }

    /// 测试面（双房批，#[doc(hidden)]）：注入/解除指定房的结算失败——
    /// 持续语义（命中即 Err，直至清 None），调用方管理清零。embed 级
    /// 一次性语义由 EmbedEngine::fail_next_settle_room_for_test 包装。
    #[doc(hidden)]
    pub fn set_fail_room_for_test(&mut self, room: Option<String>) {
        self.fail_room = room;
    }

    /// 当前运行状态（A1 批状态机观测面）。
    pub fn run_state(&self) -> &RunState {
        &self.run_state
    }

    /// 测试钩子（A1）：指定段入口注入 Err（持续语义——命中即 Err 直至清
    /// None；embed 一次性语义由 EmbedEngine::fail_next_settle_stage_for_test
    /// 包装）。注入在段入口生效：该段尚未变异任何权威状态。
    #[doc(hidden)]
    pub fn set_fail_stage_for_test(&mut self, stage: Option<TickStage>) {
        self.fail_stage = stage;
    }

    /// 显式放弃恢复（embed 的 discard_pending_round 同步面）：仅当处于
    /// NeedsRecovery 且世界仍保持失败轮原样时回到 Ready——以当前已提交
    /// 状态为新基线。
    pub fn abandon_recovery(&mut self) -> Result<(), String> {
        match &self.run_state {
            RunState::NeedsRecovery {
                tick, generation, ..
            } if self.world.time() == *tick && self.world.generation() == *generation => {
                self.run_state = RunState::Ready;
                Ok(())
            }
            RunState::NeedsRecovery {
                tick, generation, ..
            } => Err(format!(
                "放弃恢复被拒：世界已被外部变异（失败轮 tick={tick}, gen={generation}；当前 tick={}, gen={}）",
                self.world.time(),
                self.world.generation()
            )),
            _ => Err("无需恢复（不在 NeedsRecovery 态）".to_string()),
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
                let mut result =
                    sandbox.run(&view, &crate::sandbox::RunSpec::new(player.clone(), ""));
                // 结局门（B0-4 评审清偿）：只有正常执行的 intents 进结算批——
                // Terminated（硬中断/停跑丢弃 intents 的真路径语义在消费侧
                // 兜底）、InfraFailure（基础设施故障不静默当作空意图正常轮）。
                if !matches!(result.outcome, crate::sandbox::RunOutcome::Executed) {
                    continue;
                }
                // 身份边界：intent 只归本沙箱注册的玩家（结算按玩家键判所有权，
                // 冒名键直接丢弃——真沙箱接入后这是防跨玩家操作的硬不变量）
                if let Some(intents) = result.intents.users.get_mut(player) {
                    let entry = batch.users.entry(player.clone()).or_default();
                    entry.objects.extend(std::mem::take(&mut intents.objects));
                    entry.room.extend(std::mem::take(&mut intents.room));
                    // 全局通道（notify）同取（评审 3 P2-2：两处合一点只更其一
                    //——extend_from 已对齐，此处漏则假沙箱路径 notify 静默丢）
                    entry.notify.extend(std::mem::take(&mut intents.notify));
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
    ///
    /// 首跑默认路径：墙钟输入取 [`TickInput::now`] 实际值（落盘后即本轮记录值）；
    /// 重放/一致性测试走 [`Engine::tick_with_intents_with_input`] 注入记录值。
    pub fn tick_with_intents(&mut self, batch: IntentBatch) -> Result<(), SettleError> {
        self.tick_with_intents_with_input(batch, TickInput::now())
    }

    /// [`Engine::tick_with_intents`] 的重放形态：非确定性输入（墙钟）由调用方
    /// 显式注入——同初始世界 + 同 intents + 同 `TickInput` 的两次结算产物
    /// 逐字节一致（ADR-013 决策 5 的验收面）。
    ///
    /// A1 批起为分段驱动（stages.rs）：prepare → settle → market → validate
    /// → publish，段入口可注入故障；失败落 [`RunState::NeedsRecovery`] 并
    /// 保持全部权威状态（世界/玩家候选/市场快照桥）在上一 tick。
    pub fn tick_with_intents_with_input(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
    ) -> Result<(), SettleError> {
        // 状态机门（A1）：NeedsRecovery 只放行「世界保持失败轮原样」的重试
        //（同候选重结算）；期间任何外部变异（房间/地形/对象增删均 bump 代际）
        // 都会使旧候选重结算到已变世界——先显式恢复或放弃，否则拒绝推进
        match &self.run_state {
            RunState::Ready | RunState::Committed { .. } => {}
            RunState::NeedsRecovery {
                tick, generation, ..
            } => {
                if self.world.time() != *tick || self.world.generation() != *generation {
                    return Err(SettleError(format!(
                        "恢复前世界已被外部变异（失败轮 tick={tick}, gen={generation}；当前 tick={}, gen={}）",
                        self.world.time(),
                        self.world.generation()
                    )));
                }
            }
            // 同步引擎内单调用瞬态——跨调用见到即状态机接线 bug（panic 中断
            // 的半成品轮：世界可能已部分推进，拒绝继续是唯一安全选择）
            RunState::Running { .. } | RunState::Prepared { .. } => {
                return Err(SettleError(
                    "状态机接线错误：Running/Prepared 不得跨调用存在".into(),
                ));
            }
        }
        self.run_state = RunState::Running {
            tick: self.world.time(),
        };
        let mut cursor = stages::StageCursor::new(self.fail_stage);
        let result = self.staged_body(&mut cursor, batch, &input);
        match result {
            Ok(()) => {
                self.run_state = RunState::Committed {
                    tick: self.world.time(),
                };
                Ok(())
            }
            Err(e) => {
                // 市场候选桥（A1→A2 过渡）：市场段之后的失败恢复市场快照——
                // 「任一段失败 ⇒ 全部权威状态保持上一 tick」对市场域同样成立
                if let Some(snapshot) = cursor.take_market_snapshot() {
                    self.market = snapshot;
                }
                self.run_state = RunState::NeedsRecovery {
                    tick: self.world.time(),
                    generation: self.world.generation(),
                    stage: cursor.stage(),
                    reason: e.to_string(),
                };
                Err(e)
            }
        }
    }

    /// 分段主体（A1）：prepare → settle → market → validate → publish。
    /// 失败返回时由调用方（状态机驱动）负责市场快照恢复与 NeedsRecovery
    /// 落账——本函数自身不写 run_state（Prepared 观察点除外）。
    fn staged_body(
        &mut self,
        cursor: &mut stages::StageCursor,
        batch: IntentBatch,
        input: &TickInput,
    ) -> Result<(), SettleError> {
        cursor.enter(TickStage::Prepare)?;
        let mut cand = self.prepare_tick(batch)?;
        cursor.enter(TickStage::Settle)?;
        let mut settled = self.settle_rooms_stage(&mut cand, input)?;
        cursor.enter(TickStage::Market)?;
        // 市场候选桥：settle_market 仍直写 self.market（不可失败——官方
        // global 无 throw 路径的镜像语义，A2 改真候选），先落快照，其后
        // 任一段失败由驱动统一恢复。成本注记：快照为每 tick 无条件
        // MarketState 克隆（订单簿 + ≤上限流水），A2 真候选化时撤销——
        // 当前订单簿规模下可忽略（bot/套件负载无订单）
        cursor.snapshot_market(&self.market);
        market::settle_market(
            &mut settled.rooms,
            &mut cand.players,
            &mut self.market,
            &cand.batch,
            cand.time,
            input.now_ms,
        );
        cursor.enter(TickStage::Validate)?;
        self.validate_stage(&cand, &settled)?;
        // 候选完成且通过校验——publish 前的持久观察点
        self.run_state = RunState::Prepared { tick: cand.time };
        cursor.enter(TickStage::Publish)?;
        self.publish_tick(cand, settled);
        Ok(())
    }

    /// prepare 段：输入规整与候选域构建（不产生任何权威状态变异）。
    fn prepare_tick(&mut self, batch: IntentBatch) -> Result<stages::TickCandidates, SettleError> {
        let next = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let players = self.players.clone();
        let mut batch = batch;
        // 门④可信侧兜底（官方 API 层 structures.js:223-225 的等价物——"玩家
        // 名下任一 controller safeMode 生效中 → 新 activateSafeMode 全拒"）。
        // 官方 game 层跑在 isolated-vm 可信 realm，处理器无需复核；我方
        // prelude 与玩家共享 realm，原型迭代器污染可令门④扫描失明（A4-iii
        // 评审 2 必修）——结算侧唯一无兜底的门，在此按世界真值补齐：玩家
        // 已有生效中的 safeMode → 该玩家全部 ActivateSafeMode intent 丢弃
        //（官方 BUSY 拒 = intent 不记；检查时点同官方 API 调用时读上一 tick
        // 结束态，本 tick 新激活的时钟此刻尚未落库，跨房多开被结构性封死）
        let time = self.world.time();
        let mut safemode_holders: Vec<String> = self
            .world
            .rooms()
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
        let room_names: Vec<String> = self.world.rooms().keys().cloned().collect();
        Ok(stages::TickCandidates {
            time,
            room_names,
            next,
            players,
            batch,
        })
    }

    /// settle 段：逐房两阶段候选结算（clone → settle_room → 收集；全部
    /// 成功前不写回——§6.5）。
    fn settle_rooms_stage(
        &mut self,
        cand: &mut stages::TickCandidates,
        input: &TickInput,
    ) -> Result<stages::SettledRooms, SettleError> {
        let mut events = Vec::new();
        let mut rooms: Vec<(String, RoomState)> = Vec::new();
        for name in cand.room_names.clone() {
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let mut room = self.world.room(&name).expect("房间名来自键集").clone();
            let mut room_events = settle_room(
                &mut room,
                cand.time,
                input.now_ms,
                &mut cand.next,
                &mut cand.players,
                &cand.batch,
            )?;
            rooms.push((name, room));
            events.append(&mut room_events);
        }
        Ok(stages::SettledRooms { rooms, events })
    }

    /// validate 段（A1）：候选结构 tripwire——违例形态只可能来自引擎 bug
    ///（正常路径结构上不可能触达），失败等同段故障进入 NeedsRecovery。
    /// 读写纪律：只读（世界与候选均不变异）。
    fn validate_stage(
        &self,
        cand: &stages::TickCandidates,
        settled: &stages::SettledRooms,
    ) -> Result<(), SettleError> {
        // ① 房间候选覆盖：settle 段按名序逐房 push——名字序列与候选房间集
        // 必须逐位一致（缺房/错序 = settle 段装配 bug）
        if settled.rooms.len() != cand.room_names.len()
            || settled
                .rooms
                .iter()
                .zip(&cand.room_names)
                .any(|((settled_name, _), want)| settled_name != want)
        {
            return Err(SettleError(format!(
                "候选房间覆盖不完整：期望 {} 间，实得 {} 间",
                cand.room_names.len(),
                settled.rooms.len()
            )));
        }
        // ② id 分配单调：候选 next 不得低于入口基线（引擎侧与世界侧取大；
        // 世界在候选构建期间不变异，重算即入口值）
        let base = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        if cand.next < base {
            return Err(SettleError(format!(
                "id 候选回退：{} < 基线 {base}",
                cand.next
            )));
        }
        // ③ 玩家候选键不减：结算只会新增（notify 落账的 or_default），不得丢键
        if let Some(missing) = self.players.keys().find(|k| !cand.players.contains_key(*k)) {
            return Err(SettleError(format!("玩家候选丢键：{missing}")));
        }
        Ok(())
    }

    /// publish 段：一次发布（不可失败——各步均为内存写回，失败只可能来自
    /// 注入钩子且钩子在段入口先行）。
    fn publish_tick(
        &mut self,
        mut cand: stages::TickCandidates,
        mut settled: stages::SettledRooms,
    ) {
        // 市场全局阶段（2B 批 P1）：官方 main.js:63-65 的 global 阶段在房间
        // 结算全部完成后运行（processor/global.js:48-49）——我方在 commit 前
        // 对已结算工作副本执行：读到的都是本 tick 结算后状态（官方 commit1
        // 后读 db 的因果序等价），且失败原子性与「一次成功 tick ⇒ generation
        // 恰 +1」双不变量原样保持（settle_market 不可失败——官方 global 无
        // throw 路径，逐 intent 静默跳过同构）——已在 staged_body 的 Market
        // 段先行，此处仅保留发布序注释锚。
        for (name, room) in settled.rooms.drain(..) {
            self.world.commit_room(name, room);
        }
        // Game.notify 落库（M2B 扩面 P3，全局通道）：与房结算正交——房循环
        // 外每玩家恰一次；官方 groupInterval 是发送端聚合提示（送达面不在
        // 引擎），原样保留
        for (player, intents) in &cand.batch.users {
            if intents.notify.is_empty() {
                continue;
            }
            let state = cand.players.entry(player.clone()).or_default();
            for n in &intents.notify {
                state.notifications.push(NotifyRecord {
                    message: n.message.clone(),
                    group_interval: n.group_interval,
                    tick: cand.time,
                });
            }
            let len = state.notifications.len();
            if len > NOTIFY_RECORD_CAP {
                state.notifications.drain(0..len - NOTIFY_RECORD_CAP);
            }
        }
        // user.rooms 重建（官方不动点，见 PlayerState 文档）：claimController
        // 的 addRoomToUser（claimController.js:55）与每 tick 已占房重加
        // （controllers/tick.js:18）经 bulk addToSet 在 tick 末落盘——等价于
        // 结算完成后按世界真值（各房 controller.owner）重建集合；unclaim/降级
        // 到 0 的 removeRoomFromUser（unclaim.js:15 / tick.js:53）同被覆盖。
        for state in cand.players.values_mut() {
            state.rooms.clear();
        }
        for room in self.world.rooms().values() {
            if let Some(owner) = room.objects.values().find_map(|o| match o {
                RoomObject::Controller(c) => c.owner.clone(),
                _ => None,
            }) && let Some(state) = cand.players.get_mut(&owner)
            {
                state.rooms.insert(room.name.clone());
            }
        }
        self.players = cand.players;
        self.next_id = cand.next;
        self.world.bump_next_obj_id(self.next_id);
        self.last_events = settled.events;
        self.world.advance_ticks(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 两阶段回退（§6.5）：后房 Err 时先房的结算不得写回——time/能量/重生钟
    /// 全部保持上一 tick；故障解除后的首个成功 tick 才统一推进（否则先房会在
    /// 同一 time 上被重放结算——双倍采集）。
    /// M2B 前置批 P2：一次成功 tick ⇒ 世界内容代际恰 +1（提交尾 rooms 直写
    /// 经 advance_ticks 收束——P3/P5 投影缓存正确性依赖的不变量）；失败
    /// tick ⇒ +0（世界未变，两阶段回退）。
    #[test]
    fn engine_tick_bumps_generation_exactly_once() {
        let mut world = kk_world::World::new(1);
        world.register_room("W1N1", kk_core::Terrain::plains());
        let base = world.generation();
        let mut engine = Engine::new(world);
        engine.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(engine.world.generation(), base + 1, "一次成功 tick 恰 +1");
        engine.fail_room = Some("W1N1".into());
        let before = engine.world.generation();
        assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
        assert_eq!(engine.world.generation(), before, "失败 tick 不 bump");
    }

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
        assert_eq!(engine.world.time(), 0, "time 不推进");
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
        assert_eq!(engine.world.time(), 1);
        for name in ["W1N1", "W2N2"] {
            match engine.world.room(name).unwrap().objects.values().next() {
                Some(RoomObject::Source(s)) => {
                    assert_eq!(s.next_regeneration_time, Some(300), "{name}：挂钟")
                }
                _ => panic!("{name}：非 source"),
            }
        }
    }

    /// A1 状态机：Ready → Running →（段）→ Prepared → Committed 的净态观察
    /// + NeedsRecovery 的重试/放弃两条出路 + 外部变异门。
    #[test]
    fn stage_machine_transitions_recovery_and_mutation_gate() {
        let mut world = kk_world::World::new(1);
        world.register_room("W1N1", kk_core::Terrain::plains());
        let mut engine = Engine::new(world);
        assert_eq!(engine.run_state(), &RunState::Ready);
        engine.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(
            engine.run_state(),
            &RunState::Committed { tick: 1 },
            "成功轮落 Committed（可直接开下一轮）"
        );
        // Settle 段失败 → NeedsRecovery；世界未变 ⇒ 重试放行（注入仍在则再失败）
        engine.set_fail_stage_for_test(Some(TickStage::Settle));
        assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
        assert!(matches!(
            engine.run_state(),
            RunState::NeedsRecovery {
                stage: TickStage::Settle,
                tick: 1,
                ..
            }
        ));
        assert!(
            engine.tick_with_intents(IntentBatch::default()).is_err(),
            "注入仍在 ⇒ 重试再失败（世界未变 ⇒ 门放行）"
        );
        engine.set_fail_stage_for_test(None);
        engine.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(engine.run_state(), &RunState::Committed { tick: 2 });
        // 放弃恢复：净态拒绝；NeedsRecovery（世界未变）→ Ready 后可正常推进
        assert!(engine.abandon_recovery().is_err(), "净态无需恢复");
        engine.set_fail_stage_for_test(Some(TickStage::Validate));
        assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
        engine.set_fail_stage_for_test(None);
        engine.abandon_recovery().unwrap();
        assert_eq!(engine.run_state(), &RunState::Ready);
        engine.tick_with_intents(IntentBatch::default()).unwrap();
        assert_eq!(engine.world.time(), 3);
        // 外部变异门：NeedsRecovery 后世界被变异 ⇒ 重试与放弃都被拒
        engine.set_fail_stage_for_test(Some(TickStage::Publish));
        assert!(engine.tick_with_intents(IntentBatch::default()).is_err());
        engine.set_fail_stage_for_test(None);
        let g = engine.world.generation();
        engine
            .world
            .register_room("W9N9", kk_core::Terrain::plains());
        assert_ne!(
            engine.world.generation(),
            g,
            "夹具前提：register_room bump 代际（否则变异门测试失真）"
        );
        let err = engine
            .tick_with_intents(IntentBatch::default())
            .unwrap_err();
        assert!(err.to_string().contains("外部变异"), "{err}");
        assert!(engine.abandon_recovery().is_err(), "变异后放弃恢复同样被拒");
    }

    /// A1 判据：任一段入口注入失败 ⇒ 世界/玩家/市场全部保持上一 tick。
    /// 市场域判别力自证：同 fixture 无注入的成功轮确实消费订单（过期退款
    /// 变异订单簿与 money，MARKET-ORDER-009 尾段），注入轮的不变断言才有
    /// 杀伤力；Validate/Publish 注入发生在 settle_market **之后**，市场域
    /// 不变靠快照桥（A1→A2 过渡）兑现。
    #[test]
    fn stage_injection_keeps_world_and_market_at_previous_tick() {
        let make = || {
            let mut world = kk_world::World::new(1);
            world.register_room("W1N1", kk_core::Terrain::plains());
            let mut engine = Engine::new(world);
            engine.players.entry("p1".to_string()).or_default().money = 0;
            let id = engine.market.next_order_id();
            engine.market.insert_order(market::MarketOrder {
                id,
                user: "p1".to_string(),
                active: true,
                order_type: "sell".to_string(),
                resource_type: "energy".to_string(),
                price: 1000,
                amount: 0,
                remaining_amount: 100,
                total_amount: 100,
                room_name: Some("W1N1".to_string()),
                created: 0,
                // createdTimestamp+30d 过期：真实墙钟（TickInput::now）下
                // 上古时间戳必过期——退款变异 players.money + 订单簿
                created_timestamp: 0,
            });
            engine
        };
        // 判别力自证：无注入成功轮——订单被过期退款消费、money 入账
        let mut probe = make();
        probe.tick_with_intents(IntentBatch::default()).unwrap();
        assert!(
            probe.market.orders.is_empty(),
            "成功轮订单过期移除（fixture 判别力前提）"
        );
        assert!(probe.players["p1"].money > 0, "成功轮退款入账");
        // 注入轮：五段逐一——世界（time/代际/事件）、玩家 money、市场订单簿
        // 全部保持上一 tick
        for stage in [
            TickStage::Prepare,
            TickStage::Settle,
            TickStage::Market,
            TickStage::Validate,
            TickStage::Publish,
        ] {
            let mut engine = make();
            let (t0, g0) = (engine.world.time(), engine.world.generation());
            engine.set_fail_stage_for_test(Some(stage));
            let err = engine
                .tick_with_intents(IntentBatch::default())
                .unwrap_err();
            assert!(err.to_string().contains(stage.label()), "{stage:?}: {err}");
            assert_eq!(
                (engine.world.time(), engine.world.generation()),
                (t0, g0),
                "{stage:?}: 世界保持上一 tick（time/代际均不动）"
            );
            assert!(engine.last_events.is_empty(), "{stage:?}: 事件不落账");
            assert_eq!(
                engine.market.orders.len(),
                1,
                "{stage:?}: 订单簿保持（市场快照桥）"
            );
            assert!(
                engine.market.transactions.is_empty(),
                "{stage:?}: 交易流水保持"
            );
            assert_eq!(engine.players["p1"].money, 0, "{stage:?}: money 保持");
            assert!(
                matches!(engine.run_state(),
                    RunState::NeedsRecovery { stage: s, .. } if *s == stage),
                "{stage:?}: NeedsRecovery 记录失败段"
            );
            // 世界未变异 ⇒ 重试放行；注入解除后同候选重试成功 → Committed；
            // 闭环：回退后重试恰一次消费订单（无双退款/双移除——快照恢复 +
            // TickInput 钉死 ⇒ 重放同结果的显式断言）
            engine.set_fail_stage_for_test(None);
            engine.tick_with_intents(IntentBatch::default()).unwrap();
            assert!(
                matches!(engine.run_state(), RunState::Committed { tick: 1 }),
                "{stage:?}: 恢复后 Committed"
            );
            assert!(
                engine.market.orders.is_empty(),
                "{stage:?}: 重试恰一次消费（订单移除不重复）"
            );
            assert!(
                engine.players["p1"].money > 0,
                "{stage:?}: 重试恰一次退款入账"
            );
        }
    }

    /// validate 段 tripwire（A1）：正常路径结构上不可达——以篡改候选证明
    /// 三道闸（房间覆盖/id 单调/玩家键不减）各自有判别力。
    #[test]
    fn validate_stage_tripwires_discriminate() {
        let mut world = kk_world::World::new(1);
        world.register_room("W1N1", kk_core::Terrain::plains());
        world.register_room("W2N2", kk_core::Terrain::plains());
        world
            .insert(
                "W1N1",
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
        let mut engine = Engine::new(world);
        engine.players.entry("p1".to_string()).or_default();
        let mk_cand = |next| stages::TickCandidates {
            time: 0,
            room_names: vec!["W1N1".to_string(), "W2N2".to_string()],
            next,
            players: engine.players.clone(),
            batch: IntentBatch::default(),
        };
        let settled_full = stages::SettledRooms {
            rooms: vec![
                (
                    "W1N1".to_string(),
                    engine.world.room("W1N1").unwrap().clone(),
                ),
                (
                    "W2N2".to_string(),
                    engine.world.room("W2N2").unwrap().clone(),
                ),
            ],
            events: Vec::new(),
        };
        // 对照：完整候选 → Ok
        engine
            .validate_stage(&mk_cand(engine.next_id), &settled_full)
            .unwrap();
        // ① 缺一间
        let settled_short = stages::SettledRooms {
            rooms: vec![(
                "W1N1".to_string(),
                engine.world.room("W1N1").unwrap().clone(),
            )],
            events: Vec::new(),
        };
        let err = engine
            .validate_stage(&mk_cand(engine.next_id), &settled_short)
            .unwrap_err();
        assert!(err.to_string().contains("覆盖"), "{err}");
        // ①b 错序（zip 逐位比较——若被改成集合比较，此臂红）
        let settled_swapped = stages::SettledRooms {
            rooms: vec![
                (
                    "W2N2".to_string(),
                    engine.world.room("W2N2").unwrap().clone(),
                ),
                (
                    "W1N1".to_string(),
                    engine.world.room("W1N1").unwrap().clone(),
                ),
            ],
            events: Vec::new(),
        };
        let err = engine
            .validate_stage(&mk_cand(engine.next_id), &settled_swapped)
            .unwrap_err();
        assert!(err.to_string().contains("覆盖"), "错序同为覆盖违例: {err}");
        // ② id 回退（世界已有 id 0 ⇒ 基线 ≥ 1 > 0）
        let err = engine
            .validate_stage(&mk_cand(0), &settled_full)
            .unwrap_err();
        assert!(err.to_string().contains("回退"), "{err}");
        // ③ 玩家丢键
        let mut cand = mk_cand(engine.next_id);
        cand.players.remove("p1");
        let err = engine.validate_stage(&cand, &settled_full).unwrap_err();
        assert!(err.to_string().contains("丢键"), "{err}");
    }
}
