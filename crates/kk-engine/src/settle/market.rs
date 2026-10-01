//! 市场域态（2B 批 P0 地基）：订单簿 + 交易流水 + credits 的引擎侧承载。
//!
//! 官方模型（实树取证）：
//! - `market.orders` / `transactions` / `market.stats` 是 **DB 集合**（跨房世界
//!   级），`users.money` 是玩家字段（毫 credits）——处理器为
//!   `processor/global-intents/market.js`（main.js:63-65 的 global 阶段，
//!   **房间结算 commit 之后**才跑）。
//! - 我方等价落点：`Engine.market`（与 `Engine.players` 同层的引擎态，非
//!   kk-world 世界态——快照契约不动）；`PlayerState.money/resources`。
//! - `market.stats`（getHistory 数据源）官方由 backend cron 填充、引擎内恒空
//!   （driver/lib/runtime/data.js:51-75，无数据时 :60 恒初始化 `{all: []}`）——不建模，
//!   空形态在注入面直接给（P2）。
//!
//! P0 只落类型与容量纪律；P1 进驻 terminal send 消费链（executeTransfer）
//! ；订单簿生命周期/deal 随 P2 进驻本模块。

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::{ObjId, RoomState};

use crate::intent::{IntentBatch, MarketIntent};
use crate::settle::PlayerState;

/// 官方 `/^(W|E)\d+(N|S)\d+$/`（terminal/send.js:7-9 与 structures.js:720
/// 同式）——大写方向位、两段 ≥1 数字、全串匹配（无 regex 依赖的手写
/// 状态机；比 [`kk_core::RoomName::parse`] 严：不接受小写/截断前缀）。
pub fn is_valid_room_name(name: &str) -> bool {
    let b = name.as_bytes();
    let mut i = 0;
    // ^(W|E)
    if i >= b.len() || (b[i] != b'W' && b[i] != b'E') {
        return false;
    }
    i += 1;
    // \d+
    let d1 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == d1 {
        return false;
    }
    // (N|S)
    if i >= b.len() || (b[i] != b'N' && b[i] != b'S') {
        return false;
    }
    i += 1;
    // \d+$
    let d2 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i == b.len() && i > d2
}

/// 官方 utils.js:644-655 calcRoomsDistance(continuous=true)——**不环绕**：
/// 官方 MMO 闭合世界经 `getWorldSize()` 取 `min(size−d, d)` 环绕；我方单机
/// 世界不环绕（getWorldSize()=1 登记面、liveWorldSize 能力键 false），plain
/// Chebyshev——登记差异（TERMINAL-SEND-015 wrap 计费随 liveWorldSize 批）。
/// 房名不可解析 → None（官方 roomNameToXY 对畸形名产 NaN → 距离/成本
/// NaN——JS 比较对 NaN 恒假即**各门放行**、转移以 NaN 落库（官方怪癖）；
/// 我方 None→调用方拒收，同收敛更安全，登记差异）。
pub fn calc_rooms_distance(room1: &str, room2: &str) -> Option<i64> {
    let a = kk_core::RoomName::parse(room1).ok()?;
    let b = kk_core::RoomName::parse(room2).ok()?;
    Some(((a.xx - b.xx).abs().max((a.yy - b.yy).abs())) as i64)
}

/// 官方 utils.js:657-659 calcTerminalEnergyCost：
/// `Math.ceil(amount * (1 - Math.exp(-range / 30)))`。
pub fn calc_terminal_energy_cost(amount: i64, range: i64) -> i64 {
    (amount as f64 * (1.0 - (-(range as f64) / 30.0).exp())).ceil() as i64
}

/// 交易流水容量（有界截断，**登记差异**：官方 db.transactions 无界自然增长，
/// 玩家视图各 100 条——我方世界级保留最近 1000 条；超出后最早条目被逐出，
/// 单机测试/机器人场景不可观测（套件最大单 tick 交易量个位数））。
pub const TRANSACTION_CAP: usize = 1000;

