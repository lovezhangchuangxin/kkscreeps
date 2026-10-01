//! 解析玩家指令，保留同名动作的最后一次调用，并定义 creep 动作的压制关系。
//!
//! 清洗器将输入绑定到调用方给定的玩家名；它不验证对象存在或所有权。
//! 数字按 parseInt 前缀规则转换，未知动作被丢弃。对象载荷采用已知字段的
//! 并集，具体动作的合法性仍由执行器检查。来源参考：上游 common/lib/system.js
//! 及 engine/src/processor/intents/creeps/intents.js。

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::Value;

/// 引擎可识别的对象级动作。Creep、spawn、controller 和结构使用各自的执行器。
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
    /// 对象级通知开关，由 settle_room 在类型分派前处理。
    NotifyWhenAttacked,
    /// spawn 的创建指令，不属于 creep 执行序列。
    CreateCreep,
    RenewCreep,
    RecycleCreep,
    SetSpawnDirections,
    CancelSpawning,
    /// controller 的激活安全模式指令，不属于 creep 执行序列。
    ActivateSafeMode,
    Unclaim,
    SetPublic,
    /// 房间阶段校验并保存 terminal 发送请求，跨房资源划转在市场阶段执行。
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

    /// 返回会压制本动作的指令。判断只看指令是否存在，不看其执行是否成功。
    /// 来源：上游 engine/src/processor/intents/creeps/intents.js 的 priorities。
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

    /// 动作存在，且没有压制它的指令时才执行。
    /// 玩家 API 返回 OK 只表示指令已记录，不保证最终被执行。
    pub fn executes(present: &BTreeSet<Action>, action: Action) -> bool {
        present.contains(&action) && !action.suppressed_by().iter().any(|a| present.contains(a))
    }
}

/// Creep 动作执行顺序。类型专属动作另由对应处理器分派。
/// 来源：上游 engine/src/processor/intents/creeps/intents.js 的 creepActions。
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

/// 对象指令载荷。字段为已识别动作的并集，执行器仅读取对应动作所需的字段。
#[derive(Debug, Clone, PartialEq)]
pub struct IntentData {
    /// 目标对象 ID；move 可用相邻 creep 的位置确定方向。
    pub id: Option<ObjId>,
    pub direction: Option<u8>,
    pub amount: Option<i64>,
    pub resource_type: Option<String>,
    pub x: Option<i64>,
    pub y: Option<i64>,
    /// 创建 creep 时的名称。
    pub name: Option<String>,
    /// say 文本，最多 100 个 Unicode 字符；缺失时执行器使用空串。
    pub message: Option<String>,
    /// say 或 setPublic 的公开标志，按 JavaScript 真值规则转换。
    pub is_public: Option<bool>,
    /// notifyWhenAttacked 的开关，按 JavaScript 真值规则转换。
    pub enabled: Option<bool>,
    /// 控制器签名，最多 100 个 Unicode 字符；缺失与空串均表示清除签名。
    /// 截断单位不同于上游 userString 的 UTF-16 码元。
    pub sign: Option<String>,
    pub body: Vec<BodyKind>,
    /// None 表示使用房间内默认能量结构；Some(空列表) 表示显式列表，
    /// 可用能量为 0。不能把空列表改为缺省值。
    pub energy_structures: Option<Vec<ObjId>>,
    /// 方向列表：逐项按 parseInt 转换，再保序去重；无效值用 i64::MIN 保留。
    /// 执行器校验 1..=8，发现无效值时拒绝整条指令，不能在此过滤掉。
    pub directions: Vec<i64>,
    /// terminal.send 的目标房名，只接收 JSON 字符串。
    pub target_room_name: Option<String>,
    /// terminal.send 的描述，最多 100 个 Unicode 字符。
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
    DestroyStructure {
        room: String,
        id: ObjId,
    },
    /// flag.setPosition/setColor 也通过删除和重建旗帜实现。
    RemoveFlag {
        room: String,
        name: String,
    },
    /// 删除工地，结算器检查所有权并退还 progress/2 的掉落能量。
    RemoveConstructionSite {
        room: String,
        id: ObjId,
    },
}

/// 解析房间级动作。必需的房名、坐标或 ID 无效时丢弃整条；
/// 颜色缺失按 0 处理，名称经长度限制。
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
        "destroyStructure" => Some(RoomIntent::DestroyStructure {
            room,
            id: to_id(f("id"))?,
        }),
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

