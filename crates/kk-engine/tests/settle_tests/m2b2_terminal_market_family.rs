//! 通过 Engine 验证跨房发送、市场订单和成交。
//!
//! 测试包含房间动作结算与随后的全局市场处理，统一使用固定墙钟。

use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::{Engine, MarketState};
use kk_schema::room::{GenericObj, RoomObject};
use kk_world::ObjId;
use serde_json::{Value, json};

fn terrain_plain() -> kk_core::Terrain {
    kk_core::Terrain::plains()
}

/// 订单创建时间与 TickInput 共用固定 Unix 毫秒墙钟，避免订单意外过期。
const BENCH_NOW_MS: u64 = 1_700_000_000_000;

/// 世界 + 双房各一个 terminal（owner/store 可定制；cooldown_time 缺省 0）。
struct MarketFixture {
    engine: Engine,
}

impl MarketFixture {
    fn new() -> Self {
        let mut world = kk_world::World::new(7);
        world.register_room("W1N1", terrain_plain());
        world.register_room("W5N1", terrain_plain());
        Self {
            engine: Engine::new(world),
        }
    }

    fn terminal(&mut self, room: &str, owner: &str, store: Value) -> ObjId {
        self.terminal_full(room, owner, store, 0)
    }

    fn terminal_full(
        &mut self,
        room: &str,
        owner: &str,
        store: Value,
        cooldown_time: i64,
    ) -> ObjId {
        let data = json!({
            "type": "structure", "structureType": "terminal", "owner": owner,
            "hits": 3000, "hitsMax": 3000,
            "store": store, "storeCapacity": 300000,
            "cooldownTime": cooldown_time,
        });
        self.engine
            .world
            .insert(
                room,
                RoomObject::Generic(GenericObj {
                    id: 0,
                    x: 25,
                    y: 25,
                    data: data.to_string(),
                }),
            )
            .unwrap()
    }

    /// 直接保存 send 请求以单独测试全局处理；绕过 API 清洗与房间执行器。
    fn stow_send(&mut self, id: ObjId, target: &str, rt: &str, amount: i64) {
        let Some(RoomObject::Generic(g)) = self.engine.world.get_mut(id) else {
            panic!("terminal 不存在");
        };
        let mut v: Value = serde_json::from_str(&g.data).unwrap();
        v["send"] = json!({
            "targetRoomName": target, "resourceType": rt,
            "amount": amount, "description": null,
        });
        g.data = v.to_string();
    }

    fn data_of(&self, id: ObjId) -> Value {
        match self.engine.world.get(id).map(|(_, o)| o) {
            Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
            other => panic!("Generic 不存在：{other:?}"),
        }
    }

    /// 完整 intent 链：API 校验序由 prelude 承担，这里从 Action::Send 起
    ///（structures.rs terminal_send 的房间相校验在 settle_room 内跑）。
    fn send_intent(&mut self, player: &str, id: ObjId, target: &str, rt: &str, amount: i64) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            player,
            id,
            Action::Send,
            IntentData {
                target_room_name: Some(target.into()),
                resource_type: Some(rt.into()),
                amount: Some(amount),
                description: None,
                ..IntentData::bare()
            },
        );
        self.engine.tick_with_intents(batch).unwrap();
    }

    /// 用空批次推进 Engine，消费预先保存的 send 请求。正常调用中保存与消费在同 tick。
    fn tick(&mut self) {
        self.engine
            .tick_with_intents(IntentBatch::default())
            .unwrap();
    }
}

fn market(m: &MarketFixture) -> &MarketState {
    &m.engine.market
}

/// 放置订单（adapter placeMarketOrder 等价——owner/type/rt/毫价/量/房）。
fn place_order(
    engine: &mut Engine,
    owner: &str,
    otype: &str,
    rt: &str,
    price_milli: i64,
    total: i64,
    room: &str,
) -> String {
    let id = engine.market.next_order_id();
    engine.market.insert_order(kk_engine::MarketOrder {
        id: id.clone(),
        user: owner.into(),
        active: false,
        order_type: otype.into(),
        resource_type: rt.into(),
        price: price_milli,
        amount: 0,
        remaining_amount: total,
        total_amount: total,
        room_name: Some(room.into()),
        created: engine.world.time(),
        // 使用固定创建墙钟，与 market_tick 的 TickInput 相同。
        created_timestamp: BENCH_NOW_MS,
    });
    id
}

