//! 接收面行为锁定：幂等/冲突/跳号/过期、预算、流生命周期与持久化。
use super::*;
use accept::{AcceptOutcome, MAX_STREAMS_PER_PRINCIPAL, close_stream, open_stream, retire_stream};
use serde_json::json;

fn state_with_stream() -> InboxState {
    let mut state = InboxState::empty(&"a".repeat(32));
    open_stream(&mut state, "p1", "s1").expect("开流");
    state
}

fn envelope(client_seq: u64, payload: Value) -> Value {
    json!({
        "worldId": "a".repeat(32),
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": client_seq,
        "kind": "test.ping",
        "schemaVersion": 1,
        "payload": payload,
    })
}

fn accept_of(state: &mut InboxState, envelope: &Value) -> AcceptOutcome {
    accept::accept(state, envelope, &InboxLimits::default(), 1_000)
}

#[test]
fn accept_assigns_server_seq_and_advances_watermark() {
    let mut state = state_with_stream();
    let AcceptOutcome::Received { server_seq } =
        accept_of(&mut state, &envelope(1, json!({"n": 1})))
    else {
        panic!("首条应接收");
    };
    assert_eq!(server_seq, 1);
    let AcceptOutcome::Received { server_seq } =
        accept_of(&mut state, &envelope(2, json!({"n": 2})))
    else {
        panic!("第二条应接收");
    };
    assert_eq!(server_seq, 2);
    assert_eq!(state.next_server_seq, 3);
    assert_eq!(state.streams["s1"].next_client_seq, 3);
}

#[test]
fn duplicate_same_payload_returns_original_state_and_conflict_on_divergence() {
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!({"n": 1}))).expect_received();
    // 丢响应重试：同身份同载荷 → 原状态与原 serverSeq，不重复分配。
    match accept_of(&mut state, &envelope(1, json!({"n": 1}))) {
        AcceptOutcome::Duplicate { server_seq, status } => {
            assert_eq!(server_seq, 1);
            assert!(
                matches!(status, EntryStatus::Received),
                "received 状态的重复应答仍为 received：{status:?}"
            );
        }
        other => panic!("应幂等重放：{other:?}"),
    }
    assert_eq!(state.next_server_seq, 2, "重复不得分配新序号");
    // 同身份异载荷 → 冲突，原命令不受影响。
    assert!(matches!(
        accept_of(&mut state, &envelope(1, json!({"n": 999}))),
        AcceptOutcome::Conflict
    ));
    assert_eq!(state.streams["s1"].entries[&1].payload["n"], json!(1));
}

#[test]
fn sequence_gaps_are_refused_and_client_keeps_identity() {
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!(1))).expect_received();
    match accept_of(&mut state, &envelope(3, json!(1))) {
        AcceptOutcome::Refused { reason } => assert!(reason.contains("跳号"), "{reason}"),
        other => panic!("跳号应拒绝：{other:?}"),
    }
    // 补发 2 后 3 才可接收。
    accept_of(&mut state, &envelope(2, json!(2))).expect_received();
    accept_of(&mut state, &envelope(3, json!(3))).expect_received();
}

#[test]
fn evicted_identities_report_result_expired_not_reexecuted() {
    let mut state = state_with_stream();
    for seq in 1..=3 {
        accept_of(&mut state, &envelope(seq, json!(seq))).expect_received();
    }
    // 终态化 1/2 后淘汰到 3 之前。
    for seq in 1..=2 {
        settle(&mut state, "s1", seq, json!({"ok": seq}));
    }
    let evicted = accept::evict_settled_below(&mut state, "s1", 3);
    assert_eq!(evicted, 2);
    assert!(matches!(
        accept_of(&mut state, &envelope(1, json!(1))),
        AcceptOutcome::ResultExpired
    ));
    // 已淘汰且已完成的身份：统一结果过期，不补造原结果。
    assert!(matches!(
        accept_of(&mut state, &envelope(2, json!(2))),
        AcceptOutcome::ResultExpired
    ));
    // 在途明细不得被淘汰：3 是 received，floor=3 时保留。
    assert!(state.streams["s1"].entries.contains_key(&3));
}