/// 单玩家的对象、房间、通知与市场指令。对象动作同名覆盖，列表指令保留顺序。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserIntents {
    pub objects: BTreeMap<ObjId, BTreeMap<Action, IntentData>>,
    pub room: Vec<RoomIntent>,
    /// Game.notify 的全局列表，每条在 tick 发布时记录一次。
    pub notify: Vec<NotifyIntent>,
    /// Game.market 的全局列表，由市场阶段按动作类别处理。
    pub market: Vec<MarketIntent>,
}

/// 一条玩家通知意图（Game.notify）。
#[derive(Debug, Clone, PartialEq)]
pub struct NotifyIntent {
    pub message: String,
    pub group_interval: Option<i64>,
}

/// 市场动作及其清洗后的载荷。价格以千分之一 credit 表示。
///
/// 缺失或无法转换的字段保留为 None，由市场处理器拒绝或忽略。
/// CreateOrder 的 order_type=None 必须拒绝，不能默认为 buy。上游
/// common/lib/system.js 会把缺失字符串物化为 "undefined"，并把无效数字
/// 转成 NaN；这里用 Option 表示缺失，不能据此认为内部格式完全一致。
#[derive(Debug, Clone, PartialEq)]
pub enum MarketIntent {
    CreateOrder {
        /// 仅接受 buy 或 sell，None 由处理器拒绝。
        order_type: Option<String>,
        resource_type: Option<String>,
        /// 价格，单位千分之一 credit。
        price_milli: Option<i64>,
        total_amount: Option<i64>,
        room_name: Option<String>,
    },
    CancelOrder {
        order_id: Option<String>,
    },
    ChangeOrderPrice {
        order_id: Option<String>,
        new_price_milli: Option<i64>,
    },
    ExtendOrder {
        order_id: Option<String>,
        add_amount: Option<i64>,
    },
    Deal {
        order_id: Option<String>,
        amount: Option<i64>,
        target_room_name: Option<String>,
    },
}

/// 本轮各玩家的指令，按玩家名字排序以固定结算顺序。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IntentBatch {
    pub users: BTreeMap<String, UserIntents>,
}

