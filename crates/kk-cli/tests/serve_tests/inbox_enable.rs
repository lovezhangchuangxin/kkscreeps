//! S2.1 耐久命令面验收：enable-inbox 启用事务（含中断续接）、v2 目录
//! 启动完整性校验、恢复下限（锚点保护与停服语义）、received 跨 run 不丢。
use std::path::Path;

use serde_json::{Value, json};

use super::recovery::{Fixture, checkpoint_count, init_ok, spawn_run};
use super::{serve, wait_for};

/// v1 目录初始化并跑出多份检查点后停止，返回 (fixture, config)。
fn v1_directory_ready(extra: Value) -> anyhow::Result<(Fixture, std::path::PathBuf)> {
    let fixture = Fixture::new()?;
    let config = fixture.config(extra)?;
    init_ok(&fixture, &config)?;
    let (mut run, _) = spawn_run(&config, &fixture, "v1.err")?;
    wait_for(|| checkpoint_count(&fixture) >= 2)?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    let status = run.wait()?;
    anyhow::ensure!(status.code() == Some(0), "v1 预备运行应退出 0：{status:?}");
    Ok((fixture, config))
}

fn enable_inbox(config: &Path) -> std::io::Result<std::process::Output> {
    serve(&[
        "serve",
        "enable-inbox",
        "--config",
        &config.to_string_lossy(),
    ])
    .output()
}

fn identity_version(fixture: &Fixture) -> anyhow::Result<u64> {
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    Ok(identity.service_format_version as u64)
}

/// 主链路：v1→v2 enable（报告 barrier/seq）→ v2 常驻 run（恢复下限生效）
/// → 停止退出 0 → 重复 enable 拒绝。
#[test]
#[cfg(unix)]
fn enable_inbox_then_v2_run_roundtrip() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    let output = enable_inbox(&config)?;
    anyhow::ensure!(
        output.status.success(),
        "enable-inbox 应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["resumed"], false, "首次启用不是续接：{report}");
    let anchor_seq = report["barrier"]["checkpoint_seq"]
        .as_u64()
        .unwrap_or_default();
    let barrier = report["barrier"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    anyhow::ensure!(
        anchor_seq >= 1 && !barrier.is_empty(),
        "报告缺屏障：{report}"
    );
    assert_eq!(identity_version(&fixture)?, 2);
    assert!(fixture.state().join("inbox.json").is_file());
    assert!(fixture.state().join("effects.json").is_file());

    // v2 run：恢复选择不低于锚点，续存推进后正常停止。
    let (mut run, _) = spawn_run(&config, &fixture, "v2-run.err")?;
    wait_for(|| checkpoint_count(&fixture) as u64 > anchor_seq + 1)?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    let status = run.wait()?;
    anyhow::ensure!(status.code() == Some(0), "v2 运行应退出 0：{status:?}");
    // 新保存的检查点继承同一屏障身份（普通保存不建立新屏障）。
    let latest_meta = read_latest_service_meta(&fixture)?;
    assert_eq!(
        latest_meta["barrier_id"].as_str(),
        Some(barrier.as_str()),
        "普通保存必须继承锚点屏障身份"
    );

    // 重复 enable 拒绝。
    let output = enable_inbox(&config)?;
    anyhow::ensure!(output.status.code() == Some(1), "重复启用应拒绝");
    Ok(())
}

/// 启用中断（identity 切换前崩溃）重跑续接：不重复发布 v2 检查点、
/// 已有 received 不被重置。
#[test]
#[cfg(unix)]
fn enable_inbox_resumes_after_interrupted_switch() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    let output = enable_inbox(&config)?;
    anyhow::ensure!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let anchor_seq = report["barrier"]["checkpoint_seq"]
        .as_u64()
        .unwrap_or_default();
    // 中断在 identity 切换前的形态：目录完整（v2 检查点/inbox/effects 齐）
    // 但 identity 仍 v1——以进程内 API 原地重写身份模拟（其余字段不变）。
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let mut identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    anyhow::ensure!(identity.service_format_version == 2, "enable 后应为 v2");
    identity.service_format_version = kk_runtime::service::identity::SERVICE_FORMAT_V1;
    kk_runtime::service::identity::write(&dir.identity_file(), &identity)?;

    // 在中断态下进程内注入一条 received（identity 未切、模拟启用前的
    // 收件箱内容——续接不得重置已 received 命令）。
    let mut inbox = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox, "p1", "s1")
        .map_err(|error| anyhow::anyhow!("开流失败: {error}"))?;
    let envelope = json!({
        "worldId": identity.world_id,
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "test.ping",
        "schemaVersion": 1,
        "payload": {"n": 1},
    });
    match kk_runtime::service::inbox::accept::accept(
        &mut inbox,
        &envelope,
        &kk_runtime::service::inbox::InboxLimits::default(),
        1_000,
    ) {
        kk_runtime::service::inbox::accept::AcceptOutcome::Received { server_seq } => {
            assert_eq!(server_seq, 1);
        }
        other => panic!("中断态注入应接收: {other:?}"),
    }
    kk_runtime::service::inbox::store(&inbox, &dir.inbox_file())?;

    // 重跑 enable：续接（不新增 v2 检查点），inbox 保留 received。
    let before_count = checkpoint_count(&fixture);
    let output = enable_inbox(&config)?;
    anyhow::ensure!(
        output.status.success(),
        "续接应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["resumed"], true, "应报告续接：{report}");
    assert_eq!(
        report["barrier"]["checkpoint_seq"].as_u64(),
        Some(anchor_seq),
        "续接沿用同一屏障检查点"
    );
    assert_eq!(checkpoint_count(&fixture), before_count, "续接不新增检查点");
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    anyhow::ensure!(identity.service_format_version == 2);
    let inbox = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    assert!(
        inbox.streams["s1"].entries.contains_key(&1),
        "续接不得重置 received"
    );
    Ok(())
}

