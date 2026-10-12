//! S2.2/S2.3 命令消费面的行为测试：code.replace 事务、控制台表达式
//! 拍内求值与受限 Mod 输入注入的单轮与挂起重试语义。
use super::*;
use crate::EmbedEngine;
use crate::service::host::{HostParameters, RecoverySummary, ServiceHost, V2SaveContext};
use crate::service::{ServiceDir, identity, inbox};
use identity::ServiceIdentity;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

mod regression;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-cmds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// v2 宿主：单玩家引擎（Memory.n 计数）+ 初始屏障 + 空 inbox/effects。
fn host_v2(tag: &str, code: &str) -> (ServiceHost, ServiceIdentity) {
    host_with_engine(tag, code, None)
}

fn host_with_engine(
    tag: &str,
    code: &str,
    supplied: Option<EmbedEngine>,
) -> (ServiceHost, ServiceIdentity) {
    let root = temp_dir(tag);
    let ident = identity::ServiceIdentity {
        service_format_version: identity::SERVICE_FORMAT_V2,
        world_id: identity::generate_world_id(),
        world_seed: 5,
        scenario_sha256: "e".repeat(64),
        initialized: true,
    };
    let mut engine = supplied.unwrap_or_else(|| EmbedEngine::with_world_seed(ident.world_seed));
    if engine.player_state("p1").is_none() {
        engine
            .create_shard(&json!({
                "players": ["p1"],
                "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}],
            }))
            .unwrap();
    }
    let mut table = kk_runner::ModuleTable::new();
    table.insert_js("main", code);
    engine.set_player_modules("p1", table).unwrap();
    let dir = ServiceDir::new(&root);
    dir.ensure_layout().unwrap();
    let mut host = ServiceHost::new(
        engine,
        dir,
        ident.clone(),
        BTreeMap::from([("p1".to_string(), "main".to_string())]),
        HostParameters {
            auto_save_ticks: u64::MAX as u32,
            auto_save_wall_secs: u64::MAX,
            keep_checkpoints: 5,
        },
        RecoverySummary {
            selected: String::new(),
            publish_seq: 0,
            tick: 0,
            skipped_broken: 0,
        },
    )
    .unwrap();
    host.set_v2_context(V2SaveContext {
        barrier_id: "b-init".into(),
        inbox_applied: BTreeMap::new(),
        code_versions: BTreeMap::new(),
        outbox_cursor: 0,
        anchor_seq: 0,
    });
    // 初始 effects 先行：不变量是 effects 在首次 publish_barrier 前
    // 存在（它读该文件沿用 sinks，缺失/损坏是停服条件）——enable
    // 事务在启用时写 effects，此后任何屏障发布都满足此前置。
    effects::write(
        &host.dir.effects_file(),
        &effects::EffectsState::initial(&ident.world_id, 0, "b-init"),
    )
    .unwrap();
    host.publish_barrier("b-init", BTreeMap::new(), BTreeMap::new())
        .unwrap();
    inbox::store(
        &inbox::InboxState::empty(&ident.world_id),
        &host.dir.inbox_file(),
    )
    .unwrap();
    (host, ident)
}

fn host_mod(tag: &str) -> anyhow::Result<(ServiceHost, ServiceIdentity, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-player");
    let packages = kk_mod_host::discover_packages(&root)?;
    let mut engine = EmbedEngine::with_world_seed(5);
    engine.create_shard_with_mods(
        &json!({
            "players": ["p1", "p2"],
            "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}],
        }),
        &packages,
        Default::default(),
    )?;
    let id = engine.place_mod_object(
        "W1N1",
        "workload:counter",
        &json!({"owner": "p1", "x": 10, "y": 10}),
    )?;
    let (mut host, ident) = host_with_engine(tag, "exports.loop = () => {};", Some(engine));
    host.set_mod_grants(BTreeMap::from([("workload".into(), vec!["p1".into()])]));
    Ok((host, ident, id))
}

/// 注入一条命令（stream s1 主体 p1），kind/payload 参数化。
fn inject_command(
    host: &ServiceHost,
    ident: &identity::ServiceIdentity,
    kind: &str,
    payload: Value,
) {
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id).unwrap();
    if !state.streams.contains_key("s1") {
        inbox::accept::open_stream(&mut state, "p1", "s1").unwrap();
    }
    let envelope = json!({
        "worldId": ident.world_id,
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": state.streams["s1"].next_client_seq,
        "kind": kind,
        "schemaVersion": 1,
        "payload": payload,
    });
    match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
        inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    inbox::store(&state, &host.dir.inbox_file()).unwrap();
}

