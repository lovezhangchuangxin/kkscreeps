//! 2B 批 P1：terminal send 全链（房间相 stow → 全局阶段 executeTransfer）。
//!
//! 走 `Engine::tick_with_intents` 全路径（含 settle_market 挂点——与
//! Fixture::settle 的单房直调不同，这里测的是官方 main.js「房间结算→
//! global 阶段」的完整同 tick 链）。

use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::{Engine, MarketState};
use kk_schema::room::{GenericObj, RoomObject};
use kk_world::ObjId;
use serde_json::{Value, json};

fn terrain_plain() -> kk_core::Terrain {
    kk_core::Terrain::plains()
}

/// 测试工作台墙钟（评审 3 F-G：created_timestamp 与 TickInput::now_ms 两个
/// 关注点共用——不一致时订单被当 1970/2.7 年旧物过期，缺键 panic 响亮但
/// 共享常量消除地雷；裸 tick_with_intents 缺省真实墙钟仍是坑，plan §7 登记）
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

    /// 房间相 stow 的等价物：直接在 terminal data 写 send 字段（API→intent→
    /// structures.rs 执行器的上游由 embed e2e 与 013 矩阵覆盖）。
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

    /// 无 intent 推进一 tick（消费已 stow 的 send——模拟「intent 在上一轮
    /// 已 stow、本轮 global 阶段执行」的分离观察口；正常链路里两者同 tick）。
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
        // 测试工作台墙钟（与 Fixture::settle 同值）——0 会被过期门当 1970
        // 首拍移除
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
    // 固定墙钟（Fixture::settle 同值）——真实墙钟会让 1.7e12 时间戳的订单
    // 立刻过期（差 ~1041 天）
    engine
        .tick_with_intents_with_input(
            batch,
            kk_engine::TickInput {
                now_ms: BENCH_NOW_MS,
            },
        )
        .unwrap();
}

/// MARKET-DEAL 族回归锁：**买卖定向**（2B 批 P2 勘误——初版 SELL 侧
/// seller/buyer 写反导致四红）+ 三重钳 + money 划转 + 交易 order 附带。
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
    // 官方 :391-396 deal 成功挂目标 terminal 冷却（评审 3 F-C：原零锁——
    // 删 write_cooldown 全链不红）
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

/// changeOrderPrice 的 _skip：同 tick 改价后的 deal 跳过该订单（官方 :180）。
/// owner=p1 名字序先于 dealer p2——p1 的改价段先跑、skip 置位后 p2 才收集
/// deal（评审 1 F5：原夹具 dealer 无钱，绿在买方 money 钳而非 _skip；现
/// dealer 资金充足，若 skip 失效旧价 2500×100 成交即露馅）。
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

/// 费门活值读（官方 bulk.inc 立改内存对象 bulk.js:90-97——同 tick 后笔
/// 见前笔扣费；评审 1 F1/评审 2 F1：循环头快照会让 Σfee>credits 全额扣款
/// 把 money 打负）。money 恰够单笔 fee：首笔建单、次笔拒、money 不为负。
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

/// 生命周期尾段只见 tick 前存量（官方 getInterRoom 快照；本 tick createOrder
/// 的订单延迟到 bulk.execute 落库——创建 tick 恒 active=false/amount=0，
/// 下一 tick 才进尾段同步；评审 1 F2/评审 2 F2：遍历全集会提前一拍）。
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

/// 负改价拒（官方 :175 `!newPrice || newPrice <= 0`——评审 1 F3/评审 2 F2：
/// 仅拒 ==0 会落负价反转 deal 资金流向）且不置 _skip（官方先于 skip 段
/// return）——同 tick deal 照常按旧价成交。
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

/// 分节序（官方 createOrder→changeOrderPrice→extendOrder→cancelOrder→deal；
/// 评审 1 F4/评审 2 F3）：同玩家「提价+取消」——官方先按原 remaining 收
/// 提价费再取消；单趟声明序（cancel 先行）会把费算成 0 吞掉。
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
    // p1 只有 304000 毫 → 304000/2500=121.6——**非整除钉 floor**（评审 3 F-D：
    // 300000/2500=120 整除下 round/ceil 同值无判别力；round/ceil 均得 122）；
    // p2 条目（seller 收款面——get_mut 不建条目，官方 db 用户恒在）
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

/// intershard directDeals：resources+money 直接划转（玩家面无来源不可达——
/// 内部锁语义）。
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
    // from 带 H 资源：使冷却门成为唯一拦截（评审 3 判别力清偿——原夹具
    // 无 H，资源复验门会兜底拒绝，冷却门删除时测试仍绿=零判别力）
    let from = f.terminal_full(
        "W1N1",
        "p1",
        json!({"energy": 100000, "H": 1000}),
        1_000_000,
    );
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 冷却中的 terminal 房间相即拒（send.js 无 cooldown 复验——API 相已拒），
    // 此处直接 stow 模拟直注路径：global 相的冷却门仍须独立成立
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
    // 目标空间 = 300000 − (500+299000) = 500（夹具两段冗余覆写已坍缩——
    // 评审 3 P2：构造参数即终态）
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

/// 评审 2 P1-1 清偿：全局相能量门（fee=from 侧）——同 tick 并行消耗窗口的
/// 唯一保护层（API 门读 intent 时点快照）。stow 充足资源但 energy=cost−1 →
/// 无转移/无冷却/无交易（能量门变异在此测试下必炸）。
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

/// energy 型 send：`energy < amount + cost` 分支（评审 3 缺口——8 测试原全用
/// H）。range=4、amount=1000：cost=125 → 恰好 energy=1125 成功 / 1124 拒。
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

/// 官方 :91 description 的 `<`→`&lt;` 全量转义（评审 3 缺口——两侧原零断言）。
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

/// terminal/tick.js：storeCapacity 按 RCL 归一——无 controller 保持原值；
/// 同主 RCL 5（配额 0）归 0；**异主 controller**（p2 主控下 p1 的 terminal）
/// 归 0（评审 2 P2-3 清偿：owner 等值门原零判别——docstring 与夹具不符勘正）。
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
