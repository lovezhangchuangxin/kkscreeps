//! serve enable-inbox：v1→v2 显式启用耐久命令面（一次性、可重入续接）。
//!
//! 顺序不可绕过（目录租约内）：恢复 v1 最新完整份 → 发布初始 v2 屏障
//! 检查点（世界不变、仅元数据升级）→ 写空 inbox → 写初始 effects 锚点
//! → 最后原子重写 identity 为 v2（服务模式切换的完成边界）。中断后重跑
//! 即显式核验续接：已有 v2 形状检查点则补齐 inbox/effects 再切身份，
//! 不猜测缺失文件；inbox 已有内容（身份未切、无接收入口开启）时保留
//! 不覆盖。已启用（identity v2）的目录拒绝重复启用。注意：屏障检查点
//! 落盘后、identity 切换前中断的目录，serve run 会被 v2 形状份的身份门
//! 拒启（放行 v1 续跑会让续接把世界回滚到屏障、丢其后进度）——唯一
//! 恢复路径是重跑 enable-inbox 完成切换。
use anyhow::Context;
use kk_runtime::service::{self, ServiceDir, checkpoints::checkpoint_file_name};
use serde_json::json;
use std::path::Path;

pub(crate) fn enable_inbox(config_path: &Path) -> i32 {
    match enable_inner(config_path) {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(error) => {
            eprintln!("serve enable-inbox 失败: {error:#}");
            1
        }
    }
}

fn enable_inner(config_path: &Path) -> anyhow::Result<String> {
    let (config, state_dir) = service::config::ServeConfig::load(config_path)?;
    let dir = ServiceDir::new(&state_dir);
    dir.validate_for_run().context("服务目录不可用")?;
    let lease =
        kk_storage::WriterLease::acquire(&dir.writer_lock()).context("获取服务目录写者租约")?;
    // 身份读取在租约内：目录单写者后状态不再并发变化。
    let mut identity = service::identity::read(&dir.identity_file()).context("读取服务身份")?;
    anyhow::ensure!(
        identity.service_format_version == service::identity::SERVICE_FORMAT_V1,
        "目录已是 v{} 服务模式；enable-inbox 只用于 v1 目录的显式升级",
        identity.service_format_version
    );
    for clean_dir in [dir.root(), &dir.checkpoints_dir()] {
        let cleanup = kk_storage::publish::remove_stale_temporaries(clean_dir)?;
        for failure in cleanup.failed {
            eprintln!("serve enable-inbox: 残留临时文件清理失败（不阻塞）: {failure}");
        }
    }

    // 扫描已有 v2 形状检查点（上次启用中断在 identity 切换前的续接锚）。
    let existing_v2 = latest_v2_checkpoint(&dir, &identity)?;
    let (barrier_meta, resumed) = match existing_v2 {
        Some((seq, meta)) => (
            Some((seq, meta.barrier_id.clone().expect("v2 形状必有屏障"))),
            true,
        ),
        None => (None, false),
    };

    let (anchor_seq, barrier_id) = match barrier_meta {
        // 续接：沿用已发布的 v2 屏障检查点，不重复发布（世界与世界时间
        // 不变；重复发布会多占一个序号但语义等价，续接路径更严格）。
        Some((seq, barrier)) => (seq, barrier),
        None => {
            // 正常路径：恢复 v1 最新完整份，以同一世界状态发布 v2 屏障。
            let selected =
                service::checkpoints::select_latest(&dir.checkpoints_dir(), &identity, None)
                    .context("恢复最新完整检查点失败")?;
            let loaded = config
                .mods
                .as_ref()
                .map(|source| {
                    let input = crate::sim::mods::Mods {
                        root: source.root.clone(),
                        configs: source.configs.clone(),
                        selection: source.selection.clone(),
                    };
                    let base = config_path.parent().unwrap_or(Path::new("."));
                    crate::sim::mods::load(base, &input)
                })
                .transpose()
                .context("装载 Mod 包失败")?;
            let mods = loaded
                .as_ref()
                .map(|loaded| (&loaded.packages, &loaded.options));
            let mut engine = kk_runtime::EmbedEngine::new();
            engine
                .restore_checkpoint(selected.value, mods)
                .context("检查点恢复失败（引擎/Mod 身份核验）")?;
            let seq = service::checkpoints::next_publish_seq(&dir.checkpoints_dir())?;
            let barrier = format!("b-{}", service::identity::generate_world_id());
            let meta = service::metadata::new_meta_v2(
                &identity.world_id,
                seq,
                engine.get_game_time(),
                engine.world_seed(),
                &identity.scenario_sha256,
                &selected.meta.entries,
                &barrier,
                &Default::default(),
                &Default::default(),
                0,
            );
            service::checkpoints::publish_with_meta(&engine, &dir.checkpoints_dir(), &meta)
                .context("发布初始 v2 屏障检查点")?;
            (seq, barrier)
        }
    };

    // inbox：缺失写初始空态；存在但损坏是停服条件（撕裂/外来内容不能
    // 用重写消解——那等于猜测缺失）。
    if !dir.inbox_file().exists() {
        service::inbox::store(
            &service::inbox::InboxState::empty(&identity.world_id),
            &dir.inbox_file(),
        )
        .context("写入初始命令收件箱")?;
    } else {
        service::inbox::load(&dir.inbox_file(), &identity.world_id)
            .context("已有收件箱不合法；须人工核验")?;
    }
    // effects：锚点指向 v2 屏障检查点（seq 与屏障身份都对齐才保留，
    // 不重置已确认水位）；缺失写初始；存在但损坏/不符锚点即停服。
    let fresh_effects =
        service::effects::EffectsState::initial(&identity.world_id, anchor_seq, &barrier_id);
    if !dir.effects_file().exists() {
        service::effects::write(&dir.effects_file(), &fresh_effects).context("写入初始恢复锚点")?;
    } else {
        let existing = service::effects::load(&dir.effects_file(), &identity.world_id)
            .context("已有恢复锚点不合法；须人工核验")?;
        anyhow::ensure!(
            existing.anchor.checkpoint_seq == anchor_seq
                && existing.anchor.barrier_id == barrier_id,
            "已有恢复锚点（seq {} barrier {}）与启用屏障（seq {anchor_seq} barrier {barrier_id}）不符；须人工核验",
            existing.anchor.checkpoint_seq,
            existing.anchor.barrier_id
        );
    }

    // 完成边界：identity 最后原子切换（v2 目录自此要求 inbox/effects 齐）。
    identity.service_format_version = service::identity::SERVICE_FORMAT_V2;
    service::identity::write(&dir.identity_file(), &identity).context("切换目录服务模式（v2）")?;
    drop(lease);
    let report = json!({
        "command": "serve enable-inbox",
        "state_dir": dir.root(),
        "world_id": identity.world_id,
        "service_format_version": identity.service_format_version,
        "resumed": resumed,
        "barrier": {
            "id": barrier_id,
            "checkpoint_seq": anchor_seq,
            "file": dir.checkpoints_dir().join(checkpoint_file_name(anchor_seq)),
        },
    });
    Ok(serde_json::to_string_pretty(&report)?)
}