/// 真实中断形态：v2 屏障检查点已耐久、inbox/effects 未写、identity 仍
/// v1——重跑 enable 续接补齐全部缺失文件并完成切换，后续 run 可用。
#[test]
#[cfg(unix)]
fn interrupted_enable_without_side_files_resumes() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    let output = enable_inbox(&config)?;
    anyhow::ensure!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let anchor_seq = report["barrier"]["checkpoint_seq"]
        .as_u64()
        .unwrap_or_default();

    // 模拟"屏障发布后、inbox/effects 写前"崩溃：删两文件 + 身份回 v1。
    std::fs::remove_file(fixture.state().join("inbox.json"))?;
    std::fs::remove_file(fixture.state().join("effects.json"))?;
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let mut identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    identity.service_format_version = kk_runtime::service::identity::SERVICE_FORMAT_V1;
    kk_runtime::service::identity::write(&dir.identity_file(), &identity)?;

    let output = enable_inbox(&config)?;
    anyhow::ensure!(
        output.status.success(),
        "中断续接应成功: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["resumed"], true);
    assert_eq!(
        report["barrier"]["checkpoint_seq"].as_u64(),
        Some(anchor_seq),
        "沿用已发布屏障，不重复发布"
    );
    assert!(fixture.state().join("inbox.json").is_file());
    assert!(fixture.state().join("effects.json").is_file());
    assert_eq!(identity_version(&fixture)?, 2);

    // 续接完成的目录可正常运行。
    let (mut run, _) = spawn_run(&config, &fixture, "resume-run.err")?;
    wait_for(|| checkpoint_count(&fixture) as u64 > anchor_seq)?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    let status = run.wait()?;
    anyhow::ensure!(status.code() == Some(0), "续接后运行应退出 0：{status:?}");
    Ok(())
}

/// v2 目录缺 inbox/effects 拒绝启动（不猜测缺失文件）。
#[test]
#[cfg(unix)]
fn v2_run_requires_inbox_and_effects() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    anyhow::ensure!(enable_inbox(&config)?.status.success());
    let inbox_path = fixture.state().join("inbox.json");
    let effects_path = fixture.state().join("effects.json");

    let saved_inbox = std::fs::read(&inbox_path)?;
    std::fs::remove_file(&inbox_path)?;
    let output = serve(&["serve", "run", "--config", &config.to_string_lossy()]).output()?;
    anyhow::ensure!(output.status.code() == Some(1), "缺 inbox 应拒启");
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(stderr.contains("inbox"), "错误应指向 inbox：{stderr}");

    let saved_effects = std::fs::read(&effects_path)?;
    std::fs::remove_file(&effects_path)?;
    let output = serve(&["serve", "run", "--config", &config.to_string_lossy()]).output()?;
    anyhow::ensure!(output.status.code() == Some(1), "缺 effects 应拒启");
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::ensure!(stderr.contains("effects"), "错误应指向 effects：{stderr}");

    std::fs::write(&inbox_path, &saved_inbox)?;
    std::fs::write(&effects_path, &saved_effects)?;
    Ok(())
}

