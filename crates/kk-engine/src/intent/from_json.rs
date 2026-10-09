//! 沙箱 intentsList 的解析域：IntentBatch::from_json、房间/市场/power
//! 动作解析、对象意图清洗与 JS 值转换助手；序列化见 to_json.rs。
use super::*;
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
                // power 六动作按官方处理序（global-intents/power.js:8）；
                // 与市场动作名不相交，两表顺序拼接即全局解析序。
                static POWER_INTENT_ORDER: [&str; 6] = [
                    "spawnPowerCreep",
                    "suicidePowerCreep",
                    "deletePowerCreep",
                    "upgradePowerCreep",
                    "createPowerCreep",
                    "renamePowerCreep",
                ];
                let Some(actions) = actions.as_object() else {
                    continue;
                };
                let entry = batch.users.entry(player.to_string()).or_default();
                for name in MARKET_INTENT_ORDER.iter().chain(POWER_INTENT_ORDER.iter()) {
                    let Some(items) = actions.get(*name).and_then(Value::as_array) else {
                        continue;
                    };
                    for item in items {
                        if let Some(intent) = parse_power_intent(name, item) {
                            entry.power.push(intent);
                        } else if let Some(intent) = parse_market_intent(name, item) {
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
                if let Some(action) = Action::parse(name) {
                    let data = sanitize_object_intent(action, params);
                    batch.add_object_intent(player, obj, action, data);
                    continue;
                }
                // 冒号形状的动作名是扩展意图键（vanilla 动作名不含冒号）；
                // 键形非法仍丢弃。是否在冻结目录中、参数与所有权由结算侧
                // 按计划独立校验，未通过即不执行。
                if let Ok(key) = kk_mod_api::Key::new(name) {
                    batch.add_ext_intent(player, obj, key, params.clone());
                }
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

    /// 记录一条扩展意图；同对象同键后发覆盖先发（与对象动作的覆盖语义一致）。
    pub fn add_ext_intent(&mut self, player: &str, obj: ObjId, key: kk_mod_api::Key, args: Value) {
        self.users
            .entry(player.to_string())
            .or_default()
            .ext
            .entry(obj)
            .or_default()
            .insert(key, args);
    }

    /// 合并另一批：同对象只覆盖同名动作，保留其他动作；
    /// 房间、通知和市场列表追加。因此重复合并会重复列表指令，并非幂等。
    pub fn extend_from(&mut self, other: IntentBatch) {
        for (player, intents) in other.users {
            let entry = self.users.entry(player).or_default();
            for (id, actions) in intents.objects {
                entry.objects.entry(id).or_default().extend(actions);
            }
            for (id, ext) in intents.ext {
                entry.ext.entry(id).or_default().extend(ext);
            }
            entry.room.extend(intents.room);
            entry.notify.extend(intents.notify);
            entry.market.extend(intents.market);
            entry.power.extend(intents.power);
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
pub(super) fn to_int(v: Option<&Value>) -> Option<i64> {
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

/// 解析 global 容器中的六种 power 账号动作；其余全局动作不在此处理。
/// 官方载荷键：create{name,className}/rename{id,name}/delete{id,cancel}/
/// upgrade{id,power}/spawnPowerCreep{id,name}/suicidePowerCreep{id}。
fn parse_power_intent(name: &str, item: &Value) -> Option<PowerIntent> {
    let id = || to_id(item.get("id"));
    match name {
        "createPowerCreep" => Some(PowerIntent::CreatePowerCreep {
            name: to_plain_string(item.get("name"))?,
            class_name: to_plain_string(item.get("className"))?,
        }),
        "renamePowerCreep" => Some(PowerIntent::RenamePowerCreep {
            id: id()?,
            name: to_plain_string(item.get("name"))?,
        }),
        "deletePowerCreep" => Some(PowerIntent::DeletePowerCreep {
            id: id()?,
            cancel: to_bool(item.get("cancel")).unwrap_or(false),
        }),
        "upgradePowerCreep" => Some(PowerIntent::UpgradePowerCreep {
            id: id()?,
            power: to_int(item.get("power"))?,
        }),
        "spawnPowerCreep" => Some(PowerIntent::SpawnPowerCreep {
            power_spawn: id()?,
            name: to_plain_string(item.get("name"))?,
        }),
        "suicidePowerCreep" => Some(PowerIntent::SuicidePowerCreep { id: id()? }),
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
            // launchNuke 的官方载荷键是 roomName（system.js），send 用
            // targetRoomName；并集解析两键都收，消费方只读自己的字段。
            .or_else(|| f("roomName").and_then(Value::as_str))
            .map(str::to_string),
        description: to_user_string(f("description")),
        lab1: to_id(f("lab1")),
        lab2: to_id(f("lab2")),
        body_parts_count: to_int(f("bodyPartsCount")),
        power: to_int(f("power")),
    }
}