/// 注入一条 code.replace 命令（stream s1 主体 p1）。
fn inject_replace(host: &ServiceHost, ident: &identity::ServiceIdentity, payload: Value) {
    inject_command(host, ident, "code.replace", payload);
}

fn entry_status(
    host: &ServiceHost,
    ident: &identity::ServiceIdentity,
    seq: u64,
) -> inbox::EntryStatus {
    entry_status_on(host, ident, "s1", seq)
}

/// 指定流版本的终态读取（mod.input 用例走包主体流 m1）。
fn entry_status_on(
    host: &ServiceHost,
    ident: &identity::ServiceIdentity,
    stream_id: &str,
    seq: u64,
) -> inbox::EntryStatus {
    inbox::load(&host.dir.inbox_file(), &ident.world_id)
        .unwrap()
        .streams[stream_id]
        .entries[&seq]
        .status
        .clone()
}

/// 走宿主拍执行玩家（require 入口+loop），与 serve run 驱动同路径。
fn run_one_player(host: &mut ServiceHost) {
    host.tick().expect("拍应成功").expect("无故障应推进");
}

/// 主链路：替换消费即屏障——codeVersion 推进、新模块生效、Memory
/// 保留、inbox 终态与水位存档、effects 锚点换新屏障、后续保存继承。
#[test]
fn code_replace_consumes_command_with_immediate_barrier() -> anyhow::Result<()> {
    let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
    let (mut host, ident) = host_v2("main-link", code);
    // 先跑两拍积累 Memory，证明替换不重置。
    run_one_player(&mut host);
    run_one_player(&mut host);
    let tick_before = host.engine.get_game_time();
    let new_code = "exports.loop = () => { Memory.m = (Memory.n || 0) + 100; };";
    inject_replace(
        &host,
        &ident,
        json!({"modules": {"main": new_code}, "expectedCodeVersion": 0}),
    );

    let processed = host.drain_command_prefix(4)?;
    assert_eq!(processed, 1);
    assert_eq!(host.engine.get_game_time(), tick_before, "替换不推进世界");
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["codeVersion"], json!(1))
        }
        other => panic!("应完成：{other:?}"),
    }
    // 新模块生效且 Memory.n 延续（m = n+100 而非 100）。
    run_one_player(&mut host);
    let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
    let memory: Value = serde_json::from_str(&raw)?;
    assert_eq!(memory["n"], json!(2), "旧 Memory 不被初始化覆盖");
    assert_eq!(memory["m"], json!(102), "新代码在正常执行阶段运行");
    // 屏障与水位：effects 锚点指向新屏障；后续普通保存继承。
    let effects = effects::load(&host.dir.effects_file(), &ident.world_id)?;
    assert_ne!(effects.anchor.barrier_id, "b-init");
    assert_eq!(effects.anchor.inbox_applied.get("s1"), Some(&1));
    run_one_player(&mut host);
    host.save_now();
    let selected = checkpoints::select_latest(
        &host.dir.checkpoints_dir(),
        &ident,
        Some(&checkpoints::RecoveryFloor {
            checkpoint_seq: effects.anchor.checkpoint_seq,
            barrier_id: effects.anchor.barrier_id.clone(),
            inbox_applied: effects.anchor.inbox_applied.clone(),
        }),
    )?;
    assert_eq!(selected.meta.code_versions.unwrap()["p1"], 1);
    assert_eq!(selected.publish_seq, effects.anchor.checkpoint_seq + 1);
    Ok(())
}

/// CAS 过期：验证拒绝持久化终态并推进水位，不阻塞后续命令。
#[test]
fn stale_code_version_is_rejected_without_blocking() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("cas", "exports.loop = () => {};");
    inject_replace(
        &host,
        &ident,
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 7}),
    );
    let processed = host.drain_command_prefix(4)?;
    assert_eq!(processed, 1);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("CAS"), "{reason}")
        }
        other => panic!("应拒绝：{other:?}"),
    }
    // 毒消息不阻塞：正确版本的第二条命令照常完成。
    inject_replace(
        &host,
        &ident,
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
    );
    let processed = host.drain_command_prefix(4)?;
    assert_eq!(processed, 1);
    match entry_status(&host, &ident, 2) {
        inbox::EntryStatus::Completed { result } => assert_eq!(result["codeVersion"], json!(1)),
        other => panic!("第二条应完成：{other:?}"),
    }
    Ok(())
}