/// 恢复下限：锚点及之后全部缺失（只剩 v1 旧份）时 run 拒启（停服语义，
/// 不回退到锚点之前）；锚点在保留窗口外仍受保护不被清理。
#[test]
#[cfg(unix)]
fn floor_blocks_recovery_before_anchor_and_keep_protects_anchor() -> anyhow::Result<()> {
    // 小窗口保留（keep 2）验证锚点保护。
    let (fixture, config) = v1_directory_ready(
        json!({"keep_checkpoints": 2, "auto_save_ticks": 1, "tick_interval_ms": 30}),
    )?;
    let output = enable_inbox(&config)?;
    anyhow::ensure!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let anchor_seq = report["barrier"]["checkpoint_seq"]
        .as_u64()
        .unwrap_or_default();
    let anchor_file = fixture
        .checkpoints()
        .join(format!("cp-{anchor_seq:012}.json"));

    let (mut run, _) = spawn_run(&config, &fixture, "keep.err")?;
    // 保留窗口使目录条目数封顶（锚点+keep 份）；等待最大发布序号增长。
    wait_for(|| latest_seq(&fixture) > anchor_seq + 3)?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    run.wait()?;
    anyhow::ensure!(
        anchor_file.is_file(),
        "保留窗口前移不得删除锚点检查点（keep=2 且已保存多份）"
    );

    // 停服语义：删除锚点及其后继（模拟锚点损坏扩散），只剩 v1 旧份。
    for entry in std::fs::read_dir(fixture.checkpoints())? {
        let entry = entry?;
        let name = entry.file_name().into_string().unwrap_or_default();
        if let Some(seq) = name
            .strip_prefix("cp-")
            .and_then(|rest| rest.strip_suffix(".json"))
            .and_then(|digits| digits.parse::<u64>().ok())
            && seq >= anchor_seq
        {
            std::fs::remove_file(entry.path())?;
        }
    }
    let output = serve(&["serve", "run", "--config", &config.to_string_lossy()]).output()?;
    anyhow::ensure!(output.status.code() == Some(1), "无锚点后继应停服拒启");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // keep=2 时 v1 旧份已出窗，锚点后继删尽即目录空；无论落在"目录空"
    // 还是"恢复下限"分支都是拒绝启动。"仅剩 v1 旧份→恢复下限停服"的
    // 精确分支由 runtime 单测 v2_floor_selects_only_anchor_and_qualified_
    // successors 锁定。
    anyhow::ensure!(
        stderr.contains("恢复选择失败"),
        "错误应来自恢复选择停服：{stderr}"
    );
    Ok(())
}

/// received 跨 run 不丢：S2.2 起 run 会消费命令（unknown kind 走明确
/// 拒绝终态），断言现行语义——终态化+水位推进，同时明细与载荷保留。
#[test]
#[cfg(unix)]
fn inbox_received_survives_run_cycles() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    anyhow::ensure!(enable_inbox(&config)?.status.success());
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let mut inbox = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox, "p1", "s1")
        .map_err(|error| anyhow::anyhow!(error))?;
    let envelope = json!({
        "worldId": identity.world_id,
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "test.ping",
        "schemaVersion": 1,
        "payload": {"cmd": "survive"},
    });
    // S2.1 无运行时接收入口，进程内注入是构造 received 的唯一途径。
    match kk_runtime::service::inbox::accept::accept(
        &mut inbox,
        &envelope,
        &kk_runtime::service::inbox::InboxLimits::default(),
        2_000,
    ) {
        kk_runtime::service::inbox::accept::AcceptOutcome::Received { server_seq } => {
            assert_eq!(server_seq, 1);
        }
        other => panic!("注入应接收: {other:?}"),
    }
    kk_runtime::service::inbox::store(&inbox, &dir.inbox_file())?;

    let (mut run, _) = spawn_run(&config, &fixture, "cycle.err")?;
    // 等待基线取 enable 后的条目数：enable 已使目录 ≥ 基线值，直接用
    // 绝对阈值会在 run 起步前满足，SIGTERM 落在信号处理器安装前。
    let baseline = checkpoint_count(&fixture);
    wait_for(|| checkpoint_count(&fixture) > baseline)?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    let status = run.wait()?;
    anyhow::ensure!(status.code() == Some(0), "v2 run 应正常退出：{status:?}");

    let reloaded = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    let stream = reloaded.streams.get("s1").expect("流跨 run 不得丢失");
    let entry = stream
        .entries
        .get(&1)
        .expect("received 命令跨 run 不得丢失");
    // S2.2 起 run 会消费该命令：unknown kind 走明确 Rejected 终态、
    // 水位推进；明细与载荷保留（received 记录不因消费而丢失）。
    assert_eq!(entry.payload["cmd"], json!("survive"));
    assert_eq!(entry.server_seq, 1);
    assert_eq!(stream.applied_seq, 1, "消费应推进流水位");
    match &entry.status {
        kk_runtime::service::inbox::EntryStatus::Rejected { reason } => {
            assert!(reason.contains("未知命令"), "{reason}")
        }
        other => panic!("test.ping 应被消费为未知 kind 拒绝: {other:?}"),
    }
    Ok(())
}