#[test]
fn budgets_are_enforced_without_eviction() {
    let mut state = state_with_stream();
    let limits = InboxLimits {
        max_pending_per_principal: 2,
        ..Default::default()
    };
    for seq in 1..=2 {
        accept::accept(&mut state, &envelope(seq, json!(seq)), &limits, 0).expect_received();
    }
    match accept::accept(&mut state, &envelope(3, json!(3)), &limits, 0) {
        AcceptOutcome::Refused { reason } => {
            assert!(
                reason.contains("主体") && reason.contains("跨流"),
                "{reason}"
            )
        }
        other => panic!("主体级预算应拒绝：{other:?}"),
    }
    // 同主体另一流同样计入主体级预算（跨流合计）。
    open_stream(&mut state, "p1", "s2").expect("开第二流");
    let second_stream = {
        let mut envelope = envelope(1, json!("x"));
        envelope["streamId"] = json!("s2");
        envelope
    };
    match accept::accept(&mut state, &second_stream, &limits, 0) {
        AcceptOutcome::Refused { reason } => assert!(reason.contains("跨流"), "{reason}"),
        other => panic!("跨流合计应拒绝：{other:?}"),
    }
    // 终态化后预算释放。
    reject(&mut state, "s1", 1);
    accept::accept(&mut state, &envelope(3, json!(3)), &limits, 0).expect_received();
}

#[test]
fn stream_lifecycle_bounds_and_tombstones() {
    let mut state = InboxState::empty(&"a".repeat(32));
    for index in 0..MAX_STREAMS_PER_PRINCIPAL {
        open_stream(&mut state, "p1", &format!("s{index}")).expect("上限内开流");
    }
    assert!(
        open_stream(&mut state, "p1", "s-extra").is_err(),
        "活跃流超上限"
    );
    // 其他主体不受影响；重复 streamId 拒绝。
    open_stream(&mut state, "p2", "s-other").expect("另一主体可开流");
    assert!(
        open_stream(&mut state, "p2", "s0").is_err(),
        "streamId 全局唯一"
    );

    // 在途命令阻止关闭；终态后可关闭、退役并留墓碑。
    accept_of(&mut state, &{
        let mut envelope = envelope(1, json!(1));
        envelope["streamId"] = json!("s0");
        envelope
    })
    .expect_received();
    assert!(
        close_stream(&mut state, "p1", "s0").is_err(),
        "在途不得关闭"
    );
    settle(&mut state, "s0", 1, json!(null));
    close_stream(&mut state, "p1", "s0").expect("无在途可关闭");
    assert!(
        matches!(
            accept_of(&mut state, &{
                let mut envelope = envelope(2, json!(2));
                envelope["streamId"] = json!("s0");
                envelope
            }),
            AcceptOutcome::Refused { .. }
        ),
        "关闭后拒绝新命令"
    );
    retire_stream(&mut state, "p1", "s0").expect("退役");
    assert!(
        open_stream(&mut state, "p1", "s0").is_err(),
        "退役 streamId 不复用"
    );
    assert!(state.streams["s0"].retired && state.streams["s0"].entries.is_empty());
    // 退役释放活跃名额。
    open_stream(&mut state, "p1", "s-new").expect("退役后可开新流");
    // 主体不匹配不得操作流。
    assert!(close_stream(&mut state, "p2", "s-new").is_err());
}

#[test]
fn envelope_validation_rejects_foreign_world_and_shapes() {
    let mut state = state_with_stream();
    let mut foreign = envelope(1, json!(1));
    foreign["worldId"] = json!("b".repeat(32));
    assert!(matches!(
        accept_of(&mut state, &foreign),
        AcceptOutcome::Refused { .. }
    ));
    for (patch, hint) in [
        (json!(null), "clientSeq 缺失"),
        (json!(0), "clientSeq 从 1 起"),
    ] {
        let mut broken = envelope(1, json!(1));
        broken["clientSeq"] = patch;
        match accept_of(&mut state, &broken) {
            AcceptOutcome::Refused { reason } => {
                assert!(reason.contains("clientSeq"), "{hint}: {reason}")
            }
            other => panic!("{hint} 应拒绝：{other:?}"),
        }
    }
    let unknown_stream = {
        let mut envelope = envelope(1, json!(1));
        envelope["streamId"] = json!("nope");
        envelope
    };
    assert!(matches!(
        accept_of(&mut state, &unknown_stream),
        AcceptOutcome::Refused { .. }
    ));
}

#[test]
fn store_roundtrip_and_damage_stops_service() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kk-inbox-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("inbox.json");
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!({"cmd": "x"}))).expect_received();
    store(&state, &path)?;
    // 重启（重新 load）：received 不丢。
    let reloaded = load(&path, &"a".repeat(32))?;
    assert_eq!(reloaded.streams["s1"].entries[&1].server_seq, 1);
    assert_eq!(reloaded.streams["s1"].next_client_seq, 2);
    // 身份门。
    assert!(load(&path, &"b".repeat(32)).is_err());
    // 撕裂文件 = 停服条件（Broken 分类不可用于回退旧值）。
    let bytes = std::fs::read(&path)?;
    std::fs::write(&path, &bytes[..bytes.len() / 2])?;
    assert!(load(&path, &"a".repeat(32)).is_err());
    // 外来内容同样拒。
    std::fs::write(&path, "{\"inbox_format_version\": 77}")?;
    assert!(load(&path, &"a".repeat(32)).is_err());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