/// 扫描检查点目录中 v2 形状的最大序号份（启用中断的续接证据）。
fn latest_v2_checkpoint(
    dir: &ServiceDir,
    identity: &service::identity::ServiceIdentity,
) -> anyhow::Result<Option<(u64, service::metadata::ServiceCheckpointMeta)>> {
    let mut seqs: Vec<u64> = service::checkpoints::list_seqs(&dir.checkpoints_dir())?
        .into_iter()
        .collect();
    seqs.sort_unstable();
    seqs.reverse();
    for seq in seqs {
        let path = dir.checkpoints_dir().join(checkpoint_file_name(seq));
        let Ok(value) = kk_storage::envelope::read_json(&path, kk_runtime::checkpoint::SPEC) else {
            continue;
        };
        if let Ok(meta) = service::metadata::decode(&value)
            && meta.is_v2_complete()
        {
            // 与 select_latest 同口径的身份门：续接证据必须是本目录、
            // v2 版本、序号自洽的检查点；外来/篡改的 v2 形状文件是
            // 停服条件（不能猜测缺失文件，也不能采信外来内容）。
            anyhow::ensure!(
                meta.world_id == identity.world_id
                    && meta.service_format_version
                        == kk_runtime::service::identity::SERVICE_FORMAT_V2
                    && meta.publish_seq == seq,
                "目录含身份不符的 v2 形状检查点 {}（world_id/版本/序号不自洽）；须人工核验",
                path.display()
            );
            return Ok(Some((seq, meta)));
        }
    }
    Ok(None)
}