/// 目录中最大发布序号（按文件名解析）。
fn latest_seq(fixture: &Fixture) -> u64 {
    std::fs::read_dir(fixture.checkpoints())
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let name = entry.ok()?.file_name().into_string().ok()?;
                    name.strip_prefix("cp-")?
                        .strip_suffix(".json")?
                        .parse()
                        .ok()
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// S2.2 代码边界事务：注入 code.replace 命令（expected 0）→ serve run
/// 消费（即时屏障、codeVersion=1）→ 新代码在运行中生效且跨重启保留、
/// Memory 延续；CAS 过期命令被拒并持久化终态。
#[test]
#[cfg(unix)]
fn code_replace_command_consumed_with_barrier_and_memory_kept() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    let output = enable_inbox(&config)?;
    anyhow::ensure!(output.status.success());
    let enable_report: Value = serde_json::from_slice(&output.stdout)?;
    let anchor_seq = enable_report["barrier"]["checkpoint_seq"]
        .as_u64()
        .unwrap_or_default();

    // 注入两条：正确版本（v0→v1，新代码数 Memory.m）与过期版本（CAS 拒）。
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let mut inbox_state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox_state, "p1", "s1")
        .map_err(|error| anyhow::anyhow!(error))?;
    let new_code = "exports.loop = () => { Memory.m = (Memory.m || 0) + 1000; };";
    for (seq, expected) in [(1u64, 0u64), (2, 5)] {
        let envelope = json!({
            "worldId": identity.world_id,
            "principal": "p1",
            "streamId": "s1",
            "clientSeq": seq,
            "kind": "code.replace",
            "schemaVersion": 1,
            "payload": {"modules": {"main": new_code}, "expectedCodeVersion": expected},
        });
        match kk_runtime::service::inbox::accept::accept(
            &mut inbox_state,
            &envelope,
            &kk_runtime::service::inbox::InboxLimits::default(),
            3_000,
        ) {
            kk_runtime::service::inbox::accept::AcceptOutcome::Received { server_seq } => {
                assert_eq!(server_seq, seq);
            }
            other => panic!("注入 seq={seq} 应接收: {other:?}"),
        }
    }
    kk_runtime::service::inbox::store(&inbox_state, &dir.inbox_file())?;

    // 运行：消费两条命令 + 新代码执行若干拍。
    let (mut run, _) = spawn_run(&config, &fixture, "s22-run.err")?;
    wait_for(|| {
        let state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id);
        matches!(
            &state,
            Ok(state) if matches!(
                state.streams["s1"].entries.get(&2),
                Some(kk_runtime::service::inbox::CommandEntry {
                    status: kk_runtime::service::inbox::EntryStatus::Rejected { .. },
                    ..
                })
            )
        )
    })?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    let status = run.wait()?;
    anyhow::ensure!(status.code() == Some(0), "S2.2 运行应退出 0：{status:?}");

    // inbox 终态与水位：seq1 completed（codeVersion 1）、seq2 rejected（CAS）。
    let state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    let stream = &state.streams["s1"];
    assert_eq!(stream.applied_seq, 2, "验证拒绝也推进消费水位");
    match &stream.entries[&1].status {
        kk_runtime::service::inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["codeVersion"], json!(1))
        }
        other => panic!("seq1 应完成：{other:?}"),
    }
    assert!(matches!(
        &stream.entries[&2].status,
        kk_runtime::service::inbox::EntryStatus::Rejected { reason } if reason.contains("CAS")
    ));

    // 屏障推进：锚点越过 enable 屏障，codeVersion 持久，新代码跨重启生效。
    let effects = kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    anyhow::ensure!(
        effects.anchor.checkpoint_seq > anchor_seq,
        "命令屏障应推进锚点"
    );
    let latest = read_latest_service_meta(&fixture)?;
    assert_eq!(latest["code_versions"]["p1"], json!(1));
    // rejected 不建屏障：屏障只为 completed 效果的外部确认锚点，最新屏障只
    // 覆盖 seq1 的水位；seq2 的拒绝消费由 inbox 文件自身的 applied_seq=2
    // 持久化（上方断言锁定），恢复后已终态不会被重新消费。
    assert_eq!(latest["inbox_applied"]["s1"], json!(1));
    // 恢复核对：新模块已持久（玩家执行走新代码，Memory.m 增长、旧 n 保留）。
    let (packages, options) = super::recovery::mod_loadout()?;
    let mut engine = kk_runtime::EmbedEngine::new();
    engine
        .restore_checkpoint(latest_payload(&fixture)?, Some((&packages, &options)))
        .map_err(|error| anyhow::anyhow!("恢复失败: {error}"))?;
    let code = "(() => { const bot = require(\"main\"); bot.loop(); return true; })()";
    engine.run_player("p1", code)?;
    let raw = engine.player_state("p1").unwrap().raw_memory.clone();
    let memory: Value = serde_json::from_str(&raw)?;
    assert!(
        memory["m"].as_i64().unwrap_or(0) >= 1000,
        "新代码应随检查点持久并在恢复后执行：{memory}"
    );
    Ok(())
}