/// 非法载荷：验证拒绝且现役模块不动（旧代码继续跑）。含畸形
/// expectedCodeVersion（缺失/字符串/负数）——载荷校验走 Rejected
/// 终态而非 Err，队首毒消息被消费不形成活锁（三路 review 一致抓出
/// 的 P1 回归锁定）。
#[test]
fn invalid_payload_keeps_current_modules() -> anyhow::Result<()> {
    let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
    let (mut host, ident) = host_v2("invalid", code);
    for payload in [
        json!({"modules": {}, "expectedCodeVersion": 0}),
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0, "entry": "missing"}),
        json!({"expectedCodeVersion": 0}),
        json!({"modules": {"main": "exports.loop = () => {};"}}),
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": "0"}),
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": -1}),
    ] {
        inject_replace(&host, &ident, payload.clone());
        assert_eq!(
            host.drain_command_prefix(4)?,
            1,
            "每条都应被消费为终态（载荷 {payload}）"
        );
        assert!(
            host.fault().is_none(),
            "载荷校验拒绝不得设置故障态（活锁回归）"
        );
    }
    // 旧模块仍生效。
    run_one_player(&mut host);
    let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
    assert_eq!(serde_json::from_str::<Value>(&raw)?["n"], json!(1));
    Ok(())
}

/// 流绑定主体不是注册玩家：明确验证拒绝（授权类问题不升级为整服
/// 持久化冻结），毒消息不阻塞。
#[test]
fn unknown_principal_is_rejected_without_freezing() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("ghost", "exports.loop = () => {};");
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "ghost", "s1").unwrap();
    let envelope = json!({
        "worldId": ident.world_id,
        "principal": "ghost",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "code.replace",
        "schemaVersion": 1,
        "payload": {"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0},
    });
    match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
        inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    assert_eq!(host.drain_command_prefix(4)?, 1);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("不是注册玩家"), "{reason}")
        }
        other => panic!("应拒绝：{other:?}"),
    }
    assert!(host.fault().is_none(), "授权类拒绝不得冻结");
    Ok(())
}

/// 屏障确认后、inbox 存档前崩溃：恢复水位已含该身份时 drain 幂等
/// 补全（结果由当前版本号重构），不重新执行玩家模块切换。
#[test]
fn confirmed_barrier_before_archive_recovers_idempotently() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("idem", "exports.loop = () => {};");
    inject_replace(
        &host,
        &ident,
        json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 1);
    // 模拟存档前崩溃：把 inbox 的终态回退为 Received、水位归零
    // （屏障检查点与 effects 已含水位=1）。
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    state.streams.get_mut("s1").unwrap().applied_seq = 0;
    state
        .streams
        .get_mut("s1")
        .unwrap()
        .entries
        .get_mut(&1)
        .unwrap()
        .status = inbox::EntryStatus::Received;
    inbox::store(&state, &host.dir.inbox_file())?;

    // 用同一宿主实例模拟恢复后的宿主：drain 已把 v2 上下文更新为
    // 屏障值（水位=1），与真实恢复装配（run 启动自检查点 meta 重建
    // V2SaveContext）数据同源等价；端到端恢复链路由集成用例覆盖。
    // 幂等补全 Completed，不重新走 CAS/切换（expected 0 ≠ 当前 1，
    // 若误重执行将 CAS 拒绝——补全路径不触发）。
    assert_eq!(host.drain_command_prefix(4)?, 1);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["codeVersion"], json!(1), "结果由当前版本重构")
        }
        other => panic!("应幂等补全：{other:?}"),
    }
    Ok(())
}