/// 一条市场订单。price 为**整数毫 credits**（官方 db 形态——intent 清洗层
/// `parseInt((1000*price).toFixed(0))`，common/lib/system.js:8；玩家视图 ÷1000
/// 在 prelude getter 层做，与官方 _getOrders 同位）。
#[derive(Debug, Clone, PartialEq)]
pub struct MarketOrder {
    /// 订单 id（十进制序号串——官方为 db ObjectId，套件按不透明串消费）。
    pub id: String,
    /// 归属玩家名。
    pub user: String,
    /// 公开可见性（生命周期尾段按资源/credits 同步——官方 :550-587）。
    pub active: bool,
    /// "sell" | "buy"（ORDER_SELL/ORDER_BUY）。
    pub order_type: String,
    pub resource_type: String,
    /// 毫 credits。
    pub price: i64,
    /// 当前可成交量（生命周期尾段与资源/credits 同步；成交时扣减）。
    pub amount: i64,
    /// 剩余总量（成交/取消扣减；extendOrder 增加）。
    pub remaining_amount: i64,
    /// 总量（extendOrder 增加；不因成交减少——官方 :251 双加只增）。
    pub total_amount: i64,
    /// 非 intershard 订单的 terminal 所在房（intershard 为 None）。
    pub room_name: Option<String>,
    /// 创建 tick（官方非 intershard 订单 `created: gameTime`，:147）。
    pub created: u64,
    /// 创建墙钟毫秒（官方 createdTimestamp——过期判定输入，MARKET-ORDER-009）。
    pub created_timestamp: u64,
}

/// 一条交易流水（官方 transactions 集合 insert 形态，
/// global-intents/market.js:58-66 executeTransfer 的 bulkTransactions 载荷）。
#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub id: u64,
    pub time: u64,
    /// None = NPC/系统侧（官方 `fromTerminal.user ? ""+user : undefined`）。
    pub sender: Option<String>,
    pub recipient: Option<String>,
    pub resource_type: String,
    pub amount: i64,
    pub from: String,
    pub to: String,
    /// terminal.send 的 description（官方 :91 `<`→`&lt;` 转义）。
    pub description: Option<String>,
    /// deal 成交附带（官方 :339-343 `{id,type,price}`——price 已 ÷1000）。
    pub order: Option<serde_json::Value>,
}

/// 世界级市场态（Engine 持有）。
#[derive(Debug, Default, Clone)]
pub struct MarketState {
    /// 键序=BTreeMap 字典序（"mkt10"<"mkt9"，N≥10 后与官方 db/ObjectId
    /// 插入序错位）——影响 getAllOrders 返回序/my_orders 迭代序等**纯序面**
    ///（套件无序断言；逻辑面各消费点按 id 直查不依赖序——评审 2 F4 登记）
    pub orders: BTreeMap<String, MarketOrder>,
    pub transactions: VecDeque<Transaction>,
    next_order_seq: u64,
    next_transaction_id: u64,
}

impl MarketState {
    /// 生成下一个订单 id（十进制串）。
    pub fn next_order_id(&mut self) -> String {
        self.next_order_seq += 1;
        format!("mkt{}", self.next_order_seq)
    }

    /// 插入订单（id 由调用方经 [`Self::next_order_id`] 取得）。
    pub fn insert_order(&mut self, order: MarketOrder) {
        self.orders.insert(order.id.clone(), order);
    }

    /// 移除订单（官方 bulk.remove——取消/过期/填满清理由生命周期段调用）。
    pub fn remove_order(&mut self, id: &str) -> Option<MarketOrder> {
        self.orders.remove(id)
    }

    /// 追加交易流水并执行容量截断（最旧逐出）。
    pub fn record_transaction(&mut self, tx: Transaction) {
        self.next_transaction_id = self.next_transaction_id.max(tx.id);
        self.transactions.push_back(tx);
        while self.transactions.len() > TRANSACTION_CAP {
            self.transactions.pop_front();
        }
    }

    /// 生成下一交易 id（官方 db insert 自动分配 _id 的等价物——P1 send/
    /// deal 结算调用）。
    pub fn alloc_transaction_id(&mut self) -> u64 {
        self.next_transaction_id += 1;
        self.next_transaction_id
    }