/// S2.3 玩家表达式与受限 Mod 输入：真实子进程链路上挂起命令随拍求值/
/// 注入，拍提交后经屏障终态化（表达式携带求值结果、Mod 输入 accepted）。
#[test]
#[cfg(unix)]
fn console_expr_and_mod_input_complete_via_real_run() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-player");
    let config = fixture.config(json!({
        "config_format_version": 2,
        "mod_grants": {"workload": ["p1"]},
        "mods": {"root": root},
    }))?;
    let scenario_file = config.parent().unwrap().join("scenario.json");
    let mut scenario: Value = serde_json::from_slice(&std::fs::read(&scenario_file)?)?;
    scenario["mods"] = json!({"root": root});
    scenario["objects"] = json!([{
        "alias": "counter", "room": "W1N1", "kind": "mod",
        "spec": {"typeKey": "workload:counter", "fields": {"owner": "p1", "x": 10, "y": 10}},
    }]);
    std::fs::write(&scenario_file, serde_json::to_vec(&scenario)?)?;
    init_ok(&fixture, &config)?;
    anyhow::ensure!(enable_inbox(&config)?.status.success());
    let packages = kk_mod_host::discover_packages(&root)?;
    let options = kk_mod_host::RegisterOptions::default();
    let mut initial = kk_runtime::EmbedEngine::new();
    initial.restore_checkpoint(latest_payload(&fixture)?, Some((&packages, &options)))?;
    let target = initial.mod_object_snapshots()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    drop(initial);
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let mut inbox_state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox_state, "p1", "s1")
        .map_err(|error| anyhow::anyhow!(error))?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox_state, "workload", "m1")
        .map_err(|error| anyhow::anyhow!(error))?;
    let mut envelopes = Vec::new();
    for (principal, stream, seq, kind, payload) in [
        (
            "workload",
            "m1",
            1,
            "mod.input",
            json!({
                "intent": "workload:increment",
                "asPlayer": "p1",
                "targetId": target,
                "args": {"amount": 1},
            }),
        ),
        (
            "p1",
            "s1",
            1u64,
            "console.expr",
            json!({"expression": "Memory.exprDone = (Memory.exprDone || 0) + 1; 'from-expr'"}),
        ),
    ] {
        envelopes.push(json!({
            "worldId": identity.world_id,
            "principal": principal,
            "streamId": stream,
            "clientSeq": seq,
            "kind": kind,
            "schemaVersion": 1,
            "payload": payload,
        }));
    }
    for envelope in &envelopes {
        match kk_runtime::service::inbox::accept::accept(
            &mut inbox_state,
            envelope,
            &kk_runtime::service::inbox::InboxLimits::default(),
            3_000,
        ) {
            kk_runtime::service::inbox::accept::AcceptOutcome::Received { .. } => {}
            other => panic!("注入应接收: {other:?}"),
        }
    }
    kk_runtime::service::inbox::store(&inbox_state, &dir.inbox_file())?;

    let (mut run, stderr_file) = spawn_run(&config, &fixture, "s23-run.err")?;
    wait_for(|| {
        kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)
            .map(|state| {
                [
                    &state.streams["s1"].entries[&1],
                    &state.streams["m1"].entries[&1],
                ]
                .iter()
                .all(|entry| {
                    matches!(
                        entry.status,
                        kk_runtime::service::inbox::EntryStatus::Completed { .. }
                    )
                })
            })
            .unwrap_or(false)
    })
    .map_err(|error| {
        anyhow::anyhow!(
            "{error}: {} inbox={:?}",
            std::fs::read_to_string(&stderr_file).unwrap_or_default(),
            kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)
                .ok()
                .and_then(|s| serde_json::to_value(s).ok())
        )
    })?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(run.wait()?.code() == Some(0), "S2.3 运行应退出 0");

    let state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    match &state.streams["s1"].entries[&1].status {
        kk_runtime::service::inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["value"], json!("from-expr"), "表达式结果随终态返回")
        }
        other => panic!("表达式应完成：{other:?}"),
    }
    match &state.streams["m1"].entries[&1].status {
        kk_runtime::service::inbox::EntryStatus::Completed { result } => {
            assert_eq!(result, &json!({"accepted": true}))
        }
        other => panic!("Mod 输入应 accepted：{other:?}"),
    }
    // 屏障水位含两条流；表达式副作用已随检查点耐久（恢复面验证）。
    let effects = kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    assert_eq!(effects.anchor.inbox_applied.get("s1"), Some(&1));
    assert_eq!(effects.anchor.inbox_applied.get("m1"), Some(&1));
    let mut engine = kk_runtime::EmbedEngine::new();
    engine
        .restore_checkpoint(latest_payload(&fixture)?, Some((&packages, &options)))
        .map_err(|error| anyhow::anyhow!("恢复失败: {error}"))?;
    let code = "(() => { const bot = require(\"main\"); bot.loop(); return true; })()";
    engine.run_player("p1", code)?;
    let raw = engine.player_state("p1").unwrap().raw_memory.clone();
    let memory: Value = serde_json::from_str(&raw)?;
    assert_eq!(memory["exprDone"], json!(1), "表达式副作用随检查点持久");
    assert_eq!(
        engine.get_object(&target).unwrap()["fields"]["value"],
        1,
        "Mod 输入真实结算效果跨恢复保持"
    );
    // 模拟终态存档前崩溃：耐久世界/结果不动，仅 inbox 退回 Received。
    let mut replay_state = state.clone();
    {
        let st = replay_state.streams.get_mut("s1").unwrap();
        st.applied_seq = 0;
        st.entries.get_mut(&1).unwrap().status = kk_runtime::service::inbox::EntryStatus::Received;
    }
    kk_runtime::service::inbox::store(&replay_state, &dir.inbox_file())?;
    let (mut resumed, _) = spawn_run(&config, &fixture, "s23-gap.err")?;
    wait_for(|| {
        kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)
            .map(|s| {
                ["s1", "m1"]
                    .iter()
                    .all(|id| s.streams[*id].entries[&1].status.is_settled())
            })
            .unwrap_or(false)
    })?;
    unsafe {
        libc::kill(resumed.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(resumed.wait()?.code() == Some(0));
    let recovered_results =
        kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    assert_eq!(
        serde_json::to_value(&recovered_results.streams["s1"].entries[&1].status)?,
        serde_json::to_value(&state.streams["s1"].entries[&1].status)?
    );
    let mut restored = kk_runtime::EmbedEngine::new();
    restored.restore_checkpoint(latest_payload(&fixture)?, Some((&packages, &options)))?;
    assert_eq!(
        restored.get_object(&target).unwrap()["fields"]["value"],
        1,
        "缺口恢复不能重复施加 Mod 输入"
    );
    let memory: Value = serde_json::from_str(&restored.player_state("p1").unwrap().raw_memory)?;
    assert_eq!(memory["exprDone"], 1, "缺口恢复不能重复表达式副作用");
    Ok(())
}

/// 崩溃窗口（矩阵 2，链路级）：屏障已发布、effects 已更新、inbox 终态
/// 未存档——重启走真实恢复装配（v2 上下文自检查点 meta 重建）后 drain
/// 幂等补全：结果由当前 codeVersion 重构，不重新执行（无新屏障）。
#[test]
#[cfg(unix)]
fn barrier_confirmed_before_archive_recovers_via_real_run() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    anyhow::ensure!(enable_inbox(&config)?.status.success());
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let mut inbox_state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox_state, "p1", "s1")
        .map_err(|error| anyhow::anyhow!(error))?;
    let envelope = json!({
        "worldId": identity.world_id,
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "code.replace",
        "schemaVersion": 1,
        "payload": {
            "modules": {"main": "exports.loop = () => { Memory.m = (Memory.m || 0) + 1000; };"},
            "expectedCodeVersion": 0,
        },
    });
    match kk_runtime::service::inbox::accept::accept(
        &mut inbox_state,
        &envelope,
        &kk_runtime::service::inbox::InboxLimits::default(),
        3_000,
    ) {
        kk_runtime::service::inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    kk_runtime::service::inbox::store(&inbox_state, &dir.inbox_file())?;

    // 第一次 run：消费命令（屏障发布+effects 推进+终态存档）。
    let (mut run, _) = spawn_run(&config, &fixture, "s22-idem1.err")?;
    wait_for(|| entry_completed(&dir, &identity.world_id))?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(run.wait()?.code() == Some(0));
    let effects_after =
        kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;

    // 模拟存档前崩溃：终态回退 Received、水位归零（屏障检查点与
    // effects 已含水位=1）。
    let mut state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    let stream = state.streams.get_mut("s1").unwrap();
    stream.applied_seq = 0;
    stream.entries.get_mut(&1).unwrap().status = kk_runtime::service::inbox::EntryStatus::Received;
    kk_runtime::service::inbox::store(&state, &dir.inbox_file())?;

    // 重启：真实恢复装配后 drain 幂等补全——Completed 结果由当前版本
    // 重构（codeVersion=1），不重新执行替换（否则 CAS 0≠1 拒绝）、
    // 不发布新屏障（effects 锚点保持原值）。
    let (mut run, _) = spawn_run(&config, &fixture, "s22-idem2.err")?;
    wait_for(|| entry_completed(&dir, &identity.world_id))?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(run.wait()?.code() == Some(0), "幂等补全轮应正常退出");

    let state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    match &state.streams["s1"].entries[&1].status {
        kk_runtime::service::inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["codeVersion"], json!(1), "结果由当前版本重构")
        }
        other => panic!("应幂等补全为完成: {other:?}"),
    }
    let effects_final =
        kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    assert_eq!(
        effects_final.anchor.barrier_id, effects_after.anchor.barrier_id,
        "幂等补全不得发布新屏障"
    );
    assert_eq!(
        effects_final.anchor.checkpoint_seq, effects_after.anchor.checkpoint_seq,
        "补全不动锚点序号（普通保存不写 effects）"
    );
    Ok(())
}

