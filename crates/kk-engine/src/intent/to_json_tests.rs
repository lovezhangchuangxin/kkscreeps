//! to_json 逆转换的往返性质：对任意线 JSON `v`，
//! `from_json(p, to_json(from_json(p, v))) == from_json(p, v)`。
//! 夹具覆盖全部容器（对象动作/扩展意图/房间/通知/市场）与清洗边界。
use super::*;

fn wire() -> Value {
    serde_json::json!({
        "kk64f000000000001": {
            "move": {"direction": 3},
            "transfer": {"id": "kk64f000000000002", "resourceType": "energy", "amount": 50},
            "say": {"message": "hi", "isPublic": true},
            "createCreep": {"body": ["work", "carry", "move"], "name": "w1",
                            "energyStructures": ["kk64f000000000003"]},
            "setSpawnDirections": {"directions": [1, 3, 3, 300]},
            "mod:dual.gift": {"amount": 7}
        },
        "room": {
            "createConstructionSite": [{"roomName": "W1N1", "x": 10, "y": 11, "structureType": "road"}],
            "createFlag": [{"roomName": "W1N1", "x": 1, "y": 2, "name": "f", "color": 4, "secondaryColor": 5}],
            "destroyStructure": [{"roomName": "W1N1", "id": "kk64f000000000004"}],
            "removeFlag": [{"roomName": "W1N1", "name": "f"}],
            "removeConstructionSite": [{"roomName": "W1N1", "id": "kk64f000000000005"}]
        },
        "notify": [{"message": "hello", "groupInterval": 30}, {"message": "m"}],
        "global": {
            "createOrder": [{"type": "buy", "resourceType": "energy", "price": 0.027, "totalAmount": 1000, "roomName": "W1N1"}],
            "cancelOrder": [{"orderId": "ord1"}],
            "changeOrderPrice": [{"orderId": "ord1", "newPrice": 1.5}],
            "extendOrder": [{"orderId": "ord1", "addAmount": 20}],
            "deal": [{"orderId": "ord1", "amount": 5, "targetRoomName": "W2N2"}]
        }
    })
}

#[test]
fn to_json_round_trips_from_json_output() {
    let first = IntentBatch::from_json("p1", &wire());
    // to_json 产出玩家键映射；from_json 的配对单位是单玩家列表，
    // 回放按玩家取子对象（G25 日志的回放口径同形）。
    let json = first.to_json();
    let second = IntentBatch::from_json("p1", json.get("p1").expect("p1 在位"));
    assert_eq!(first, second, "规范批次经 to_json/from_json 必须保持不变");
}

#[test]
fn to_json_is_key_order_insensitive_input_output_deterministic() {
    // 同一批次两次转换输出逐字节相等（BTreeMap/固定市场序决定键序）。
    let a = IntentBatch::from_json("p1", &wire()).to_json().to_string();
    let b = IntentBatch::from_json("p1", &wire()).to_json().to_string();
    assert_eq!(a, b);
}

#[test]
fn to_json_survives_sanitizing_edges() {
    // 带越界值/未知动作/垃圾键的输入：from_json 清洗后的批次经
    // to_json 再入 from_json 保持不变（清洗是幂等的收敛）。
    let dirty = serde_json::json!({
        "kk64f000000000001": {
            "move": {"direction": 999, "unknown": 1},
            "nonsense-action": {},
            "bad:": {"x": 1},
            "sign": {"sign": "签名文本"}
        },
        "notify": [{"message": "x", "groupInterval": -5}],
        "garbage-container": 7
    });
    let first = IntentBatch::from_json("p1", &dirty);
    let second = IntentBatch::from_json("p1", first.to_json().get("p1").expect("p1 在位"));
    assert_eq!(first, second);
}

#[test]
fn empty_batch_and_multi_player() {
    assert_eq!(IntentBatch::default().to_json(), serde_json::json!({}));
    let mut batch = IntentBatch::from_json("a", &wire());
    batch.extend_from(IntentBatch::from_json(
        "b",
        &serde_json::json!({
            "kk64f000000000009": {"harvest": {"id": "kk64f000000000001"}}
        }),
    ));
    let json = batch.to_json();
    let mut back = IntentBatch::from_json("a", json.get("a").unwrap());
    back.extend_from(IntentBatch::from_json("b", json.get("b").unwrap()));
    assert_eq!(back, batch);
}

/// 价格反解验证：朴素 `m/1000` 会漂移的区间（首个反例 milli≈2^52−5·10^4，
/// 相邻 f64 的千倍取整恰跨半格）内，to_json 发射的值必须精确回到原
/// milli——外部轮以 to_json 为唯一持久形态，差 1 milli 即回放分歧。
#[test]
fn price_emission_round_trips_exactly_in_the_ambiguous_band() {
    let anchor: i64 = 4_503_599_627_320_523; // 复审给出的朴素除法反例
    let mut suspects = vec![anchor, -anchor];
    // 系统性扫描混叠带内一段连续值，再加确定性伪随机抽样到 2^52。
    suspects.extend((anchor - 64)..(anchor + 64));
    suspects.extend((-anchor - 64)..(-anchor + 65));
    let mut state = 0x9e3779b97f4a7c15_u64;
    for _ in 0..512 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        suspects.push((state % (1_u64 << 52)) as i64);
    }
    for milli in suspects {
        let mut batch = IntentBatch::default();
        batch
            .users
            .entry("p".into())
            .or_default()
            .market
            .push(MarketIntent::CreateOrder {
                order_type: Some("sell".into()),
                resource_type: Some("energy".into()),
                price_milli: Some(milli),
                total_amount: Some(1),
                room_name: None,
            });
        let wire = batch.to_json();
        let back = IntentBatch::from_json("p", wire.get("p").unwrap());
        assert_eq!(
            back.users["p"].market, batch.users["p"].market,
            "milli={milli} 的价格必须在 to_json→from_json 往返后逐位不变"
        );
    }
}
