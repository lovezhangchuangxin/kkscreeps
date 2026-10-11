//! 服务检查点选择/序号/保留的行为测试：合成载荷覆盖错误分类与身份门，
//! 真实引擎覆盖发布-选择-恢复链路。
use super::*;
use crate::service::identity::{self, ServiceIdentity};
use serde_json::{Value, json};

fn temp_dir(tag: &str) -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("kk-service-cp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn sample_identity() -> ServiceIdentity {
    ServiceIdentity {
        service_format_version: identity::SERVICE_FORMAT_VERSION,
        world_id: identity::generate_world_id(),
        world_seed: 7,
        scenario_sha256: "b".repeat(64),
        initialized: true,
    }
}

/// 合成一份带服务元数据与世界时间/种子的检查点载荷（信封机制不关心
/// 引擎细节，但身份门要求 /engine/world/time 与 /engine/world/seed 在）。
fn synthetic(seq: u64, tick: u64, ident: &ServiceIdentity) -> Value {
    let meta = metadata::new_meta(
        &ident.world_id,
        seq,
        tick,
        ident.world_seed,
        &ident.scenario_sha256,
        &BTreeMap::from([("p1".to_string(), "main".to_string())]),
    );
    json!({
        "engine": {"world": {"time": tick, "seed": ident.world_seed}},
        "metadata": {"service": meta},
    })
}

fn write_raw(dir: &Path, seq: u64, bytes: &[u8]) {
    std::fs::write(dir.join(checkpoint_file_name(seq)), bytes).unwrap();
}

fn write_synthetic(dir: &Path, seq: u64, tick: u64, ident: &ServiceIdentity) {
    kk_storage::publish_json(
        &dir.join(checkpoint_file_name(seq)),
        SPEC,
        &synthetic(seq, tick, ident),
        |_| Ok(()),
    )
    .unwrap();
}