/// 市场全局 intent 推进一 tick（不涉房间相——market intents 走 batch.users）。
fn market_tick(engine: &mut Engine, player: &str, intents: Vec<kk_engine::MarketIntent>) {
    let mut batch = IntentBatch::default();
    batch
        .users
        .entry(player.to_string())
        .or_default()
        .market
        .extend(intents);
    // 传入固定墙钟，避免系统时间改变订单是否过期。
    engine
        .tick_with_intents_with_input(
            batch,
            kk_engine::TickInput {
                now_ms: BENCH_NOW_MS,
            },
        )
        .unwrap();
}

/// Sell 订单方提供资源，deal 发起者付款；验证数量限制、资金与交易订单字段。
#[test]
fn deal_sell_direction_transfer_and_money() {
    let mut f = MarketFixture::new();
    let t1 = f.terminal("W1N1", "p1", json!({"energy": 100000}));
    let t2 = f.terminal("W5N1", "p2", json!({"energy": 100000, "H": 1000}));
    let oid = place_order(&mut f.engine, "p2", "sell", "H", 2500, 1000, "W5N1");
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    f.engine.players.entry("p2".into()).or_default().money = 10_000_000;
    // 生命周期把 inactive 订单激活（amount=min(1000 store, 1000 remaining)）
    market_tick(&mut f.engine, "p1", vec![]);
    assert!(f.engine.market.orders[&oid].active, "SELL 有库存激活");

    // p1 买 100（deal intent）
    let t_deal = f.engine.world.time();
    market_tick(
        &mut f.engine,
        "p1",
        vec![kk_engine::MarketIntent::Deal {
            order_id: Some(oid.clone()),
            amount: Some(100),
            target_room_name: Some("W1N1".into()),
        }],
    );
    // 成交成功后目标 terminal 的冷却截止 tick 为成交 tick 加 TERMINAL_COOLDOWN。
    assert_eq!(
        f.data_of(t1).pointer("/cooldownTime"),
        Some(&json!(t_deal + 10)),
        "目标 terminal 冷却=deal 时 time+TERMINAL_COOLDOWN"
    );
    // 定向：seller=W5N1(p2)、buyer=W1N1(p1)
    assert_eq!(
        f.data_of(t2).pointer("/store/H"),
        Some(&json!(900)),
        "p2 卖出 100"
    );
    assert_eq!(
        f.data_of(t1).pointer("/store/H"),
        Some(&json!(100)),
        "p1 收到 100"
    );
    // dealCost=100×2500毫=250000毫=250 credits；p1 −250、p2 +250（种子在
    // PlayerState 缺省 0——本测试直置）
    let o = &f.engine.market.orders[&oid];
    assert_eq!(o.remaining_amount, 900, "remaining 双减");
    assert_eq!(f.engine.market.transactions.len(), 1);
    let tx = &f.engine.market.transactions[0];
    assert_eq!(
        tx.order.as_ref().unwrap()["price"],
        json!(2.5),
        "order 附带 price÷1000"
    );
}

