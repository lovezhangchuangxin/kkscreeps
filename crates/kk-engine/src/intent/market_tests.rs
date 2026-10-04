use super::*;
use serde_json::json;

#[test]
fn global_container_parses_all_five_actions() {
    let list = json!({
        "global": {
            "createOrder": [
                {"type": "sell", "resourceType": "H", "price": 2.5,
                 "totalAmount": "100", "roomName": "W1N1"}
            ],
            "cancelOrder": [{"orderId": "mkt3"}],
            "changeOrderPrice": [{"orderId": "mkt3", "newPrice": 0.75}],
            "extendOrder": [{"orderId": "mkt3", "addAmount": 50}],
            "deal": [{"orderId": "mkt3", "amount": 10, "targetRoomName": "W1N1"}]
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    let m = &batch.users["p1"].market;
    assert_eq!(m.len(), 5);
    // 迭代序 = 官方 intentTypes 声明序（system.js:23-29）：
    // cancelOrder → changeOrderPrice → createOrder → deal → extendOrder
    assert_eq!(
        m[0],
        MarketIntent::CancelOrder {
            order_id: Some("mkt3".into())
        }
    );
    assert_eq!(
        m[1],
        MarketIntent::ChangeOrderPrice {
            order_id: Some("mkt3".into()),
            new_price_milli: Some(750),
        }
    );
    assert_eq!(
        m[2],
        MarketIntent::CreateOrder {
            order_type: Some("sell".into()),
            resource_type: Some("H".into()),
            price_milli: Some(2500),
            total_amount: Some(100),
            room_name: Some("W1N1".into()),
        },
        "totalAmount 字符串走 JS parseInt 前缀规则（\"100\" → 100）"
    );
    assert_eq!(
        m[3],
        MarketIntent::Deal {
            order_id: Some("mkt3".into()),
            amount: Some(10),
            target_room_name: Some("W1N1".into()),
        }
    );
    assert_eq!(
        m[4],
        MarketIntent::ExtendOrder {
            order_id: Some("mkt3".into()),
            add_amount: Some(50),
        }
    );
}

#[test]
fn price_transform_milli_and_edges() {
    let mk = |price: Value| {
        let list = json!({"global": {"changeOrderPrice": [{"orderId": "x", "newPrice": price}]}});
        IntentBatch::from_json("p1", &list).users["p1"]
            .market
            .clone()
    };
    let case = |price: Value| match mk(price).remove(0) {
        MarketIntent::ChangeOrderPrice {
            new_price_milli, ..
        } => new_price_milli,
        _ => unreachable!(),
    };
    assert_eq!(case(json!(2)), Some(2000));
    assert_eq!(case(json!(0.5)), Some(500));
    assert_eq!(case(json!(0.75)), Some(750));
    assert_eq!(case(json!(1.4999)), Some(1500), "toFixed 最近邻");
    assert_eq!(
        case(json!("3.25")),
        Some(3250),
        "JS 乘法字符串强转（1000*\"3.25\"）"
    );
    // 清洗层保留零与负价，并转换布尔值；是否允许成交由 API 和结算器检查。
    assert_eq!(case(json!(-0.5)), Some(-500));
    assert_eq!(
        case(json!(-0.0025)),
        Some(-3),
        "负恰半远零（node 实测对齐）"
    );
    assert_eq!(
        case(json!(0)),
        Some(0),
        "0 是合法毫值——`!price` 门在 API 层"
    );
    assert_eq!(case(json!(true)), Some(1000), "JS 1000*true");
    // 十六进制串与超出 i64 的固定点值返回 None；达到指数表示范围后取首位数字。
    assert_eq!(case(json!("0x10")), None, "域①真不可达（parseFloat 门）");
    assert_eq!(
        case(json!(1e18)),
        Some(1),
        "域③指数形首位（milli=1e21，node 实测官方=1）"
    );
    assert_eq!(case(json!(2.5e18)), Some(2), "域③ 2.5e21→首位 2");
    assert_eq!(case(json!(-1e18)), Some(-1), "域③负向 −1e21→−1");
    assert_eq!(
        case(json!(1e16)),
        None,
        "域②milli=1e19∈[2^63,1e21) 表示域强制（官方精确 1e19）"
    );
    assert_eq!(
        case(json!(-1e19)),
        Some(-1),
        "域③负向 −1e22 指数形首位 −1（负向下界对称守卫=域②内才 None）"
    );
    assert_eq!(case(json!(-1e16)), None, "域②负向 milli=−1e19 表示域强制");
    assert_eq!(case(Value::Null), None, "不可达路径按 falsy 折叠");
    // 缺键 → None（官方 NaN）
    let list = json!({"global": {"changeOrderPrice": [{"orderId": "x"}]}});
    match IntentBatch::from_json("p1", &list)
        .users
        .get_mut("p1")
        .unwrap()
        .market
        .remove(0)
    {
        MarketIntent::ChangeOrderPrice {
            new_price_milli, ..
        } => assert_eq!(new_price_milli, None),
        _ => unreachable!(),
    }
}

#[test]
fn global_unknown_action_dropped_and_missing_fields_none() {
    let list = json!({
        "global": {
            "generatePixel": [{}],
            "deal": [{}]
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    let m = &batch.users["p1"].market;
    assert_eq!(m.len(), 1, "白名单外动作丢弃");
    assert_eq!(
        m[0],
        MarketIntent::Deal {
            order_id: None,
            amount: None,
            target_room_name: None,
        },
        "缺键全 None（官方物化 undefined/NaN——falsy 门等价）"
    );
}

#[test]
fn extend_from_merges_market_vecs() {
    let a = IntentBatch::from_json(
        "p1",
        &json!({"global": {"cancelOrder": [{"orderId": "mkt1"}]}}),
    );
    let b = IntentBatch::from_json(
        "p1",
        &json!({"global": {"cancelOrder": [{"orderId": "mkt2"}]}}),
    );
    let mut merged = a;
    merged.extend_from(b);
    assert_eq!(merged.users["p1"].market.len(), 2);
    // 数量相等仍可能错排，还须检查合并后的顺序。
    assert_eq!(
        merged.users["p1"]
            .market
            .iter()
            .map(|i| match i {
                MarketIntent::CancelOrder { order_id } => order_id.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>(),
        vec![Some("mkt1".into()), Some("mkt2".into())]
    );
}