/// 屏障发布失败：持久化故障冻结推进（保留内存版本、不发布完成）。
#[test]
fn publish_failure_freezes_as_persistence_fault() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let (mut host, ident) = host_v2("freeze", "exports.loop = () => {};");
        struct Restore(PathBuf);
        impl Drop for Restore {
            fn drop(&mut self) {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        let checkpoints = host.dir.checkpoints_dir();
        let _restore = Restore(checkpoints.clone());
        std::fs::set_permissions(&checkpoints, std::fs::Permissions::from_mode(0o500))?;
        inject_replace(
            &host,
            &ident,
            json!({"modules": {"main": "exports.loop = () => {};"}, "expectedCodeVersion": 0}),
        );
        assert!(host.drain_command_prefix(4).is_err(), "发布失败应上抛");
        assert!(matches!(
            host.fault(),
            Some(crate::service::Fault::Persistence { .. })
        ));
        assert!(matches!(
            entry_status(&host, &ident, 1),
            inbox::EntryStatus::Received
        ));
    }
    Ok(())
}

/// 主链路：表达式挂起（Deferred 不终态）→ 拍内 loop 前求值（副作用
/// 与 Memory 写入共存）→ 拍提交后屏障终态化（completed 携带结果）。
#[test]
fn console_expr_defers_then_completes_with_barrier() -> anyhow::Result<()> {
    let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
    let (mut host, ident) = host_v2("expr-main", code);
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "Memory.m = 7; 'ok-result'"}),
    );
    // drain：挂起而非终态——processed=0，条目仍 Received。
    assert_eq!(host.drain_command_prefix(4)?, 0);
    assert!(matches!(
        entry_status(&host, &ident, 1),
        inbox::EntryStatus::Received
    ));
    run_one_player(&mut host);
    // 拍成功即终态化：completed 携带求值结果，水位推进，屏障换新。
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["ok"], json!(true));
            assert_eq!(result["value"], json!("ok-result"));
        }
        other => panic!("应完成：{other:?}"),
    }
    let state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    assert_eq!(state.streams["s1"].applied_seq, 1);
    let effects = effects::load(&host.dir.effects_file(), &ident.world_id)?;
    assert_ne!(effects.anchor.barrier_id, "b-init");
    assert_eq!(effects.anchor.inbox_applied.get("s1"), Some(&1));
    // 表达式副作用与 loop 共存：m 由表达式写、n 由 loop 写。
    let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
    let memory: Value = serde_json::from_str(&raw)?;
    assert_eq!(memory["m"], json!(7), "表达式在 loop 前执行且写候选");
    assert_eq!(memory["n"], json!(1), "loop 照常执行一次");
    Ok(())
}

/// 表达式软异常：有界错误结果（{ok:false,...}）仍是 completed，loop
/// 照常运行——不把玩家可写出的异常升级为拒绝或故障。
#[test]
fn console_expr_soft_error_completes_and_loop_continues() -> anyhow::Result<()> {
    let code = "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };";
    let (mut host, ident) = host_v2("expr-err", code);
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "throw new Error('表达式炸了')"}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["ok"], json!(false), "软异常是错误结果：{result}");
            assert!(result["kind"].is_string());
        }
        other => panic!("软异常应完成（携带错误结果）：{other:?}"),
    }
    let raw = host.engine.player_state("p1").unwrap().raw_memory.clone();
    assert_eq!(
        serde_json::from_str::<Value>(&raw)?["n"],
        json!(1),
        "loop 继续"
    );
    Ok(())
}

/// 校验拒绝：超 8KiB / 缺字段 / 未知主体——明确 Rejected 终态不挂起。
#[test]
fn console_expr_invalid_inputs_are_rejected() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("expr-invalid", "exports.loop = () => {};");
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": " ".repeat(EXPR_MAX_BYTES + 1)}),
    );
    inject_command(&host, &ident, "console.expr", json!({"expression": 42}));
    assert_eq!(host.drain_command_prefix(4)?, 2, "两条都应被消费为终态");
    for seq in [1, 2] {
        match entry_status(&host, &ident, seq) {
            inbox::EntryStatus::Rejected { reason } => {
                assert!(
                    reason.contains("表达式") || reason.contains("expression"),
                    "{reason}"
                )
            }
            other => panic!("seq={seq} 应拒绝：{other:?}"),
        }
    }
    assert!(host.fault().is_none());
    assert!(host.pending_expr.is_empty(), "非法输入不挂起");
    Ok(())
}

