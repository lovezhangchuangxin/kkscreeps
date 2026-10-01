//! 玩家指令（intent）模型：输入清洗、同对象覆盖、冲突裁决。
//!
//! 官方语义（本地官方源码 ~/Desktop/projects/sources/screeps/）：
//! - 字段白名单与 transforms：`common/lib/system.js`——number 字段一律按
//!   parseInt 截断（见 [`to_int`]）、bodypart[] 过滤非白名单部件、userString
//!   截 100、白名单外的未知字段丢弃；
//! - 同对象同名指令后者覆盖前者（只记最后一次）；
//! - 冲突裁决优先级图：`engine/src/processor/intents/creeps/intents.js`——
//!   动作执行当且仅当其 intent 存在**且**无更高优先级 intent 存在。被压制的
//!   动作不执行，但玩家 API 早已返回成功——这是官方语义，不是缺陷；
//! - 执行顺序：creepActions 数组序（intents.js:15-17）。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::Value;

/// 动作名（官方 creepActions 全集 + spawn 的 createCreep + controller 的
/// activateSafeMode）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Drop,
    Transfer,
    Withdraw,
    Pickup,
    Heal,
    RangedHeal,
    Dismantle,
    Attack,
    Harvest,
    Move,
    Repair,
    Build,
    RangedMassAttack,
    RangedAttack,
    Say,
    Suicide,
    ClaimController,
    UpgradeController,
    ReserveController,
    AttackController,
    GenerateSafeMode,
    SignController,
    Pull,
    /// 结构（及 creep——API 面只在结构类暴露）的通知开关（官方挂在
    /// processor.js:313-314 的对象级通用段，不在 creepActions 序列）。
    NotifyWhenAttacked,
    /// spawn 对象的动作（官方 spawns/intents.js:3-24 的分派序）。
    CreateCreep,
    /// renewCreep（白名单 `{id: 'string'}`，system.js:62；结算链见
    /// settle/spawn.rs intent_renew_creep——对照 spawns/renew-creep.js）。
    RenewCreep,
    /// recycleCreep（白名单 `{id: 'string'}`，system.js:60；
    /// spawns/recycle-creep.js）。
    RecycleCreep,
    /// setSpawnDirections（白名单 `{directions: 'number[]'}`，system.js:73；
    /// spawns/set-spawn-directions.js）。
    SetSpawnDirections,
    /// cancelSpawning（空白名单 `{}`，system.js:39；
    /// spawns/cancel-spawning.js）。
    CancelSpawning,
    /// controller 对象的动作（官方 processor.js:306-311 分派，intent 白名单
    /// `activateSafeMode: {}`——system.js:34；语义链见 settle/controller.rs）。
    /// 与 CreateCreep 同理不在 creepActions 序列（EXECUTION_ORDER）里。
    ActivateSafeMode,
    /// controller 对象的动作（官方 processor.js:307-309 先于 activateSafeMode
    /// 分派，intent 白名单 `unclaim: {}`——system.js:78；语义链见
    /// settle/controller.rs）。A4-iii 补（B2 时登记"白名单外动作清洗层丢弃"）。
    Unclaim,
    /// rampart 对象的动作（官方 processor.js:266-269 分派，白名单
    /// `setPublic: {isPublic: 'boolean'}`——system.js:72；执行器
    /// ramparts/set-public.js 仅 `isPublic: !!intent.isPublic` 写回）。
    /// M2B 扩面 P5。
    SetPublic,
    /// terminal 对象的动作（官方 processor.js:272-275 分派，白名单
    /// `send: {targetRoomName: 'string', resourceType: 'string',
    /// amount: 'number', description: 'userString'}`——system.js:69）。
    /// 房间相执行器（terminal/send.js）**只校验并落 `object.send` 字段**，
    /// 跨房转移在全局阶段执行（global-intents/market.js:71-95——2B 批 P1）。
    Send,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Transfer => "transfer",
            Self::Withdraw => "withdraw",
            Self::Pickup => "pickup",
            Self::Heal => "heal",
            Self::RangedHeal => "rangedHeal",
            Self::Dismantle => "dismantle",
            Self::Attack => "attack",
            Self::Harvest => "harvest",
            Self::Move => "move",
            Self::Repair => "repair",
            Self::Build => "build",
            Self::RangedMassAttack => "rangedMassAttack",
            Self::RangedAttack => "rangedAttack",
            Self::Say => "say",
            Self::Suicide => "suicide",
            Self::ClaimController => "claimController",
            Self::UpgradeController => "upgradeController",
            Self::ReserveController => "reserveController",
            Self::AttackController => "attackController",
            Self::GenerateSafeMode => "generateSafeMode",
            Self::SignController => "signController",
            Self::Pull => "pull",
            Self::NotifyWhenAttacked => "notifyWhenAttacked",
            Self::CreateCreep => "createCreep",
            Self::RenewCreep => "renewCreep",
            Self::RecycleCreep => "recycleCreep",
            Self::SetSpawnDirections => "setSpawnDirections",
            Self::CancelSpawning => "cancelSpawning",
            Self::ActivateSafeMode => "activateSafeMode",
            Self::Unclaim => "unclaim",
            Self::SetPublic => "setPublic",
            Self::Send => "send",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "drop" => Self::Drop,
            "transfer" => Self::Transfer,
            "withdraw" => Self::Withdraw,
            "pickup" => Self::Pickup,
            "heal" => Self::Heal,
            "rangedHeal" => Self::RangedHeal,
            "dismantle" => Self::Dismantle,
            "attack" => Self::Attack,
            "harvest" => Self::Harvest,
            "move" => Self::Move,
            "repair" => Self::Repair,
            "build" => Self::Build,
            "rangedMassAttack" => Self::RangedMassAttack,
            "rangedAttack" => Self::RangedAttack,
            "say" => Self::Say,
            "suicide" => Self::Suicide,
            "claimController" => Self::ClaimController,
            "upgradeController" => Self::UpgradeController,
            "reserveController" => Self::ReserveController,
            "attackController" => Self::AttackController,
            "generateSafeMode" => Self::GenerateSafeMode,
            "signController" => Self::SignController,
            "pull" => Self::Pull,
            "notifyWhenAttacked" => Self::NotifyWhenAttacked,
            "createCreep" => Self::CreateCreep,
            "renewCreep" => Self::RenewCreep,
            "recycleCreep" => Self::RecycleCreep,
            "setSpawnDirections" => Self::SetSpawnDirections,
            "cancelSpawning" => Self::CancelSpawning,
            "activateSafeMode" => Self::ActivateSafeMode,
            "unclaim" => Self::Unclaim,
            "setPublic" => Self::SetPublic,
            "send" => Self::Send,
            _ => return None,
        })
    }

    /// 官方 priorities 图（intents.js:3-13）：返回能压制本动作的高优先级动作集。
    fn suppressed_by(self) -> &'static [Action] {
        match self {
            Self::RangedHeal => &[Self::Heal],
            Self::AttackController => &[Self::RangedHeal, Self::Heal],
            Self::Dismantle => &[Self::AttackController, Self::RangedHeal, Self::Heal],
            Self::Repair => &[
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Build => &[
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Attack => &[
                Self::Build,
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::Harvest => &[
                Self::Attack,
                Self::Build,
                Self::Repair,
                Self::Dismantle,
                Self::AttackController,
                Self::RangedHeal,
                Self::Heal,
            ],
            Self::RangedMassAttack => &[Self::Build, Self::Repair, Self::RangedHeal],
            Self::RangedAttack => &[
                Self::RangedMassAttack,
                Self::Build,
                Self::Repair,
                Self::RangedHeal,
            ],
            _ => &[],
        }
    }

    /// checkPriorities（intents.js:21-23）：动作存在且未被更高优先级动作压制。
    pub fn executes(present: &BTreeSet<Action>, action: Action) -> bool {
        present.contains(&action) && !action.suppressed_by().iter().any(|a| present.contains(a))
    }
}

/// 官方执行顺序（creepActions，intents.js:15-17）。CreateCreep/RenewCreep/
/// RecycleCreep/SetSpawnDirections/CancelSpawning 是 spawn 对象的动作
/// （spawns/intents.js:3-24 按 createCreep→renewCreep→recycleCreep→
/// setSpawnDirections→cancelSpawning 的序分派），不在 creep 序列里、由
/// settle/spawn.rs 的 spawn 分派单独执行。
pub const EXECUTION_ORDER: [Action; 23] = [
    Action::Drop,
    Action::Transfer,
    Action::Withdraw,
    Action::Pickup,
    Action::Heal,
    Action::RangedHeal,
    Action::Dismantle,
    Action::Attack,
    Action::Harvest,
    Action::Move,
    Action::Repair,
    Action::Build,
    Action::RangedMassAttack,
    Action::RangedAttack,
    Action::Say,
    Action::Suicide,
    Action::ClaimController,
    Action::UpgradeController,
    Action::ReserveController,
    Action::AttackController,
    Action::GenerateSafeMode,
    Action::SignController,
    Action::Pull,
];

/// 单条对象级指令的载荷（M1 所需字段；字段名对齐官方白名单）。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentData {
    /// move 的 direction / 各动作的 target id（官方字段 id）。
    pub id: Option<ObjId>,
    pub direction: Option<u8>,
    pub amount: Option<i64>,
    pub resource_type: Option<String>,
    pub x: Option<i64>,
    pub y: Option<i64>,
    /// createCreep 专属（挂在 spawn 对象上）。
    pub name: Option<String>,
    /// say 专属（system.js:68 `message: 'userString'`——截 100；官方 transform
    /// 对缺省折叠为 ""，我方 None 由执行器按 "" 对齐）。
    pub message: Option<String>,
    /// say 专属（`isPublic: 'boolean'`——!!value）。
    pub is_public: Option<bool>,
    /// notifyWhenAttacked 专属（system.js:51 `enabled: 'boolean'`——!!value）。
    pub enabled: Option<bool>,
    /// signController 专属：官方白名单 `sign: 'userString'`（system.js:74），
    /// 截 100（UTF-16 码元 vs Unicode chars 的差异沿用既有灰区登记）。
    /// 官方形态缺省与空串同值（userString 的 `value||""` 折叠），signController
    /// 结算层 `intent.sign ?` 判空——两者都落 None 语义（清除签名）。
    pub sign: Option<String>,
    pub body: Vec<BodyKind>,
    /// None = 玩家未传（缺省全房扣费）；Some([]) = 显式空列表（官方走
    /// "仅列表扣费"模式——可用 0 → 失败）。presence 由 JSON 键存在性区分
    /// （官方 system.js transforms 不区分，语义分叉在 create-creep.js:19-24）。
    pub energy_structures: Option<Vec<ObjId>>,
    /// 官方链路分两步：system.js 的 number[] transform 逐元素 parseInt（在此
    /// 完成，无法解析的元素以 i64::MIN 哨兵传递）；create-creep.js:27 再
    /// _.uniq 保序全局去重。1..=8 校验在 createCreep 侧整单拒绝（官方语义是
    /// 拒绝整条指令，不是过滤元素）。
    pub directions: Vec<i64>,
    /// send（terminal）专属：官方白名单 `send: {targetRoomName: 'string',
    /// resourceType: 'string', amount: 'number', description: 'userString'}`
    /// （common/lib/system.js:69）——targetRoomName 严格串（沿用
    /// resource_type 的 as_str 口径）；description userString 截 100。
    pub target_room_name: Option<String>,
    pub description: Option<String>,
}

