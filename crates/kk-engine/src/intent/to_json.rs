//! [`IntentBatch`] 到线格式 JSON 的逆转换（G25.a 输入日志用）。
//!
//! `from_json` 是有损清洗（文本截断、数值钳制、非白名单过滤）；本模块
//! 输出的是**清洗后载荷**的规范线形态。往返性质按规范批次定义：
//! 对任意线 JSON `v`，`from_json(p, to_json(from_json(p, v))) ==
//! from_json(p, v)`（to_json_tests 钉死）。它不是原始玩家 JSON 的复原
//! ——玩家轮的日志记录沙箱原始 JSON，本逆函数服务调用方直接构造
//! `IntentBatch` 的入口（如 `settle_external` 轮）。价格毫单位经反解
//! 验证发射，|milli| < 2^52 内往返精确（见 `price_value`；更大量级是真实
//! 市场不可能达到的，无解值退回朴素值）。

use serde_json::{Map, Value, json};

use super::{IntentBatch, IntentData, MarketIntent, RoomIntent, UserIntents};

fn put(m: &mut Map<String, Value>, key: &str, value: Option<i64>) {
    if let Some(v) = value {
        m.insert(key.into(), json!(v));
    }
}

fn put_str(m: &mut Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(v) = value {
        m.insert(key.into(), json!(v));
    }
}

/// 千分之一 credit 价格回到线格式的 credit 数。朴素除法 `m/1000` 在
/// |milli| ≳ 2^51 起会撞上 f64 舍入（首个反例 milli≈2^52−5·10^4：
/// `fl(m/1000)` 距半整数恰 0.5 ulp，from_json 半远离零取整得 m±1 且逐跳
/// 漂移），因此发射前反解验证：`(1000·x).round() == m` 不成立时向相邻
/// 可表示值试探（每个 |milli| < 2^52 的值都存在精确反解；更大的区间存在
/// 无解值，退回朴素值并在往返测试钉住的界限内如实声明）。
fn price_value(milli: Option<i64>) -> Option<Value> {
    let m = milli?;
    let mut x = m as f64 / 1000.0;
    if round_milli(x) != m {
        // 双向各试两步：相邻可表示值的千倍取整步长 ≤ 2 milli。
        let mut up = x;
        let mut down = x;
        for _ in 0..2 {
            up = up.next_up();
            down = down.next_down();
            if round_milli(up) == m {
                x = up;
                break;
            }
            if round_milli(down) == m {
                x = down;
                break;
            }
        }
    }
    Some(json!(x))
}

/// 与 from_json 的价格变换同式（`1000·x` 后半远离零取整）。
fn round_milli(x: f64) -> i64 {
    (1000.0 * x).round() as i64
}

fn market_entry(intent: &MarketIntent) -> (&'static str, Value) {
    let mut m = Map::new();
    let name = match intent {
        MarketIntent::CreateOrder {
            order_type,
            resource_type,
            price_milli,
            total_amount,
            room_name,
        } => {
            put_str(&mut m, "type", order_type);
            put_str(&mut m, "resourceType", resource_type);
            if let Some(price) = price_value(*price_milli) {
                m.insert("price".into(), price);
            }
            put(&mut m, "totalAmount", *total_amount);
            put_str(&mut m, "roomName", room_name);
            "createOrder"
        }
        MarketIntent::CancelOrder { order_id } => {
            put_str(&mut m, "orderId", order_id);
            "cancelOrder"
        }
        MarketIntent::ChangeOrderPrice {
            order_id,
            new_price_milli,
        } => {
            put_str(&mut m, "orderId", order_id);
            if let Some(price) = price_value(*new_price_milli) {
                m.insert("newPrice".into(), price);
            }
            "changeOrderPrice"
        }
        MarketIntent::ExtendOrder {
            order_id,
            add_amount,
        } => {
            put_str(&mut m, "orderId", order_id);
            put(&mut m, "addAmount", *add_amount);
            "extendOrder"
        }
        MarketIntent::Deal {
            order_id,
            amount,
            target_room_name,
        } => {
            put_str(&mut m, "orderId", order_id);
            put(&mut m, "amount", *amount);
            put_str(&mut m, "targetRoomName", target_room_name);
            "deal"
        }
    };
    (name, Value::Object(m))
}