/// 崩溃窗口（矩阵 3，链路级）：屏障检查点已发布、effects 未更新（孤儿
/// 屏障）——恢复下限的屏障身份门过滤孤儿链，世界回旧链（enable 屏障）
/// 重新消费该命令，CAS 基准随旧 meta 回滚后重新匹配，最终收敛到新屏障。
#[test]
#[cfg(unix)]
fn orphan_barrier_filtered_back_to_old_chain_reruns() -> anyhow::Result<()> {
    let (fixture, config) = v1_directory_ready(json!({}))?;
    anyhow::ensure!(enable_inbox(&config)?.status.success());
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    // 保存 enable 锚点副本（回拨目标）。
    let enable_effects =
        kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    let mut inbox_state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    kk_runtime::service::inbox::accept::open_stream(&mut inbox_state, "p1", "s1")
        .map_err(|error| anyhow::anyhow!(error))?;
    let envelope = json!({
        "worldId": identity.world_id,
        "principal": "p1",
        "streamId": "s1",
        "clientSeq": 1,
        "kind": "code.replace",
        "schemaVersion": 1,
        "payload": {
            "modules": {"main": "exports.loop = () => { Memory.m = (Memory.m || 0) + 1000; };"},
            "expectedCodeVersion": 0,
        },
    });
    match kk_runtime::service::inbox::accept::accept(
        &mut inbox_state,
        &envelope,
        &kk_runtime::service::inbox::InboxLimits::default(),
        3_000,
    ) {
        kk_runtime::service::inbox::accept::AcceptOutcome::Received { .. } => {}
        other => panic!("注入应接收: {other:?}"),
    }
    kk_runtime::service::inbox::store(&inbox_state, &dir.inbox_file())?;

    // 第一次 run：命令消费完成（孤儿屏障链的来源）。
    let (mut run, _) = spawn_run(&config, &fixture, "s22-orphan1.err")?;
    wait_for(|| entry_completed(&dir, &identity.world_id))?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(run.wait()?.code() == Some(0));
    let orphan_barrier =
        kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?
            .anchor
            .barrier_id;

    // 模拟"屏障已发布、effects 未更新"：effects 回拨 enable 锚点、
    // inbox 终态回退。孤儿链检查点仍在目录中，但恢复选择按身份门
    // 排除它们，回到 enable 屏障重新起步。隐含依赖 enable 屏障检查点
    // 在 keep 窗口内存活（每轮保存份数少）；若未来保存密度增大致旧链
    // 份被清理，恢复下限会正确停服、本用例响亮失败（时序假设显式化）。
    kk_runtime::service::effects::write(&dir.effects_file(), &enable_effects)?;
    let mut state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    let stream = state.streams.get_mut("s1").unwrap();
    stream.applied_seq = 0;
    stream.entries.get_mut(&1).unwrap().status = kk_runtime::service::inbox::EntryStatus::Received;
    kk_runtime::service::inbox::store(&state, &dir.inbox_file())?;

    // 重启：回旧链重新消费——CAS 以回滚后的 codeVersion（无 p1 记录，
    // 即 0）重新匹配，重新执行替换并发布新屏障（不等于孤儿身份）。
    let (mut run, _) = spawn_run(&config, &fixture, "s22-orphan2.err")?;
    wait_for(|| entry_completed(&dir, &identity.world_id))?;
    unsafe {
        libc::kill(run.id() as i32, libc::SIGTERM);
    }
    anyhow::ensure!(run.wait()?.code() == Some(0), "孤儿屏障窗口应收敛退出 0");

    let state = kk_runtime::service::inbox::load(&dir.inbox_file(), &identity.world_id)?;
    match &state.streams["s1"].entries[&1].status {
        kk_runtime::service::inbox::EntryStatus::Completed { result } => {
            assert_eq!(result["codeVersion"], json!(1), "回旧链重新消费后完成")
        }
        other => panic!("应重新消费完成: {other:?}"),
    }
    let effects_final =
        kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    assert_ne!(
        effects_final.anchor.barrier_id, orphan_barrier,
        "收敛屏障应为新身份，不復用孤儿"
    );
    assert_eq!(effects_final.anchor.inbox_applied.get("s1"), Some(&1));
    Ok(())
}