impl IntentData {
    pub fn target(id: ObjId) -> Self {
        Self {
            id: Some(id),
            direction: None,
            amount: None,
            resource_type: None,
            x: None,
            y: None,
            name: None,
            message: None,
            is_public: None,
            enabled: None,
            sign: None,
            body: Vec::new(),
            energy_structures: None,
            directions: Vec::new(),
            target_room_name: None,
            description: None,
        }
    }

    pub fn bare() -> Self {
        Self {
            id: None,
            direction: None,
            amount: None,
            resource_type: None,
            x: None,
            y: None,
            name: None,
            message: None,
            is_public: None,
            enabled: None,
            sign: None,
            body: Vec::new(),
            energy_structures: None,
            directions: Vec::new(),
            target_room_name: None,
            description: None,
        }
    }
}

/// 房间级指令（官方挂在 objects['room'] 下，数组形态）。
#[derive(Debug, Clone, PartialEq)]
pub enum RoomIntent {
    CreateConstructionSite {
        room: String,
        x: i64,
        y: i64,
        structure_type: String,
        name: Option<String>,
    },
    CreateFlag {
        room: String,
        x: i64,
        y: i64,
        name: String,
        color: i64,
        secondary_color: i64,
    },
    /// structure.destroy()（B3）：官方白名单 `destroyStructure: {roomName:
    /// 'string', id: 'string'}`（system.js:20）；结算链 settle/room.rs
    /// 对照 room/destroy-structure.js。
    DestroyStructure { room: String, id: ObjId },
    /// flag.remove/setPosition/setColor（M2B 扩面 P2）：官方均为 room 级
    /// removeFlag+createFlag 组合（flags.js:59-90）；结算对照 room/remove-flag.js
    /// （按玩家旗名删——只删自家）。
    RemoveFlag { room: String, name: String },
    /// constructionSite.remove()（P2）：官方 room 级 `removeConstructionSite:
    /// {roomName, id}`；结算对照 room/remove-construction-site.js（所有权门 +
    /// progress/2 退能量掉落）。
    RemoveConstructionSite { room: String, id: ObjId },
}

