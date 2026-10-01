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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Default, Clone, PartialEq)]
pub struct MarketState {
    /// 订单按 ID 字典序存储，例如 mkt10 排在 mkt9 前，不表示创建顺序。
    pub orders: BTreeMap<String, MarketOrder>,
    pub transactions: VecDeque<Transaction>,
    next_order_seq: u64,
    next_transaction_id: u64,
}

impl MarketState {
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

/// 保存 tick 开始时的订单 ID，后续引用与生命周期只处理这些存量订单。
/// 每个玩家按 create、changePrice、extend、cancel、deal 分组执行，同组保持提交顺序；
/// 新订单本轮保持 inactive/amount=0，下一 tick 才同步可成交量。
#[allow(clippy::too_many_arguments)]
fn process_market_intents(
    rooms: &mut [(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    batch: &IntentBatch,
    time: u64,
    now_ms: u64,
) {
    // 创建订单不会加入此集合，避免同轮提前激活或引用尚未发布的订单。
    let pre_tick_ids: BTreeSet<String> = market.orders.keys().cloned().collect();
    // 改价请求在费用检查前标记跳过成交，因此扣费失败也会阻止本轮 deal。
    let mut skip: BTreeSet<String> = BTreeSet::new();
    let mut cancelled: BTreeSet<String> = BTreeSet::new();
    let mut terminal_deals: Vec<(
        String, /*player*/
        String, /*order*/
        i64,
        String, /*room*/
    )> = Vec::new();
    let mut direct_deals: Vec<(String, String, i64)> = Vec::new();

    for (player, intents) in &batch.users {
        fn section_rank(i: &MarketIntent) -> u8 {
            match i {
                MarketIntent::CreateOrder { .. } => 0,
                MarketIntent::ChangeOrderPrice { .. } => 1,
                MarketIntent::ExtendOrder { .. } => 2,
                MarketIntent::CancelOrder { .. } => 3,
                MarketIntent::Deal { .. } => 4,
            }
        }
        let mut ordered: Vec<&MarketIntent> = intents.market.iter().collect();
        ordered.sort_by_key(|i| section_rank(i));
        for intent in ordered {
            match intent {
                MarketIntent::CreateOrder {
                    order_type,
                    resource_type,
                    price_milli,
                    total_amount,
                    room_name,
                } => {
                    let (Some(price), Some(total)) = (*price_milli, *total_amount) else {
                        continue;
                    };
                    if price == 0 || total == 0 {
                        continue;
                    }
                    let Some(rt) = resource_type.as_deref() else {
                        continue;
                    };
                    if !c::RESOURCES_ALL.contains(&rt) && !c::INTERSHARD_RESOURCES.contains(&rt) {
                        continue;
                    }
                    let Some(otype) = order_type.as_deref() else {
                        continue;
                    };
                    if otype != c::ORDER_BUY && otype != c::ORDER_SELL {
                        continue;
                    }
                    let intershard = c::INTERSHARD_RESOURCES.contains(&rt);
                    if !intershard {
                        let Some(rn) = room_name.as_deref() else {
                            continue;
                        };
                        if !room_has_terminal_of(rooms, rn, player) {
                            continue;
                        }
                    }
                    if price <= 0 || total <= 0 {
                        continue;
                    }
                    let fee = (price as f64 * total as f64 * c::MARKET_FEE).ceil() as i64;
                    if players.get(player).map_or(0, |p| p.money) < fee {
                        continue;
                    }
                    let money = &mut players.entry(player.clone()).or_default().money;
                    *money -= fee;
                    let id = market.next_order_id();
                    market.insert_order(MarketOrder {
                        id: id.clone(),
                        user: player.clone(),
                        active: false,
                        order_type: otype.to_string(),
                        resource_type: rt.to_string(),
                        price,
                        amount: 0,
                        remaining_amount: total,
                        total_amount: total,
                        room_name: room_name.clone().filter(|_| !intershard),
                        created: time,
                        created_timestamp: now_ms,
                    });
                }
                MarketIntent::ChangeOrderPrice {
                    order_id,
                    new_price_milli,
                } => {
                    let Some(oid) = order_id.as_deref() else {
                        continue;
                    };
                    if !pre_tick_ids.contains(oid) {
                        continue;
                    }
                    let Some(order) = market.orders.get(oid) else {
                        continue;
                    };
                    if order.user != *player {
                        continue;
                    }
                    let Some(new_price) = *new_price_milli else {
                        continue;
                    };
                    if new_price <= 0 {
                        continue;
                    }
                    let old_price = order.price;
                    if new_price != old_price {
                        skip.insert(oid.to_string());
                    }
                    if new_price > old_price {
                        let fee = ((new_price - old_price) as f64
                            * order.remaining_amount as f64
                            * c::MARKET_FEE)
                            .ceil() as i64;
                        if players.get(player).map_or(0, |p| p.money) < fee {
                            continue;
                        }
                        players.entry(player.clone()).or_default().money -= fee;
                    }
                    if let Some(order) = market.orders.get_mut(oid) {
                        order.price = new_price;
                    }
                }
                MarketIntent::ExtendOrder {
                    order_id,
                    add_amount,
                } => {
                    let Some(oid) = order_id.as_deref() else {
                        continue;
                    };
                    if !pre_tick_ids.contains(oid) {
                        continue;
                    }
                    let Some(order) = market.orders.get(oid) else {
                        continue;
                    };
                    if order.user != *player {
                        continue;
                    }
                    let Some(add) = *add_amount else {
                        continue;
                    };
                    if add <= 0 {
                        continue;
                    }
                    let fee = (order.price as f64 * add as f64 * c::MARKET_FEE).ceil() as i64;
                    if players.get(player).map_or(0, |p| p.money) < fee {
                        continue;
                    }
                    players.entry(player.clone()).or_default().money -= fee;
                    if let Some(order) = market.orders.get_mut(oid) {
                        order.remaining_amount += add;
                        order.total_amount += add;
                    }
                }
                MarketIntent::CancelOrder { order_id } => {
                    let Some(oid) = order_id.as_deref() else {
                        continue;
                    };
                    if !pre_tick_ids.contains(oid) {
                        continue;
                    }
                    if market.orders.get(oid).is_some_and(|o| o.user == *player) {
                        if let Some(order) = market.orders.get_mut(oid) {
                            order.remaining_amount = 0;
                        }
                        cancelled.insert(oid.to_string());
                    }
                }
                MarketIntent::Deal {
                    order_id,
                    amount,
                    target_room_name,
                } => {
                    let Some(oid) = order_id.as_deref() else {
                        continue;
                    };
                    if !pre_tick_ids.contains(oid) || skip.contains(oid) {
                        continue;
                    }
                    let Some(order) = market.orders.get(oid) else {
                        continue;
                    };
                    let Some(amount) = *amount else {
                        continue;
                    };
                    if amount <= 0 {
                        continue;
                    }
                    if c::INTERSHARD_RESOURCES.contains(&order.resource_type.as_str()) {
                        direct_deals.push((player.clone(), oid.to_string(), amount));
                        continue;
                    }
                    let Some(target) = target_room_name.as_deref() else {
                        continue;
                    };
                    if !room_has_terminal_of(rooms, target, player) {
                        continue;
                    }
                    terminal_deals.push((
                        player.clone(),
                        oid.to_string(),
                        amount,
                        target.to_string(),
                    ));
                }
            }
        }
    }

    // Terminal deal 按两房距离升序执行，距离相同保持收集顺序。
    terminal_deals.sort_by_key(|(_, oid, _, target)| {
        let room = market
            .orders
            .get(oid)
            .and_then(|o| o.room_name.clone())
            .unwrap_or_default();
        calc_rooms_distance(target, &room).unwrap_or(i64::MAX)
    });
    for (dealer, oid, deal_amount, target_room) in terminal_deals {
        execute_terminal_deal(
            rooms,
            players,
            market,
            time,
            &dealer,
            &oid,
            deal_amount,
            &target_room,
        );
    }

    for (dealer, oid, deal_amount) in direct_deals {
        execute_direct_deal(players, market, &dealer, &oid, deal_amount);
    }

    order_lifecycle(rooms, players, market, &cancelled, &pre_tick_ids, now_ms);
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
    let ok = execute_transfer(
        rooms,
        seller,
        buyer,
        &rt,
        amount,
        FeeSide::To,
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
        time + c::TERMINAL_COOLDOWN as u64,
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

/// 只处理 tick 开始已有订单，先删除取消或过期订单，再同步可成交量和 active。
/// 过期依据墙钟毫秒，退费截断到整数毫 credits；主动取消不退费。
fn order_lifecycle(
    rooms: &[(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    cancelled: &BTreeSet<String>,
    pre_tick_ids: &BTreeSet<String>,
    now_ms: u64,
) {
    let ids: Vec<String> = market
        .orders
        .keys()
        .filter(|id| pre_tick_ids.contains(*id))
        .cloned()
        .collect();
    for id in ids {
        let Some(o) = market.orders.get(&id) else {
            continue;
        };
        let (order_user, otype, rt, room_name, created_ts) = (
            o.user.clone(),
            o.order_type.clone(),
            o.resource_type.clone(),
            o.room_name.clone(),
            o.created_timestamp,
        );
        if cancelled.contains(&id) {
            market.remove_order(&id);
            continue;
        }
        if now_ms.saturating_sub(created_ts) > c::MARKET_ORDER_LIFE_TIME as u64 {
            let (remaining, price) = {
                let Some(o) = market.orders.get(&id) else {
                    continue;
                };
                (o.remaining_amount, o.price)
            };
            let refund = (remaining as f64 * price as f64 * c::MARKET_FEE) as i64;
            if refund > 0
                && let Some(state) = players.get_mut(&order_user)
            {
                state.money = state.money.saturating_add(refund);
            }
            market.remove_order(&id);
            continue;
        }
        let intershard = c::INTERSHARD_RESOURCES.contains(&rt.as_str());
        if otype == c::ORDER_SELL {
            let remaining = market
                .orders
                .get(&id)
                .map(|o| o.remaining_amount)
                .unwrap_or(0);
            let available = if intershard {
                players
                    .get(&order_user)
                    .map(|p| p.resources.get(&rt).copied().unwrap_or(0))
                    .unwrap_or(0)
            } else {
                match room_name.as_deref() {
                    Some(rn) if room_has_terminal_of(rooms, rn, &order_user) => {
                        find_user_terminal(rooms, rn)
                            .and_then(|(ri, tid)| {
                                read_terminal_data(rooms, ri, tid).and_then(|v| {
                                    v.pointer(&format!("/store/{rt}")).and_then(|s| s.as_i64())
                                })
                            })
                            .unwrap_or(0)
                    }
                    _ => 0,
                }
            };
            let available = available.min(remaining).max(0);
            let Some(order) = market.orders.get_mut(&id) else {
                continue;
            };
            if order.active {
                if available <= 0 {
                    order.active = false;
                    order.amount = 0;
                } else if order.amount != available {
                    order.amount = available;
                }
            } else if available > 0 {
                order.active = true;
                order.amount = available;
            }
        }
        if otype == c::ORDER_BUY {
            let (price, remaining) = {
                let Some(o) = market.orders.get(&id) else {
                    continue;
                };
                (o.price, o.remaining_amount)
            };
            let money = players.get(&order_user).map(|p| p.money).unwrap_or(0);
            let terminal = room_name
                .as_deref()
                .and_then(|rn| find_user_terminal(rooms, rn));
            let is_owner = intershard
                || room_name
                    .as_deref()
                    .is_some_and(|rn| room_has_terminal_of(rooms, rn, &order_user));
            let mut new_amount = if price > 0 { money / price } else { 0 }.min(remaining);
            if let Some((ri, tid)) = terminal {
                let td = read_terminal_data(rooms, ri, tid);
                let has_user = td
                    .as_ref()
                    .map(|v| v.get("owner").and_then(|s| s.as_str()).is_some())
                    .unwrap_or(false);
                if has_user {
                    let total: i64 = td
                        .as_ref()
                        .and_then(|v| v.get("store").and_then(|s| s.as_object()))
                        .map(|m| m.values().filter_map(|x| x.as_i64()).sum())
                        .unwrap_or(0);
                    let cap = td
                        .as_ref()
                        .and_then(|v| v.get("storeCapacity").and_then(|s| s.as_i64()))
                        .unwrap_or(0);
                    new_amount = new_amount.min((cap - total).max(0));
                }
            }
            let new_active = is_owner && new_amount > 0;
            let Some(order) = market.orders.get_mut(&id) else {
                continue;
            };
            if order.amount != new_amount || order.active != new_active {
                order.amount = new_amount;
                order.active = new_active;
            }
        }
    }
}

/// 消费已清除 send 字段的请求，转移成功才设置发送方 cooldownTime。
fn execute_send(
    rooms: &mut [(String, RoomState)],
    market: &mut MarketState,
    time: u64,
    s: PendingSend,
) {
    let from_data = read_terminal_data(rooms, s.room, s.id);
    if from_data
        .as_ref()
        .and_then(|v| v.get("cooldownTime"))
        .and_then(|t| t.as_i64())
        .is_some_and(|t| t > time as i64)
    {
        return;
    }
    let Some((to_room, to_id)) = find_user_terminal(rooms, &s.target_room) else {
        return;
    };
    let description = s.description.map(|d| d.replace('<', "&lt;"));
    let ok = execute_transfer(
        rooms,
        (s.room, s.id),
        (to_room, to_id),
        &s.resource_type,
        s.amount,
        FeeSide::From,
        description,
        None,
        market,
        time,
    );
    if ok {
        write_cooldown(rooms, s.room, s.id, time + c::TERMINAL_COOLDOWN as u64);
    }
}

/// 资源转移费用由发送方或接收方承担，指资源方向，不表示 deal 的发起者。
#[derive(Debug, Clone, Copy, PartialEq)]
enum FeeSide {
    From,
    #[allow(dead_code)]
    To,
}

/// 按目标共享容量限制数量，再对限制后的数量计算费用。成功时写回两侧资源
/// 和费用、记录交易；失败时不写冷却。当前不模拟 terminal power 效果。
#[allow(clippy::too_many_arguments)]
fn execute_transfer(
    rooms: &mut [(String, RoomState)],
    from: (usize, ObjId),
    to: (usize, ObjId),
    resource_type: &str,
    mut amount: i64,
    fee_side: FeeSide,
    description: Option<String>,
    order: Option<serde_json::Value>,
    market: &mut MarketState,
    time: u64,
) -> bool {
    let Some(from_data) = read_terminal_data(rooms, from.0, from.1) else {
        return false;
    };
    let from_user = from_data
        .get("owner")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let have = from_data
        .pointer(&format!("/store/{resource_type}"))
        .and_then(|s| s.as_i64())
        .unwrap_or(0);
    if from_data.get("store").is_none() || have < amount {
        return false;
    }
    let Some(to_data) = read_terminal_data(rooms, to.0, to.1) else {
        return false;
    };
    let to_user = to_data
        .get("owner")
        .and_then(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if to_user.is_some() {
        let total: i64 = to_data
            .get("store")
            .and_then(|s| s.as_object())
            .map(|m| m.values().filter_map(|v| v.as_i64()).sum())
            .unwrap_or(0);
        let capacity = to_data
            .get("storeCapacity")
            .and_then(|s| s.as_i64())
            .unwrap_or(0);
        amount = amount.min((capacity - total).max(0));
    }
    if !(amount > 0) {
        return false;
    }
    let from_room = rooms[from.0].0.clone();
    let to_room = rooms[to.0].0.clone();
    let Some(range) = calc_rooms_distance(&from_room, &to_room) else {
        return false;
    };
    let cost = calc_terminal_energy_cost(amount, range);
    let energy_ok = match fee_side {
        FeeSide::From => {
            let energy = from_data
                .pointer("/store/energy")
                .and_then(|s| s.as_i64())
                .unwrap_or(0);
            if resource_type == c::RESOURCE_ENERGY {
                energy >= amount + cost
            } else {
                energy >= cost
            }
        }
        FeeSide::To => {
            let to_energy = to_data
                .pointer("/store/energy")
                .and_then(|s| s.as_i64())
                .unwrap_or(0);
            to_energy >= cost
        }
    };
    if !energy_ok {
        return false;
    }
    // 发送到同一 terminal 时必须连续更新同一 JSON，避免源、目标快照相互覆盖。
    // 有主终端资源转移抵消，仅扣能量费用；无主接收方不执行资源入账。
    let same = from == to;
    if same {
        let Some(RoomObject::Generic(g)) = rooms[from.0].1.objects.get_mut(&from.1) else {
            return false;
        };
        let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
        if v.get("store").is_none() {
            return false;
        }
        if to_user.is_some() {
            bump_store(&mut v, resource_type, amount);
        }
        bump_store(&mut v, resource_type, -amount);
        bump_store(&mut v, "energy", -cost);
        g.data = v.to_string();
    } else {
        if to_user.is_some()
            && let Some(RoomObject::Generic(g)) = rooms[to.0].1.objects.get_mut(&to.1)
        {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, amount);
            g.data = v.to_string();
        }
        if let Some(RoomObject::Generic(g)) = rooms[from.0].1.objects.get_mut(&from.1) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, -amount);
            g.data = v.to_string();
        }
        let (fee_room, fee_id) = match fee_side {
            FeeSide::From => from,
            FeeSide::To => to,
        };
        if let Some(RoomObject::Generic(g)) = rooms[fee_room].1.objects.get_mut(&fee_id) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, "energy", -cost);
            g.data = v.to_string();
        }
    }
    let tx_id = market.alloc_transaction_id();
    market.record_transaction(Transaction {
        id: tx_id,
        time,
        sender: from_user,
        recipient: to_user,
        resource_type: resource_type.to_string(),
        amount,
        from: from_room,
        to: to_room,
        description,
        order,
    });
    true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_ids_are_monotonic() {
        let mut m = MarketState::default();
        assert_eq!(m.next_order_id(), "mkt1");
        assert_eq!(m.next_order_id(), "mkt2");
    }

    #[test]
    fn transaction_cap_evicts_oldest() {
        let mut m = MarketState::default();
        for i in 0..(TRANSACTION_CAP as u64 + 5) {
            m.record_transaction(Transaction {
                id: i,
                time: i,
                sender: Some("p1".into()),
                recipient: Some("p2".into()),
                resource_type: "energy".into(),
                amount: 1,
                from: "W1N1".into(),
                to: "W5N1".into(),
                description: None,
                order: None,
            });
        }
        assert_eq!(m.transactions.len(), TRANSACTION_CAP);
        assert_eq!(m.transactions.front().unwrap().id, 5, "最旧逐出");
        assert_eq!(
            m.transactions.back().unwrap().id,
            TRANSACTION_CAP as u64 + 4
        );
    }

    #[test]
    fn transactions_for_filters_direction_and_take_limit() {
        let mut m = MarketState::default();
        for i in 0..5 {
            m.record_transaction(Transaction {
                id: i,
                time: i,
                sender: Some("p1".into()),
                recipient: Some("p2".into()),
                resource_type: "energy".into(),
                amount: 1,
                from: "W1N1".into(),
                to: "W5N1".into(),
                description: None,
                order: None,
            });
        }
        let out: Vec<u64> = m
            .transactions_for("p1", true, 2)
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(out, vec![4, 3]);
        assert_eq!(m.transactions_for("p2", false, 10).len(), 5);
        assert!(m.transactions_for("p1", false, 10).is_empty());
    }

    #[test]
    fn transactions_for_tie_order_and_zero_limit() {
        let mut m = MarketState::default();
        for i in 0..3 {
            m.record_transaction(Transaction {
                id: i,
                time: 42,
                sender: Some("p1".into()),
                recipient: Some("p2".into()),
                resource_type: "energy".into(),
                amount: 1,
                from: "W1N1".into(),
                to: "W5N1".into(),
                description: None,
                order: None,
            });
        }
        let ids: Vec<u64> = m
            .transactions_for("p1", true, 10)
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, vec![2, 1, 0], "同刻按入账倒序");
        assert!(m.transactions_for("p1", true, 0).is_empty(), "limit=0");
        let empty = MarketState::default();
        assert!(
            empty.transactions_for("p1", true, 10).is_empty(),
            "空 ledger"
        );
    }

    #[test]
    fn alloc_transaction_id_monotonic() {
        let mut m = MarketState::default();
        assert_eq!(m.alloc_transaction_id(), 1);
        assert_eq!(m.alloc_transaction_id(), 2);
        m.record_transaction(Transaction {
            id: 10,
            time: 0,
            sender: None,
            recipient: None,
            resource_type: "energy".into(),
            amount: 1,
            from: "W1N1".into(),
            to: "W5N1".into(),
            description: None,
            order: None,
        });
        assert_eq!(m.alloc_transaction_id(), 11, "max 抬升后继续单调");
    }
}
