//! terminal 发送与成交共用的库存、手续费和交易流水写入。
use super::*;

/// 消费已清除 send 字段的请求，转移成功才设置发送方 cooldownTime。
pub(super) fn execute_send(
    rooms: &mut [(String, RoomState)],
    market: &mut MarketState,
    time: u64,
    s: PendingSend,
) {
    // 跨房发送明确只认内建资源：Mod 声明的命名空间资源在此拒绝，
    // 不依赖房间阶段已拦一道（测试与未来旁路也不放过）。
    if !c::RESOURCES_ALL.contains(&s.resource_type.as_str()) {
        return;
    }
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
        write_cooldown(
            rooms,
            s.room,
            s.id,
            time + terminal_cooldown(rooms, (s.room, s.id), time),
        );
    }
}

/// 资源转移费用由发送方或接收方承担，指资源方向，不表示 deal 的发起者。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FeeSide {
    From,
    To,
}

/// 按目标共享容量限制数量，再对限制后的数量计算费用。成功时写回两侧资源
/// 和费用、记录交易；失败时不写冷却。费用端由调用方按官方语义选择：
/// send 为发送终端（From），deal 恒为 dealer 终端（market.js:339）。
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_transfer(
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
    // 官方 terminal/send.js:19-25：距离取 calcRoomsDistance(continuous=true)
    //（按当前房集活算跨度逐轴环绕）。
    let span = super::world_span(rooms.iter().map(|(name, _)| name.as_str()));
    let Some(range) = calc_rooms_distance(&from_room, &to_room, span) else {
        return false;
    };
    let mut cost = calc_terminal_energy_cost(amount, range);
    // 官方 market.js:33-40：费用按承担方 terminal 的 OPERATE_TERMINAL
    // 倍率 ceil；全局 market 相边界为 endTime > time（严格）。
    let fee_terminal = match fee_side {
        FeeSide::From => from,
        FeeSide::To => to,
    };
    if let Some(effect) = rooms[fee_terminal.0]
        .1
        .effects
        .get(&fee_terminal.1)
        .and_then(|entries| entries.iter().find(|e| e.power == c::PWR_OPERATE_TERMINAL))
        && effect.end_time > time
        && let Some(multiplier) =
            super::super::powers::power_effect_value(c::PWR_OPERATE_TERMINAL, effect.level)
    {
        cost = ((cost as f64) * multiplier).ceil() as i64;
    }
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
        g.data = v.to_string().into();
    } else {
        if to_user.is_some()
            && let Some(RoomObject::Generic(g)) = rooms[to.0].1.objects.get_mut(&to.1)
        {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, amount);
            g.data = v.to_string().into();
        }
        if let Some(RoomObject::Generic(g)) = rooms[from.0].1.objects.get_mut(&from.1) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, -amount);
            g.data = v.to_string().into();
        }
        let (fee_room, fee_id) = match fee_side {
            FeeSide::From => from,
            FeeSide::To => to,
        };
        if let Some(RoomObject::Generic(g)) = rooms[fee_room].1.objects.get_mut(&fee_id) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, "energy", -cost);
            g.data = v.to_string().into();
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

/// 官方 market.js:83-88（send）与 :391-396（deal，效果读 targetTerminal）：
/// terminal 冷却按 OPERATE_TERMINAL 倍率 round，边界 endTime > time（严格）。
pub(super) fn terminal_cooldown(
    rooms: &[(String, RoomState)],
    terminal: (usize, ObjId),
    time: u64,
) -> u64 {
    if let Some(effect) = rooms[terminal.0]
        .1
        .effects
        .get(&terminal.1)
        .and_then(|entries| entries.iter().find(|e| e.power == c::PWR_OPERATE_TERMINAL))
        && effect.end_time > time
        && let Some(multiplier) =
            super::super::powers::power_effect_value(c::PWR_OPERATE_TERMINAL, effect.level)
    {
        (((c::TERMINAL_COOLDOWN as f64) * multiplier).round()) as u64
    } else {
        c::TERMINAL_COOLDOWN as u64
    }
}
