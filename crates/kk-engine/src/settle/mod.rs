//! 房间规则结算与世界 tick 的协调。
//!
//! 房间先处理指令，再仲裁移动，最后按对象 ID 执行 tick 规则。
//! Engine 在副本中结算所有房间和市场，校验成功后统一写回内存并推进
//! 时间；直接调用 settle_room 则由调用方负责副本和失败处理。

pub mod contrib;
pub mod controller;
pub mod creep;
pub mod damage;
pub mod economy;
pub mod inter_room;
pub mod market;
mod mod_intents;
pub mod movement;
pub mod room;
pub mod spawn;
mod stages;
pub mod structures;
mod txn;
mod writeback;

pub use stages::{RunState, TickStage};
pub use txn::TickState;
pub use writeback::{PlayerWriteback, SegmentWriteback, WritebackStep};

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use kk_schema::room::{CreepObj, RoomObject};
use kk_world::{ObjId, RoomIndex, RoomState};
use serde_json::{Value, json};

use crate::intent::{Action, IntentBatch, IntentData};

/// 检查控制器是否存在、等级是否至少为 1，以及是否属于结构的主人。
/// 无主结构是否启用由调用方决定；本函数不检查结构数量配额。
/// 来源：上游 engine/src/utils.js 的 checkStructureAgainstController。
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

/// 查询给定 RCL 允许的结构数量；未知类型或等级返回 0。
pub fn controller_allowed(structure_type: &str, rcl: i64) -> i64 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<serde_json::Value> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str(kk_core::constants::CONTROLLER_STRUCTURES)
            .expect("CONTROLLER_STRUCTURES 常量必为合法 JSON（kk-core 生成物测试锁定）")
    });
    table[structure_type][rcl.to_string()].as_i64().unwrap_or(0)
}

/// 规则结算错误。Engine 丢弃候选；直接调用 settle_room 的调用方须自行丢弃副本。
#[derive(Debug)]
pub struct SettleError(pub String);

impl std::fmt::Display for SettleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "结算失败: {}", self.0)
    }
}

/// 跨 tick 保留的玩家内存状态。
///
/// rooms 在全部房间结算后按控制器所有权重建；结算期间仍是上一 tick 的集合。
/// 因此同 tick 的后一次 claim 看不到前一次新占的房间。
/// 来源：上游 engine/src/processor/intents/creeps/claimController.js
/// 与 driver/lib/index.js 的 addRoomToUser（排队写入，未修改运行中的用户对象）。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PlayerState {
    /// 已提交 CPU 桶；执行预算与计费候选从此值派生，随世界发布。
    pub cpu: kk_core::cpu::Bucket,
    /// 累计 GCL 进度，不是玩家 API 显示的 GCL 等级。
    pub gcl: i64,
    pub rooms: BTreeSet<String>,
    /// 玩家 RawMemory。运行结果在候选中合并，随世界发布；下一次执行时注入。
    pub raw_memory: String,
    /// Game.notify 的内存记录，跨 tick 保留最近 100 条；这里不发送邮件。
    pub notifications: Vec<NotifyRecord>,
    /// RawMemory 段内容。执行器限制 ID 为 0..=99，每段最多 100×1024 个
    /// UTF-16 码元（JavaScript 字符串 length）。Map 不自行校验；
    /// PlayerWriteback 在候选中合并段内容及请求。
    pub segments: BTreeMap<u8, String>,
    /// 最近一次 setActiveSegments 请求，跨 tick 保留；空集表示空活动窗口。
    pub active_request: Vec<u8>,
    /// 公开段 ID。正常执行通道不提交空列表，已有公开列表因而保留。
    /// 直接由宿主修改此字段不会经过该请求规则。
    pub public_ids: Vec<u8>,
    /// 默认公开段 ID；None 表示未设置或已清除。
    pub default_public: Option<u8>,
    /// 外部段请求：发布者与可选 ID；省略 ID 时使用其默认公开段。
    /// kk-runtime 每轮按当前公开列表重新授权，撤回公开后停止提供数据。
    pub foreign_request: Option<(String, Option<u8>)>,
    /// 余额，单位千分之一 credit；玩家 API 读取时除以 1000。
    /// Default 的余额为 0，初始资金由调用方设置。
    pub money: i64,
    /// token、pixel 等跨 shard 资源余额，供市场成交划转；缺省为空。
    pub resources: BTreeMap<String, i64>,
}

/// 一条已提交的内存通知；tick 是处理指令时的世界时间。
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyRecord {
    pub message: String,
    pub group_interval: Option<i64>,
    pub tick: u64,
}

/// 内存通知最多保留最近 100 条。
const NOTIFY_RECORD_CAP: usize = 100;

/// 本轮结算的外部输入；控制器签名与市场订单时间使用此墙钟。
/// 重试或重放须复用首次尝试的值，由调用方保存并注入。
/// 来源：ADR-013 的非确定性输入约定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickInput {
    /// Unix 毫秒墙钟。
    pub now_ms: u64,
}

impl TickInput {
    /// 读取当前 Unix 时间；系统时间早于 Unix 纪元时返回 0。
    pub fn now() -> Self {
        Self {
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
}

/// 一次结算携带的贡献平面快照：有效规则、槽执行表、扩展字段目录与
/// 冻结计划本体。由引擎装配时生成，结算全程只读；四个成员必须来自
/// 同一冻结计划。
///
/// 三个派生物持 `Rc`/`Arc` 而非借用：整槽宿主在本次调用期间通过
/// OpState 持有候选句柄，返回时移除；借用栈上平面无法进入该拥有状态。
///
/// 克隆成本：两次非原子 `Rc` 递增 + 一次原子 `Arc` 递增 + 120B
/// `LinkRules` memcpy，每房每 tick 一次，无深拷贝。
#[derive(Clone)]
pub struct Plane {
    pub links: kk_mod_api::builtin::LinkRules,
    pub contrib: Rc<contrib::ContributionDispatch>,
    pub extensions: Rc<kk_mod_api::extension::ExtensionCatalog>,
    /// 冻结计划：意图描述符与投影类型目录经它解析（键解析发生在
    /// 装配期以外此处，是扩展意图的低频路径）。
    pub plan: Arc<kk_mod_api::ContributionPlan>,
}

/// 单房间结算上下文：房间副本、位置索引及共享候选状态，
/// 以及本轮的事件、伤害与移动记录。
///
/// 房间按值持有而非借用，候选可以在一次宿主调用期间进入 OpState。
/// 宿主返回后移除句柄；native op 只取短借用，候选不跨 tick 留存。
pub struct Working {
    pub room: RoomState,
    pub index: RoomIndex,
    pub time: u64,
    /// Unix 毫秒墙钟，来自本轮 TickInput。
    pub now_ms: u64,
    pub state: TickState,
    pub events: Vec<Value>,
    /// 处理 move 时记录的疲劳，供移动仲裁判断是否能移动。
    pub old_fatigue: BTreeMap<ObjId, u32>,
    /// 本轮疲劳累加值；保留负值用于拉拽传递，写回对象时取 max(0, 值)。
    pub tick_fatigue: BTreeMap<ObjId, i64>,
    /// 本轮各控制器的累计升级量，用于限制 RCL 8 的升级。
    pub upgraded: BTreeMap<ObjId, i64>,
    /// 指令阶段累计的 creep 伤害，在该 creep 的 tick 阶段施加。
    pub damage_to_apply: BTreeMap<ObjId, i64>,
    /// 待施加的治疗量，与伤害一起在 creep 死亡判断前处理。
    pub heal_to_apply: BTreeMap<ObjId, i64>,
    /// attackController 设置的升级封锁到期 tick，由 controller tick 写回对象。
    pub upgrade_block_pending: BTreeMap<ObjId, u64>,
    /// 本轮激活安全模式的控制器，controller tick 据此处理时钟。
    pub safe_mode_activated: BTreeSet<ObjId>,
    /// 采矿时设置的提取器冷却，由 extractor tick 写回冷却计数。
    pub extractor_cooldown_pending: BTreeMap<ObjId, u64>,
    pub movement: movement::Movement,
    /// 本 tick 登记的跨房过渡：位于边缘格的 creep 在全部房间结算后由
    /// 引擎阶段移入邻房候选。
    pub pending_inter_room: Vec<inter_room::InterRoomMove>,
    /// 本轮生效的贡献平面：link 规则、槽执行表与扩展字段目录。
    pub plane: Plane,
}

/// 共享的房间候选句柄。进入 JS 或执行 Rust next 前必须释放可变借用；
/// 该纪律由 RefCell 动态检查，类型不会自动证明。宿主在调用期间可以
/// 放入 OpState，但返回前必须清空；处理器不得跨调用留存候选句柄。
pub type RoomCtx = Rc<RefCell<Working>>;

impl Working {
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

    /// 写入对象扩展字段的候选值。先解析对象类型（目前仅 Generic 结构
    /// 按 structureType 映射），再校验字段键合法、已在目录中声明，
    /// 最后按字段 schema 校验值（类型、取值域、长度限额、有限浮点）。
    /// 写入只修改本房候选，结算失败时随房间候选一并丢弃。
    pub fn set_extension(
        &mut self,
        id: ObjId,
        field: &str,
        value: serde_json::Value,
    ) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        let Some(rule) = self.plane.extensions.field(&type_key, &field_key) else {
            return Err(format!("扩展字段未声明：{field}"));
        };
        kk_mod_api::validate::check_value(&value, &rule.schema, field)?;
        self.room
            .extensions
            .entry(id)
            .or_default()
            .insert(field.to_string(), value);
        Ok(())
    }

    /// 移除对象的扩展字段候选；字段未在目录声明时拒绝。
    pub fn remove_extension(&mut self, id: ObjId, field: &str) -> Result<(), String> {
        let type_key = self.extension_type_of(id)?;
        let field_key =
            kk_mod_api::Key::new(field).map_err(|e| format!("扩展字段键非法 {field:?}: {e}"))?;
        if self.plane.extensions.field(&type_key, &field_key).is_none() {
            return Err(format!("扩展字段未声明：{field}"));
        }
        if let Some(fields) = self.room.extensions.get_mut(&id) {
            fields.remove(field);
            if fields.is_empty() {
                self.room.extensions.remove(&id);
            }
        }
        Ok(())
    }

    /// 解析对象在扩展目录中的类型键。目前只有 Generic 结构按
    /// structureType 映射 builtin:{structure_type}；其他对象族
    /// 尚未声明扩展类型，一律拒绝。
    fn extension_type_of(&self, id: ObjId) -> Result<kk_mod_api::Key, String> {
        let structure_type = match self.room.objects.get(&id) {
            Some(RoomObject::Generic(g)) => {
                let v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
                v.get("structureType")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            }
            _ => None,
        };
        let Some(st) = structure_type else {
            return Err(format!("对象 {} 的类型未接入扩展平面", id.as_str()));
        };
        let key = kk_mod_api::Key::new(format!("builtin:{st}"))
            .map_err(|e| format!("结构类型 {st:?} 不能构成类型键: {e}"))?;
        if self.plane.extensions.fields_of(&key).is_empty() {
            return Err(format!("结构类型 {st} 未声明扩展字段"));
        }
        Ok(key)
    }

    /// 使用共享游标创建对象，并登记位置索引。
    /// 调用方须保证游标不与已有 ID 冲突；游标饱和后无法继续可靠分配。
    pub fn insert_new(&mut self, object: RoomObject) -> ObjId {
        let id = self.state.allocate_id();
        let (x, y) = object.pos();
        self.room
            .insert_with_id(id, object)
            .expect("结算 id 分配器不得复用已有对象");
        self.index.insert(ObjId(id), (x, y));
        ObjId(id)
    }

    /// 删除对象及位置索引，同时清理它的疲劳、待伤害、待治疗和扩展字段。
    pub fn remove_obj(&mut self, id: ObjId) -> Option<RoomObject> {
        let obj = self.room.objects.remove(&id)?;
        let (x, y) = obj.pos();
        self.index.remove(id, (x, y));
        self.room.extensions.remove(&id);
        self.old_fatigue.remove(&id);
        self.tick_fatigue.remove(&id);
        self.damage_to_apply.remove(&id);
        self.heal_to_apply.remove(&id);
        Some(obj)
    }

    /// 读取 Mod 对象的字段 payload；非 Mod 对象或 data 非法 JSON 返回 None。
    pub fn mod_fields(&self, id: ObjId) -> Option<Value> {
        match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => serde_json::from_str(&o.data).ok(),
            _ => None,
        }
    }