#[test]
fn sequence_allocation_never_reuses_published_names() -> anyhow::Result<()> {
    let dir = temp_dir("seq")?;
    let ident = sample_identity();
    assert_eq!(next_publish_seq(&dir)?, 1);
    write_synthetic(&dir, 1, 5, &ident);
    write_synthetic(&dir, 2, 9, &ident);
    assert_eq!(next_publish_seq(&dir)?, 3);
    // 损坏份占用的序号同样不复用：回滚到旧版后下一个序号仍接目录实况。
    write_raw(&dir, 5, b"{torn");
    assert_eq!(next_publish_seq(&dir)?, 6);
    // 非规范命名不参与序号分配。
    std::fs::write(dir.join("cp-99.jsonx"), b"noise")?;
    std::fs::write(dir.join("not-a-checkpoint.json"), b"noise")?;
    assert_eq!(next_publish_seq(&dir)?, 6);
    // 手工放置 u64 上限序号：分配显式报错而不是回卷复用。
    std::fs::write(dir.join(checkpoint_file_name(u64::MAX)), b"noise")?;
    assert!(next_publish_seq(&dir).is_err());
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn selection_skips_broken_but_stops_on_identity_and_format_errors() -> anyhow::Result<()> {
    let dir = temp_dir("select")?;
    let ident = sample_identity();
    write_synthetic(&dir, 1, 1, &ident);
    write_synthetic(&dir, 2, 2, &ident);
    // 最新份撕裂 → 跳过选次新。
    write_raw(&dir, 3, b"{\"format\":");
    let selected = select_latest(&dir, &ident)?;
    assert_eq!((selected.publish_seq, selected.meta.tick), (2, 2));
    assert_eq!(selected.skipped_broken, 1);

    // 哈希篡改（信封可解析但校验值不符）→ 跳过。
    let mut envelope = json!({
        "format": SPEC.format, "version": SPEC.version,
        "sha256": "0".repeat(64),
        "payload": serde_json::to_string(&synthetic(4, 4, &ident))?,
    });
    write_raw(&dir, 4, &serde_json::to_vec(&envelope)?);
    let selected = select_latest(&dir, &ident)?;
    assert_eq!(selected.publish_seq, 2);
    assert_eq!(selected.skipped_broken, 2);

    // 未来版本/异格式/未知字段信封 → 停止（不能以坏文件为由退到旧规则版本）。
    envelope["version"] = json!(SPEC.version + 1);
    write_raw(&dir, 4, &serde_json::to_vec(&envelope)?);
    assert!(select_latest(&dir, &ident).is_err());
    envelope["version"] = json!(SPEC.version);
    envelope["format"] = json!("other-format");
    write_raw(&dir, 4, &serde_json::to_vec(&envelope)?);
    assert!(select_latest(&dir, &ident).is_err());
    let mut future_field = envelope;
    future_field["format"] = json!(SPEC.format);
    future_field["compression"] = json!("zstd");
    write_raw(&dir, 4, &serde_json::to_vec(&future_field)?);
    assert!(select_latest(&dir, &ident).is_err());

    // 恢复正常最新份后逐一验证服务身份门。
    write_synthetic(&dir, 4, 4, &ident);
    let selected = select_latest(&dir, &ident)?;
    assert_eq!(selected.publish_seq, 4);
    assert_eq!(selected.skipped_broken, 0);
    // world_id 不一致（同目录混入另一世界）→ 停止。
    let mut foreign = sample_identity();
    foreign.world_seed = ident.world_seed; // 仅 world_id 不同。
    write_synthetic(&dir, 5, 5, &foreign);
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // world_seed 与目录身份不一致 → 停止。
    let mut other_seed = sample_identity();
    other_seed.world_id = ident.world_id.clone();
    other_seed.scenario_sha256 = ident.scenario_sha256.clone();
    other_seed.world_seed = ident.world_seed + 1;
    write_synthetic(&dir, 5, 5, &other_seed);
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // scenario_sha256 与目录身份不一致 → 停止。
    let mut other_scene = sample_identity();
    other_scene.world_id = ident.world_id.clone();
    other_scene.world_seed = ident.world_seed;
    other_scene.scenario_sha256 = "c".repeat(64);
    write_synthetic(&dir, 5, 5, &other_scene);
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // 引擎载荷种子与服务元数据不一致（重哈希的篡改载荷）→ 停止。
    let mut tampered = synthetic(5, 5, &ident);
    tampered["engine"]["world"]["seed"] = json!(ident.world_seed + 1);
    kk_storage::publish_json(&dir.join(checkpoint_file_name(5)), SPEC, &tampered, |_| {
        Ok(())
    })?;
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // 服务格式版本与目录身份不一致 → 停止。
    let mut future_format = synthetic(5, 5, &ident);
    future_format["metadata"]["service"]["service_format_version"] = json!(2);
    kk_storage::publish_json(
        &dir.join(checkpoint_file_name(5)),
        SPEC,
        &future_format,
        |_| Ok(()),
    )?;
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // 载荷序号与文件名不一致 → 停止。
    kk_storage::publish_json(
        &dir.join(checkpoint_file_name(5)),
        SPEC,
        &synthetic(3, 5, &ident),
        |_| Ok(()),
    )?;
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    // 服务元数据缺失（外部放置的普通检查点）→ 停止。
    kk_storage::publish_json(
        &dir.join(checkpoint_file_name(5)),
        SPEC,
        &json!({"engine": {"world": {"time": 5, "seed": ident.world_seed}}}),
        |_| Ok(()),
    )?;
    assert!(select_latest(&dir, &ident).is_err());
    std::fs::remove_file(dir.join(checkpoint_file_name(5)))?;
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn selection_fails_when_no_complete_version_remains() -> anyhow::Result<()> {
    let dir = temp_dir("all-broken")?;
    let ident = sample_identity();
    write_raw(&dir, 1, b"garbage");
    write_raw(&dir, 2, b"{\"format\":");
    let error = match select_latest(&dir, &ident) {
        Err(error) => error.to_string(),
        Ok(_) => anyhow::bail!("全部损坏时应报错"),
    };
    assert!(error.contains("2 份"), "报错应如实计入跳过份数: {error}");
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

#[test]
fn keep_retains_newest_complete_versions_and_protected() -> anyhow::Result<()> {
    let dir = temp_dir("keep")?;
    let ident = sample_identity();
    for seq in 1..=5 {
        write_synthetic(&dir, seq, seq, &ident);
    }
    let report = enforce_keep(&dir, 3, &[])?;
    assert_eq!(report.deleted, vec![1, 2]);
    let remaining: Vec<u64> = list_seqs(&dir)?.into_iter().collect();
    assert_eq!(remaining, vec![3, 4, 5]);
    // 非规范命名文件不被保留策略删除。
    std::fs::write(dir.join("cp-1.jsonx"), b"noise")?;
    let report = enforce_keep(&dir, 1, &[])?;
    assert_eq!(report.deleted, vec![3, 4]);
    assert!(dir.join("cp-1.jsonx").exists());
    std::fs::remove_file(dir.join("cp-1.jsonx"))?;
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// 损坏份不占保留名额；恢复基线（降级选中的最旧完整版）不被自己删除。
#[test]
fn keep_counts_complete_versions_and_preserves_recovery_baseline() -> anyhow::Result<()> {
    let dir = temp_dir("keep-broken")?;
    let ident = sample_identity();
    // 基线 cp-1 完整，cp-2..4 撕裂；完整版本数不足 keep 时不删任何份。
    write_synthetic(&dir, 1, 1, &ident);
    for seq in 2..=4 {
        write_raw(&dir, seq, b"{torn");
    }
    assert_eq!(enforce_keep(&dir, 3, &[])?.deleted, Vec::<u64>::new());
    write_synthetic(&dir, 5, 5, &ident);
    assert_eq!(enforce_keep(&dir, 3, &[])?.deleted, Vec::<u64>::new());
    write_synthetic(&dir, 6, 6, &ident);
    write_synthetic(&dir, 7, 7, &ident);
    // 窗口=最近 3 个完整版 {5,6,7}；严格早于窗口下界 5 的名字（含撕裂
    // 2..4 与完整 1）全部出窗删除——释放的序号都小于窗口下界，不复用。
    let report = enforce_keep(&dir, 3, &[])?;
    assert_eq!(report.deleted, vec![1, 2, 3, 4]);
    let remaining: Vec<u64> = list_seqs(&dir)?.into_iter().collect();
    assert_eq!(remaining, vec![5, 6, 7]);
    assert_eq!(next_publish_seq(&dir)?, 8);
    // 再发布两份使窗口前移到 {7,8,9}：早于 7 的完整 5/6 出窗删除。
    write_synthetic(&dir, 8, 8, &ident);
    write_synthetic(&dir, 9, 9, &ident);
    let report = enforce_keep(&dir, 3, &[])?;
    assert_eq!(report.deleted, vec![5, 6]);
    let remaining: Vec<u64> = list_seqs(&dir)?.into_iter().collect();
    assert_eq!(remaining, vec![7, 8, 9]);
    // 保护集不占用保留名额且恒保留（含撕裂份也不删）。
    write_raw(&dir, 6, b"{torn");
    let report = enforce_keep(&dir, 2, &[6])?;
    assert_eq!(report.deleted, vec![7]);
    assert!(dir.join(checkpoint_file_name(6)).exists());
    let _ = report;
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}

/// 真实引擎链路：发布带服务元数据的检查点 → 选择 → 替代引擎恢复成功。
#[test]
fn publish_select_restore_roundtrip_with_real_engine() -> anyhow::Result<()> {
    let dir = temp_dir("real")?;
    let ident = sample_identity();
    let entries = BTreeMap::from([("p1".to_string(), "main".to_string())]);
    let mut engine = crate::EmbedEngine::with_world_seed(ident.world_seed);
    engine.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}],
    }))?;
    let mut table = kk_runner::ModuleTable::new();
    table.insert_js(
        "main",
        "exports.loop = () => { Memory.n = (Memory.n || 0) + 1; };",
    );
    engine.set_player_modules("p1", table)?;
    engine.run_players(&BTreeMap::from([(
        "p1".to_string(),
        "(() => { require('main').loop(); return true; })()".to_string(),
    )]))?;
    let tick = engine.get_game_time();
    publish(&engine, &dir, next_publish_seq(&dir)?, &ident, &entries)?;
    // 序号不紧接目录实况时拒绝，防止回滚后复用。
    assert!(publish(&engine, &dir, 1, &ident, &entries).is_err());

    let selected = select_latest(&dir, &ident)?;
    assert_eq!(selected.meta.tick, tick);
    assert_eq!(selected.meta.entries, entries);
    assert_eq!(selected.meta.world_seed, ident.world_seed);

    let mut restored = crate::EmbedEngine::new();
    restored.restore_checkpoint(selected.value, None)?;
    assert_eq!(restored.get_game_time(), tick);
    assert_eq!(restored.world_seed(), ident.world_seed);
    assert_eq!(
        restored.player_state("p1").unwrap().raw_memory,
        engine.player_state("p1").unwrap().raw_memory
    );
    // 恢复后的目录续存：下一序号紧接已发布版本，不因重新恢复而回卷。
    assert_eq!(next_publish_seq(&dir)?, 2);
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