    /// 玩家视角的最近交易（官方 findEx sort time desc limit N——同刻多条按
    /// 入账倒序；方向 = sender/recipient 匹配）。
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

/// 一个待执行的全局 send（房间相 stow 的载荷，官方 object.send 字段）。
#[derive(Debug, Clone)]
struct PendingSend {
    /// 发送方 terminal 所在 `settled` 下标与对象 id。
    room: usize,
    id: ObjId,
    target_room: String,
    resource_type: String,
    amount: i64,
    description: Option<String>,
}

/// 市场全局阶段（2B 批 P1：terminal send 消费；订单簿/deal 随 P2 进驻）。
///
/// 官方 main.js:63-65 的 global 阶段（processor/global.js:48-49 调
/// marketProcessor）：**房间结算全部完成后**运行，读到的都是本 tick 结算后
/// 状态——我方在 `tick_with_intents_with_input` 的 commit 循环**之前**对
/// 已结算工作副本执行（`settled` Vec），失败原子性与「一次成功 tick ⇒
/// generation 恰 +1」双不变量原样保持。官方 global 阶段无 throw 路径
/// （逐 intent 静默跳过）——本函数不可失败同构。
pub fn settle_market(
    rooms: &mut [(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    batch: &IntentBatch,
    time: u64,
    now_ms: u64,
) {
    // ---- pass 1：收集全部 terminal 的 send 载荷并**无条件清字段**
    //（官方 :71-75 `_.filter(terminals, i => !!i.send)` 后先
    // `bulk.update(terminal, {send: null})` 再做任何 return——冷却/无目标
    // 等拒绝路径同样消费掉本 tick 的 send）
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
            // 官方 `!!i.send` 的 JS truthy：非 false/0/""/null 均收集（字段
            // 缺省走下游门全拒——与官方畸形 send 物化 undefined 同收敛）
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
                // 官方 :91 `intent.description ? replace : undefined`——空串
                // falsy 无 description 键（评审 1 P2-1 清偿：玩家可达）
                description: send
                    .get("description")
                    .and_then(|s| s.as_str())
                    .filter(|d| !d.is_empty())
                    .map(str::to_string),
            });
        }
    }
    // ---- pass 2：逐条执行（官方 :77-94 逐 terminal——房间结算序后按
    // 对象迭代序；官方 db 顺序=插入序，我方=房名+id 升序，同 tick 多
    // terminal send 的执行序差异登记【套件单 tick 单 send 不区分】）
    for s in sends {
        execute_send(rooms, market, time, s);
    }
    // ---- P2：订单簿（官方 :104-589——send 消费先行同官方段序）
    process_market_intents(rooms, players, market, batch, time, now_ms);
}