/// room 级 intent 参数解析（官方 system.js:116-140 sanitizeUserRoomIntents 的
/// 字段面 + room/intents.js 消费字段；坐标/颜色走 parseInt，名称走 userString）。
fn parse_room_intent(name: &str, raw: &Value) -> Option<RoomIntent> {
    let f = |k: &str| raw.get(k);
    let room = f("roomName").and_then(Value::as_str)?.to_string();
    match name {
        "createConstructionSite" => Some(RoomIntent::CreateConstructionSite {
            room,
            x: to_int(f("x"))?,
            y: to_int(f("y"))?,
            structure_type: f("structureType").and_then(Value::as_str)?.to_string(),
            name: to_user_string(f("name")),
        }),
        "createFlag" => Some(RoomIntent::CreateFlag {
            room,
            x: to_int(f("x"))?,
            y: to_int(f("y"))?,
            name: to_user_string(f("name"))?,
            color: to_int(f("color")).unwrap_or(0),
            secondary_color: to_int(f("secondaryColor")).unwrap_or(0),
        }),
        // system.js:20：`destroyStructure: {roomName: 'string', id: 'string'}`
        "destroyStructure" => Some(RoomIntent::DestroyStructure {
            room,
            id: to_id(f("id"))?,
        }),
        // P2：`removeFlag: {roomName: 'string', name: 'userString'}` /
        // `removeConstructionSite: {roomName: 'string', id: 'string'}`
        // （flags.js:59-64 / construction-sites.js:56 的官方 intent 形态）
        "removeFlag" => Some(RoomIntent::RemoveFlag {
            room,
            name: to_user_string(f("name"))?,
        }),
        "removeConstructionSite" => Some(RoomIntent::RemoveConstructionSite {
            room,
            id: to_id(f("id"))?,
        }),
        _ => None,
    }
}

/// 单玩家的指令集：对象级（对象 → 动作 → 载荷，同名覆盖）+ 房间级列表。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserIntents {
    pub objects: BTreeMap<ObjId, BTreeMap<Action, IntentData>>,
    pub room: Vec<RoomIntent>,
    /// Game.notify（M2B 扩面 P3）：官方全局 intent 通道（runtime.js:78-86
    /// `intents.push('notify', ...)`——list 顶层按动作名键的数组，非对象 id
    /// 路由）。清洗 schema：`notify: {message: 'userText', groupInterval:
    /// 'number'}`（common/lib/system.js:17 notify 行）
    pub notify: Vec<NotifyIntent>,
    /// Game.market 全局 intent 族（2B 批）：官方 `intents.pushByName('global',
    /// …)` 容器（runtime.js:82-90）→ storeIntents 的 global 分支
    /// （engine/src/utils.js:421-424）→ sanitizeUserIntents 白名单（五个
    /// 市场键，common/lib/system.js:23-29）。maxLen 门（deal 10 /
    /// cancelOrder·changeOrderPrice·extendOrder 50）在 prelude 发射层。
    pub market: Vec<MarketIntent>,
}

/// 一条玩家通知意图（Game.notify）。
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyIntent {
    pub message: String,
    pub group_interval: Option<i64>,
}

/// 一条全局市场意图（Game.market 五动作，清洗后形态）。
///
/// 字段全 Option：官方 sanitizeIntent 对白名单字段**无条件物化**（缺键 →
/// string transform 得 "undefined" / number transform 得 NaN）——我方 None
/// ≙ 官方 "undefined"/NaN。净等价性**逐字段**成立但路径各异（评审 1 勘误
/// 泛化声明）：多数字段经下游 falsy 门/查表 miss 净跳过；
/// **CreateOrder.order_type 例外**——官方处理器
/// `type: intent.type == ORDER_SELL ? SELL : BUY`（global-intents/market.js:139）
/// 把物化串默认落为合法 ORDER_BUY，等价性纯靠 prelude 不可达（game 侧
/// `type != BUY && != SELL → ERR_INVALID_ARGS` market.js:75 先于 push）——
/// **P2 处理器落地时 order_type=None 必须直接拒绝，不得默认 buy/sell**；
/// Deal.amount 的 NaN 首门是比较门（`amount <= 0` 对 NaN 假——market.js:272），
/// 净跳过由下游 terminalsByRoom miss / `!(amount > 0)` 兜底，处理器注释勿引
/// 用「falsy 首门」。（登记：官方 intershard 订单 roomName 落 "undefined"
/// 字面串、我方 None，未测面。）
#[derive(Debug, Clone, PartialEq)]
pub enum MarketIntent {
    /// `createOrder: {type:'string', resourceType:'string', price:'price',
    /// totalAmount:'number', roomName:'string'}`（common/lib/system.js:25）。
    CreateOrder {
        /// P2 处理器契约：None/非 buy|sell 值直接拒（见枚举文档）。
        order_type: Option<String>,
        resource_type: Option<String>,
        /// 官方 price transform：credits 浮点 → **整数毫 credits**。
        price_milli: Option<i64>,
        total_amount: Option<i64>,
        room_name: Option<String>,
    },
    /// `cancelOrder: {orderId: 'string'}`（system.js:24）。
    CancelOrder { order_id: Option<String> },
    /// `changeOrderPrice: {orderId:'string', newPrice:'price'}`（system.js:26）。
    ChangeOrderPrice {
        order_id: Option<String>,
        new_price_milli: Option<i64>,
    },
    /// `extendOrder: {orderId:'string', addAmount:'number'}`（system.js:32）。
    ExtendOrder {
        order_id: Option<String>,
        add_amount: Option<i64>,
    },
    /// `deal: {orderId:'string', amount:'number', targetRoomName:'string'}
    /// （system.js:27）。
    Deal {
        order_id: Option<String>,
        amount: Option<i64>,
        target_room_name: Option<String>,
    },
}

/// 一 tick 的全部玩家指令（玩家名排序——结算序确定性，方案 D5）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntentBatch {
    pub users: BTreeMap<String, UserIntents>,
}

impl IntentBatch {
    /// 沙箱传出的 intentsList（官方形态：`{对象id: {动作名: 参数}}`，room 级
    /// 挂 'room' 键数组）→ 清洗后的 IntentBatch。**生产清洗入口**（M2A-A2
    /// 接线）：对照官方链路——沙箱传出原始 list（runtime.js:271-296），宿主
    /// 进程逐字段白名单清洗（utils.js:405-436 storeIntents → system.js）。
    /// 不可信输入纪律：未知动作名丢弃（sanitize 白名单语义）、对象 id 不可
    /// 解析丢弃（官方 utils.js:427-429 同款）、参数只保留白名单字段。
    /// 身份边界：全部归 `player`（调用方保证 = 沙箱注册键；settle 侧另有
    /// 结构性复核）。
    pub fn from_json(player: &str, list: &Value) -> IntentBatch {
        let mut batch = IntentBatch::default();
        let Some(objects) = list.as_object() else {
            return batch;
        };
        for (obj_key, actions) in objects {
            if obj_key == "notify" {
                // 全局通道（官方 utils.js storeIntents :411-414 的 notify 分支：
                // sanitizeUserIntents 白名单清洗）：message userText、
                // groupInterval number；缺省 message 容空串（官方 userText
                // 对 undefined 的强转语义近似——落库面非玩家可观测）
                let Some(items) = actions.as_array() else {
                    continue;
                };
                let entry = batch.users.entry(player.to_string()).or_default();
                for item in items {
                    entry.notify.push(NotifyIntent {
                        message: to_user_text(item.get("message")),
                        group_interval: to_int(item.get("groupInterval")),
                    });
                }
                continue;
            }
            if obj_key == "global" {
                // 全局市场通道（官方 storeIntents utils.js:421-424 的 global 分支）：
                // 容器形态 {动作名: [载荷…]}（runtime.js:82-90 pushByName
                // ('global',…)）。迭代序=官方 sanitizeUserIntents 的
                // `for(name in intentTypes)` **声明序**（common/lib/system.js
                // :23-29 的五市场键子集——非 JSON 键序；跨动作序对结算不可
                // 观测——market 处理器按分节序消费，此处固定序保确定性）；
                // 同动作数组保持 push 序。白名单外丢弃（intentTypes 键集语义）
                static MARKET_INTENT_ORDER: [&str; 5] = [
                    "cancelOrder",
                    "changeOrderPrice",
                    "createOrder",
                    "deal",
                    "extendOrder",
                ];
                let Some(actions) = actions.as_object() else {
                    continue;
                };
                let entry = batch.users.entry(player.to_string()).or_default();
                for name in MARKET_INTENT_ORDER {
                    let Some(items) = actions.get(name).and_then(Value::as_array) else {
                        continue;
                    };
                    for item in items {
                        if let Some(intent) = parse_market_intent(name, item) {
                            entry.market.push(intent);
                        }
                    }
                }
                continue;
            }
            if obj_key == "room" {
                // room 级：pushByName('room', name, data) 的数组形态
                // （runtime.js:80-90；room/intents.js 逐条消费）
                let Some(actions) = actions.as_object() else {
                    continue;
                };
                for (name, arr) in actions {
                    let Some(items) = arr.as_array() else {
                        continue;
                    };
                    for item in items {
                        if let Some(intent) = parse_room_intent(name, item) {
                            batch.add_room_intent(player, intent);
                        }
                    }
                }
                continue;
            }
            let Some(obj) = ObjId::parse(obj_key) else {
                continue; // 官方 utils.js:427-429：反查不到的对象丢弃
            };
            let Some(actions) = actions.as_object() else {
                continue;
            };
            for (name, params) in actions {
                let Some(action) = Action::parse(name) else {
                    continue; // 未知动作名：sanitizeUserIntents 白名单外丢弃
                };
                let data = sanitize_object_intent(action, params);
                batch.add_object_intent(player, obj, action, data);
            }
        }
        batch
    }