/// 每主体每拍一条：第二条同主体表达式等首条终态化后的下一轮才挂起。
#[test]
fn console_expr_one_per_player_per_tick() -> anyhow::Result<()> {
    let code = "exports.loop = () => {};";
    let (mut host, ident) = host_v2("expr-serial", code);
    inject_command(&host, &ident, "console.expr", json!({"expression": "1"}));
    inject_command(&host, &ident, "console.expr", json!({"expression": "2"}));
    // 第一轮：仅挂起第一条（Deferred break，第二条不动）。
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    assert!(matches!(
        entry_status(&host, &ident, 1),
        inbox::EntryStatus::Completed { .. }
    ));
    assert!(matches!(
        entry_status(&host, &ident, 2),
        inbox::EntryStatus::Received
    ));
    // 第二轮 drain：第二条挂起，下一拍求值。
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    match entry_status(&host, &ident, 2) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["value"], json!(2))
        }
        other => panic!("第二条应完成：{other:?}"),
    }
    Ok(())
}

/// 晚期失败缓存重试不重复求值：结算失败（Paused/Settle）时候选已
/// 含表达式结果，重试复用缓存——表达式的 V8 全局副作用只发生一次。
#[test]
fn console_expr_settle_retry_does_not_reevaluate() -> anyhow::Result<()> {
    let code = "exports.loop = () => {};";
    let (mut host, ident) = host_v2("expr-retry", code);
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "globalThis.calls = (globalThis.calls || 0) + 1; globalThis.calls"}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 0);
    // 结算失败：玩家（含表达式）已执行、候选缓存、世界未提交。
    host.engine().fail_next_settle_for_test();
    assert!(host.tick()?.is_none(), "结算失败应暂停");
    assert!(matches!(
        host.fault(),
        Some(crate::service::Fault::Paused {
            kind: crate::service::host::PauseKind::Settle,
            ..
        })
    ));
    // 暂停期间命令面冻结：终态未写、挂起保留。
    assert!(matches!(
        entry_status(&host, &ident, 1),
        inbox::EntryStatus::Received
    ));
    // 重试复用缓存轮：表达式不重新求值（calls 仍 1），提交后终态化。
    let report = host.retry()?.expect("重试成功提交缓存轮");
    assert!(report.player_errors.is_empty());
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["value"], json!(1), "缓存轮结果（只求值一次）")
        }
        other => panic!("应完成：{other:?}"),
    }
    assert!(host.pending_expr.is_empty());
    Ok(())
}

/// 结果超限：64KiB 序列化上限把超大返回替换为 serialization 有界
/// 错误结果（仍是 completed，不撑爆回执与失败候选缓存）。
#[test]
fn console_expr_oversize_result_is_clamped() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("expr-clamp", "exports.loop = () => {};");
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "'x'.repeat(70000)"}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["ok"], json!(false), "超限被钳制：{result}");
            assert_eq!(result["kind"], json!("serialization"));
        }
        other => panic!("超限应完成（serialization 错误结果）：{other:?}"),
    }
    Ok(())
}

/// Mod 输入主链路：校验通过挂起（Deferred）→ 下一拍以授予玩家
/// 身份注入真实计数器结算批（字段增量证明效果生效）→ 拍提交后 accepted 终态 + 屏障水位推进。
#[test]
fn mod_input_defers_then_completes_accepted() -> anyhow::Result<()> {
    let (mut host, ident, id) = host_mod("mod-main")?;
    host.set_mod_grants(BTreeMap::from([(
        "workload".to_string(),
        vec!["p1".to_string()],
    )]));
    inject_command(
        &host,
        &ident,
        "mod.input",
        json!({
            "intent": "workload:increment",
            "asPlayer": "p1",
            "targetId": id,
            "args": {"amount": 1},
        }),
    );
    // 注入辅助固定主体 p1——此处流主体应为包 namespace。改写主体：
    // 直接重新构造（覆盖注入），以包主体开流。
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "workload", "m1").map_err(|e| anyhow::anyhow!(e))?;
    let envelope = json!({
        "worldId": ident.world_id,
        "principal": "workload",
        "streamId": "m1",
        "clientSeq": 1,
        "kind": "mod.input",
        "schemaVersion": 1,
        "payload": {
            "intent": "workload:increment",
            "asPlayer": "p1",
            "targetId": id,
            "args": {"amount": 1},
        },
    });
    match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
        inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    assert_eq!(
        host.drain_command_prefix(4)?,
        1,
        "s1 拒绝为终态后，m1 挂起应停止本轮 drain"
    );
    // 第一条（主体 p1）已被 drain 处理：其 intent namespace≠主体——
    // 命令层 reject（p1 不是包主体）。核对其终态：
    let st = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    match &st.streams["s1"].entries[&1].status {
        inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("不一致"), "{reason}")
        }
        other => panic!("主体不匹配应拒绝：{other:?}"),
    }
    // 第二条（包主体 workload）挂起，下一拍注入结算批后终态化。
    run_one_player(&mut host);
    let st = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    match &st.streams["m1"].entries[&1].status {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result, &json!({"accepted": true}))
        }
        other => panic!("应 accepted 完成：{other:?}"),
    }
    let reloaded = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    assert_eq!(reloaded.streams["m1"].applied_seq, 1);
    let effects = effects::load(&host.dir.effects_file(), &ident.world_id)?;
    assert_eq!(effects.anchor.inbox_applied.get("m1"), Some(&1));
    assert_eq!(host.engine.get_object(&id).unwrap()["fields"]["value"], 1);
    Ok(())
}