    /// 校验并整值写入 Mod 对象的字段 payload：类型必须在计划中声明，
    /// 键集合与字段描述完全一致；声明的位置字段同步到对象信封坐标与
    /// 位置索引。只修改本房候选，失败随房间候选一并丢弃。
    pub fn update_mod_fields(&mut self, id: ObjId, fields: &Value) -> Result<(), String> {
        let mut fields = fields.clone();
        let type_key = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => o.type_key.clone(),
            _ => return Err(format!("对象 {} 不是 Mod 对象", id.as_str())),
        };
        let key = kk_mod_api::Key::new(type_key.clone())
            .map_err(|e| format!("Mod 对象类型键非法 {type_key:?}: {e}"))?;
        let Some(handle) = self.plane.plan.projection().resolve(&key) else {
            return Err(format!("Mod 对象类型 {type_key} 不在计划中"));
        };
        let ty = self
            .plane
            .plan
            .projection()
            .get(handle)
            .expect("句柄来自计划");
        kk_mod_api::validate::check_object_payload(&fields, &ty.descriptor.fields, &type_key)?;
        // 数值 backing 规范化（Float 域统一浮点 backing）：KKPV 列式编码
        // 按值分派列型，整数/浮点 backing 混桶会使整轮投影失败。
        kk_mod_api::validate::normalize_object_payload(&mut fields, &ty.descriptor.fields);
        let (mut x, mut y) = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => (o.x, o.y),
            _ => unreachable!("上面已判 Mod"),
        };
        if let Some([fx, fy]) = &ty.descriptor.position_fields {
            let px = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fx))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            let py = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fy))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            if px != x || py != y {
                self.index.apply_move(id, (x, y), (px, py));
                x = px;
                y = py;
            }
        }
        if let Some(RoomObject::Mod(o)) = self.room.objects.get_mut(&id) {
            o.data = fields.to_string();
            o.x = x;
            o.y = y;
        }
        Ok(())
    }

    /// 对象移动（索引增量更新）。Mod 对象拒绝：其位置同时存在于
    /// payload 字段与信封坐标，发布前校验要求两者一致，原始索引移动
    /// 不维护该不变量——Mod 的位置变更走 update_mod_fields。
    pub fn move_obj(&mut self, id: ObjId, to: (u8, u8)) -> Result<(), String> {
        if let Some(obj) = self.room.objects.get_mut(&id) {
            if matches!(obj, RoomObject::Mod(_)) {
                return Err(format!(
                    "对象 {} 是 Mod 对象：位置变更须走 update_mod_fields（payload 字段与信封坐标同步）",
                    id.as_str()
                ));
            }
            let from = obj.pos();
            obj.set_pos(to.0, to.1);
            self.index.apply_move(id, from, to);
        }
        Ok(())
    }

    /// 累加疲劳，沿拉拽链传到链首。
    /// 负贡献先抵消本体已有疲劳，剩余部分才传给拉者；对象上只保存非负值。
    /// 来源：上游 engine/src/processor/intents/creeps/_add-fatigue.js。
    pub fn add_fatigue(&mut self, id: ObjId, mut d: i64) {
        // 第一次累加时以对象现有疲劳为起点。
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
        // 链上的拉者不存在时停止向上查找。
        let head = self.movement.head_puller(&self.room, id);
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

    /// 在指定位置放下 energy：先填充存活容器，再合并同资源掉落堆。
    pub fn create_energy(&mut self, x: u8, y: u8, amount: i64) {
        self.create_energy_typed(x, y, amount, 0);
    }

    /// 按旧资源编码放下资源：1 为 power，2 为 G，其余为 energy。
    pub fn create_energy_typed(&mut self, x: u8, y: u8, amount: i64, res_code: u8) {
        let name = match res_code {
            1 => "power",
            2 => kk_core::constants::RESOURCE_GHODIUM,
            _ => "energy",
        };
        self.create_resource(x, y, amount, name);
    }

    /// 放下任意命名资源；非正量不处理。
    /// 同格存活容器先接收，按全部资源合计计算余量；剩余与同格同名堆合并。
    /// 来源：上游 engine/src/processor/intents/_create-energy.js。
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
                    // 数量存储为 u32，超出范围时饱和，避免溢出回绕。
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

    /// 放入同格第一个存活容器，返回未放入的数量。
    /// 容器使用 Generic.data 的 JSON store，容量按全部资源之和计算。
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
        // 非整数槽位按 0 计入容量，写入槽位时同样按 0 处理。
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
        // 缺少 store 时创建空对象；JSON 或 store 类型错误时，保留全部掉落量。
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

    /// 在指令处理后仲裁移动冲突，使用此时生效的安全模式主人。
    pub fn check_movement(&mut self) {
        let safe_mode_user: Option<String> = self.controller().and_then(|ctl| {
            if ctl.safe_mode > self.time {
                ctl.owner.clone()
            } else {
                None
            }
        });
        self.movement.check_impl(
            &self.room,
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

/// 更新 creep 和可建造结构的受袭通知开关，拒绝修改他人对象。
/// 这里只保存标志，不发送受袭通知。
/// 来源：上游 engine/src/processor.js 的 notifyWhenAttacked 通用处理。
fn notify_when_attacked(w: &mut Working, player: &str, id: ObjId, data: &IntentData) {
    let Some(obj) = w.room.objects.get(&id) else {
        return;
    };
    if kk_world::domain::object_owner(obj).is_some_and(|owner| owner != player) {
        return; // 拒绝他人对象；无主结构不在此拦截。
    }
    // 控制器、资源源、工地和旗帜不接受此开关；无主可建造结构可以接受。
    let type_gated = matches!(obj, RoomObject::Creep(_))
        || kk_world::types::construction_cost(&movement::type_name_of(obj)).is_some();
    if !type_gated {
        return;
    }
    let enabled = data.enabled.unwrap_or(false);
    match w.room.objects.get_mut(&id) {
        Some(RoomObject::Creep(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Spawn(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extension(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Storage(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::ConstructedWall(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Road(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Extractor(o)) => o.notify_when_attacked = Some(enabled),
        Some(RoomObject::Generic(o)) => {
            // Generic 将标志保存在 JSON 中，供世界投影读取。
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or_default();
            v["notifyWhenAttacked"] = Value::Bool(enabled);
            o.data = v.to_string();
        }
        _ => {}
    }
}

/// 读取掉落资源名：命名字段优先，否则按旧编码 1=power、2=G、其余=energy。
pub fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> &str {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => name.as_str(),
        (None, 1) => "power",
        (None, 2) => kk_core::constants::RESOURCE_GHODIUM,
        (None, _) => "energy",
    }
}

/// 原地结算房间，返回事件并替换 room.events。
///
/// 本函数不创建副本，也不回滚 room 与共享状态。调用方须提供
/// 候选，返回 Err 后丢弃整个候选。Engine 负责跨房间的提交边界。
/// room.name 须为世界目录中的房名，用于筛选房间级指令；time 是结算前
/// 的绝对 tick，now_ms 是 Unix 毫秒墙钟。对象级指令仅处理本房间存在的 ID。
pub fn settle_room(
    room: RoomState,
    time: u64,
    now_ms: u64,
    state: &TickState,
    batch: &IntentBatch,
    plane: &Plane,
) -> Result<(RoomState, Vec<Value>, Vec<inter_room::InterRoomMove>), SettleError> {
    let index = RoomIndex::build(&room.objects);
    // 候选以共享句柄持有：Rust 链与 JS 整槽宿主在本次调用期间共享
    // 同一候选，见 RoomCtx 的说明。
    let ctx: RoomCtx = Rc::new(RefCell::new(Working {
        room,
        index,
        time,
        now_ms,
        state: state.clone(),
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
        pending_inter_room: Vec::new(),
        plane: plane.clone(),
    }));

    // 先清空动作记录和说话载荷，避免上一轮内容继续可见。
    {
        let mut w = ctx.borrow_mut();
        for creep in w.room.objects.values_mut() {
            if let RoomObject::Creep(c) = creep {
                c.action_log = 0;
                c.say_message = None;
                c.say_public = None;
            }
        }
        // 处理指令前，按控制器配额计算 spawn/extension 是否启用。
        spawn::calc_spawns(&mut w);
    }

    // 玩家按名字排序，先处理房间指令，再按对象 ID 处理对象动作。
    for (player, intents) in &batch.users {
        room::process_room_intents(&mut ctx.borrow_mut(), player, &intents.room);
        for (obj_id, actions) in &intents.objects {
            // 通知开关是对象通用指令，在类型分派前处理。
            if let Some(data) = actions.get(&Action::NotifyWhenAttacked) {
                notify_when_attacked(&mut ctx.borrow_mut(), player, *obj_id, data);
            }
            // Creep 和 spawn 须属于当前玩家；controller 另允许无主对象进入处理器。
            // 先取对象类别再释放借用：处理器链需要共享句柄，不能持借用进入。
            let kind = {
                let w = ctx.borrow();
                match w.room.objects.get(obj_id) {
                    Some(RoomObject::Creep(o)) if o.owner == *player => Some("creep"),
                    Some(RoomObject::Spawn(o)) if o.owner == *player => Some("spawn"),
                    Some(RoomObject::Controller(o))
                        if o.owner.as_deref().is_none_or(|u| u == player.as_str()) =>
                    {
                        Some("controller")
                    }
                    // 真实类型在 structureType 中，处理器再检查结构类型和所有权。
                    Some(RoomObject::Generic(_)) => Some("structure"),
                    _ => None,
                }
            };
            match kind {
                Some("creep") => {
                    creep::process_creep_intents(&mut ctx.borrow_mut(), *obj_id, actions);
                }
                Some("spawn") => {
                    spawn::process_spawn_intents(&mut ctx.borrow_mut(), *obj_id, actions);
                }
                Some("controller") => {
                    controller::process_controller_intents(&mut ctx.borrow_mut(), *obj_id, actions);
                }
                Some("structure") => {
                    structures::process_structure_intents(&ctx, *obj_id, player, actions)
                        .map_err(SettleError)?;
                }
                _ => continue,
            }
        }
        // Mod 对象的扩展意图在既有动作之后按对象 ID 顺序派发；目标不是
        // Mod 对象的扩展意图在此静默丢弃（与未知 vanilla 动作同待遇）。
        for (obj_id, ext_intents) in &intents.ext {
            let is_mod = matches!(
                ctx.borrow().room.objects.get(obj_id),
                Some(RoomObject::Mod(_))
            );
            if is_mod {
                mod_intents::process_object_ext_intents(&ctx, player, *obj_id, ext_intents)
                    .map_err(SettleError)?;
            }
        }
    }

    // 所有 move/pull 登记完成后，再统一仲裁目的格冲突。
    ctx.borrow_mut().check_movement();

    // 按指令处理后的存量重算可用能量，供 spawn tick 的低能量补给判断。
    let _energy_available = spawn::energy_available(&ctx.borrow());

    // 此时固定 ID 列表：指令阶段创建的对象参加 tick，
    // 逐对象 tick 中新建的对象不参加这一轮。对象按 ID 升序处理。
    let ids: Vec<ObjId> = ctx.borrow().room.objects.keys().copied().collect();
    for id in ids {
        let kind = {
            let w = ctx.borrow();
            match w.room.objects.get(&id) {
                Some(RoomObject::Energy(_)) => "energy",
                Some(RoomObject::Source(_)) => "source",
                Some(RoomObject::Creep(_)) => "creep",
                Some(RoomObject::Spawn(_)) => "spawn",
                Some(RoomObject::Controller(_)) => "controller",
                Some(RoomObject::Tombstone(_)) => "tombstone",
                Some(RoomObject::Ruin(_)) => "ruin",
                Some(RoomObject::Mineral(_)) => "mineral",
                Some(RoomObject::Extractor(_)) => "extractor",
                Some(RoomObject::Road(_)) => "road",
                Some(RoomObject::Generic(g)) => match generic_structure_type(&g.data).as_deref() {
                    Some("container" | "rampart") => "decay",
                    Some("link") => "link",
                    Some("terminal") => "terminal",
                    _ => "",
                },
                _ => "",
            }
        };
        match kind {
            "energy" => economy::energy_tick(&mut ctx.borrow_mut(), id),
            "source" => economy::source_tick(&mut ctx.borrow_mut(), id),
            "creep" => creep::creep_tick(&mut ctx.borrow_mut(), id)?,
            "spawn" => spawn::spawn_tick(&mut ctx.borrow_mut(), id, _energy_available),
            "controller" => economy::controller_tick(&mut ctx.borrow_mut(), id),
            "tombstone" => economy::tombstone_tick(&mut ctx.borrow_mut(), id),
            "ruin" => economy::ruin_tick(&mut ctx.borrow_mut(), id),
            "mineral" => economy::mineral_tick(&mut ctx.borrow_mut(), id),
            "extractor" => economy::extractor_tick(&mut ctx.borrow_mut(), id),
            "road" => economy::road_decay_tick(&mut ctx.borrow_mut(), id),
            "decay" => economy::generic_decay_tick(&mut ctx.borrow_mut(), id),
            "link" => {
                // 贡献链经共享句柄执行：wrap 的 next() 会重新借用候选，
                // 此处不得持有借用。
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_tick(&ctx, id).map_err(SettleError)?
            }
            "terminal" => economy::terminal_capacity_tick(&mut ctx.borrow_mut(), id),
            _ => {}
        }
    }

    // 整体替换本轮事件，下一轮玩家执行时由 room.getEventLog 读取。
    // settle_room 是本房间候选的唯一持有者，Rc 强计数为 1；
    // 处理器链只在调用期间借用，不留存句柄。
    let mut w = Rc::into_inner(ctx)
        .expect("房间候选在结算期间不应被处理器留存")
        .into_inner();
    w.room.events = w.events.iter().map(Value::to_string).collect();

    let pending_inter_room = std::mem::take(&mut w.pending_inter_room);
    Ok((w.room, w.events, pending_inter_room))
}

/// 读取 Generic 的结构类型；缺失、类型错误或 JSON 无效时返回 None。
fn generic_structure_type(data: &str) -> Option<String> {
    serde_json::from_str::<Value>(data)
        .ok()?
        .get("structureType")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 按固定顺序结算世界、玩家和市场的内存状态。
///
/// 房间、玩家和市场先在副本中处理，所有返回 Result 的检查通过后才发布。
/// 这是同步内存提交，不保证 panic、进程崩溃或磁盘写入时的原子性。
/// 沙箱执行和内部状态不在结算副本内；失败重试的输入缓存由调用方负责。
pub struct Engine {
    pub world: kk_world::World,
    pub players: BTreeMap<String, PlayerState>,
    pub sandboxes: BTreeMap<String, Box<dyn crate::sandbox::Sandbox>>,
    /// 跨房间订单簿和交易记录，不在 kk-world 的二进制房间快照中。
    pub market: market::MarketState,
    next_id: u64,
    /// 最近成功 tick 的事件，按房间结算顺序拼接；失败不覆盖。
    pub last_events: Vec<serde_json::Value>,
    /// 持续生效的房间故障注入，由调用方显式解除。
    fail_room: Option<String>,
    /// 失败后记录恢复状态；世界版本检查不能检测玩家或市场的直接修改。
    run_state: stages::RunState,
    /// 持续生效的阶段入口故障注入。
    fail_stage: Option<TickStage>,
    fail_writeback: Option<WritebackStep>,
    /// 本引擎生效的 link 有效规则；放置、结算与常量同步读取。
    /// 装配后不应直接改写——规则与已装配的处理链须一起换，经 with_plan。
    pub(crate) links: kk_mod_api::builtin::LinkRules,
    /// link 槽执行表；默认直连 native，装配计划后可携带贡献链。
    pub(crate) contrib: Rc<contrib::ContributionDispatch>,
    /// 扩展字段目录；默认为空（不接受扩展写入），with_plan 装配。
    pub(crate) extensions: Rc<kk_mod_api::extension::ExtensionCatalog>,
    /// 已提交的各 Mod 命名空间状态；每 tick 克隆为候选，发布时整体换回。
    /// 服务端私有，不进世界快照与玩家投影。
    pub mod_states: BTreeMap<String, serde_json::Value>,
    /// 当前生效的冻结计划（默认只含 builtin:link）。扩展意图描述符与
    /// Mod 类型目录从此解析；规则/处理链/扩展目录是它的派生物。
    plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
}

/// 只含 builtin:link 的冻结计划；无 Mod 引擎的默认贡献平面。
/// 测试夹具构造 Plane 时也使用它。
pub fn builtin_plan() -> std::sync::Arc<kk_mod_api::ContributionPlan> {
    static PLAN: std::sync::OnceLock<std::sync::Arc<kk_mod_api::ContributionPlan>> =
        std::sync::OnceLock::new();
    PLAN.get_or_init(|| {
        std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![],
                kk_mod_api::FreezeOptions::default(),
            )
            .expect("builtin:link 贡献冻结必成"),
        )
    })
    .clone()
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
            fail_writeback: None,
            links: kk_world::place::builtin_link_rules(),
            contrib: Rc::new(contrib::ContributionDispatch::default()),
            extensions: Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            mod_states: BTreeMap::new(),
            plan: builtin_plan(),
        }
    }

    /// 核对计划的全部声明绑定已有实现：两个 native 键由本引擎内置，
    /// 声明性绑定须出现在已接线消费者清单（见下方常量），处理器绑定
    /// 已在装配时逐键消费。声明未安装的绑定在此拒绝。
    fn check_installed_bindings(
        plan: &kk_mod_api::ContributionPlan,
        handler_keys: &BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        /// 已接线的声明性绑定：每项对应一个真实消费者。
        /// 新增声明绑定时须先接线，或在此登记例外依据。
        const WIRED: &[&str] = &[
            // 放置与建造默认值经 PlaceCtx.links / Working.links。
            "builtin:link.init",
            "builtin:link.build",
            // Store 容量在放置/建造时落入权威字段。
            "builtin:link.capacity",
            // isActive 与配额检查读取 links.rcl。
            "builtin:link.active",
            // 玩家预检读取权威字段与常量覆盖通道。
            "builtin:link.precheck",
            // 玩家常量经 SandboxConfig 覆盖注入。
            "builtin:link.constants",
            // 投影直接输出权威字段。
            "builtin:link.project",
            // vanilla 参数转换沿用现有意图清洗。
            "builtin:link.sanitize",
            // 字段适配绑定：权威 Generic JSON 的对应键。
            "builtin:link.field.id",
            "builtin:link.field.room",
            "builtin:link.field.x",
            "builtin:link.field.y",
            "builtin:link.field.owner",
            "builtin:link.field.structureType",
            "builtin:link.field.hits",
            "builtin:link.field.hitsMax",
            "builtin:link.field.cooldown",
            "builtin:link.field.store",
            "builtin:link.field.storeCapacity",
            "builtin:link.field.storeCapacityResource",
        ];
        const NATIVE: &[&str] = &["builtin:link.transfer.native", "builtin:link.tick.native"];
        for binding in plan.required_bindings() {
            let name = binding.as_str();
            // 只认显式清单与调用方提供的处理器键：新增声明绑定必须先
            // 接线再进清单，前缀放行会让未接线的 field 键静默通过。
            let wired =
                WIRED.contains(&name) || NATIVE.contains(&name) || handler_keys.contains(binding);
            if !wired {
                return Err(format!("计划声明了未安装的绑定 {binding}"));
            }
        }
        Ok(())
    }

    /// Mod 自有意图的可执行性门：只放行已接线的执行形态，其余在装配期
    /// 显式拒绝而不是静默不执行。豁免只按**意图键**的命名空间（builtin
    /// 意图由 Rust native 执行器消费）——按槽豁免会让 Mod 意图挂 builtin
    /// 槽逃过全部门检查，运行期找不到扩展执行器而成为可重复触发的
    /// 整轮失败。中心检查在调用方（须结合 Replace 处理器实现判断）。
    fn check_executable_intents(plan: &kk_mod_api::ContributionPlan) -> Result<(), String> {
        use kk_mod_api::{ArgumentConversion, IntentAuthority, IntentScope, Merge};
        for intent in plan.intents() {
            if intent.key.namespace() == "builtin" {
                continue; // builtin:link.transfer 由 Rust native 执行器消费
            }
            let Some(slot_handle) = plan.resolve_slot(&intent.slot) else {
                return Err(format!(
                    "计划缺少动作 {} 的入口 {}",
                    intent.key, intent.slot
                ));
            };
            let slot = plan.get_slot(slot_handle).expect("句柄来自计划");
            if slot.stage != kk_mod_api::Stage::ObjectAction {
                return Err(format!(
                    "动作 {} 的入口 {} 阶段为 {:?}，扩展意图只在对象动作阶段派发",
                    intent.key, intent.slot, slot.stage
                ));
            }
            if intent.scope != IntentScope::Object {
                return Err(format!(
                    "动作 {} 的 {} 域执行通道未接线（当前只接线对象域）",
                    intent.key,
                    match intent.scope {
                        IntentScope::Room => "房间",
                        IntentScope::Player => "玩家",
                        _ => "世界",
                    }
                ));
            }
            if !matches!(intent.merge, Merge::Last) {
                return Err(format!(
                    "动作 {} 声明队列合并模式，该模式的执行装配未接线",
                    intent.key
                ));
            }
            if !matches!(intent.conversion, ArgumentConversion::Strict) {
                return Err(format!(
                    "动作 {} 声明非严格参数转换，该形态的执行装配未接线",
                    intent.key
                ));
            }
            if intent.authority != IntentAuthority::Player {
                return Err(format!(
                    "动作 {} 声明非玩家授权，服务端发射通道未接线",
                    intent.key
                ));
            }
            if !intent.suppressed_by.is_empty() {
                return Err(format!(
                    "动作 {} 声明压制关系，压制判定的执行装配未接线",
                    intent.key
                ));
            }
        }
        // 非 link 槽必须被至少一个意图挂载：无意图的槽（生命周期槽、
        // 其他阶段槽）没有派发点，装配了执行器也只是静默死执行。
        for slot in plan.slots() {
            if contrib::LINK_SLOT_KEYS.contains(&slot.key.as_str()) {
                continue;
            }
            let referenced = plan.intents().iter().any(|intent| intent.slot == slot.key);
            if !referenced {
                return Err(format!(
                    "入口 {} 没有挂载任何动作，该槽位的执行接线未开放（生命周期/其他阶段槽归后续批次）",
                    slot.key
                ));
            }
        }
        Ok(())
    }

    /// 中心规则：动作入口的中心须是自身 process 绑定，或由调用方以
    /// Replace 处理器占位（跨 Mod 替换动作处理）。多意图共享一槽时
    /// 非自有中心的意图在此拒绝。
    fn check_intent_centers(
        plan: &kk_mod_api::ContributionPlan,
        replacements: &BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        for intent in plan.intents() {
            if intent.key.namespace() == "builtin" {
                continue;
            }
            let own_process = match kk_mod_api::Key::new(format!("{}.process", intent.key)) {
                Ok(key) => key,
                Err(_) => {
                    return Err(format!(
                        "动作 {} 的键过长，无法派生 process 绑定（键上限 128 字节）",
                        intent.key
                    ));
                }
            };
            let slot_handle = plan
                .resolve_slot(&intent.slot)
                .expect("check_executable_intents 已保证入口存在");
            let slot = plan.get_slot(slot_handle).expect("句柄来自计划");
            // 冻结后的入口计划只保留 dispatch；Native 绑定或未被占位的
            // Chained 中心即 native 绑定。
            let center = match &slot.dispatch {
                kk_mod_api::Dispatch::Native(binding) => binding,
                kk_mod_api::Dispatch::Chained { center, .. } => center,
            };
            let replaced = replacements.contains(center);
            if center != &own_process && !replaced {
                return Err(format!(
                    "动作 {} 的入口 {} 中心不是自身 process，也不是已提供的 Replace 处理器（多意图共享入口或显式入口的执行装配未接线）",
                    intent.key, intent.slot
                ));
            }
        }
        Ok(())
    }

    /// 用冻结计划装配引擎；处理器实现随计划提供，缺失或 kind
    /// 不符时拒绝。计划须包含 builtin:link 贡献。
    pub fn with_plan(
        world: kk_world::World,
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
    ) -> Result<Self, String> {
        let mut engine = Self::new(world);
        engine.install_plan(plan, action_handlers, tick_handlers)?;
        Ok(engine)
    }

    /// 就地装配冻结计划：换入有效规则、处理链、扩展目录与计划本体，
    /// 保留世界、玩家与沙箱。与 [`Self::with_plan`] 走同一校验；装配
    /// 失败时引擎保持原状。规则与已装配的处理链须一起换，不允许只动其一。
    pub fn install_plan(
        &mut self,
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
    ) -> Result<(), String> {
        self.install_plan_with_ext(
            plan,
            action_handlers,
            tick_handlers,
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    /// [`Self::install_plan`] 的完整形态：`ext_handlers` 是 Mod 自有槽上的
    /// 贡献处理器，`ext_processes` 是各槽中心的动作处理实现。
    pub fn install_plan_with_ext(
        &mut self,
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
        ext_handlers: BTreeMap<kk_mod_api::Key, contrib::ExtHandler>,
        ext_processes: BTreeMap<kk_mod_api::Key, contrib::ExtSimple>,
    ) -> Result<(), String> {
        Self::check_executable_intents(&plan)?;
        Self::check_intent_centers(
            &plan,
            &ext_handlers
                .iter()
                .filter_map(|(key, handler)| {
                    matches!(handler, contrib::ExtHandler::Replace(_)).then_some(key.clone())
                })
                .collect(),
        )?;
        // 先取调用方提供的处理器键，再交给装配消耗。
        let handler_keys: BTreeSet<kk_mod_api::Key> = action_handlers
            .keys()
            .chain(tick_handlers.keys())
            .chain(ext_handlers.keys())
            .chain(ext_processes.keys())
            .cloned()
            .collect();
        let links = kk_mod_api::builtin::LinkRules::from_plan(plan.rules())?;
        let contrib = contrib::ContributionDispatch::assemble_with_ext(
            &plan,
            action_handlers,
            tick_handlers,
            ext_handlers,
            ext_processes,
        )?;
        let extensions = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan)?;
        let mut installed = handler_keys;
        installed.extend(extensions.field_bindings().cloned());
        // Mod 类型字段绑定的实现是 payload 存储与写入门（创建/处理器回写/
        // 发布前校验），随计划装配即安装——与扩展目录的安装口径同原则。
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() == "builtin" {
                continue;
            }
            installed.extend(ty.fields.iter().map(|f| f.descriptor.binding.clone()));
        }
        Self::check_installed_bindings(&plan, &installed)?;
        self.links = links;
        self.contrib = Rc::new(contrib);
        self.extensions = Rc::new(extensions);
        self.plan = plan;
        Ok(())
    }

    /// 安装整槽宿主执行器，维持同一冻结计划与装配校验。
    /// bindings 是已注册函数，replacements 是其中确认的 Replace 绑定。
    /// 失败时所有装配域保持原状。
    pub fn install_hosted_plan(
        &mut self,
        plan: Arc<kk_mod_api::ContributionPlan>,
        executor: Rc<dyn contrib::SlotExecutor>,
        bindings: BTreeSet<kk_mod_api::Key>,
        replacements: BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        Self::check_executable_intents(&plan)?;
        if !replacements.is_subset(&bindings) {
            return Err("Replace 绑定必须是已安装函数".into());
        }
        Self::check_intent_centers(&plan, &replacements)?;
        let links = kk_mod_api::builtin::LinkRules::from_plan(plan.rules())?;
        let contrib = contrib::ContributionDispatch::assemble_hosted(&plan, executor, &bindings)?;
        let extensions = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan)?;
        let mut installed = bindings;
        installed.extend(extensions.field_bindings().cloned());
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() != "builtin" {
                installed.extend(
                    ty.fields
                        .iter()
                        .map(|field| field.descriptor.binding.clone()),
                );
            }
        }
        Self::check_installed_bindings(&plan, &installed)?;
        self.links = links;
        self.contrib = Rc::new(contrib);
        self.extensions = Rc::new(extensions);
        self.plan = plan;
        Ok(())
    }

    /// 当前已装配冻结计划的指纹：KKPV 块头的 plan_hash 来源——计划变化
    /// 使旧块失效，块库按指纹淘汰代际。
    pub fn plan_hash(&self) -> kk_mod_api::PlanHash {
        self.plan.hash()
    }

    /// 计划中的 Mod 类型描述符（管理面创建对象与诊断用）；builtin 类型
    /// 不在此列。类型键须为 `namespace:name`。
    pub fn mod_object_type(&self, type_key: &str) -> Option<&kk_mod_api::ObjectTypeDescriptor> {
        let key = kk_mod_api::Key::new(type_key).ok()?;
        // builtin 类型有 Rust 权威表示，以 Mod 信封再造一份会形成双表示，
        // 挂 builtin 键的 Mod 对象也会把玩家面引向无执行器的 builtin 槽。
        if key.namespace() == "builtin" {
            return None;
        }
        let handle = self.plan.projection().resolve(&key)?;
        Some(&self.plan.projection().get(handle)?.descriptor)
    }

    /// 结算指定房间前返回错误，传 None 解除。
    /// 公开方法只隐藏文档，发布版本仍可调用；并非一次性故障。
    #[doc(hidden)]
    pub fn set_fail_room_for_test(&mut self, room: Option<String>) {
        self.fail_room = room;
    }

    /// 读取当前结算状态。
    pub fn run_state(&self) -> &RunState {
        &self.run_state
    }

    /// 当前生效的 link 有效规则（只读快照，拷贝语义）。
    pub fn links(&self) -> kk_mod_api::builtin::LinkRules {
        self.links
    }

    /// 当前生效的扩展字段目录（只读引用）。
    pub fn extensions(&self) -> &kk_mod_api::extension::ExtensionCatalog {
        &self.extensions
    }

    /// 进入指定阶段前返回错误，传 None 解除。
    /// 公开方法只隐藏文档，发布版本仍可调用。
    #[doc(hidden)]
    pub fn set_fail_stage_for_test(&mut self, stage: Option<TickStage>) {
        self.fail_stage = stage;
    }

    /// 在指定候选子步完成后注入错误；不触碰已发布状态。传 None 解除。
    #[doc(hidden)]
    pub fn set_fail_writeback_for_test(&mut self, step: Option<WritebackStep>) {
        self.fail_writeback = step;
    }

    fn writeback_checkpoint(&self, step: WritebackStep) -> Result<(), SettleError> {
        if self.fail_writeback == Some(step) {
            return Err(SettleError(format!(
                "故障注入：{} 候选子步完成后失败",
                step.label()
            )));
        }
        Ok(())
    }

    /// 将 NeedsRecovery 改回 Ready，放弃恢复。
    /// 仅允许世界时间和版本未变；不删除调用方保存的运行结果。
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

    /// 调用注册沙箱收集指令，再推进一个 tick。
    ///
    /// 只收集 Executed 且属于注册玩家的指令，其余结果被忽略。
    /// 不提交 Memory、segments 或 CPU 候选；完整执行流程由 kk-runtime 协调。
    /// 重试本方法会再次调用沙箱，不复用上次执行结果。
    pub fn tick(&mut self) -> Result<(), SettleError> {
        let mut batch = IntentBatch::default();
        {
            let view = crate::sandbox::WorldView { world: &self.world };
            for (player, sandbox) in &mut self.sandboxes {
                let mut result =
                    sandbox.run(&view, &crate::sandbox::RunSpec::new(player.clone(), ""));
                // 此简化入口忽略所有非 Executed 结果，包括基础设施故障。
                if !matches!(result.outcome, crate::sandbox::RunOutcome::Executed) {
                    continue;
                }
                // 只取注册玩家的键，防止沙箱伪造其他玩家的指令。
                if let Some(intents) = result.intents.users.get_mut(player) {
                    let entry = batch.users.entry(player.clone()).or_default();
                    entry.objects.extend(std::mem::take(&mut intents.objects));
                    entry.room.extend(std::mem::take(&mut intents.room));
                    entry.notify.extend(std::mem::take(&mut intents.notify));
                    entry.market.extend(std::mem::take(&mut intents.market));
                    for (id, ext) in std::mem::take(&mut intents.ext) {
                        entry.ext.entry(id).or_default().extend(ext);
                    }
                }
            }
        }
        self.tick_with_intents(batch)
    }

    /// 结算调用方提供的指令，并读取当前墙钟。
    /// 调用方负责清洗与身份绑定；此处不执行沙箱或提交玩家 Memory。
    /// 重试需固定墙钟时使用 tick_with_intents_with_input。
    pub fn tick_with_intents(&mut self, batch: IntentBatch) -> Result<(), SettleError> {
        self.tick_with_intents_with_input(batch, TickInput::now())
    }

    /// 使用指定指令和外部时间推进一个 tick。
    ///
    /// 返回 Err 时世界、玩家、市场、ID 游标和 last_events 保持提交前状态，
    /// 并记录 NeedsRecovery。重试须重新提供相同 batch 和 TickInput，且不可
    /// 直接修改玩家或市场；引擎只检查世界时间和版本是否改变。
    /// 不捕获 panic，发布中的 panic 不享有上述回滚保证。
    pub fn tick_with_intents_with_input(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
    ) -> Result<(), SettleError> {
        self.tick_with_writebacks(batch, input, BTreeMap::new())
    }

    /// 将玩家内存候选与房间、市场一起发布；失败时所有已提交数据保持旧值。
    /// 调用方负责绑定玩家身份，并在重试时复用相同的指令、时间及写回候选。
    /// 不捕获 panic；桶值须由调用方根据固定预算和执行回执计算。
    pub fn tick_with_writebacks(
        &mut self,
        batch: IntentBatch,
        input: TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> Result<(), SettleError> {
        // 失败后仅在世界时间和版本未变时放行；输入是否相同由调用方保证。
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
            // 同步调用结束后若残留这些状态，可能由 panic 中断导致；拒绝继续。
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
        let result = self.staged_body(&mut cursor, batch, &input, writebacks);
        match result {
            Ok(()) => {
                self.run_state = RunState::Committed {
                    tick: self.world.time(),
                };
                Ok(())
            }
            Err(e) => {
                // 丢弃候选并记录失败阶段，不缓存指令或墙钟。
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

    /// 按准备、房间、市场、校验、发布的顺序处理候选。
    fn staged_body(
        &mut self,
        cursor: &mut stages::StageCursor,
        batch: IntentBatch,
        input: &TickInput,
        writebacks: BTreeMap<String, PlayerWriteback>,
    ) -> Result<(), SettleError> {
        cursor.enter(TickStage::Prepare)?;
        if let Some(handle) = writebacks
            .keys()
            .find(|handle| !self.players.contains_key(*handle))
        {
            return Err(SettleError(format!(
                "玩家写回候选不属于已注册玩家：{handle}"
            )));
        }
        let mut txn = self.prepare_tick(batch, *input)?;
        cursor.enter(TickStage::Settle)?;
        self.settle_rooms_stage(&mut txn)?;
        cursor.enter(TickStage::Market)?;
        {
            let mut state = txn.state.inner.borrow_mut();
            market::settle_market(
                &mut txn.rooms,
                &mut state.players,
                &mut txn.market,
                &txn.batch,
                txn.time,
                txn.input.now_ms,
            );
        }
        self.writeback_checkpoint(WritebackStep::Market)?;
        cursor.enter(TickStage::Validate)?;
        {
            let mut state = txn.state.inner.borrow_mut();
            for (handle, patch) in writebacks {
                let player = state.players.get_mut(&handle).ok_or_else(|| {
                    SettleError(format!("玩家写回候选不属于已注册玩家：{handle}"))
                })?;
                patch.apply_with_checkpoints(player, |step| self.writeback_checkpoint(step))?;
            }
        }
        self.validate_stage(&mut txn)?;
        let time = txn.time;
        let prepared = txn.into_prepared()?;
        self.run_state = RunState::Prepared { tick: time };
        cursor.enter(TickStage::Publish)?;
        self.publish_tick(prepared);
        Ok(())
    }

    /// 构造玩家副本和 ID 起点，并复核安全模式指令。
    fn prepare_tick(
        &self,
        batch: IntentBatch,
        input: TickInput,
    ) -> Result<txn::OwnedTickTxn, SettleError> {
        let next = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let players = self.players.clone();
        let mut batch = batch;
        // 按已提交世界复核：玩家已有生效的安全模式时，丢弃其新的激活指令。
        // 不依赖 JavaScript 的扫描结果，也不计入本轮尚未生效的激活。
        // 来源：上游 engine/src/game/structures.js 的 StructureController.activateSafeMode。
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
        Ok(txn::OwnedTickTxn {
            time,
            input,
            room_names,
            state: TickState::with_mod_states(players, next, self.mod_states.clone()),
            batch,
            rooms: Vec::new(),
            events: Vec::new(),
            market: self.market.clone(),
        })
    }

    /// 按房名结算副本，共用候选玩家与 ID 游标；任一房间失败均不写回。
    fn settle_rooms_stage(&self, txn: &mut txn::OwnedTickTxn) -> Result<(), SettleError> {
        let mut inter_room_moves = Vec::new();
        for name in &txn.room_names {
            if self.fail_room.as_deref() == Some(name.as_str()) {
                return Err(SettleError(format!("故障注入：房间 {name} 结算失败")));
            }
            let mut room = self.world.room(name).expect("房间名来自键集").clone();
            // 二进制房间快照不保存房名；结算指令按世界目录中的名称匹配。
            room.name.clone_from(name);
            let plane = Plane {
                links: self.links,
                contrib: self.contrib.clone(),
                extensions: self.extensions.clone(),
                plan: self.plan.clone(),
            };
            let (room, mut room_events, mut moves) = settle_room(
                room,
                txn.time,
                txn.input.now_ms,
                &txn.state,
                &txn.batch,
                &plane,
            )?;
            inter_room_moves.append(&mut moves);
            txn.rooms.push((name.clone(), room));
            txn.events.append(&mut room_events);
        }
        // 跨房过渡在全部房间结算完成后应用到候选：到达对象不参与目的房
        // 本 tick 的结算（其对象 tick 已在来源房执行），发布后对下一轮可见。
        inter_room::apply_inter_room_moves(&mut txn.rooms, &inter_room_moves);
        Ok(())
    }

    /// 只读检查候选房间覆盖、ID 游标和玩家键，失败进入 NeedsRecovery。
    fn validate_stage(&self, cand: &mut txn::OwnedTickTxn) -> Result<(), SettleError> {
        // 房间数和顺序均须与准备阶段一致，集合相等不能验证顺序。
        if cand.rooms.len() != cand.room_names.len()
            || cand
                .rooms
                .iter()
                .zip(&cand.room_names)
                .any(|((settled_name, _), want)| settled_name != want)
        {
            return Err(SettleError(format!(
                "候选房间覆盖不完整：期望 {} 间，实得 {} 间",
                cand.room_names.len(),
                cand.rooms.len()
            )));
        }
        // 游标不得低于入口的 ID 起点，避免后续创建对象复用旧 ID。
        let base = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let state = cand.state.inner.borrow();
        if state.next_id < base {
            return Err(SettleError(format!(
                "id 候选回退：{} < 基线 {base}",
                state.next_id
            )));
        }
        // 候选可以增加玩家，不能删除已提交的玩家。
        if let Some(missing) = self
            .players
            .keys()
            .find(|k| !state.players.contains_key(*k))
        {
            return Err(SettleError(format!("玩家候选丢键：{missing}")));
        }
        drop(state);
        self.validate_extensions(cand)?;
        self.validate_mod_objects(cand)?;
        Ok(())
    }

    /// 发布前校验候选中的 Mod 对象：类型在计划中、字段 payload 整体
    /// 满足描述符、声明的位置字段与信封坐标同步。写入路径（创建与
    /// 处理器回写）已各自校验；此处封住绕过写入口的直改形态。
    fn validate_mod_objects(&self, cand: &mut txn::OwnedTickTxn) -> Result<(), SettleError> {
        // 短路按候选内容而非计划布尔量：本函数封的是"绕过写入口的直改"，
        // 计划里没有 Mod 类型不能成为跳过校验的理由——快照恢复等新来源
        // 仍可能带入计划外对象。扫描是一次枚举判别匹配，与结算既有的
        // 逐对象遍历同阶。
        if !cand.rooms.iter().any(|(_, room)| {
            room.objects
                .values()
                .any(|o| matches!(o, RoomObject::Mod(_)))
        }) {
            return Ok(());
        }
        for (room_name, room) in &cand.rooms {
            for (id, obj) in &room.objects {
                let kk_schema::room::RoomObject::Mod(o) = obj else {
                    continue;
                };
                let key = kk_mod_api::Key::new(o.type_key.clone()).map_err(|e| {
                    SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 类型键非法：{e}",
                        id.as_str()
                    ))
                })?;
                let Some(handle) = self.plan.projection().resolve(&key) else {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 类型 {} 不在计划中",
                        id.as_str(),
                        o.type_key
                    )));
                };
                let ty = self.plan.projection().get(handle).expect("句柄来自计划");
                let payload: serde_json::Value = serde_json::from_str(&o.data).map_err(|e| {
                    SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} payload 非法 JSON：{e}",
                        id.as_str()
                    ))
                })?;
                kk_mod_api::validate::check_object_payload(
                    &payload,
                    &ty.descriptor.fields,
                    &o.type_key,
                )
                .map_err(SettleError)?;
                if let Some([fx, fy]) = &ty.descriptor.position_fields {
                    let px = payload
                        .get(kk_mod_api::validate::field_name(fx))
                        .and_then(serde_json::Value::as_u64);
                    let py = payload
                        .get(kk_mod_api::validate::field_name(fy))
                        .and_then(serde_json::Value::as_u64);
                    if px != Some(o.x as u64) || py != Some(o.y as u64) {
                        return Err(SettleError(format!(
                            "房间 {room_name} 的 Mod 对象 {} 位置字段与信封坐标不同步",
                            id.as_str()
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// 规整并校验候选房间的扩展字段：键必须在目录中声明（封住绕过
    /// 写入 API 的直改），未声明或键非法仍然失败；ObjectRef 字段的
    /// 悬空引用在发布前收敛——nullable 字段写 null、非 nullable 字段
    /// 整项删除（与玩家侧 getObjectById 对失效引用返回 null 同形）。
    /// 收敛使被引用对象销毁的 tick 仍可提交。
    fn validate_extensions(&self, cand: &mut txn::OwnedTickTxn) -> Result<(), SettleError> {
        // 短路按候选房间的扩展条目而非扩展目录：目录为空不是跳过的理由
        // （候选里的未声明条目正是要抓的直改形态），这里只省掉空候选
        // 的扫描成本。
        if cand
            .rooms
            .iter()
            .all(|(_, room)| room.extensions.is_empty())
        {
            return Ok(());
        }
        // 收敛动作先收集后应用，避免边遍历边改。
        let mut reconcile: Vec<(String, ObjId, String, bool)> = Vec::new();
        for (room_name, room) in &cand.rooms {
            for (id, fields) in &room.extensions {
                for (field, value) in fields {
                    let structure_type: Option<String> =
                        room.objects.get(id).and_then(|o| match o {
                            RoomObject::Generic(g) => {
                                let v: serde_json::Value =
                                    serde_json::from_str(&g.data).unwrap_or_default();
                                v.get("structureType")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            }
                            _ => None,
                        });
                    let type_key = structure_type
                        .as_deref()
                        .and_then(|st| kk_mod_api::Key::new(format!("builtin:{st}")).ok());
                    let Some(type_key) = type_key else {
                        return Err(SettleError(format!(
                            "扩展候选引用了未接入扩展平面的对象：房间 {room_name} {}",
                            id.as_str()
                        )));
                    };
                    let Some(rule) =
                        self.extensions.field(
                            &type_key,
                            &kk_mod_api::Key::new(field.clone()).ok().ok_or(SettleError(
                                format!("扩展候选键非法：房间 {room_name} {field}"),
                            ))?,
                        )
                    else {
                        return Err(SettleError(format!(
                            "扩展候选含未声明字段：房间 {room_name} {field}"
                        )));
                    };
                    if matches!(rule.schema.kind, kk_mod_api::ValueKind::ObjectRef(_)) {
                        // nullable 字段的 null 是合法清除态，跳过引用检查。
                        if rule.schema.nullable && value.is_null() {
                            continue;
                        }
                        let Some(referred) = value.as_str().and_then(kk_world::ObjId::parse) else {
                            return Err(SettleError(format!(
                                "扩展引用值不是合法对象 ID：房间 {room_name} {field}"
                            )));
                        };
                        let exists = cand
                            .rooms
                            .iter()
                            .any(|(_, r)| r.objects.contains_key(&referred));
                        if !exists {
                            reconcile.push((
                                room_name.clone(),
                                *id,
                                field.clone(),
                                rule.schema.nullable,
                            ));
                        }
                    }
                }
            }
        }
        for (room_name, id, field, nullable) in reconcile {
            let (_, room) = cand
                .rooms
                .iter_mut()
                .find(|(name, _)| name == &room_name)
                .expect("房间来自候选");
            let Some(fields) = room.extensions.get_mut(&id) else {
                continue;
            };
            if nullable {
                fields.insert(field, serde_json::Value::Null);
            } else {
                fields.remove(&field);
                if fields.is_empty() {
                    room.extensions.remove(&id);
                }
            }
        }
        Ok(())
    }

    /// 将已校验候选写回内存；不返回 Result，也不捕获 panic。
    /// World::commit_room 不递增版本。全部房间写回后须先推进时间，再允许
    /// 查询世界，使目录和投影缓存一起失效。
    fn publish_tick(&mut self, prepared: txn::PreparedTick) {
        self.market = prepared.market;
        for (name, room) in prepared.rooms {
            self.world.commit_room(name, room);
        }
        self.players = prepared.players;
        self.mod_states = prepared.mod_states;
        self.next_id = prepared.next_id;
        self.world.bump_next_obj_id(self.next_id);
        self.last_events = prepared.events;
        self.world.advance_ticks(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_writebacks_publish_with_market_and_retry_without_partial_commit() {
        let stages = [
            TickStage::Prepare,
            TickStage::Settle,
            TickStage::Market,
            TickStage::Validate,
            TickStage::Publish,
        ]
        .map(|stage| (Some(stage), None));
        let steps = [
            WritebackStep::Market,
            WritebackStep::Cpu,
            WritebackStep::RawMemory,
            WritebackStep::Segments,
        ]
        .map(|step| (None, Some(step)));
        for (stage, step) in stages.into_iter().chain(steps) {
            let mut engine = Engine::default();
            engine.players.insert(
                "p1".into(),
                PlayerState {
                    money: 10_000_000,
                    gcl: 17,
                    raw_memory: "old".into(),
                    segments: BTreeMap::from([(5, "old segment".into())]),
                    ..Default::default()
                },
            );
            let original = engine.players.clone();
            let market = engine.market.clone();
            let generation = engine.world.generation();
            let next_id = engine.next_id;
            let mut batch = IntentBatch::default();
            batch.users.entry("p1".into()).or_default().market.push(
                crate::intent::MarketIntent::CreateOrder {
                    order_type: Some("sell".into()),
                    resource_type: Some("token".into()),
                    price_milli: Some(1000),
                    total_amount: Some(100),
                    room_name: None,
                },
            );
            let patches = BTreeMap::from([(
                "p1".into(),
                PlayerWriteback {
                    cpu: Some(kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, 13.0)),
                    raw_memory: Some("new".into()),
                    segments: Some(SegmentWriteback::from_runner(&json!({
                        "writes": [[5, "new segment"]], "active": [5], "public": [5],
                        "defaultPublic": 5, "foreign": {"username": "p2", "id": 7}
                    }))),
                },
            )]);
            let input = TickInput { now_ms: 1234 };
            engine.set_fail_stage_for_test(stage);
            engine.set_fail_writeback_for_test(step);
            assert!(
                engine
                    .tick_with_writebacks(batch.clone(), input, patches.clone())
                    .is_err()
            );
            assert_eq!(engine.players, original, "{stage:?}");
            assert_eq!(engine.market, market, "{stage:?}");
            assert_eq!(engine.world.time(), 0);
            assert_eq!(engine.world.generation(), generation);
            assert_eq!(engine.next_id, next_id);
            engine.set_fail_stage_for_test(None);
            engine.set_fail_writeback_for_test(None);
            engine.tick_with_writebacks(batch, input, patches).unwrap();
            let player = &engine.players["p1"];
            assert_eq!(player.cpu.bucket(), 13.0);
            assert_eq!(player.raw_memory, "new");
            assert_eq!(player.segments[&5], "new segment");
            assert_eq!(player.active_request, vec![5]);
            assert_eq!(player.public_ids, vec![5]);
            assert_eq!(player.default_public, Some(5));
            assert_eq!(player.foreign_request, Some(("p2".into(), Some(7))));
            assert_eq!(player.gcl, 17);
            assert_eq!(player.money, 9_995_000, "内存写回不能覆盖市场扣费");
            assert_eq!(engine.market.orders.len(), 1);
            assert_eq!(engine.world.time(), 1);
        }
    }

    #[test]
    fn unknown_writeback_player_rejects_entire_tick() {
        let mut engine = Engine::default();
        engine.players.entry("p1".into()).or_default();
        let original = engine.players.clone();
        let patches = BTreeMap::from([
            (
                "p1".into(),
                PlayerWriteback {
                    raw_memory: Some("new".into()),
                    ..Default::default()
                },
            ),
            ("unknown".into(), PlayerWriteback::default()),
        ]);
        assert!(
            engine
                .tick_with_writebacks(IntentBatch::default(), TickInput { now_ms: 0 }, patches)
                .is_err()
        );
        assert_eq!(engine.players, original);
        assert_eq!(engine.world.time(), 0);
    }

    #[test]
    fn scripted_tick_keeps_market_intents_bound_to_registered_player() {
        let mut engine = Engine::default();
        engine.players.entry("p1".into()).or_default().money = 10_000_000;
        engine.players.entry("p2".into()).or_default().money = 10_000_000;
        let mut batch = IntentBatch::default();
        for player in ["p1", "p2"] {
            batch.users.entry(player.into()).or_default().market.push(
                crate::intent::MarketIntent::CreateOrder {
                    order_type: Some("sell".into()),
                    resource_type: Some("token".into()),
                    price_milli: Some(1000),
                    total_amount: Some(100),
                    room_name: None,
                },
            );
        }
        let mut sandbox = crate::sandbox::ScriptedSandbox::new();
        sandbox.at(0, batch);
        engine.sandboxes.insert("p1".into(), Box::new(sandbox));
        engine.tick().unwrap();
        assert_eq!(engine.market.orders.len(), 1);
        assert_eq!(engine.market.orders["mkt1"].user, "p1");
        assert_eq!(engine.players["p1"].money, 9_995_000);
        assert_eq!(engine.players["p2"].money, 10_000_000);
    }

    #[test]
    fn restored_room_uses_directory_name_during_settlement() {
        let mut world = kk_world::World::new(1);
        world.register_room("W1N1", kk_core::Terrain::plains());
        let room = world.room("W1N1").unwrap().clone();
        let restored = RoomState::decode_snapshot(&room.encode_snapshot(0, 1, 0)).unwrap();
        assert!(restored.name.is_empty());
        world.commit_room("W1N1".into(), restored);
        let mut engine = Engine::new(world);
        let mut batch = IntentBatch::default();
        batch.add_room_intent(
            "p1",
            crate::intent::RoomIntent::CreateFlag {
                room: "W1N1".into(),
                x: 10,
                y: 10,
                name: "restored".into(),
                color: 1,
                secondary_color: 1,
            },
        );
        engine.tick_with_intents(batch).unwrap();
        let room = engine.world.room("W1N1").unwrap();
        assert_eq!(room.name, "W1N1");
        assert!(
            room.objects
                .values()
                .any(|o| matches!(o, RoomObject::Flag(f) if f.name == "restored"))
        );
    }

    /// 成功 tick 的全部房间写回只使世界版本递增一次；失败不递增。
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

    // 后一个房间失败时，前一个房间的资源和再生时钟也不能写回。
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

    /// 成功后可继续推进；失败后可重试或放弃，世界已修改时两者均被拒绝。
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
        // 失败后修改世界，使版本检查拒绝重试和放弃恢复。
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

    /// 逐个阶段注入错误，检查世界、玩家资金和市场仍是提交前状态。
    /// 成功对照会使订单过期并退款，因此后两个阶段的失败能检验市场副本隔离。
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
                // 时间戳为 0，在当前墙钟下已超过 30 天，会触发移除和退款。
                created_timestamp: 0,
            });
            engine
        };
        // 先确认无故障时会移除订单并退款，避免对未执行的规则只断言状态不变。
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
                "{stage:?}: 订单簿保持（市场真候选——publish 前不可回退写入不存在）"
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
            // 解除注入后重新结算：订单移除且退款生效。此入口会重新读取墙钟。
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

    /// 市场建单后注入失败，完整市场状态（含私有计数器）与资金均保持不变。
    /// 重试只建一张 mkt1 并扣一次费；下一轮建单使用 mkt2。
    /// 使用 token 订单免去 terminal 条件，集中检验建单、序号和扣费的提交时点。
    #[test]
    fn market_candidacy_counter_snapshot_and_retry_id_stability() {
        let mut world = kk_world::World::new(1);
        world.register_room("W1N1", kk_core::Terrain::plains());
        let mut engine = Engine::new(world);
        const SEED: i64 = 10_000_000;
        engine.players.entry("p1".to_string()).or_default().money = SEED;
        // fee = ceil(毫price × total × 0.05) = ceil(1000×100×0.05) = 5000
        const FEE: i64 = 5000;
        let mut batch = IntentBatch::default();
        batch
            .users
            .entry("p1".to_string())
            .or_default()
            .market
            .push(crate::intent::MarketIntent::CreateOrder {
                order_type: Some("sell".to_string()),
                resource_type: Some("token".to_string()),
                price_milli: Some(1000),
                total_amount: Some(100),
                room_name: None,
            });
        let before = engine.market.clone();
        // 建单之后、发布之前注入失败，检查候选没有改动已提交状态。
        engine.set_fail_stage_for_test(Some(TickStage::Validate));
        assert!(engine.tick_with_intents(batch.clone()).is_err());
        assert_eq!(
            engine.market, before,
            "失败轮 committed 市场整态保持（含私有计数器——seq 不前进）"
        );
        assert_eq!(engine.players["p1"].money, SEED, "失败轮扣费不落 committed");
        // 重试（同 batch）：恰一次建单、id 稳定为首次分配值、扣费恰一次
        engine.set_fail_stage_for_test(None);
        engine.tick_with_intents(batch).unwrap();
        assert_eq!(engine.market.orders.len(), 1, "重试恰一次建单");
        assert!(
            engine.market.orders.contains_key("mkt1"),
            "订单 id 与首次尝试一致（失败轮候选丢弃 ⇒ 序号未消耗）"
        );
        assert_eq!(
            engine.players["p1"].money,
            SEED - FEE,
            "重试恰一次扣费（双扣形态为 SEED−2×FEE）"
        );
        // 第二次建单继续递增序号，不能重用 mkt1。
        let mut second = IntentBatch::default();
        second
            .users
            .entry("p1".to_string())
            .or_default()
            .market
            .push(crate::intent::MarketIntent::CreateOrder {
                order_type: Some("sell".to_string()),
                resource_type: Some("pixel".to_string()),
                price_milli: Some(1000),
                total_amount: Some(100),
                room_name: None,
            });
        engine.tick_with_intents(second).unwrap();
        assert!(
            engine.market.orders.contains_key("mkt2"),
            "第二张订单 id=mkt2（序号从 committed 续档，非重用）"
        );
    }

    /// 分别破坏房间覆盖、顺序、ID 游标和玩家集合，验证各项候选校验会拒绝。
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
        let mk_cand = |next| {
            let mut txn = engine
                .prepare_tick(IntentBatch::default(), TickInput { now_ms: 0 })
                .unwrap();
            txn.state.inner.borrow_mut().next_id = next;
            txn.rooms = txn
                .room_names
                .iter()
                .map(|name| (name.clone(), engine.world.room(name).unwrap().clone()))
                .collect();
            txn
        };
        engine.validate_stage(&mut mk_cand(engine.next_id)).unwrap();
        let mut short = mk_cand(engine.next_id);
        short.rooms.pop();
        let err = engine.validate_stage(&mut short).unwrap_err();
        assert!(err.to_string().contains("覆盖"), "{err}");
        let mut swapped = mk_cand(engine.next_id);
        swapped.rooms.swap(0, 1);
        let err = engine.validate_stage(&mut swapped).unwrap_err();
        assert!(err.to_string().contains("覆盖"), "{err}");
        let err = engine.validate_stage(&mut mk_cand(0)).unwrap_err();
        assert!(err.to_string().contains("回退"), "{err}");
        let mut cand = mk_cand(engine.next_id);
        cand.state.inner.borrow_mut().players.remove("p1");
        let err = engine.validate_stage(&mut cand).unwrap_err();
        assert!(err.to_string().contains("丢键"), "{err}");
    }
}