    pub fn add_object_intent(
        &mut self,
        player: &str,
        obj: ObjId,
        action: Action,
        data: IntentData,
    ) {
        self.users
            .entry(player.to_string())
            .or_default()
            .objects
            .entry(obj)
            .or_default()
            .insert(action, data);
    }

    /// 合并另一批（同玩家键合并 objects/room；后写覆盖同名动作——幂等语义）。
    pub fn extend_from(&mut self, other: IntentBatch) {
        for (player, intents) in other.users {
            let entry = self.users.entry(player).or_default();
            entry.objects.extend(intents.objects);
            entry.room.extend(intents.room);
            entry.notify.extend(intents.notify);
            entry.market.extend(intents.market);
        }
    }

    pub fn add_room_intent(&mut self, player: &str, intent: RoomIntent) {
        self.users
            .entry(player.to_string())
            .or_default()
            .room
            .push(intent);
    }
}

/// 按 JS `parseInt` 的前缀规则读取 JSON 数字或字符串；超出 i64 的值无效。
/// `number[]` 也使用这条规则，调用方保留无效值供后续校验。
fn to_int(v: Option<&Value>) -> Option<i64> {
    let text = match v? {
        Value::Number(n) => {
            let value = n.as_f64()?;
            if !value.is_finite() {
                return None;
            }
            let magnitude = value.abs();
            // 非零 JS Number 在这个区间外使用指数形式；parseInt 只读首位数字。
            if magnitude < 1e-6 || magnitude >= 1e21 {
                let first = format!("{magnitude:e}").as_bytes()[0] - b'0';
                return Some(if value.is_sign_negative() {
                    -(first as i64)
                } else {
                    first as i64
                });
            }
            let integer = value.trunc();
            if !(i64::MIN as f64..9_223_372_036_854_775_808.0).contains(&integer) {
                return None;
            }
            return Some(integer as i64);
        }
        Value::String(s) => s.clone(),
        _ => return None,
    };
    let text = text.trim_start();
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = if digits.starts_with("0x") || digits.starts_with("0X") {
        (16, &digits[2..])
    } else {
        (10, digits)
    };
    let prefix = digits
        .bytes()
        .take_while(|b| (*b as char).is_digit(radix))
        .count();
    if prefix == 0 {
        return None;
    }
    let value = i128::from_str_radix(&digits[..prefix], radix).ok()?;
    i64::try_from(if negative { -value } else { value }).ok()
}

fn to_id(v: Option<&Value>) -> Option<ObjId> {
    v.and_then(Value::as_str).and_then(ObjId::parse)
}

/// 官方 boolean transform（system.js:8 `value => !!value`）：JS 真值表——
/// null/false/0/-0/NaN/'' 假，其余真（NaN 不出现在严格 JSON）。缺键 None
/// （与官方 transform 后的显式 false 区分——say/notifyWhenAttacked 结算层
/// 均按 falsy 折叠，None ≙ undefined）。
fn to_bool(v: Option<&Value>) -> Option<bool> {
    v.map(|x| match x {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    })
}

/// 官方 userString（system.js:12）：`("" + (value||"")).substring(0, 100)`。
/// 官方 userText 清洗（common/lib/system.js:13）：`("" + (value||""))`
/// .substring(0, 1000)——falsy 原语归空串、数组 join(",")、对象
/// "[object Object]" 的 JS ToString 语义（评审 1 P1-1 清偿：notify message
/// 原误走 userString(100) 帽）。浮点指数形与 JS 数字字面量的格式差异
/// 登记兼容性.md
fn js_value_falsy(x: &Value) -> bool {
    match x {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        _ => false,
    }
}