impl IntentBatch {
    /// 将沙箱 intentsList 解析为指定玩家的指令。
    ///
    /// 识别对象 ID 及 room、notify、global 容器，丢弃未知动作或格式错误的容器。
    /// 全部条目绑定 player，不读取输入自报的玩家名；调用方必须提供注册身份。
    /// 这里只解析和转换字段，目标存在、所有权、距离与资源数量由结算器检查。
    pub fn from_json(player: &str, list: &Value) -> IntentBatch {
        let mut batch = IntentBatch::default();
        let Some(objects) = list.as_object() else {
            return batch;
        };
        for (obj_key, actions) in objects {
            if obj_key == "notify" {
                // 通知为顶层数组；文本最多 1000 个字符，间隔按整数转换。
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
                // 按固定动作顺序解析，保留同动作数组的顺序，不依赖输入的 JSON 键序。
                // 来源：上游 common/lib/system.js 的 intentTypes 声明顺序。
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
                // 房间指令按动作名分组，每个动作对应一个数组。
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
                continue; // ID 格式错误时丢弃；此处不查询对象是否存在。
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

    /// 合并另一批：同对象只覆盖同名动作，保留其他动作；
    /// 房间、通知和市场列表追加。因此重复合并会重复列表指令，并非幂等。
    pub fn extend_from(&mut self, other: IntentBatch) {
        for (player, intents) in other.users {
            let entry = self.users.entry(player).or_default();
            for (id, actions) in intents.objects {
                entry.objects.entry(id).or_default().extend(actions);
            }
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

/// 按 JavaScript 真值转换；空数组、空对象和非空串（包括 "false"）均为真。
/// 缺失保留 None，null、false、0 和空串为假。
/// 来源：上游 common/lib/system.js 的 boolean transform。
fn to_bool(v: Option<&Value>) -> Option<bool> {
    v.map(|x| match x {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_none_or(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    })
}

/// 检查 JSON 值的 JavaScript 假值规则，用于通知文本的 value || ""。
fn js_value_falsy(x: &Value) -> bool {
    match x {
        Value::Null => true,
        Value::Bool(b) => !*b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        _ => false,
    }
}

/// 模拟数组 join(",")：嵌套数组递归字符串化，null 为空，
/// 0 与 false 保留为字符串，对象变为 "[object Object]"。
/// 顶层文本的假值折叠不能应用到数组元素。
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

/// 通知文本转换，最多 1000 个 Unicode 字符。假值归空串，数组用 join，
/// 对象用 "[object Object]"。上游 common/lib/system.js 的 userText 使用
/// UTF-16 码元截断；数字的字符串格式也可能不同。
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

// 保留缺失为 None，null 为空串，其余非字符串使用 JSON 字符串化，
// 最多 100 个 Unicode 字符。此处没有完整模拟上游 userString 的假值
// 折叠、数组 join 或 UTF-16 截断，不能与 to_user_text 互换。
fn to_user_string(v: Option<&Value>) -> Option<String> {
    v.map(|x| match x {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
    .map(|s| s.chars().take(100).collect())
}

/// 市场字符串字段：字符串原样保留，其余使用 JSON 字符串化，缺失为 None。
/// 与上游 common/lib/system.js 的 string transform 不同，数组和对象
/// 不会使用 JavaScript ToString，缺失也不会生成 "undefined"。
fn to_plain_string(v: Option<&Value>) -> Option<String> {
    v.map(|x| match x {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// 将 credit 价格转成千分之一 credit，乘以 1000 后四舍五入。
/// 来源：上游 common/lib/system.js 的 price transform。
///
/// 绝对值至少 1e21 时按指数表示只取首位数字，保留 parseInt 的特殊结果。
/// 较小但超出 i64 的值和非有限值返回 None；与上游的大整数表示存在差异。
/// 字符串仅用 Rust 浮点解析，不完整模拟 JavaScript ToNumber。
fn to_price_milli(v: Option<&Value>) -> Option<i64> {
    let x = match v? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Bool(b) => i64::from(*b) as f64,
        // 此转换只接收数字、可解析字符串和布尔值，null 不按 JavaScript 的 0 处理。
        _ => return None,
    };
    let milli = 1000.0 * x;
    if !milli.is_finite() {
        return None;
    }
    if milli.abs() >= 1e21 {
        // 上游 toFixed 在此范围输出指数形式，parseInt 到小数点或 e 就停止。
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

/// 解析 global 容器中的五种市场动作；其余全局动作不在此处理。
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

/// 转换对象载荷中已知字段，忽略未知字段；缺失值通常保留为 None。
///
/// 当前读取所有动作字段的并集，不根据 action 分别执行上游白名单。
/// 它不验证对象类型或动作参数是否合法；执行器须只读取自己的字段并校验。
/// 来源参考：上游 common/lib/system.js 的 intentTypes 和 transforms。
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

/// 合计已解析身体部件的成本。
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

    /// 无效对象键、未知动作和错误容器格式被丢弃，合法 move 保留。
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

    /// 缺失能量结构列表与显式空列表必须保持区别，创建执行器据此选择扣费方式。
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

    /// 解析 spawn 动作、转换并去重方向；这些动作不属于 creep 执行序列。
    /// 这里仅检查给定输入的结果，不证明按动作分别执行了字段白名单。
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

    /// 解析控制器相关动作，签名截为 100 个字符，未知字段不进入载荷。
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
    fn extend_from_preserves_other_actions() {
        let id = ObjId(1);
        let mut first = IntentBatch::default();
        first.add_object_intent("p1", id, Action::Move, IntentData::bare());
        first.add_object_intent("p1", id, Action::Harvest, IntentData::target(ObjId(2)));
        let mut second = IntentBatch::default();
        second.add_object_intent("p1", id, Action::Move, IntentData::target(ObjId(3)));
        second.add_object_intent("p1", id, Action::Say, IntentData::bare());
        first.extend_from(second);
        let actions = &first.users["p1"].objects[&id];
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[&Action::Move].id, Some(ObjId(3)));
        assert_eq!(actions[&Action::Harvest].id, Some(ObjId(2)));
        assert!(actions.contains_key(&Action::Say));
    }

    #[test]
    fn priority_graph_matches_official() {
        // heal 的存在足以压制 harvest；其余压制关系定义于 suppressed_by。
        let mut present = BTreeSet::new();
        present.insert(Action::Harvest);
        assert!(Action::executes(&present, Action::Harvest));
        present.insert(Action::Heal);
        assert!(
            !Action::executes(&present, Action::Harvest),
            "heal 压制 harvest"
        );
        assert!(Action::executes(&present, Action::Heal));
        // repair 压制 build，build 压制 attack；压制不取决于执行成功与否。
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

/// 检查 say、pull 和通知开关的载荷，以及 creep 执行顺序。
/// 字符串 "false" 在 JavaScript 真值规则下为 true。
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

/// 检查市场容器、价格单位、缺失字段及批合并。
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
        // 清洗层保留零与负价，并转换布尔值；是否允许成交由 API 和结算器检查。
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
        // 十六进制串与超出 i64 的固定点值返回 None；达到指数表示范围后取首位数字。
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
        // 数量相等仍可能错排，还须检查合并后的顺序。
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