fn settle(state: &mut InboxState, stream_id: &str, seq: u64, result: Value) {
    state
        .streams
        .get_mut(stream_id)
        .unwrap()
        .entries
        .get_mut(&seq)
        .unwrap()
        .status = EntryStatus::Completed { result };
}

fn reject(state: &mut InboxState, stream_id: &str, seq: u64) {
    state
        .streams
        .get_mut(stream_id)
        .unwrap()
        .entries
        .get_mut(&seq)
        .unwrap()
        .status = EntryStatus::Rejected {
        reason: "框架测试拒绝".into(),
    };
}

impl AcceptOutcome {
    #[track_caller]
    fn expect_received(&self) -> u64 {
        match self {
            AcceptOutcome::Received { server_seq } => *server_seq,
            other => panic!("期望 Received：{other:?}"),
        }
    }
}

#[test]
fn duplicate_survives_stream_close_with_original_status() {
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!({"n": 1}))).expect_received();
    settle(&mut state, "s1", 1, json!({"done": true}));
    close_stream(&mut state, "p1", "s1").expect("无在途可关闭");
    // 关闭后同身份同载荷重放：返回原终态（丢响应的结果找回不因关闭
    // 失效）；新 clientSeq 仍被拒。
    match accept_of(&mut state, &envelope(1, json!({"n": 1}))) {
        AcceptOutcome::Duplicate { server_seq, status } => {
            assert_eq!(server_seq, 1);
            assert!(
                matches!(&status, EntryStatus::Completed { result } if result == &json!({"done": true})),
                "重放应携带原终态结果：{status:?}"
            );
        }
        other => panic!("关闭后重放应幂等：{other:?}"),
    }
    assert!(matches!(
        accept_of(&mut state, &envelope(2, json!(2))),
        AcceptOutcome::Refused { .. }
    ));
}

#[test]
fn same_payload_with_different_kind_or_schema_is_conflict() {
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!({"n": 1}))).expect_received();
    // 幂等摘要覆盖命令规范形：异 kind（或 schemaVersion）同 payload 是
    // 载荷分歧，不是重放。
    let mut other_kind = envelope(1, json!({"n": 1}));
    other_kind["kind"] = json!("other.kind");
    assert!(matches!(
        accept_of(&mut state, &other_kind),
        AcceptOutcome::Conflict
    ));
    let mut other_schema = envelope(1, json!({"n": 1}));
    other_schema["schemaVersion"] = json!(2);
    assert!(matches!(
        accept_of(&mut state, &other_schema),
        AcceptOutcome::Conflict
    ));
}

#[test]
fn stream_is_bound_to_its_principal_for_accept() {
    let mut state = state_with_stream();
    // 同 streamId 异主体提交：拒绝（不信 payload 自报，流绑定签发主体）。
    let mut foreign = envelope(1, json!(1));
    foreign["principal"] = json!("p2");
    match accept_of(&mut state, &foreign) {
        AcceptOutcome::Refused { reason } => assert!(reason.contains("流不存在"), "{reason}"),
        other => panic!("异主体应被拒：{other:?}"),
    }
}

#[test]
fn restart_replay_returns_original_server_seq_from_durable_state() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kk-inbox-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("inbox.json");
    let mut state = state_with_stream();
    accept_of(&mut state, &envelope(1, json!({"cmd": "r"}))).expect_received();
    store(&state, &path)?;
    // 重启（重新 load）后同身份重放：经持久 next_server_seq/明细取回
    // 原 serverSeq 与 received 状态，不重新分配。
    let mut reloaded = load(&path, &"a".repeat(32))?;
    match accept_of(&mut reloaded, &envelope(1, json!({"cmd": "r"}))) {
        AcceptOutcome::Duplicate { server_seq, status } => {
            assert_eq!(server_seq, 1);
            assert!(matches!(status, EntryStatus::Received));
        }
        other => panic!("重启后重放应取回原序号：{other:?}"),
    }
    assert_eq!(reloaded.next_server_seq, 2);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[allow(dead_code)]
fn outcome_kind(outcome: &AcceptOutcome) -> &'static str {
    match outcome {
        AcceptOutcome::Received { .. } => "received",
        AcceptOutcome::Duplicate { .. } => "duplicate",
        AcceptOutcome::Conflict => "conflict",
        AcceptOutcome::ResultExpired => "expired",
        AcceptOutcome::Refused { .. } => "refused",
    }
}