/// JS Array.prototype.toString = join(",")（嵌套递归展平；元素强转是**普通
/// ToString**——join 只对 null/undefined 产空串，0/"false" 等照常字符串化，
/// 元素对象 → "[object Object]"。清偿复审阻断项修复：原实现把顶层
/// `value||""` 的 falsy 折叠误用到 join 元素上）
fn js_array_to_string(items: &[Value]) -> String {
    items
        .iter()
        .map(|it| match it {
            Value::Null => String::new(),
            Value::String(s2) => s2.clone(),
            Value::Bool(_) | Value::Number(_) => it.to_string(),
            Value::Array(nested) => js_array_to_string(nested),
            _ => "[object Object]".to_string(),
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// 官方 userText 清洗（common/lib/system.js:13）：`("" + (value||""))`
/// .substring(0, 1000)——falsy 原语归空串、数组 join、对象
/// "[object Object]" 的 JS ToString 语义（评审 1 P1-1 清偿：notify message
/// 原误走 userString(100) 帽）。浮点指数形与 JS 数字字面量的格式差异
/// 登记兼容性.md
fn to_user_text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(x) if js_value_falsy(x) => String::new(),
        Some(Value::String(s)) => s.chars().take(1000).collect(),
        Some(x @ (Value::Bool(_) | Value::Number(_))) => x.to_string().chars().take(1000).collect(),
        Some(Value::Array(items)) => js_array_to_string(items).chars().take(1000).collect(),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

fn to_user_string(v: Option<&Value>) -> Option<String> {
    v.map(|x| match x {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
    .map(|s| s.chars().take(100).collect())
}

/// 官方 string transform（common/lib/system.js:5 `value => "" + value`，**无长度帽**）：
/// 市场意图的 type/resourceType/roomName/orderId/targetRoomName 面。缺键 →
/// None（官方物化 "undefined" 串——消费端查表 miss/`!intent.x` falsy 门
/// 等价，见 MarketIntent 文档）。数值 ToString 浮点格式差异沿用既有登记；
/// 数组/对象面官方 JS ToString（join(",")/"[object Object]"）vs serde
/// `to_string()`（"[1,2]"/"{...}"）分歧——经官方门查表 miss 不可达，登记
/// （评审 2 P2-3；既有 js_array_to_string 可在需要时复用）。
fn to_plain_string(v: Option<&Value>) -> Option<String> {
    v.map(|x| match x {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// 官方 price transform（common/lib/system.js:8
/// `parseInt((1000 * value).toFixed(0))`）：玩家 credits 浮点 → **整数毫
/// credits**。Rust 等价 `(1000.0*x).round()`：toFixed(0) 取最近邻、恰半时
/// 取大 n（ECMA-262 Number.prototype.toFixed 步骤 9——与 round() 远零半舍
/// 同向，评审 1 node 实测 0.0005→1/−0.0025→−3 双侧吻合）；乘积的 f64
/// 双侧同算。套件价格域（≤3 位小数）乘积为精确整数、无舍入发生。
///
/// **分歧域三处（清偿复审 P1-a 裁决后的最终口径）**：
/// ①JS ToNumber 分歧串（"0x10"=16/""=0 等）→ 我方 parse 失败 None——
///   **真不可达**：game 侧 parseFloat 先于 push（market.js:78）把任何输入
///   变 number（parseFloat("0x10")=0 被 `!price` 门拒），字符串不达 transform；
/// ②milli ∈ [2^63, 1e21) → 我方 None——**表示域强制**（官方 toFixed 产全
///   数字串得 >i64 的精确大整；玩家可达——remaining=0 订单的 changeOrderPrice
///   费恒 0 放行任意 newPrice，清偿复审取证 market.js:163-168——但
///   MarketOrder.price 为 i64 无法承载，登记已知偏差：该域官方改价生效、
///   我方 None→处理器跳过改价不生效，套件零覆盖）；
/// ③|milli| ≥ 1e21 → **保真实现官方怪癖**：toFixed 溢出固定点域走
///   ToString 指数形 "2.5e+21"，parseInt 读首位数字即停 → ±首位（同
///   remaining=0 洞玩家可达——node 实测 t(1e18)=1/t(2.5e18)=2/t(−1e18)=−1；
///   与 to_int 的指数首位先例 :613-619 同型）。
/// 非有限值（NaN/Inf）→ None ≙ 官方 NaN（下游 `!intent.price` falsy 门等价；
/// 官方 (Infinity).toFixed(0) 抛 RangeError 属 runner 崩溃怪癖，我方安全拒绝）。
fn to_price_milli(v: Option<&Value>) -> Option<i64> {
    let x = match v? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Bool(b) => i64::from(*b) as f64,
        // null（JS 1000*null=0）与对象/数组不可达：prelude createOrder 已
        // parseFloat 门（game/market.js:78 parseFloat、:80 `!price` 拒）
        _ => return None,
    };
    let milli = 1000.0 * x;
    if !milli.is_finite() {
        return None;
    }
    if milli.abs() >= 1e21 {
        // 域③官方怪癖：指数形首尾——String(milli) 指数形（Rust {:e} 同为
        // 最短往返表示），parseInt 读到 '.'/'e' 即停 → 首位数字（带号）
        let text = format!("{:e}", milli);
        let digits: String = text
            .trim_start_matches('-')
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let first: i64 = digits.parse().unwrap_or(0);
        return Some(if milli < 0.0 { -first } else { first });
    }
    if !(i64::MIN as f64..9_223_372_036_854_775_808.0).contains(&milli) {
        return None; // 域②：表示域强制（见文档）
    }
    Some(milli.round() as i64)
}

/// 'global' 容器逐条清洗（sanitizeUserIntents 的市场键子集，
/// common/lib/system.js:23-29 intentTypes 五市场键；其余键丢弃——官方
/// 'global' 容器还承载 power-creep 六键（createPowerCreep 等，:26/:28-33，
/// 归 M4 power 批进驻本分支；官方 global 阶段 power 处理器**先于** market，
/// processor/global.js:41-49））。
fn parse_market_intent(name: &str, item: &Value) -> Option<MarketIntent> {
    match name {
        "createOrder" => Some(MarketIntent::CreateOrder {
            order_type: to_plain_string(item.get("type")),
            resource_type: to_plain_string(item.get("resourceType")),
            price_milli: to_price_milli(item.get("price")),
            total_amount: to_int(item.get("totalAmount")),
            room_name: to_plain_string(item.get("roomName")),
        }),
        "cancelOrder" => Some(MarketIntent::CancelOrder {
            order_id: to_plain_string(item.get("orderId")),
        }),
        "changeOrderPrice" => Some(MarketIntent::ChangeOrderPrice {
            order_id: to_plain_string(item.get("orderId")),
            new_price_milli: to_price_milli(item.get("newPrice")),
        }),
        "extendOrder" => Some(MarketIntent::ExtendOrder {
            order_id: to_plain_string(item.get("orderId")),
            add_amount: to_int(item.get("addAmount")),
        }),
        "deal" => Some(MarketIntent::Deal {
            order_id: to_plain_string(item.get("orderId")),
            amount: to_int(item.get("amount")),
            target_room_name: to_plain_string(item.get("targetRoomName")),
        }),
        _ => None,
    }
}

/// 从官方 intent JSON 白名单清洗（system.js intentTypes 对应项）。
/// 未知动作名返回 None；字段缺失按官方 transforms 得 undefined（Rust 侧 None）。
pub fn sanitize_object_intent(_action: Action, raw: &Value) -> IntentData {
    let f = |k: &str| raw.get(k);
    IntentData {
        id: to_id(f("id")),
        direction: to_int(f("direction")).map(|d| d.clamp(0, 255) as u8),
        amount: to_int(f("amount")),
        resource_type: f("resourceType")
            .and_then(Value::as_str)
            .map(str::to_string),
        x: to_int(f("x")),
        y: to_int(f("y")),
        name: to_user_string(f("name")),
        message: to_user_string(f("message")),
        is_public: to_bool(f("isPublic")),
        enabled: to_bool(f("enabled")),
        sign: to_user_string(f("sign")),
        body: f("body")
            .and_then(Value::as_array)
            .map(|arr| {
                // bodypart[]：过滤非白名单部件（system.js:11）
                arr.iter()
                    .filter_map(|p| p.as_str().and_then(BodyKind::parse))
                    .collect()
            })
            .unwrap_or_default(),
        energy_structures: f("energyStructures").and_then(Value::as_array).map(|arr| {
            arr.iter()
                .filter_map(|p| p.as_str().and_then(ObjId::parse))
                .collect()
        }),
        directions: f("directions")
            .and_then(Value::as_array)
            .map(|arr| {
                // 无法解析的值仍须进入后续校验，不能静默过滤。
                let mut seen = BTreeSet::new();
                arr.iter()
                    .map(|p| to_int(Some(p)).unwrap_or(i64::MIN))
                    .filter(|d| seen.insert(*d))
                    .collect()
            })
            .unwrap_or_default(),
        target_room_name: f("targetRoomName")
            .and_then(Value::as_str)
            .map(str::to_string),
        description: to_user_string(f("description")),
    }
}

/// 官方 calcCreepCost（utils.js）：部件成本合计（部件已过白名单过滤）。
pub fn creep_cost(body: &[BodyKind]) -> i64 {
    body.iter()
        .map(|p| {
            let name = p.name();
            c::BODYPART_COST
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| *v)
                .unwrap_or(0)
        })
        .sum()
}

#[cfg(test)]
mod from_json_tests {
    use super::*;

    /// 生产清洗入口的对抗面：伪造对象 id / 未知动作名 / 畸形参数 / 'room'
    /// 键伪造（数组形态检查）——全部 fail-closed 丢弃，白名单内条目存活。
    #[test]
    fn from_json_adversarial_inputs() {
        let list = serde_json::json!({
            "kk0000000000000001": { "move": { "direction": 3 }, "explode": {}, "5": {} },
            "not-an-id": { "move": { "direction": 3 } },
            "room": "forged-non-object",
            "kk0000000000000002": 7,
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        assert_eq!(user.objects.len(), 1, "只有合法对象键存活");
        let obj = kk_world::ObjId::parse("kk0000000000000001").unwrap();
        let actions = &user.objects[&obj];
        assert_eq!(actions.len(), 1, "未知动作名 'explode' 丢弃");
        assert_eq!(actions[&Action::Move].direction, Some(3));
        assert!(user.room.is_empty(), "'room' 键非对象丢弃");
    }

    /// createCreep 经通道往返（评审 F1：parse 曾缺此臂——静默黑洞）+ presence：
    /// 缺键 None / 空数组 Some([])（显式列表模式）。
    #[test]
    fn from_json_create_creep_roundtrip_and_presence() {
        let list = serde_json::json!({
            "kk0000000000000003": {
                "createCreep": { "body": ["move", "work"], "name": "c1", "energyStructures": [] }
            },
            "kk0000000000000004": {
                "createCreep": { "body": ["move"], "name": "c2" }
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let s1 = kk_world::ObjId::parse("kk0000000000000003").unwrap();
        let s2 = kk_world::ObjId::parse("kk0000000000000004").unwrap();
        assert_eq!(
            user.objects[&s1][&Action::CreateCreep].energy_structures,
            Some(vec![]),
            "空数组保持 Some（显式列表模式）"
        );
        assert_eq!(
            user.objects[&s2][&Action::CreateCreep].energy_structures,
            None,
            "缺键读 None（缺省全房扣费）"
        );
        assert_eq!(user.objects[&s1][&Action::CreateCreep].body.len(), 2);
    }

    /// room 级数组通道：createFlag/createConstructionSite 解析 + 未知名丢弃。
    #[test]
    fn from_json_room_intents() {
        let list = serde_json::json!({
            "room": {
                "createFlag": [
                    { "roomName": "W1N1", "x": 10, "y": 11, "name": "f1", "color": 1, "secondaryColor": 1 },
                    { "roomName": "W1N1", "x": 12, "y": 13, "name": "f2" }
                ],
                "teleport": [{ "roomName": "W1N1", "x": 0, "y": 0 }]
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        assert_eq!(batch.users["p1"].room.len(), 2, "未知 room intent 名丢弃");
    }

    /// A4-i spawn 族白名单（system.js:39/60/62/73）：renewCreep/recycleCreep
    /// 只认 id、setSpawnDirections 的 number[]、cancelSpawning 空载荷；四者
    /// 都不在 EXECUTION_ORDER（spawns/intents.js 的 spawn 分派序）里。
    #[test]
    fn from_json_spawn_family_whitelist() {
        let spawn = "kk0000000000000021";
        let creep = "kk0000000000000022";
        let list = serde_json::json!({
            spawn: {
                "renewCreep": { "id": creep, "forged": 1 },
                "recycleCreep": { "id": creep },
                "setSpawnDirections": { "directions": [3, 5, "7tail", 3] },
                "cancelSpawning": { "forged": "field" }
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let spawn_id = ObjId::parse(spawn).unwrap();
        let creep_id = ObjId::parse(creep).unwrap();
        let actions = &user.objects[&spawn_id];
        assert_eq!(actions.len(), 4);
        assert_eq!(
            actions[&Action::RenewCreep].id,
            Some(creep_id),
            "renewCreep 只留 id"
        );
        assert_eq!(
            actions[&Action::RecycleCreep].id,
            Some(creep_id),
            "recycleCreep 只留 id"
        );
        assert_eq!(
            actions[&Action::SetSpawnDirections].directions,
            vec![3, 5, 7],
            "number[]：parseInt 截断（\"7tail\"→7）+ 保序去重"
        );
        let cancel = &actions[&Action::CancelSpawning];
        assert_eq!(cancel.id, None, "cancelSpawning 空白名单——无字段");
        // 名字与 parse 往返 + 不在 creep 执行序
        for (action, name) in [
            (Action::RenewCreep, "renewCreep"),
            (Action::RecycleCreep, "recycleCreep"),
            (Action::SetSpawnDirections, "setSpawnDirections"),
            (Action::CancelSpawning, "cancelSpawning"),
        ] {
            assert_eq!(Action::parse(name), Some(action));
            assert_eq!(action.name(), name);
            assert!(
                !EXECUTION_ORDER.contains(&action),
                "{name} 不在 creepActions 序列（spawns/intents.js 的 spawn 分派序）"
            );
        }
    }

    /// B2 controller 族白名单（system.js:34/36/40/46/67/74）：五个 creep 动作
    /// 只认 id；signController 另有 sign（userString 截 100）；activateSafeMode
    /// 无字段。白名单外的未知字段丢弃。
    #[test]
    fn from_json_controller_family_whitelist() {
        let creep = "kk0000000000000009";
        let ctl = "kk0000000000000010";
        let list = serde_json::json!({
            creep: {
                "claimController": { "id": ctl, "unknown": 1 },
                "reserveController": { "id": ctl },
                "attackController": { "id": ctl },
                "generateSafeMode": { "id": ctl },
                "signController": { "id": ctl, "sign": "x".repeat(150) }
            },
            ctl: {
                "activateSafeMode": { "forged": "field" },
                "unclaim": { "forged": 1 }
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let user = &batch.users["p1"];
        let creep = ObjId::parse(creep).unwrap();
        let ctl_id = ObjId::parse(ctl).unwrap();
        for action in [
            Action::ClaimController,
            Action::ReserveController,
            Action::AttackController,
            Action::GenerateSafeMode,
        ] {
            assert_eq!(
                user.objects[&creep][&action].id,
                Some(ctl_id),
                "{:?} 白名单只留 id",
                action.name()
            );
        }
        let sign = &user.objects[&creep][&Action::SignController];
        assert_eq!(sign.id, Some(ctl_id));
        assert_eq!(
            sign.sign.as_ref().map(|s| s.chars().count()),
            Some(100),
            "sign 走 userString 截 100"
        );
        // controller 对象的 activateSafeMode（system.js:34 空白名单——无字段）
        let activate = &user.objects[&ctl_id][&Action::ActivateSafeMode];
        assert_eq!(activate.id, None, "activateSafeMode 无字段");
        assert_eq!(
            Action::parse("activateSafeMode"),
            Some(Action::ActivateSafeMode)
        );
        assert_eq!(Action::ActivateSafeMode.name(), "activateSafeMode");
        // unclaim（system.js:78 空白名单——无字段；processor.js:307-309 分派）
        let unclaim = &user.objects[&ctl_id][&Action::Unclaim];
        assert_eq!(unclaim.id, None, "unclaim 无字段");
        // 缺省 → None / 显式空串 → Some("")：清洗层保留 presence 区分，
        // 语义分叉在 signController 结算层（两者都落"清除签名"）
        let d = sanitize_object_intent(Action::SignController, &serde_json::json!({ "id": ctl }));
        assert_eq!(d.sign, None, "缺省 sign → None");
        let d2 = sanitize_object_intent(Action::SignController, &serde_json::json!({ "sign": "" }));
        assert_eq!(d2.sign, Some(String::new()), "显式空串保留 Some(\"\")");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn priority_graph_matches_official() {
        // intents.js:3-13 抽样：harvest 被 attack..heal 全家压制；move 无压制
        let mut present = BTreeSet::new();
        present.insert(Action::Harvest);
        assert!(Action::executes(&present, Action::Harvest));
        present.insert(Action::Heal);
        assert!(
            !Action::executes(&present, Action::Harvest),
            "heal 压制 harvest"
        );
        assert!(Action::executes(&present, Action::Heal));
        // repair 压制 build（官方 priorities：build 的压制表含 repair）；attack 压制 build
        let p2 = BTreeSet::from([Action::Build, Action::Repair]);
        assert!(!Action::executes(&p2, Action::Build));
        assert!(Action::executes(&p2, Action::Repair));
        let p3 = BTreeSet::from([Action::Attack, Action::Build]);
        assert!(
            !Action::executes(&p3, Action::Attack),
            "build 在 attack 的压制表里"
        );
        assert!(Action::executes(&p3, Action::Build));
        // 执行顺序：drop 最先、move 在 harvest 后
        assert_eq!(EXECUTION_ORDER[0], Action::Drop);
        assert!(
            EXECUTION_ORDER.iter().position(|a| *a == Action::Harvest)
                < EXECUTION_ORDER.iter().position(|a| *a == Action::Move)
        );
    }

    #[test]
    fn sanitize_translates_official_transforms() {
        let raw = json!({
            "id": "kk0000000000000001",
            "amount": 12.7,
            "resourceType": "energy",
            "unknown": "dropped",
        });
        let d = sanitize_object_intent(Action::Transfer, &raw);
        assert_eq!(d.id, Some(ObjId(1)));
        assert_eq!(d.amount, Some(12), "parseInt 截断");
        assert_eq!(d.resource_type.as_deref(), Some("energy"));
        // bodypart[] 过滤
        let raw2 = json!({ "body": ["work", "fly", "move"] });
        let d2 = sanitize_object_intent(Action::Suicide, &raw2);
        assert_eq!(d2.body, vec![BodyKind::Work, BodyKind::Move]);
        assert_eq!(creep_cost(&d2.body), 150);
        // userString 截 100
        let long = "x".repeat(150);
        let raw3 = json!({ "name": long });
        let d3 = sanitize_object_intent(Action::Say, &raw3);
        assert_eq!(d3.name.unwrap().chars().count(), 100);
        // directions：parseInt 截断（2.5→2）+ 保序去重；越界值原样传递——
        // 整单拒绝发生在 createCreep（1..=8 校验），无法 parseInt 的元素以
        // i64::MIN 哨兵传递（天然落入 1..=8 之外）
        let raw4 = json!({ "directions": [1, 9, 3, 3, 2.5] });
        let d4 = sanitize_object_intent(Action::Suicide, &raw4);
        assert_eq!(d4.directions, vec![1, 9, 3, 2]);
        let raw5 = json!({ "directions": [1, "abc"] });
        let d5 = sanitize_object_intent(Action::Suicide, &raw5);
        assert_eq!(d5.directions, vec![1, i64::MIN]);
        let raw6 = json!({ "directions": [1, 2, 1, "3tail", "0x2"] });
        let d6 = sanitize_object_intent(Action::CreateCreep, &raw6);
        assert_eq!(d6.directions, vec![1, 2, 3]);
        assert_eq!(to_int(Some(&json!("1e2"))), Some(1));
        assert_eq!(to_int(Some(&json!(-12.7))), Some(-12));
        assert_eq!(to_int(Some(&json!("-9223372036854775808"))), Some(i64::MIN));
        assert_eq!(to_int(Some(&json!(1e-6))), Some(0));
        assert_eq!(to_int(Some(&json!(1e-7))), Some(1));
        assert_eq!(to_int(Some(&json!(-2e-7))), Some(-2));
        assert_eq!(to_int(Some(&json!(1e21))), Some(1));
        assert_eq!(
            sanitize_object_intent(Action::CreateCreep, &json!({ "directions": [1e-6] }))
                .directions,
            vec![0]
        );
    }
}

/// A4-iv 清偿（评审 3 建议）：say/pull/notifyWhenAttacked 的载荷清洗面
/// 白名单臂 + to_bool 真值表（JS `!!value` 语义）+ EXECUTION_ORDER 的
/// suicide/say/pull 位次锁（官方 creepActions intents.js:15-17）。
#[test]
fn from_json_misc_family_whitelist_and_order() {
    let creep = "kk0000000000000011";
    let target = "kk0000000000000012";
    let list = serde_json::json!({
        creep: {
            "say": { "message": "hello", "isPublic": "yes", "unknown": 1 },
            "pull": { "id": target, "extra": true },
            "suicide": { "unknown": "dropped" },
        },
        target: {
            "notifyWhenAttacked": { "enabled": "false", "junk": 0 },
        },
    });
    let batch = IntentBatch::from_json("p1", &list);
    let u = &batch.users["p1"];
    let c_intents = &u.objects[&ObjId::parse(creep).unwrap()];
    // say：message 经 userString（截 100）；isPublic 经 to_bool——JS !! 真值表
    //（"yes" 非空串 → true，"false" 同样 true——字符串除 "" 外恒真）
    let say = &c_intents[&Action::Say];
    assert_eq!(say.message.as_deref(), Some("hello"));
    assert_eq!(
        say.is_public,
        Some(true),
        "to_bool('yes') === true（JS !! 真值表）"
    );
    assert_eq!(say.id, None, "say 无 id 字段，未知字段丢弃");
    // pull：只认 id
    let pull = &c_intents[&Action::Pull];
    assert_eq!(pull.id, Some(ObjId::parse(target).unwrap()));
    // suicide：空白名单（未知字段丢弃）
    assert_eq!(c_intents[&Action::Suicide].id, None);
    // notifyWhenAttacked：enabled 折叠（"false" 字符串在 JS !! 下是 true！）
    let t_intents = &u.objects[&ObjId::parse(target).unwrap()];
    assert_eq!(
        t_intents[&Action::NotifyWhenAttacked].enabled,
        Some(true),
        "to_bool('false') === true——官方 transform 是 !!value 非 JSON 解析"
    );
    // EXECUTION_ORDER 位次（官方 creepActions：harvest(8) < say(14) <
    // suicide(15) < pull(22)）
    let pos = |a: Action| EXECUTION_ORDER.iter().position(|x| *x == a).unwrap();
    assert!(pos(Action::Harvest) < pos(Action::Say));
    assert!(pos(Action::Say) < pos(Action::Suicide));
    assert!(pos(Action::Suicide) < pos(Action::Pull));
}

/// 2B 批 P0：'global' 市场容器清洗（白名单五键 + price 毫变换 + 缺键 None
/// ≙ 官方 "undefined"/NaN）。
#[cfg(test)]
mod market_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn global_container_parses_all_five_actions() {
        let list = json!({
            "global": {
                "createOrder": [
                    {"type": "sell", "resourceType": "H", "price": 2.5,
                     "totalAmount": "100", "roomName": "W1N1"}
                ],
                "cancelOrder": [{"orderId": "mkt3"}],
                "changeOrderPrice": [{"orderId": "mkt3", "newPrice": 0.75}],
                "extendOrder": [{"orderId": "mkt3", "addAmount": 50}],
                "deal": [{"orderId": "mkt3", "amount": 10, "targetRoomName": "W1N1"}]
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let m = &batch.users["p1"].market;
        assert_eq!(m.len(), 5);
        // 迭代序 = 官方 intentTypes 声明序（system.js:23-29）：
        // cancelOrder → changeOrderPrice → createOrder → deal → extendOrder
        assert_eq!(
            m[0],
            MarketIntent::CancelOrder {
                order_id: Some("mkt3".into())
            }
        );
        assert_eq!(
            m[1],
            MarketIntent::ChangeOrderPrice {
                order_id: Some("mkt3".into()),
                new_price_milli: Some(750),
            }
        );
        assert_eq!(
            m[2],
            MarketIntent::CreateOrder {
                order_type: Some("sell".into()),
                resource_type: Some("H".into()),
                price_milli: Some(2500),
                total_amount: Some(100),
                room_name: Some("W1N1".into()),
            },
            "totalAmount 字符串走 JS parseInt 前缀规则（\"100\" → 100）"
        );
        assert_eq!(
            m[3],
            MarketIntent::Deal {
                order_id: Some("mkt3".into()),
                amount: Some(10),
                target_room_name: Some("W1N1".into()),
            }
        );
        assert_eq!(
            m[4],
            MarketIntent::ExtendOrder {
                order_id: Some("mkt3".into()),
                add_amount: Some(50),
            }
        );
    }

    #[test]
    fn price_transform_milli_and_edges() {
        let mk = |price: Value| {
            let list =
                json!({"global": {"changeOrderPrice": [{"orderId": "x", "newPrice": price}]}});
            IntentBatch::from_json("p1", &list).users["p1"]
                .market
                .clone()
        };
        let case = |price: Value| match mk(price).remove(0) {
            MarketIntent::ChangeOrderPrice {
                new_price_milli, ..
            } => new_price_milli,
            _ => unreachable!(),
        };
        assert_eq!(case(json!(2)), Some(2000));
        assert_eq!(case(json!(0.5)), Some(500));
        assert_eq!(case(json!(0.75)), Some(750));
        assert_eq!(case(json!(1.4999)), Some(1500), "toFixed 最近邻");
        assert_eq!(
            case(json!("3.25")),
            Some(3250),
            "JS 乘法字符串强转（1000*\"3.25\"）"
        );
        // 负价/零/布尔：官方通道不可达（game 侧 `!price || price <= 0` 门拒），
        // 清洗层按 toFixed/round 远零半舍同向保留（−0.5→"−500"、true→1000）
        assert_eq!(case(json!(-0.5)), Some(-500));
        assert_eq!(
            case(json!(-0.0025)),
            Some(-3),
            "负恰半远零（node 实测对齐）"
        );
        assert_eq!(
            case(json!(0)),
            Some(0),
            "0 是合法毫值——`!price` 门在 API 层"
        );
        assert_eq!(case(json!(true)), Some(1000), "JS 1000*true");
        // 分歧域终口径（清偿复审 P1-a 后，登记于 to_price_milli 文档）：
        // ①ToNumber 分歧串（"0x10" 官方 16000）——真不可达（parseFloat 门）
        //   钉 None；②[2^63,1e21) 表示域强制——钉 None；③≥1e21 指数形
        //   首位怪癖——**保真实现**（node 实测 t(1e18)=1/t(2.5e18)=2/
        //   t(−1e18)=−1；玩家可达路径=remaining=0 订单 changeOrderPrice
        //   费恒 0 放行任意 newPrice）
        assert_eq!(case(json!("0x10")), None, "域①真不可达（parseFloat 门）");
        assert_eq!(
            case(json!(1e18)),
            Some(1),
            "域③指数形首位（milli=1e21，node 实测官方=1）"
        );
        assert_eq!(case(json!(2.5e18)), Some(2), "域③ 2.5e21→首位 2");
        assert_eq!(case(json!(-1e18)), Some(-1), "域③负向 −1e21→−1");
        assert_eq!(
            case(json!(1e16)),
            None,
            "域②milli=1e19∈[2^63,1e21) 表示域强制（官方精确 1e19）"
        );
        assert_eq!(
            case(json!(-1e19)),
            Some(-1),
            "域③负向 −1e22 指数形首位 −1（负向下界对称守卫=域②内才 None）"
        );
        assert_eq!(case(json!(-1e16)), None, "域②负向 milli=−1e19 表示域强制");
        assert_eq!(case(Value::Null), None, "不可达路径按 falsy 折叠");
        // 缺键 → None（官方 NaN）
        let list = json!({"global": {"changeOrderPrice": [{"orderId": "x"}]}});
        match IntentBatch::from_json("p1", &list)
            .users
            .get_mut("p1")
            .unwrap()
            .market
            .remove(0)
        {
            MarketIntent::ChangeOrderPrice {
                new_price_milli, ..
            } => assert_eq!(new_price_milli, None),
            _ => unreachable!(),
        }
    }

    #[test]
    fn global_unknown_action_dropped_and_missing_fields_none() {
        let list = json!({
            "global": {
                "generatePixel": [{}],
                "deal": [{}]
            }
        });
        let batch = IntentBatch::from_json("p1", &list);
        let m = &batch.users["p1"].market;
        assert_eq!(m.len(), 1, "白名单外动作丢弃");
        assert_eq!(
            m[0],
            MarketIntent::Deal {
                order_id: None,
                amount: None,
                target_room_name: None,
            },
            "缺键全 None（官方物化 undefined/NaN——falsy 门等价）"
        );
    }

    #[test]
    fn extend_from_merges_market_vecs() {
        let a = IntentBatch::from_json(
            "p1",
            &json!({"global": {"cancelOrder": [{"orderId": "mkt1"}]}}),
        );
        let b = IntentBatch::from_json(
            "p1",
            &json!({"global": {"cancelOrder": [{"orderId": "mkt2"}]}}),
        );
        let mut merged = a;
        merged.extend_from(b);
        assert_eq!(merged.users["p1"].market.len(), 2);
        // 序也锚定（评审 3：len 断言对错排无判别力）
        assert_eq!(
            merged.users["p1"]
                .market
                .iter()
                .map(|i| match i {
                    MarketIntent::CancelOrder { order_id } => order_id.clone(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>(),
            vec![Some("mkt1".into()), Some("mkt2".into())]
        );
    }
}
