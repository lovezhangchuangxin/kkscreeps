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