/// 订单簿全生命周期（2B 批 P2，官方 global-intents/market.js:104-589）。
///
/// **快照语义**（官方 getInterRoom 在 global 阶段开始前取 `orders` 数组，
/// 本 tick 内 createOrder 插入的订单不在其中——intent 的订单 id 引用与
/// 生命周期尾段都只见**tick 前存量**；我方以 `pre_tick_ids` 集合等价实现）。
/// 分节消费序=官方段序（createOrder→changeOrderPrice→extendOrder→
/// cancelOrder→deal 收集）——MarketIntent Vec 的清洗序不可用于消费
///（P0 登记「消费者须分节遍历」）。
#[allow(clippy::too_many_arguments)]
fn process_market_intents(
    rooms: &mut [(String, RoomState)],
    players: &mut BTreeMap<String, PlayerState>,
    market: &mut MarketState,
    batch: &IntentBatch,
    time: u64,
    now_ms: u64,
) {
    let pre_tick_ids: BTreeSet<String> = market.orders.keys().cloned().collect();
    // changeOrderPrice 置 _skip（同 tick 后续 deal 跳过该订单——官方 :180）
    let mut skip: BTreeSet<String> = BTreeSet::new();
    // cancelOrder 置 _cancelled（生命周期尾段移除——官方 :260）
    let mut cancelled: BTreeSet<String> = BTreeSet::new();
    // deal 收集（官方 :265-285；terminalDeals 在全部玩家分节后统一处理）
    let mut terminal_deals: Vec<(
        String, /*player*/
        String, /*order*/
        i64,
        String, /*room*/
    )> = Vec::new();
    let mut direct_deals: Vec<(String, String, i64)> = Vec::new();

    for (player, intents) in &batch.users {
        // 官方 :104-107 userIntents.forEach——玩家序官方=db 插入序，我方
        // =BTreeMap 名字序（确定性；登记差异）
        // 分节消费（官方段序 createOrder(:109)→changeOrderPrice(:168)→
        // extendOrder(:215)→cancelOrder(:256)→deal(:265)；段内=注册序，
        // 各段 intents 数组官方=push 序）。sort_by_key 稳定排序=按段稳定
        // 划分——同玩家「先改价后取消」官方按原 remaining 收提价费再取消
        //（评审 2 F3/评审 1 F4：单趟声明序会先 cancel 清 remaining 吞费）
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
                    // 官方 :112-125 复验（清洗后毫价/整量；None≙NaN falsy 拒）
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
                    // P2 处理器契约（P0 评审 1 P2-1）：order_type=None 直接拒
                    //——官方 :139 物化串默认落 ORDER_BUY，等价性纯靠 prelude
                    // type 门不可达（勿仿官方默认）
                    let Some(otype) = order_type.as_deref() else {
                        continue;
                    };
                    if otype != c::ORDER_BUY && otype != c::ORDER_SELL {
                        continue;
                    }
                    let intershard = c::INTERSHARD_RESOURCES.contains(&rt);
                    // 官方 :119-121 terminal 归属门（intershard 免）
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
                    // 官方 :127-131 fee=ceil(毫price×total×FEE)——f64 ceil
                    //（官方 JS number 同算；毫级整数域精确）
                    let fee = (price as f64 * total as f64 * c::MARKET_FEE).ceil() as i64;
                    // 费门逐 intent 读活值（官方 user.money 每条重读+bulk.inc
                    // 立改内存对象 bulk.js:90-97——同 tick 前笔扣费对后笔可见；
                    // 循环头快照会让 Σfee>credits 时全额扣款把 money 打负）
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
                        // 官方 :139 `type == SELL ? SELL : BUY`（此处已过
                        // 双值门，直取）
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
                    // 官方 :170-175 订单存在+归属（快照外 id=miss 拒）
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
                    // 官方 :175 `!newPrice || newPrice <= 0`——负价同拒
                    //（评审 2 F2/评审 1 F3：仅拒 ==0 会让负毫价落库反转
                    // deal 资金流向；prelude 正数门外裸 intent 域可达）
                    if new_price <= 0 {
                        continue;
                    }
                    let old_price = order.price;
                    if new_price != old_price {
                        skip.insert(oid.to_string());
                    }
                    // 官方 :183-190 提价差额费（降/平价免费）——费门读活值
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
                    // 费门读活值（同 createOrder 注——官方逐 intent 重读）
                    if players.get(player).map_or(0, |p| p.money) < fee {
                        continue;
                    }
                    players.entry(player.clone()).or_default().money -= fee;
                    // 官方 :250-251 remaining/total 双加
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
                    // 官方 :269-274：订单存在（快照）且非 _skip；`amount <= 0`
                    // 拒——NaN 过比较门（我方 None 恒拒，P0 登记）
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
                    // 官方 :279-280 目标房 terminal 归属复验
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

    // ---- terminalDeals（官方 :289-398：按目标房→订单房距离升序）
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

    // ---- directDeals（官方 :400-495 intershard——官方 _.shuffle 非确定，
    // 我方稳定序，登记差异；玩家面不可达【INTERSHARD_RESOURCES 无来源】）
    for (dealer, oid, deal_amount) in direct_deals {
        execute_direct_deal(players, market, &dealer, &oid, deal_amount);
    }

    // ---- 生命周期尾段（官方 :497-589——**只见 tick 前存量**：尾段遍历的是
    // getInterRoom 快照 orders 数组，本 tick createOrder 插入延迟到 :591-599
    // bulk.execute 落库——新订单创建 tick 恒 active=false/amount=0，下一
    // tick 才进尾段同步（评审 1 F2/评审 2 F2：遍历全集会提前一拍同步）
    order_lifecycle(rooms, players, market, &cancelled, &pre_tick_ids, now_ms);
}

/// 房内归属玩家的 terminal 存在性（官方 `terminalsByRoom[room].user ==
/// user` 归属门——createOrder :119-121 / deal 收集 :279-280）。
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
        // 同房多 terminal 官方 indexBy 末者胜、我方首个（既有登记）——归属
        // 门语义下首个非属即整门 false（官方末者非属同 false；同房混主为
        // 夹具态不可达）
        return v.get("owner").and_then(|s| s.as_str()) == Some(player);
    }
    false
}