fn room_item(intent: &RoomIntent) -> (&'static str, Value) {
    match intent {
        RoomIntent::CreateConstructionSite {
            room,
            x,
            y,
            structure_type,
            name,
        } => {
            let mut m = Map::new();
            m.insert("roomName".into(), json!(room));
            m.insert("x".into(), json!(x));
            m.insert("y".into(), json!(y));
            m.insert("structureType".into(), json!(structure_type));
            put_str(&mut m, "name", name);
            ("createConstructionSite", Value::Object(m))
        }
        RoomIntent::CreateFlag {
            room,
            x,
            y,
            name,
            color,
            secondary_color,
        } => (
            "createFlag",
            json!({"roomName": room, "x": x, "y": y, "name": name,
                   "color": color, "secondaryColor": secondary_color}),
        ),
        RoomIntent::DestroyStructure { room, id } => (
            "destroyStructure",
            json!({"roomName": room, "id": id.as_str()}),
        ),
        RoomIntent::RemoveFlag { room, name } => {
            ("removeFlag", json!({"roomName": room, "name": name}))
        }
        RoomIntent::RemoveConstructionSite { room, id } => (
            "removeConstructionSite",
            json!({"roomName": room, "id": id.as_str()}),
        ),
    }
}

fn object_params(data: &IntentData) -> Value {
    let mut m = Map::new();
    if let Some(id) = &data.id {
        m.insert("id".into(), json!(id.as_str()));
    }
    put(&mut m, "direction", data.direction.map(i64::from));
    put(&mut m, "amount", data.amount);
    if let Some(r) = &data.resource_type {
        m.insert("resourceType".into(), json!(r));
    }
    put(&mut m, "x", data.x);
    put(&mut m, "y", data.y);
    put_str(&mut m, "name", &data.name);
    put_str(&mut m, "message", &data.message);
    if let Some(p) = data.is_public {
        m.insert("isPublic".into(), json!(p));
    }
    if let Some(e) = data.enabled {
        m.insert("enabled".into(), json!(e));
    }
    put_str(&mut m, "sign", &data.sign);
    if !data.body.is_empty() {
        let names: Vec<&str> = data.body.iter().map(|b| b.name()).collect();
        m.insert("body".into(), json!(names));
    }
    if let Some(structures) = &data.energy_structures {
        let ids: Vec<String> = structures.iter().map(|id| id.as_str()).collect();
        m.insert("energyStructures".into(), json!(ids));
    }
    put_ints(&mut m, "directions", &data.directions);
    put_str(&mut m, "targetRoomName", &data.target_room_name);
    put_str(&mut m, "description", &data.description);
    Value::Object(m)
}

fn put_ints(m: &mut Map<String, Value>, key: &str, values: &[i64]) {
    if !values.is_empty() {
        m.insert(key.into(), json!(values));
    }
}

fn user_to_json(user: &UserIntents) -> Value {
    let mut top = Map::new();
    // 对象动作与扩展意图同层混排（from_json 的同一名空间）；
    // BTreeMap 键序保证输出确定。
    let mut objects: Map<String, Value> = Map::new();
    for (obj, actions) in &user.objects {
        let mut inner = Map::new();
        for (action, data) in actions {
            inner.insert(action.name().into(), object_params(data));
        }
        objects.insert(obj.as_str(), Value::Object(inner));
    }
    for (obj, exts) in &user.ext {
        let inner = objects
            .entry(obj.as_str())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(map) = inner.as_object_mut() {
            for (key, args) in exts {
                map.insert(key.to_string(), args.clone());
            }
        }
    }
    for (id, inner) in objects {
        top.insert(id, inner);
    }
    if !user.room.is_empty() {
        let mut grouped: Map<String, Value> = Map::new();
        for intent in &user.room {
            let (name, item) = room_item(intent);
            grouped
                .entry(name.to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("房间分组恒为数组")
                .push(item);
        }
        top.insert("room".into(), Value::Object(grouped));
    }
    if !user.notify.is_empty() {
        let items: Vec<Value> = user
            .notify
            .iter()
            .map(|n| {
                let mut m = Map::new();
                m.insert("message".into(), json!(n.message));
                put(&mut m, "groupInterval", n.group_interval);
                Value::Object(m)
            })
            .collect();
        top.insert("notify".into(), json!(items));
    }
    if !user.market.is_empty() {
        let mut grouped: Map<String, Value> = Map::new();
        for intent in &user.market {
            let (name, item) = market_entry(intent);
            grouped
                .entry(name.to_string())
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .expect("市场分组恒为数组")
                .push(item);
        }
        top.insert("global".into(), Value::Object(grouped));
    }
    Value::Object(top)
}

impl IntentBatch {
    /// 规范线形态 JSON：键序由 BTreeMap/固定市场序决定，值是清洗后
    /// 载荷。`from_json(p, &batch.to_json()) == batch` 对 from_json 能
    /// 产生的任意批次成立；直接构造的非规范批次（如超长文本）不保证。
    pub fn to_json(&self) -> Value {
        let mut users = Map::new();
        for (player, intents) in &self.users {
            users.insert(player.clone(), user_to_json(intents));
        }
        Value::Object(users)
    }
}
