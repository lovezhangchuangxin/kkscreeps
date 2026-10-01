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
//! P0 只落类型与容量纪律；结算执行器（send 消费/订单簿生命周期/deal）随
//! P1/P2 进驻本模块。

use std::collections::{BTreeMap, VecDeque};

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