/// 官方 terminalDeals 单条（:291-398）。
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
    // 官方 :292-299 双 terminal 存在（订单房 terminal=terminalsByRoom 任意
    // 首个；目标房 terminal=dealer 的——收集段已验归属）。无主 terminal 官方
    // 走 `if(seller.user)` NPC 分支（:316-337，含无买家钱凭空入账怪癖）；
    // 我方 find_user_terminal 要求有主→整单中断——方向保守（更安全），
    // execute_transfer P1 已登记同族，评审 2 F7 登记本分支
    let Some(order_room) = market.orders.get(oid).and_then(|o| o.room_name.clone()) else {
        return;
    };
    let Some((order_room_idx, order_terminal)) = find_user_terminal(rooms, &order_room) else {
        return;
    };
    let Some((target_idx, target_terminal)) = find_user_terminal(rooms, target_room) else {
        return;
    };
    // 官方 :300-302 目标 terminal 冷却门
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
    // 官方 :306-313 买卖定向：SELL——dealer 是买家（buyer=targetTerminal、
    // seller=orderTerminal）；BUY——dealer 是卖家（seller=targetTerminal、
    // buyer=orderTerminal）。【勘误：初版写反——SELL 侧 seller 误取 dealer
    // terminal，无库存钳 0 静默不成交，MARKET-DEAL 族四红根因】
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
    // 官方 :315-326 三重钳：deal 量 → seller 资源 → buyer 空间
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
    // 官方 :328-337 buyer money 钳链（dealCost=毫×量；floor 除法回推量）：
    // buyer=SELL 型的 dealer / BUY 型的订单主（usersById[buyer.user].money）
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
    // 官方 :339-343 executeTransfer（fee=目标 terminal=dealer 侧）+ order 附带
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
    // 官方 :345-386 双向 money（seller += dealCost；buyer −= dealCost——毫）
    if let Some(so) = &seller_owner
        && let Some(state) = players.get_mut(so)
    {
        // 饱和加（评审 2 F5：deal_cost 钳后 ≤buyer money<i64 域，防御注入域）
        state.money = state.money.saturating_add(deal_cost);
    }
    if let Some(bo) = &buyer_owner
        && let Some(state) = players.get_mut(bo)
    {
        state.money -= deal_cost;
    }
    // 官方 :387-390 订单 amount/remaining 双减
    if let Some(order) = market.orders.get_mut(oid) {
        order.amount -= amount;
        order.remaining_amount -= amount;
    }
    // 官方 :391-396 目标 terminal 冷却
    write_cooldown(
        rooms,
        target_idx,
        target_terminal,
        time + c::TERMINAL_COOLDOWN as u64,
    );
}

/// 官方 directDeals 单条（intershard，:402-495）——玩家 resources+money
/// 直接划转（无 terminal）。
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
    // 官方 :407-414 买卖定向（用户对用户）
    let (seller, buyer) = if order_type == c::ORDER_SELL {
        (order_user.clone(), dealer.to_string())
    } else {
        (dealer.to_string(), order_user.clone())
    };
    // 官方 :416-418 缺一拒
    if !players.contains_key(&seller) || !players.contains_key(&buyer) {
        return;
    }
    // 官方 :423 四重量钳
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
    // 官方 :430-432 `buyer.user && (!buyer.money || buyer.money < dealCost)` 拒
    let buyer_money = players.get(&buyer).map(|p| p.money).unwrap_or(0);
    if buyer_money < deal_cost {
        return;
    }
    // 官方 :434-436 seller += money、resources −=（:436 原地同步）
    let seller_state = players.get_mut(&seller).unwrap();
    seller_state.money = seller_state.money.saturating_add(deal_cost);
    *seller_state.resources.entry(rt.clone()).or_insert(0) -= amount;
    // 官方 :464-465 buyer −= money、resources +=
    let buyer_state = players.get_mut(&buyer).unwrap();
    buyer_state.money -= deal_cost;
    *buyer_state.resources.entry(rt.clone()).or_insert(0) += amount;
    // 官方 :481-483 订单 amount/remaining 双减
    if let Some(order) = market.orders.get_mut(oid) {
        order.amount -= amount;
        order.remaining_amount -= amount;
    }
}