/// 授予表核验：asPlayer 不在授予列表/无授予表/缺 asPlayer——明确
/// 拒绝终态（跨玩家代表提交不升级为故障）。
#[test]
fn mod_input_grant_violations_are_rejected() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("mod-grant", "exports.loop = () => {};");
    host.set_mod_grants(BTreeMap::from([(
        "nursery".to_string(),
        vec!["p1".to_string()],
    )]));
    // 主体 p1（非包）三条全在 drain 时拒绝：namespace 不一致即先拒。
    inject_command(
        &host,
        &ident,
        "mod.input",
        json!({"intent": "nursery:poke", "asPlayer": "p2", "targetId": "kk1", "args": {}}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 1);
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("不一致"), "{reason}")
        }
        other => panic!("应拒绝：{other:?}"),
    }
    // 主体=包 namespace 的流：授予外玩家。
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "nursery", "m1").map_err(|e| anyhow::anyhow!(e))?;
    for (seq, payload) in [
        (
            1u64,
            json!({"intent": "nursery:poke", "asPlayer": "p2", "targetId": "kk1", "args": {}}),
        ),
        (
            2,
            json!({"intent": "nursery:poke", "targetId": "kk1", "args": {}}),
        ),
        (
            3,
            json!({"intent": "nursery:poke", "asPlayer": "p1", "targetId": "abc!", "args": {}}),
        ),
    ] {
        let envelope = json!({
            "worldId": ident.world_id,
            "principal": "nursery",
            "streamId": "m1",
            "clientSeq": seq,
            "kind": "mod.input",
            "schemaVersion": 1,
            "payload": payload,
        });
        match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
            inbox::accept::AcceptOutcome::Received { .. } => {}
            other => panic!("注入应接收: {other:?}"),
        }
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    assert_eq!(host.drain_command_prefix(4)?, 3, "三条全为终态");
    for (seq, frag) in [(1u64, "未被授予"), (2, "asPlayer"), (3, "十六进制")] {
        let st = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
        match &st.streams["m1"].entries[&seq].status {
            inbox::EntryStatus::Rejected { reason } => {
                assert!(reason.contains(frag), "seq={seq} 拒因 {reason}")
            }
            other => panic!("seq={seq} 应拒绝：{other:?}"),
        }
    }
    assert!(host.pending_mod_input.is_empty(), "违规不挂起");
    Ok(())
}

/// 屏障确认而 inbox 存档缺失：恢复原结果，不重新求值或发布屏障。
#[test]
fn console_expr_barrier_gap_restores_result_without_reevaluation() -> anyhow::Result<()> {
    let code = "exports.loop = () => {};";
    let (mut host, ident) = host_v2("expr-idem", code);
    inject_command(
        &host,
        &ident,
        "console.expr",
        json!({"expression": "globalThis.calls = (globalThis.calls || 0) + 1; globalThis.calls"}),
    );
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    // 模拟存档前崩溃：终态回退 Received、水位归零（屏障与 effects 已含）。
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    state.streams.get_mut("s1").unwrap().applied_seq = 0;
    state
        .streams
        .get_mut("s1")
        .unwrap()
        .entries
        .get_mut(&1)
        .unwrap()
        .status = inbox::EntryStatus::Received;
    inbox::store(&state, &host.dir.inbox_file())?;
    // drain：水位已含，从耐久窗口恢复原结果，不重新挂起。
    assert_eq!(host.drain_command_prefix(4)?, 1);
    assert!(host.pending_expr.is_empty());
    match entry_status(&host, &ident, 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["value"], json!(1), "从耐久窗口恢复原结果")
        }
        other => panic!("应恢复原结果：{other:?}"),
    }
    Ok(())
}

