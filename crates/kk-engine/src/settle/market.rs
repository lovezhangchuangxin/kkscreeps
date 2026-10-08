//! 结算 terminal 跨房发送、市场订单生命周期和成交，维护交易流水。
//!
//! 市场使用引擎级候选状态，世界快照不包含订单簿；玩家资金单位为毫 credits。
//! 价格、数量和资金会读取此前动作的修改，订单集合仍只引用 tick 开始的存量。
//! Intershard 成交按确定性收集顺序执行，上游使用随机顺序。
//! 来源：上游 engine/src/processor/global-intents/market.js。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::{ObjId, RoomState};

use crate::intent::{IntentBatch, MarketIntent};
use crate::settle::PlayerState;

/// 校验完整的大写房名 W/E + 数字 + N/S + 数字，每段至少一位数字。
pub fn is_valid_room_name(name: &str) -> bool {
    let b = name.as_bytes();
    let mut i = 0;
    if i >= b.len() || (b[i] != b'W' && b[i] != b'E') {
        return false;
    }
    i += 1;
    let d1 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == d1 {
        return false;
    }
    if i >= b.len() || (b[i] != b'N' && b[i] != b'S') {
        return false;
    }
    i += 1;
    let d2 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i == b.len() && i > d2
}

/// 解析房名并返回 Chebyshev 距离，解析失败返回 None。
/// 当前世界不环绕，与上游闭合 MMO 世界的最短距离计费不同。
pub fn calc_rooms_distance(room1: &str, room2: &str) -> Option<i64> {
    let a = kk_core::RoomName::parse(room1).ok()?;
    let b = kk_core::RoomName::parse(room2).ok()?;
    Some(((a.xx - b.xx).abs().max((a.yy - b.yy).abs())) as i64)
}

/// 按 ceil(amount * (1-exp(-range/30))) 计算能量费用。
pub fn calc_terminal_energy_cost(amount: i64, range: i64) -> i64 {
    (amount as f64 * (1.0 - (-(range as f64) / 30.0).exp())).ceil() as i64
}

/// 全世界保留最近 1000 笔交易，超限删除最早记录；上游数据库没有此容量限制。
pub const TRANSACTION_CAP: usize = 1000;

/// 市场订单；price 使用整数毫 credits，玩家视图读取时除以 1000。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketOrder {
    pub id: String,
    pub user: String,
    pub active: bool,
    pub order_type: String,
    pub resource_type: String,
    pub price: i64,
    /// 当前可成交量，生命周期会根据资源或资金同步。
    pub amount: i64,
    /// 未成交的总量，成交时减少，extendOrder 时增加。
    pub remaining_amount: i64,
    /// 累计订购总量，extendOrder 时增加，成交不减少。
    pub total_amount: i64,
    /// 普通资源的订单 terminal 房间；intershard 订单为 None。
    pub room_name: Option<String>,
    /// 创建时的游戏 tick，与 created_timestamp 的墙钟不同。
    pub created: u64,
    /// 创建墙钟，单位为 Unix 毫秒，供过期判定使用。
    pub created_timestamp: u64,
}

/// 一次资源交易；sender/recipient 为 None 表示无归属一方。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub id: u64,
    /// 成交发生的游戏 tick。
    pub time: u64,
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub resource_type: String,
    pub amount: i64,
    pub from: String,
    pub to: String,
    pub description: Option<String>,
    pub order: Option<serde_json::Value>,
}

/// 由 Engine 持有的世界级订单簿与交易记录；克隆时包含 ID 计数器，
/// 候选 tick 失败后必须保留所有字段的旧值。
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketState {
    /// 订单按 ID 字典序存储，例如 mkt10 排在 mkt9 前，不表示创建顺序。
    pub orders: BTreeMap<String, MarketOrder>,
    pub transactions: VecDeque<Transaction>,
    next_order_seq: u64,
    next_transaction_id: u64,
}

impl MarketState {
    pub(super) fn validate_checkpoint(&self) -> Result<(), String> {
        if self.transactions.len() > TRANSACTION_CAP
            || self.next_order_seq == u64::MAX
            || self.next_transaction_id == u64::MAX
        {
            return Err("检查点市场游标或交易记录数量非法".into());
        }
        for (id, order) in &self.orders {
            let sequence = id.strip_prefix("mkt").and_then(|v| v.parse::<u64>().ok());
            if order.id != *id || sequence.is_none_or(|n| n == 0 || n > self.next_order_seq) {
                return Err(format!("检查点市场订单 ID 非法：{id}"));
            }
        }
        let mut ids = BTreeSet::new();
        if self
            .transactions
            .iter()
            .any(|tx| tx.id == 0 || tx.id > self.next_transaction_id || !ids.insert(tx.id))
        {
            return Err("检查点交易 ID 非法或重复".into());
        }
        Ok(())
    }