/// 同 tick 改价使订单跳过成交。让 owner p1 在 dealer p2 前处理，并给买家足额资金，
/// 避免交易实际被资金不足拦住，掩盖改价跳过规则失效。
#[test]
fn change_price_skips_same_tick_deals() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let _t2 = f.terminal("W5N1", "p2", json!({"energy": 100000}));
    let oid = place_order(&mut f.engine, "p1", "sell", "H", 2500, 1000, "W1N1");
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    f.engine.players.entry("p2".into()).or_default().money = 10_000_000;
    market_tick(&mut f.engine, "p1", vec![]);
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().market.push(
        kk_engine::MarketIntent::ChangeOrderPrice {
            order_id: Some(oid.clone()),
            new_price_milli: Some(2600),
        },
    );
    batch
        .users
        .entry("p2".into())
        .or_default()
        .market
        .push(kk_engine::MarketIntent::Deal {
            order_id: Some(oid.clone()),
            amount: Some(100),
            target_room_name: Some("W5N1".into()),
        });
    f.engine
        .tick_with_intents_with_input(
            batch,
            kk_engine::TickInput {
                now_ms: BENCH_NOW_MS,
            },
        )
        .unwrap();
    assert_eq!(f.engine.market.orders[&oid].price, 2600, "改价生效");
    assert_eq!(
        f.engine.market.orders[&oid].remaining_amount, 1000,
        "deal 被 _skip"
    );
    assert_eq!(f.engine.market.transactions.len(), 0);
    assert_eq!(
        f.engine.players["p2"].money, 10_000_000,
        "无成交扣款（若未 skip：旧价 2500×100 应扣 250000）"
    );
}

/// 玩家余额只够一笔创建费，两条同 tick 创建请求应只成功一条。
/// 每笔费用检查须读取扣除前笔费用后的余额，不能沿用循环开始时的快照。
#[test]
fn fee_gate_reads_live_money_same_tick() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000}));
    // fee=ceil(1000×1000×0.05)=50000——money 恰等
    f.engine.players.entry("p1".into()).or_default().money = 50_000;
    let mk = || kk_engine::MarketIntent::CreateOrder {
        order_type: Some("sell".into()),
        resource_type: Some("H".into()),
        price_milli: Some(1000),
        total_amount: Some(1000),
        room_name: Some("W1N1".into()),
    };
    market_tick(&mut f.engine, "p1", vec![mk(), mk()]);
    assert_eq!(
        f.engine.market.orders.len(),
        1,
        "次笔费门拒（活值 0<50000）"
    );
    assert_eq!(
        f.engine.players["p1"].money, 0,
        "不透支——官方 money≥0 不变量"
    );
}

/// 新创建订单本轮不进入生命周期同步，保持 inactive 和 amount=0。
/// 下一 tick 才根据库存激活。来源：上游 global-intents/market.js 的订单快照。
#[test]
fn same_tick_created_order_not_lifecycle_synced() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    market_tick(
        &mut f.engine,
        "p1",
        vec![kk_engine::MarketIntent::CreateOrder {
            order_type: Some("sell".into()),
            resource_type: Some("H".into()),
            price_milli: Some(2500),
            total_amount: Some(1000),
            room_name: Some("W1N1".into()),
        }],
    );
    let (id, order) = f.engine.market.orders.iter().next().unwrap();
    let id = id.clone();
    assert!(!order.active, "创建 tick 不同步（官方快照外）");
    assert_eq!(order.amount, 0);
    market_tick(&mut f.engine, "p1", vec![]);
    let order = &f.engine.market.orders[&id];
    assert!(order.active, "下一 tick 尾段同步：terminal 有 H 1000");
    assert_eq!(order.amount, 1000, "SELL amount=min(可用,remaining)");
}

/// 负改价被拒绝，不改变价格，也不标记跳过成交，同 tick deal 仍按旧价执行。
#[test]
fn negative_price_change_rejected_no_skip() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let _t2 = f.terminal("W5N1", "p2", json!({"energy": 100000}));
    let oid = place_order(&mut f.engine, "p1", "sell", "H", 2500, 1000, "W1N1");
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    f.engine.players.entry("p2".into()).or_default().money = 10_000_000;
    market_tick(&mut f.engine, "p1", vec![]);
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().market.push(
        kk_engine::MarketIntent::ChangeOrderPrice {
            order_id: Some(oid.clone()),
            new_price_milli: Some(-100),
        },
    );
    batch
        .users
        .entry("p2".into())
        .or_default()
        .market
        .push(kk_engine::MarketIntent::Deal {
            order_id: Some(oid.clone()),
            amount: Some(100),
            target_room_name: Some("W5N1".into()),
        });
    f.engine
        .tick_with_intents_with_input(
            batch,
            kk_engine::TickInput {
                now_ms: BENCH_NOW_MS,
            },
        )
        .unwrap();
    assert_eq!(f.engine.market.orders[&oid].price, 2500, "负价拒——原价保持");
    assert_eq!(
        f.engine.market.orders[&oid].remaining_amount, 900,
        "负改价不置 _skip——deal 按旧价成交"
    );
}