/// 订单生命周期尾段（官方 :497-589——只见 tick 前存量 `pre_tick_ids`；
/// `cancelled` 集=_cancelled 标记）。
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
        // 头部快照取值落本地（避免跨 get_mut 的长借用）
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
        // 官方 :502-505 _cancelled → 移除
        if cancelled.contains(&id) {
            market.remove_order(&id);
            continue;
        }
        // 官方 :507-534 过期（墙钟 createdTimestamp+30d）→ 退 remaining×毫price
        // ×FEE（官方不 ceil——f64 毫；我方 i64 截断，登记亚毫精度差异）→ 移除
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
        // 官方 :536-538 无主订单跳过同步（我方订单恒有主——防御分支）
        let intershard = c::INTERSHARD_RESOURCES.contains(&rt.as_str());
        // 官方 SELL 同步（:542-567）：可用=min(资源, remaining)
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
                // 官方 :546 `terminal && terminal.user == order.user ? store[rt] : 0`
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
        // 官方 BUY 同步（:569-587）：newAmount=min(floor(money/price),
        // remaining, terminal 空间)；newActive=isOwner && newAmount>0
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
            // 官方 isOwner（:572-573）：intershard 或房内归属 terminal
            let is_owner = intershard
                || room_name
                    .as_deref()
                    .is_some_and(|rn| room_has_terminal_of(rooms, rn, &order_user));
            // price=0 注入订单分歧（评审 2 F6 登记）：官方 floor(money/0)
            // =Infinity→newAmount=min(Inf,remaining,space)=有限值→active；
            // 我方 else 0→inactive。仅 place_market_order 注入域可达
            //（createOrder/changeOrderPrice 处理器双门 0 拒）——deal 面两侧
            // 同收敛（官方 0/0=NaN→!amount 拒 ↔ 我方 amount=0 return）
            let mut new_amount = if price > 0 { money / price } else { 0 }.min(remaining);
            // 官方 :576-580 terminal 有主时的空间钳
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