    pub fn next_order_id(&mut self) -> String {
        self.next_order_seq += 1;
        format!("mkt{}", self.next_order_seq)
    }

    pub fn insert_order(&mut self, order: MarketOrder) {
        self.orders.insert(order.id.clone(), order);
    }

    pub fn remove_order(&mut self, id: &str) -> Option<MarketOrder> {
        self.orders.remove(id)
    }

    /// 追加交易，抬高 ID 计数器并淘汰超出容量的最早记录。
    pub fn record_transaction(&mut self, tx: Transaction) {
        self.next_transaction_id = self.next_transaction_id.max(tx.id);
        self.transactions.push_back(tx);
        while self.transactions.len() > TRANSACTION_CAP {
            self.transactions.pop_front();
        }
    }

    pub fn alloc_transaction_id(&mut self) -> u64 {
        self.next_transaction_id += 1;
        self.next_transaction_id
    }

    /// 按入账倒序筛选玩家的发送或接收记录，再限制条数；同 tick 也按入账倒序。
    pub fn transactions_for(
        &self,
        player: &str,
        outgoing: bool,
        limit: usize,
    ) -> Vec<&Transaction> {
        self.transactions
            .iter()
            .rev()
            .filter(|tx| {
                if outgoing {
                    tx.sender.as_deref() == Some(player)
                } else {
                    tx.recipient.as_deref() == Some(player)
                }
            })
            .take(limit)
            .collect()
    }
}

#[derive(Debug, Clone)]
struct PendingSend {
    room: usize,
    id: ObjId,
    target_room: String,
    resource_type: String,
    amount: i64,
    description: Option<String>,
}

/// 所有房间已结算后处理 terminal send 和市场订单，输入仍是待发布的候选副本。
/// 先清除 send 字段再执行请求，拒绝路径也不将请求留到下一 tick。
/// 来源：上游 engine/src/processor/global-intents/market.js。
pub fn settle_market(
    rooms: &mut [(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    batch: &IntentBatch,
    time: u64,
    now_ms: u64,
) {
    let mut sends: Vec<PendingSend> = Vec::new();
    for (ri, (_, room)) in rooms.iter_mut().enumerate() {
        for (id, obj) in room.objects.iter_mut() {
            let RoomObject::Generic(g) = obj else {
                continue;
            };
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
                continue;
            };
            if v.get("structureType").and_then(|s| s.as_str()) != Some("terminal") {
                continue;
            }
            let truthy = |s: &serde_json::Value| match s {
                serde_json::Value::Null => false,
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::Number(n) => n.as_f64() != Some(0.0),
                serde_json::Value::String(s) => !s.is_empty(),
                _ => true,
            };
            let Some(send) = v.get("send").cloned().filter(truthy) else {
                continue;
            };
            v["send"] = serde_json::Value::Null;
            g.data = v.to_string();
            sends.push(PendingSend {
                room: ri,
                id: *id,
                target_room: send
                    .get("targetRoomName")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                resource_type: send
                    .get("resourceType")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string(),
                amount: send.get("amount").and_then(|s| s.as_i64()).unwrap_or(0),
                description: send
                    .get("description")
                    .and_then(|s| s.as_str())
                    .filter(|d| !d.is_empty())
                    .map(str::to_string),
            });
        }
    }
    for s in sends {
        execute_send(rooms, market, time, s);
    }
    process_market_intents(rooms, players, market, batch, time, now_ms);
}

fn room_has_terminal_of(rooms: &[(String, RoomState)], room: &str, player: &str) -> bool {
    let Some((_, state)) = rooms.iter().find(|(n, _)| n == room) else {
        return false;
    };
    for obj in state.objects.values() {
        let RoomObject::Generic(g) = obj else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
            continue;
        };
        if v.get("structureType").and_then(|s| s.as_str()) != Some("terminal") {
            continue;
        }
        return v.get("owner").and_then(|s| s.as_str()) == Some(player);
    }
    false
}