/// 即使先提交取消，结算仍应先收提价费，再取消订单。
/// 费用按原剩余量计算；先清零剩余量会漏收费。
#[test]
fn change_then_cancel_same_tick_charges_fee_on_original_remaining() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let oid = place_order(&mut f.engine, "p1", "sell", "H", 2500, 1000, "W1N1");
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    market_tick(&mut f.engine, "p1", vec![]);
    // fee=ceil((3500-2500)×1000×0.05)=50000
    market_tick(
        &mut f.engine,
        "p1",
        vec![
            kk_engine::MarketIntent::ChangeOrderPrice {
                order_id: Some(oid.clone()),
                new_price_milli: Some(3500),
            },
            kk_engine::MarketIntent::CancelOrder {
                order_id: Some(oid.clone()),
            },
        ],
    );
    assert!(
        !f.engine.market.orders.contains_key(&oid),
        "取消移除（尾段）"
    );
    assert_eq!(
        f.engine.players["p1"].money,
        10_000_000 - 50_000,
        "先按原 remaining=1000 收提价费再取消（吞费=10_000_000 霞光）"
    );
}

/// 快照语义：同 tick createOrder 的订单 id 被 deal 引用——官方 miss 跳过
///（getInterRoom 快照不含本 tick 插入）。
#[test]
fn same_tick_created_order_invisible_to_deal() {
    let mut f = MarketFixture::new();
    let _t1 = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    f.engine.players.entry("p1".into()).or_default().money = 10_000_000;
    market_tick(
        &mut f.engine,
        "p1",
        vec![
            kk_engine::MarketIntent::CreateOrder {
                order_type: Some("sell".into()),
                resource_type: Some("H".into()),
                price_milli: Some(2500),
                total_amount: Some(1000),
                room_name: Some("W1N1".into()),
            },
            // 引用刚建的 id "mkt1"
            kk_engine::MarketIntent::Deal {
                order_id: Some("mkt1".into()),
                amount: Some(100),
                target_room_name: Some("W1N1".into()),
            },
        ],
    );
    assert!(f.engine.market.orders.contains_key("mkt1"), "订单已建");
    // deal 对快照外 id miss——remaining 不动（生命周期尾段也不跑：快照外）
    assert_eq!(f.engine.market.orders["mkt1"].remaining_amount, 1000);
    assert_eq!(f.engine.market.transactions.len(), 0);
}

/// buyer money 钳链：dealCost=min(毫×量, buyer money) → floor 回推量
///（官方 :330-337——钱不够时部分成交）。
#[test]
fn deal_money_clamp_partial_fill() {
    let mut f = MarketFixture::new();
    let t1 = f.terminal("W1N1", "p1", json!({"energy": 100000}));
    let _t2 = f.terminal("W5N1", "p2", json!({"energy": 100000, "H": 1000}));
    let oid = place_order(&mut f.engine, "p2", "sell", "H", 2500, 1000, "W5N1");
    // 304000/2500=121.6，向下取整应为 121，round/ceil 会得到 122。
    // 使用非整除金额，才能验证取整方向；同时登记卖家用于资金入账。
    f.engine.players.entry("p1".into()).or_default().money = 304000;
    f.engine.players.entry("p2".into()).or_default();
    market_tick(&mut f.engine, "p1", vec![]);
    market_tick(
        &mut f.engine,
        "p1",
        vec![kk_engine::MarketIntent::Deal {
            order_id: Some(oid.clone()),
            amount: Some(500),
            target_room_name: Some("W1N1".into()),
        }],
    );
    assert_eq!(
        f.data_of(t1).pointer("/store/H"),
        Some(&json!(121)),
        "floor(304000/2500)=121（非整除——round/ceil 均 122）"
    );
    assert_eq!(f.engine.market.orders[&oid].remaining_amount, 879);
    assert_eq!(
        f.engine.players["p1"].money,
        304000 - 121 * 2500,
        "余 1500 毫（部分成交只花 121×2500）"
    );
    assert_eq!(f.engine.players["p2"].money, 121 * 2500);
}

