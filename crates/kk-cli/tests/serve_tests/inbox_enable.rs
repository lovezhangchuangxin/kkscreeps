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

/// received 跨 run 不丢：run 周期保存/恢复不触碰 inbox 内容。
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
    let entry = reloaded
        .streams
        .get("s1")
        .and_then(|stream| stream.entries.get(&1))
        .expect("received 命令跨 run 不得丢失");
    assert_eq!(entry.payload["cmd"], json!("survive"));
    assert_eq!(entry.server_seq, 1);
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

/// 读最新检查点的 metadata.service（信封内 JSON 载荷）。
fn read_latest_service_meta(fixture: &Fixture) -> anyhow::Result<Value> {
    let dir = kk_runtime::service::ServiceDir::new(fixture.state());
    let identity = kk_runtime::service::identity::read(&dir.identity_file())?;
    let selected =
        kk_runtime::service::checkpoints::select_latest(&dir.checkpoints_dir(), &identity, None)?;
    Ok(serde_json::to_value(&selected.meta)?)
}