/// 官方 global-intents/market.js:77-94 的单 terminal send 消费段。
fn execute_send(
    rooms: &mut [(String, RoomState)],
    market: &mut MarketState,
    time: u64,
    s: PendingSend,
) {
    // 冷却门（send 字段已在收集轮被清——官方同序）
    let from_data = read_terminal_data(rooms, s.room, s.id);
    if from_data
        .as_ref()
        .and_then(|v| v.get("cooldownTime"))
        .and_then(|t| t.as_i64())
        .is_some_and(|t| t > time as i64)
    {
        return;
    }
    // 目标房 terminal 存在且有主（官方 :80-82 terminalsByRoom + `.user`）
    let Some((to_room, to_id)) = find_user_terminal(rooms, &s.target_room) else {
        return;
    };
    // 官方 :84-88 cooldown=TERMINAL_COOLDOWN（PWR_OPERATE_TERMINAL 折扣归
    // power 批——结构无 effects 数据，分支死码，登记）；:90-94 附加
    // description 的 `<`→`&lt;` 转义（:91）；executeTransfer 成功才挂冷却
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

/// 转移能量成本的承担侧（官方 executeTransfer 的 transferFeeTerminal）：
/// send 链恒为发送方（market.js:90）；deal 链为目标房 terminal（:339）。
#[derive(Debug, Clone, Copy, PartialEq)]
enum FeeSide {
    From,
    /// deal 链（P2）消费——send 链不可达
    #[allow(dead_code)]
    To,
}

/// 官方 executeTransfer（global-intents/market.js:15-69）——P1 send 链形态
/// （from/to 双侧恒有 user；fee 恒 From；order 附带归 P2）。返回是否成功
/// （false/undefined 两侧同不挂冷却——官方 `!(amount > 0)` return undefined
/// 与 return false 在 :92-94 的 truthy 判定下等价）。
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
    // :22-24 发送方资源面复验（store 缺键 unwrap_or(0) → 0 < amount 拒，
    // 官方 `undefined >= amount` 为假的等价收紧——TowerState 同款 DB 不变量
    // 口径，P5 登记）。**登记分歧**：官方复验带 `fromTerminal.user &&` 前置
    // ——无主发送方跳过复验可写负 store（裸 intent 注入域可达；prelude
    // this.my 门+processor :248 所有权门使玩家面不可达）；我方无条件复验拒
    let Some(from_data) = read_terminal_data(rooms, from.0, from.1) else {
        return false;
    };
    // JS falsy 语义：官方 `fromTerminal.user &&`/`toTerminal.user ?` 对空串
    // 同 undefined 处理（m2b2 测试抓出的保真点）——"" → None
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
    // :25-29 接收方空间钳：amount = min(amount, max(0, storeCapacity − Σstore))
    //（calcResources = Σstore.values()，utils.js:618-622）
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
    // :30-32 `!(amount > 0)` → return undefined（钳到 0/负：静默不转移、
    // 不挂冷却、无交易记录）
    if !(amount > 0) {
        return false;
    }
    // :34-35 距离/成本（对**钳后** amount 计算——官方序）
    let from_room = rooms[from.0].0.clone();
    let to_room = rooms[to.0].0.clone();
    let Some(range) = calc_rooms_distance(&from_room, &to_room) else {
        return false;
    };
    let cost = calc_terminal_energy_cost(amount, range);
    // :37-40 PWR_OPERATE_TERMINAL 折扣归 power 批（死码，登记）
    // :42-47 费用承担侧能量门（send 链 fee===from：非 energy `energy < cost`
    // 拒 / energy `energy < amount + cost` 拒；fee===to 形态供 P2 deal 链）
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
    // :49-56 三方写回——单次解析顺序变更（P5 link 自指清偿纪律）：官方三步
    // ①to.store[rt]+=amount ②from.store[rt]−=amount ③fee.store.energy−=cost
    //；自指（send 到本房=同一 terminal）时同对象三步序净收敛 rt 不动、
    // energy −= cost——对同一 data 连续变更，防快照覆写
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
        // ①接收方
        if to_user.is_some()
            && let Some(RoomObject::Generic(g)) = rooms[to.0].1.objects.get_mut(&to.1)
        {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, amount);
            g.data = v.to_string();
        }
        // ②发送方资源
        if let Some(RoomObject::Generic(g)) = rooms[from.0].1.objects.get_mut(&from.1) {
            let mut v: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            bump_store(&mut v, resource_type, -amount);
            g.data = v.to_string();
        }
        // ③费用侧能量（send 链=发送方；fee===to 形态供 P2）
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
    // :58-66 bulkTransactions.insert（sender/recipient = 终端归属玩家名
    //——我方键制，官方 user _id，视图层适配【登记】）
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

/// store 键增量（缺键按 0 起——官方 db 终端 store 恒含 energy；资源键可能
/// 减到 0 后保留显式 0 键，官方 JS 同形 `store[rt] − amount`）。
fn bump_store(v: &mut serde_json::Value, key: &str, delta: i64) {
    let cur = v
        .pointer(&format!("/store/{key}"))
        .and_then(|s| s.as_i64())
        .unwrap_or(0);
    v["store"][key] = serde_json::json!(cur + delta);
}

/// 读 terminal 的 Generic data JSON（房 idx + 对象 id）。
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

/// 官方 terminalsByRoom 房间索引（:13 `_.indexBy(terminals, 'room')`——
/// 同房多 terminal 时官方**后者覆盖前者**=取最后一个，我方取首个【同房
/// 单 terminal 不变量下不可区分，登记】）+ `.user` 门（:80-82）。
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
        // 官方 `.user` 门：无主 terminal → 拒（`!terminalsByRooms[x].user`
        // 的 JS falsy——**空串同 undefined 拒**，非 is_some 判定）
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

/// 写 terminal cooldownTime（绝对钟——place setup 契约 P4）。
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
        // 倒序（time desc）：p1 出向取最近 2 条 = id 4,3
        let out: Vec<u64> = m
            .transactions_for("p1", true, 2)
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(out, vec![4, 3]);
        // 入向按 recipient 匹配
        assert_eq!(m.transactions_for("p2", false, 10).len(), 5);
        assert!(m.transactions_for("p1", false, 10).is_empty());
    }

    #[test]
    fn transactions_for_tie_order_and_zero_limit() {
        // 同刻（time 相同）多条：入账倒序（后入账在前——官方 db sort time desc
        // 的同刻序未定义，我方以入账倒序钉死，登记差异）
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
        // record_transaction 抬升计数器（外部注入 id 场景——adapter 注入面）
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