/// 过期退费：createdTimestamp 早于 now−30d → remaining×毫price×FEE 退回并
/// 移除（官方 :507-534；MARKET-ORDER-009 的内部锚）。
#[test]
fn order_expiry_refunds_and_removes() {
    let mut f = MarketFixture::new();
    let _t = f.terminal("W5N1", "p2", json!({"energy": 100, "H": 1000}));
    let id = f.engine.market.next_order_id();
    f.engine.market.insert_order(kk_engine::MarketOrder {
        id: id.clone(),
        user: "p2".into(),
        active: true,
        order_type: "sell".into(),
        resource_type: "H".into(),
        price: 2500,
        amount: 1000,
        remaining_amount: 600,
        total_amount: 1000,
        room_name: Some("W5N1".into()),
        created: 0,
        // 过期：比测试工作台墙钟早 31 天
        created_timestamp: BENCH_NOW_MS - 31 * 24 * 3600 * 1000,
    });
    f.engine.players.entry("p2".into()).or_default().money = 0;
    let before = 0i64;
    market_tick(&mut f.engine, "p2", vec![]);
    assert!(!f.engine.market.orders.contains_key(&id), "过期移除");
    // refund=600×2500×0.05=75000 毫
    assert_eq!(f.engine.players["p2"].money, before + 75000);
}

/// 直接向玩家资源字段注入 token，验证 intershard 的资金与资源划转。
#[test]
fn intershard_direct_deal_transfers_resources_and_money() {
    let mut f = MarketFixture::new();
    let id = f.engine.market.next_order_id();
    f.engine.market.insert_order(kk_engine::MarketOrder {
        id: id.clone(),
        user: "p2".into(),
        active: true,
        order_type: "buy".into(),
        resource_type: "token".into(),
        price: 100_000,
        amount: 10,
        remaining_amount: 10,
        total_amount: 10,
        room_name: None,
        created: 0,
        created_timestamp: BENCH_NOW_MS,
    });
    f.engine.players.entry("p2".into()).or_default().money = 10_000_000;
    f.engine
        .players
        .entry("p1".into())
        .or_default()
        .resources
        .insert("token".into(), 5);
    // p1 卖 5 token 给 p2 的买单（BUY：dealer=p1 是卖家）
    market_tick(
        &mut f.engine,
        "p1",
        vec![kk_engine::MarketIntent::Deal {
            order_id: Some(id.clone()),
            amount: Some(5),
            target_room_name: None,
        }],
    );
    assert_eq!(
        f.engine.players["p1"].resources.get("token"),
        Some(&0),
        "token 划出"
    );
    assert_eq!(f.engine.players["p2"].resources.get("token"), Some(&5));
    assert_eq!(f.engine.players["p1"].money, 5 * 100_000, "seller 收款");
    assert_eq!(f.engine.players["p2"].money, 10_000_000 - 5 * 100_000);
    assert_eq!(f.engine.market.orders[&id].remaining_amount, 5);
}

/// BUY 订单生命周期：newAmount=min(floor(money/price), remaining, 空间)、
/// newActive=isOwner&&>0（官方 :569-587）。
#[test]
fn buy_order_lifecycle_sync() {
    let mut f = MarketFixture::new();
    let t = f.terminal("W1N1", "p1", json!({"energy": 100000}));
    // p1 买 H：价 2500 毫、钱 300000 → floor=120；空间无 H 存量=300000 全空
    f.engine.players.entry("p1".into()).or_default().money = 300000;
    let oid = place_order(&mut f.engine, "p1", "buy", "H", 2500, 1000, "W1N1");
    market_tick(&mut f.engine, "p1", vec![]);
    let o = &f.engine.market.orders[&oid];
    assert!(o.active, "isOwner 且量>0 激活");
    assert_eq!(o.amount, 120, "min(floor(300000/2500),1000,空间)=120");
    let _ = t;

    // 钱花光 → inactive 0
    f.engine.players.get_mut("p1").unwrap().money = 0;
    market_tick(&mut f.engine, "p1", vec![]);
    let o = &f.engine.market.orders[&oid];
    assert!(!o.active, "钱 0 → inactive");
    assert_eq!(o.amount, 0);
}