/// 流 s1 的 seq1 是否已达 Completed 终态。
fn entry_completed(dir: &kk_runtime::service::ServiceDir, world_id: &str) -> bool {
    kk_runtime::service::inbox::load(&dir.inbox_file(), world_id)
        .ok()
        .and_then(|state| {
            state
                .streams
                .get("s1")
                .and_then(|stream| stream.entries.get(&1))
                .map(|entry| {
                    matches!(
                        entry.status,
                        kk_runtime::service::inbox::EntryStatus::Completed { .. }
                    )
                })
        })
        .unwrap_or(false)
}

/// 读最新检查点的 metadata.service（信封内 JSON 载荷）。
fn latest_payload(fixture: &Fixture) -> anyhow::Result<Value> {
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let floor = kk_runtime::service::effects::load(&dir.effects_file(), &identity.world_id)?;
    let selected = kk_runtime::service::checkpoints::select_latest(
        &dir.checkpoints_dir(),
        &identity,
        Some(&kk_runtime::service::checkpoints::RecoveryFloor {
            checkpoint_seq: floor.anchor.checkpoint_seq,
            barrier_id: floor.anchor.barrier_id.clone(),
            inbox_applied: floor.anchor.inbox_applied.clone(),
        }),
    )?;
    Ok(selected.value)
}

fn read_latest_service_meta(fixture: &Fixture) -> anyhow::Result<Value> {
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let selected =
        kk_runtime::service::checkpoints::select_latest(&dir.checkpoints_dir(), &identity, None)?;
    Ok(serde_json::to_value(&selected.meta)?)
}