/// Sell 订单由订单方供货，Buy 订单由 deal 发起者供货。
/// 按剩余量、卖家库存、买家容量和资金限制数量；成功转移后结算资金、订单和冷却。
#[allow(clippy::too_many_arguments)]
fn execute_terminal_deal(
    rooms: &mut [(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    time: u64,
    dealer: &str,
    oid: &str,
    deal_amount: i64,
    target_room: &str,
) {
    let Some(order_room) = market.orders.get(oid).and_then(|o| o.room_name.clone()) else {
        return;
    };
    let Some((order_room_idx, order_terminal)) = find_user_terminal(rooms, &order_room) else {
        return;
    };
    let Some((target_idx, target_terminal)) = find_user_terminal(rooms, target_room) else {
        return;
    };
    if read_terminal_data(rooms, target_idx, target_terminal)
        .and_then(|v| v.get("cooldownTime").and_then(|t| t.as_i64()))
        .is_some_and(|t| t > time as i64)
    {
        return;
    }
    let (order_type, rt, price_milli, remaining) = {
        let Some(o) = market.orders.get(oid) else {
            return;
        };
        (
            o.order_type.clone(),
            o.resource_type.clone(),
            o.price,
            o.remaining_amount,
        )
    };
    let (seller, buyer) = if order_type == c::ORDER_SELL {
        (
            (order_room_idx, order_terminal),
            (target_idx, target_terminal),
        )
    } else {
        (
            (target_idx, target_terminal),
            (order_room_idx, order_terminal),
        )
    };
    let mut amount = deal_amount.min(remaining.max(0));
    let seller_data = read_terminal_data(rooms, seller.0, seller.1);
    let seller_owner = seller_data
        .as_ref()
        .and_then(|v| v.get("owner"))
        .and_then(|s| s.as_str())
        .map(str::to_string);
    if let Some(sd) = &seller_data
        && seller_owner.is_some()
    {
        let have = sd
            .pointer(&format!("/store/{rt}"))
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        amount = amount.min(have);
    }
    let buyer_data = read_terminal_data(rooms, buyer.0, buyer.1);
    let buyer_owner = buyer_data
        .as_ref()
        .and_then(|v| v.get("owner"))
        .and_then(|s| s.as_str())
        .map(str::to_string);
    if let Some(bd) = &buyer_data
        && buyer_owner.is_some()
    {
        let total: i64 = bd
            .get("store")
            .and_then(|s| s.as_object())
            .map(|m| m.values().filter_map(|v| v.as_i64()).sum())
            .unwrap_or(0);
        let cap = bd
            .get("storeCapacity")
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        amount = amount.min((cap - total).max(0));
    }
    if !(amount > 0) {
        return;
    }
    let mut deal_cost = amount.checked_mul(price_milli).unwrap_or(i64::MAX / 2);
    if buyer_owner.is_some() {
        let buyer_user = if order_type == c::ORDER_SELL {
            dealer.to_string()
        } else {
            market
                .orders
                .get(oid)
                .map(|o| o.user.clone())
                .unwrap_or_default()
        };
        let buyer_money = players.get(&buyer_user).map(|p| p.money).unwrap_or(0);
        deal_cost = deal_cost.min(buyer_money);
        amount = if price_milli > 0 {
            deal_cost / price_milli
        } else {
            0
        };
        deal_cost = amount.checked_mul(price_milli).unwrap_or(i64::MAX / 2);
        if amount == 0 {
            return;
        }
    }
    let order_payload = serde_json::json!({
        "id": oid,
        "type": order_type,
        "price": price_milli as f64 / 1000.0,
    });
    // 官方 market.js:339 executeTransfer 的费用端恒为 targetTerminal
    // （dealer 终端）：SELL 单 dealer 是接收端（To），BUY 单是发出端（From）。
    let fee_side = if order_type == c::ORDER_SELL {
        FeeSide::To
    } else {
        FeeSide::From
    };
    let ok = execute_transfer(
        rooms,
        seller,
        buyer,
        &rt,
        amount,
        fee_side,
        None,
        Some(order_payload),
        market,
        time,
    );
    if !ok {
        return;
    }
    if let Some(so) = &seller_owner
        && let Some(state) = players.get_mut(so)
    {
        state.money = state.money.saturating_add(deal_cost);
    }
    if let Some(bo) = &buyer_owner
        && let Some(state) = players.get_mut(bo)
    {
        state.money -= deal_cost;
    }
    if let Some(order) = market.orders.get_mut(oid) {
        order.amount -= amount;
        order.remaining_amount -= amount;
    }
    write_cooldown(
        rooms,
        target_idx,
        target_terminal,
        time + terminal::terminal_cooldown(rooms, (target_idx, target_terminal), time),
    );
}

/// Intershard 资源直接在玩家 resources 和 money 间划转，不使用 terminal。
/// 数量受订单可成交量、剩余量和卖家库存限制，买家资金不足时整笔拒绝。
fn execute_direct_deal(
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    dealer: &str,
    oid: &str,
    deal_amount: i64,
) {
    let Some(order) = market.orders.get(oid) else {
        return;
    };
    let (order_type, rt, price_milli) = (
        order.order_type.clone(),
        order.resource_type.clone(),
        order.price,
    );
    let order_user = order.user.clone();
    let (seller, buyer) = if order_type == c::ORDER_SELL {
        (order_user.clone(), dealer.to_string())
    } else {
        (dealer.to_string(), order_user.clone())
    };
    if !players.contains_key(&seller) || !players.contains_key(&buyer) {
        return;
    }
    let order_amount = market.orders.get(oid).map(|o| o.amount).unwrap_or(0);
    let order_remaining = market
        .orders
        .get(oid)
        .map(|o| o.remaining_amount)
        .unwrap_or(0);
    let seller_have = players
        .get(&seller)
        .map(|p| p.resources.get(&rt).copied().unwrap_or(0))
        .unwrap_or(0);
    let amount = deal_amount
        .min(order_amount)
        .min(order_remaining)
        .min(seller_have);
    if amount <= 0 {
        return;
    }
    let deal_cost = amount.checked_mul(price_milli).unwrap_or(i64::MAX / 2);
    let buyer_money = players.get(&buyer).map(|p| p.money).unwrap_or(0);
    if buyer_money < deal_cost {
        return;
    }
    let seller_state = players.get_mut(&seller).unwrap();
    seller_state.money = seller_state.money.saturating_add(deal_cost);
    *seller_state.resources.entry(rt.clone()).or_insert(0) -= amount;
    let buyer_state = players.get_mut(&buyer).unwrap();
    buyer_state.money -= deal_cost;
    *buyer_state.resources.entry(rt.clone()).or_insert(0) += amount;
    if let Some(order) = market.orders.get_mut(oid) {
        order.amount -= amount;
        order.remaining_amount -= amount;
    }
}

fn bump_store(v: &mut serde_json::Value, key: &str, delta: i64) {
    let cur = v
        .pointer(&format!("/store/{key}"))
        .and_then(|s| s.as_i64())
        .unwrap_or(0);
    v["store"][key] = serde_json::json!(cur + delta);
}

fn read_terminal_data(
    rooms: &[(String, RoomState)],
    ri: usize,
    id: ObjId,
) -> Option<serde_json::Value> {
    let obj = rooms.get(ri)?.1.objects.get(&id)?;
    let RoomObject::Generic(g) = obj else {
        return None;
    };
    serde_json::from_str(&g.data).ok()
}

/// 取房间中首个 terminal，要求非空主人；首个无主时不再查找后面的终端。
/// 上游同房多终端取最后一个，此差异在正常单终端房间不可见。
fn find_user_terminal(rooms: &[(String, RoomState)], room: &str) -> Option<(usize, ObjId)> {
    let (ri, found) = rooms.iter().enumerate().find(|(_, (n, _))| n == room)?;
    let state = &found.1;
    for (id, obj) in &state.objects {
        let RoomObject::Generic(g) = obj else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&g.data) else {
            continue;
        };
        if v.get("structureType").and_then(|s| s.as_str()) != Some("terminal") {
            continue;
        }
        if v.get("owner")
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            return Some((ri, *id));
        }
        return None;
    }
    None
}

/// 保存冷却截止 tick，是绝对时钟，不是剩余时长。
fn write_cooldown(rooms: &mut [(String, RoomState)], ri: usize, id: ObjId, t: u64) {
    if let Some(RoomObject::Generic(g)) =
        rooms.get_mut(ri).and_then(|(_, r)| r.objects.get_mut(&id))
        && let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&g.data)
    {
        v["cooldownTime"] = serde_json::json!(t);
        g.data = v.to_string();
    }
}

mod orders;
mod terminal;
use orders::process_market_intents;
use terminal::{FeeSide, execute_send, execute_transfer};
#[cfg(test)]
mod tests;