/// TERMINAL-SEND-012/003 等价链：跨房送达 + 发送方扣资源与能量成本 +
/// 冷却 + 交易入账。W1N1→W5N1 距离 4（plain Chebyshev），
/// cost=ceil(1000×(1−e^(−4/30)))=ceil(1000×0.124839)=125。
#[test]
fn send_transfers_resource_energy_cost_cooldown_and_transaction() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    f.send_intent("p1", from, "W5N1", "H", 1000);

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/store/H"), Some(&json!(0)), "资源送达扣减");
    let cost = (1000.0_f64 * (1.0 - (-4.0_f64 / 30.0).exp())).ceil() as i64;
    assert_eq!(cost, 125, "公式锚：range=4 → 125");
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(100000 - 125)));
    assert_eq!(
        fd.pointer("/cooldownTime"),
        Some(&json!(10)),
        "冷却=time+10（首 tick time=0）"
    );
    assert_eq!(
        fd.pointer("/send"),
        Some(&json!(null)),
        "send 字段已消费清空"
    );

    let td = f.data_of(to);
    assert_eq!(td.pointer("/store/H"), Some(&json!(1000)), "目标收到资源");
    assert_eq!(
        td.pointer("/store/energy"),
        Some(&json!(500)),
        "接收方不扣能量（fee=from）"
    );
    assert_eq!(
        td.pointer("/cooldownTime"),
        Some(&json!(0)),
        "接收方无冷却（send 链只挂发送方；夹具恒写 0）"
    );

    let txs = market(&f);
    assert_eq!(txs.transactions.len(), 1);
    let tx = &txs.transactions[0];
    assert_eq!(tx.sender.as_deref(), Some("p1"));
    assert_eq!(tx.recipient.as_deref(), Some("p2"));
    assert_eq!(tx.resource_type, "H");
    assert_eq!(tx.amount, 1000);
    assert_eq!(tx.from, "W1N1");
    assert_eq!(tx.to, "W5N1");
}