/// mod.input 幂等补全：屏障水位已含时按常量结果重构终态，不重新
/// 挂起注入（意图效果已随屏障链耐久，重注入会双重施加）。
#[test]
fn mod_input_barrier_gap_reconstructs_without_reinjection() -> anyhow::Result<()> {
    let (mut host, ident, id) = host_mod("mod-idem")?;
    host.set_mod_grants(BTreeMap::from([(
        "workload".to_string(),
        vec!["p1".to_string()],
    )]));
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "workload", "m1").map_err(|e| anyhow::anyhow!(e))?;
    let envelope = json!({
        "worldId": ident.world_id,
        "principal": "workload",
        "streamId": "m1",
        "clientSeq": 1,
        "kind": "mod.input",
        "schemaVersion": 1,
        "payload": {
            "intent": "workload:increment",
            "asPlayer": "p1",
            "targetId": id,
            "args": {"amount": 1},
        },
    });
    match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
        inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    // 首轮：挂起→拍注入→屏障+终态。
    assert_eq!(host.drain_command_prefix(4)?, 0);
    run_one_player(&mut host);
    assert!(matches!(
        entry_status_on(&host, &ident, "m1", 1),
        inbox::EntryStatus::Completed { .. }
    ));
    let barrier_before = effects::load(&host.dir.effects_file(), &ident.world_id)?
        .anchor
        .barrier_id;
    // 模拟存档前崩溃：终态回退 Received、水位归零（屏障/effects 已含）。
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    state.streams.get_mut("m1").unwrap().applied_seq = 0;
    state
        .streams
        .get_mut("m1")
        .unwrap()
        .entries
        .get_mut(&1)
        .unwrap()
        .status = inbox::EntryStatus::Received;
    inbox::store(&state, &host.dir.inbox_file())?;
    // 补全轮：drain 直接重构终态（accepted），不挂起（无新屏障）。
    assert_eq!(host.drain_command_prefix(4)?, 1);
    assert!(host.pending_mod_input.is_empty(), "补全不得重新挂起注入");
    match entry_status_on(&host, &ident, "m1", 1) {
        inbox::EntryStatus::Completed { result } => {
            assert_eq!(result, json!({"accepted": true}))
        }
        other => panic!("应重构完成：{other:?}"),
    }
    let barrier_after = effects::load(&host.dir.effects_file(), &ident.world_id)?
        .anchor
        .barrier_id;
    assert_eq!(barrier_before, barrier_after, "补全不发布新屏障");
    assert_eq!(host.engine.get_object(&id).unwrap()["fields"]["value"], 1);
    Ok(())
}

/// console.expr 未知主体：明确拒绝不挂起（与 code.replace 同口径）。
#[test]
fn console_expr_unknown_principal_is_rejected() -> anyhow::Result<()> {
    let (mut host, ident) = host_v2("expr-ghost", "exports.loop = () => {};");
    let mut state = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    inbox::accept::open_stream(&mut state, "ghost", "s1").map_err(|e| anyhow::anyhow!(e))?;
    let envelope = json!({
        "worldId": ident.world_id,
        "principal": "ghost",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "console.expr",
        "schemaVersion": 1,
        "payload": {"expression": "1"},
    });
    match inbox::accept::accept(&mut state, &envelope, &inbox::InboxLimits::default(), 1) {
        inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    inbox::store(&state, &host.dir.inbox_file())?;
    assert_eq!(host.drain_command_prefix(4)?, 1);
    let st = inbox::load(&host.dir.inbox_file(), &ident.world_id)?;
    match &st.streams["s1"].entries[&1].status {
        inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("不是注册玩家"), "{reason}")
        }
        other => panic!("应拒绝：{other:?}"),
    }
    assert!(host.pending_expr.is_empty());
    Ok(())
}
