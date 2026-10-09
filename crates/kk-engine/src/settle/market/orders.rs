//! 订单创建、修改、取消和存量订单的生命周期。
use super::*;

/// 保存 tick 开始时的订单 ID，后续引用与生命周期只处理这些存量订单。
/// 每个玩家按 create、changePrice、extend、cancel、deal 分组执行，同组保持提交顺序；
/// 新订单本轮保持 inactive/amount=0，下一 tick 才同步可成交量。
#[allow(clippy::too_many_arguments)]
pub(super) fn process_market_intents(
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
                    // 市场订单明确只认内建资源与跨 shard 资源：Mod 声明的
                    // 命名空间资源（键含冒号）不在此列，玩家面预检随
                    // RESOURCES_ALL 目录放行后在此丢弃，不产生订单；
                    // Mod 资源市场随后续批次开放（见 mod-resource-catalog）。
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

    // Terminal deal 按两房距离升序执行，距离相同保持收集顺序；官方
    // global-intents/market.js:34-35 用 continuous 距离（环绕同源）。
    let span = world_span(rooms.iter().map(|(name, _)| name.as_str()));
    terminal_deals.sort_by_key(|(_, oid, _, target)| {
        let room = market
            .orders
            .get(oid)
            .and_then(|o| o.room_name.clone())
            .unwrap_or_default();
        calc_rooms_distance(target, &room, span).unwrap_or(i64::MAX)
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

/// 只处理 tick 开始已有订单，先删除取消或过期订单，再同步可成交量和 active。
/// 过期依据墙钟毫秒，退费截断到整数毫 credits；主动取消不退费。
pub(super) fn order_lifecycle(
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