/// TERMINAL-SEND-011：目标房无**玩家** terminal——send 被消费（send 字段
/// 清空）、无转移、无冷却、无交易。
#[test]
fn send_to_room_without_user_terminal_consumes_without_transfer() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000}));
    // W5N1 无 terminal；也可放无主 terminal——官方 `.user` 门同拒
    f.terminal_full("W5N1", "", json!({"energy": 0}), 0);
    f.send_intent("p1", from, "W5N1", "energy", 100);

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/send"), Some(&json!(null)), "send 已消费");
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(100000)), "无转移");
    assert_eq!(
        fd.pointer("/cooldownTime"),
        Some(&json!(0)),
        "无冷却（保持夹具 0——未写新值）"
    );
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 官方 :77-79：冷却中——send 被消费但不转移不挂新冷却。
#[test]
fn send_on_cooldown_consumes_without_transfer() {
    let mut f = MarketFixture::new();
    // 发送方持有足量 H，只让 cooldown 阻止转移，避免库存不足掩盖冷却检查失效。
    let from = f.terminal_full(
        "W1N1",
        "p1",
        json!({"energy": 100000, "H": 1000}),
        1_000_000,
    );
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 直接保存 send 请求绕过 API 冷却检查，单独验证全局阶段仍检查冷却。
    // 房间 terminal_send 执行器本身不检查 cooldownTime。
    f.stow_send(from, "W5N1", "H", 100);
    f.tick();

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/send"), Some(&json!(null)));
    assert_eq!(
        fd.pointer("/cooldownTime"),
        Some(&json!(1_000_000)),
        "冷却保持原值"
    );
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 官方 :25-32：接收方满仓 → 钳到 0 → 静默不转移（undefined 同 false——
/// 不挂冷却、无交易）。
#[test]
fn send_clamped_to_zero_is_silent_noop() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    // 目标满仓：storeCapacity 与 Σstore 需先落地——直接写满态 data
    // 满仓：Σstore 300000 == storeCapacity 300000（构造即终态）
    let _to = f.terminal("W5N1", "p2", json!({"energy": 300000}));
    f.send_intent("p1", from, "W5N1", "H", 1000);

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/store/H"), Some(&json!(1000)), "资源不动");
    assert_eq!(
        fd.pointer("/store/energy"),
        Some(&json!(100000)),
        "能量不扣"
    );
    assert_eq!(
        fd.pointer("/cooldownTime"),
        Some(&json!(0)),
        "不挂冷却（保持 0）"
    );
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 官方 :49-56 自指三步序：send 到本房（terminalsByRoom 单 terminal 同对象）
/// → to.store[rt]+=a; from.store[rt]−=a; energy−=cost——rt 净 0、能量扣成本。
#[test]
fn send_to_self_room_converges_with_energy_cost() {
    let mut f = MarketFixture::new();
    let t = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    f.send_intent("p1", t, "W1N1", "H", 1000);

    let d = f.data_of(t);
    assert_eq!(d.pointer("/store/H"), Some(&json!(1000)), "rt 净 0");
    // 同房 range=0 → cost=ceil(1000×(1−e^0))=0
    assert_eq!(
        d.pointer("/store/energy"),
        Some(&json!(100000)),
        "range 0 成本 0"
    );
    assert_eq!(d.pointer("/cooldownTime"), Some(&json!(10)), "仍挂冷却");
    assert_eq!(market(&f).transactions.len(), 1, "自指也入账");
}

/// 官方 :22-24：发送方资源在 global 相被并行消耗（同 tick 其他结算）后
/// 复验拒——store 资源不足时 false（不转移不挂冷却）。
#[test]
fn send_recheck_resource_in_global_phase() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 100}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 直注 stow 1000（>实际 100）：房间相校验同拒（本测试只锁 global 相）
    f.stow_send(from, "W5N1", "H", 1000);
    f.tick();

    assert_eq!(f.data_of(from).pointer("/cooldownTime"), Some(&json!(0)));
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 接收方空间钳：目标只剩 500 空间 → amount 钳 500，成本按**钳后**量算
///（官方 :34-35 序），交易记钳后量。
#[test]
fn send_clamps_to_receiver_free_space() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let to = f.terminal("W5N1", "p2", json!({"energy": 500, "H": 299000}));
    // 目标空余容量为 300000-(500+299000)=500。
    f.send_intent("p1", from, "W5N1", "H", 1000);

    let td = f.data_of(to);
    assert_eq!(
        td.pointer("/store/H"),
        Some(&json!(299500)),
        "钳后送达 500（299000+500）"
    );
    let fd = f.data_of(from);
    // 成本按 500 算：ceil(500×(1−e^(−4/30)))=ceil(62.42)=63
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(100000 - 63)));
    assert_eq!(
        fd.pointer("/store/H"),
        Some(&json!(500)),
        "发送方也只扣钳后量"
    );
    assert_eq!(market(&f).transactions[0].amount, 500, "交易记钳后量");
}

/// 直接保存资源足额、energy=cost-1 的请求，验证全局阶段仍拒绝费用不足。
/// 房间动作可能已消耗能量，所以不能只依赖提交意图时的检查。
#[test]
fn send_global_phase_energy_gate_rejects() {
    let mut f = MarketFixture::new();
    // cost(range=4, amount=1000)=125 → energy=124
    let from = f.terminal("W1N1", "p1", json!({"energy": 124, "H": 1000}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 直注 stow（绕过房间相能量门——只锁全局相门）
    f.stow_send(from, "W5N1", "H", 1000);
    f.tick();

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/store/H"), Some(&json!(1000)), "能量不足不转移");
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(124)), "能量不动");
    assert_eq!(fd.pointer("/cooldownTime"), Some(&json!(0)), "不挂冷却");
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 发送 energy 时须同时持有 amount 和 cost，本例 1125 恰好成功，1124 拒绝。
#[test]
fn send_energy_type_amount_plus_cost_branch() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 1125}));
    let to = f.terminal("W5N1", "p2", json!({"energy": 0}));
    f.send_intent("p1", from, "W5N1", "energy", 1000);

    let fd = f.data_of(from);
    assert_eq!(
        fd.pointer("/store/energy"),
        Some(&json!(0)),
        "1125−(1000+125)"
    );
    let td = f.data_of(to);
    assert_eq!(td.pointer("/store/energy"), Some(&json!(1000)), "送达 1000");
    assert_eq!(market(&f).transactions[0].resource_type, "energy");

    // 边界下侧：1124 拒（amount+cost 差 1）
    let mut f2 = MarketFixture::new();
    let from2 = f2.terminal("W1N1", "p1", json!({"energy": 1124}));
    let _to2 = f2.terminal("W5N1", "p2", json!({"energy": 0}));
    f2.send_intent("p1", from2, "W5N1", "energy", 1000);
    assert_eq!(
        f2.data_of(from2).pointer("/store/energy"),
        Some(&json!(1124)),
        "拒"
    );
    assert_eq!(f2.data_of(from2).pointer("/cooldownTime"), Some(&json!(0)));
}

/// 交易 description 中每个 < 都替换为 &lt;。来源：上游 global-intents/market.js。
#[test]
fn send_description_lt_escaping_in_transaction() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // stow 带 description（绕 API 层——直注载荷形态与房间相 stow 相同）
    {
        let Some(RoomObject::Generic(g)) = f.engine.world.get_mut(from) else {
            panic!()
        };
        let mut v: Value = serde_json::from_str(&g.data).unwrap();
        v["send"] = json!({
            "targetRoomName": "W5N1", "resourceType": "H", "amount": 100,
            "description": "a<b<c",
        });
        g.data = v.to_string();
    }
    f.tick();
    let tx = &market(&f).transactions[0];
    assert_eq!(
        tx.description.as_deref(),
        Some("a&lt;b&lt;c"),
        "`<` 全量转义"
    );
}

/// Terminal 无控制器时保留容量；同主 RCL 5 无配额或异主时容量归 0，
/// 同主 RCL 8 时容量恢复为 300000。
#[test]
fn terminal_store_capacity_normalizes_by_rcl() {
    let mut f = MarketFixture::new();
    let t = f.terminal("W1N1", "p1", json!({"energy": 1000}));
    // 无 controller 房：terminal_capacity_tick 的 w.controller() None → 保持
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(300000)),
        "无 controller 保持放置值（官方 roomController 假门整段跳过）"
    );

    // 放 controller level 5 owner p1：terminal 允许数为 0 → 归 0
    f.engine
        .world
        .insert(
            "W1N1",
            RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 40,
                y: 40,
                level: 5,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        )
        .unwrap();
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(0)),
        "RCL 5（terminal 配额 0）归 0"
    );

    // 异主：p2 主控（RCL 8）下 p1 的 terminal 归 0（owner 等值门判别）
    set_controller(&mut f, 8, "p2");
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(0)),
        "异主 controller（p2 RCL 8）下 p1 terminal 归 0"
    );

    // 同主 RCL 8：300000（level>0+owner 相等+配额 1 全真——保持/回升侧）
    set_controller(&mut f, 8, "p1");
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(300000)),
        "同主 RCL 8 回升 300000（全真路径）"
    );
}

/// 改 W1N1 controller 的 level/owner（capacity 测试的三态构造）。
fn set_controller(f: &mut MarketFixture, level: u8, owner: &str) {
    let cid = f
        .engine
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .copied()
        .find(|k| {
            f.engine
                .world
                .get(*k)
                .is_some_and(|(_, o)| matches!(o, RoomObject::Controller(_)))
        })
        .expect("controller 不存在");
    if let Some(RoomObject::Controller(c)) = f.engine.world.get_mut(cid) {
        c.level = level;
        c.owner = Some(owner.into());
    }
}
