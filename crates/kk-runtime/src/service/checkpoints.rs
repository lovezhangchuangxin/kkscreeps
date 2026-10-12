//! 服务检查点：命名、发布序号分配、无日志恢复选择与保留策略。
//!
//! 文件名按发布序号（`cp-<publishSeq>.json`）而非游戏 tick 排序，为同
//! tick 的代码边界事务预留空间；序号由目录已发布版本核对后分配，不因
//! 回滚恢复重复使用。恢复先验目录/世界身份，再按序号降序逐份校验：
//! 撕裂/哈希损坏记录并跳过，I/O 故障、未来格式或身份不匹配必须停止，
//! 不以“坏文件”为由静默退到另一规则版本。
use super::identity::ServiceIdentity;
use super::metadata::{self, ServiceCheckpointMeta};
use crate::EmbedEngine;
use crate::checkpoint::SPEC;
use anyhow::Context;
use kk_storage::ReadError;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 规范文件名（序号定宽，便于目录清单目视排序；解析按数值）。
pub fn checkpoint_file_name(seq: u64) -> String {
    format!("cp-{seq:012}.json")
}

/// 解析规范命名文件中的发布序号；非规范命名返回 None（选择器与保留
/// 策略都忽略它们，既不删除也不计入坏份）。
pub fn parse_checkpoint_name(name: &str) -> Option<u64> {
    let seq = name.strip_prefix("cp-")?.strip_suffix(".json")?;
    let seq: u64 = seq.parse().ok()?;
    (name == checkpoint_file_name(seq)).then_some(seq)
}

/// 目录中规范命名的发布序号集合（含损坏份：序号已占用即不得复用）。
/// 目录枚举错误按 I/O 故障上抛——静默缩小清点面会让序号分配/恢复选择
/// 在残缺视图上工作（可能覆盖既有检查点或伪装成干净恢复）。
pub fn list_seqs(dir: &Path) -> anyhow::Result<BTreeSet<u64>> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("清点服务检查点目录 {}", dir.display()))?;
    let mut seqs = BTreeSet::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("清点目录项 {}", dir.display()))?;
        // 非 UTF-8 文件名不可能是规范命名，忽略即可（不参与任何策略）。
        if let Ok(name) = entry.file_name().into_string()
            && let Some(seq) = parse_checkpoint_name(&name)
        {
            seqs.insert(seq);
        }
    }
    Ok(seqs)
}

/// 下一个发布序号：目录最大序号 +1（空目录为 1）。回滚到旧版后续存
/// 仍从目录实况分配，不重复使用已出现的序号。
pub fn next_publish_seq(dir: &Path) -> anyhow::Result<u64> {
    match list_seqs(dir)?.last() {
        None => Ok(1),
        Some(&seq) => seq
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("发布序号已达 u64 上限，目录不可继续发布")),
    }
}

/// 到点发布：以当前引擎已提交状态构造服务元数据并写出 `cp-<seq>.json`。
/// 要求无开放轮次（`EmbedEngine::save_checkpoint` 自带门）；序号必须紧接
/// 目录已发布版本，防止调用方在回滚后复用序号。发布失败时旧版本文件
/// 不被替换（rename 前失败）；rename 后父目录同步失败属“耐久未知”——
/// 目标文件可能已是新版本，调用方须按持久化故障处置，不能当作可原地
/// 重试的干净失败。
pub fn publish(
    engine: &EmbedEngine,
    dir: &Path,
    seq: u64,
    identity: &ServiceIdentity,
    entries: &BTreeMap<String, String>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        seq == next_publish_seq(dir)?,
        "发布序号 {seq} 不紧接目录已发布版本；须由 next_publish_seq 分配"
    );
    let meta = metadata::new_meta(
        &identity.world_id,
        seq,
        engine.get_game_time(),
        engine.world_seed(),
        &identity.scenario_sha256,
        entries,
    );
    publish_with_meta(engine, dir, &meta)
}

/// 按调用方给定的完整服务元数据发布（v2 屏障/普通检查点走此入口）；
/// 序号紧接性在此统一把关，元数据字段值由调用方负责。
pub fn publish_with_meta(
    engine: &EmbedEngine,
    dir: &Path,
    meta: &super::metadata::ServiceCheckpointMeta,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        meta.publish_seq == next_publish_seq(dir)?,
        "发布序号 {} 不紧接目录已发布版本；须由 next_publish_seq 分配",
        meta.publish_seq
    );
    let path = dir.join(checkpoint_file_name(meta.publish_seq));
    engine
        .save_checkpoint(&path, metadata::engine_metadata(meta))
        .with_context(|| format!("发布服务检查点 {}", path.display()))
}

/// 恢复选择的产物；`value` 为完整检查点载荷，宿主随后在替代引擎上核验
/// 引擎/Mod 身份并切换（restore 失败按停止处置，不改原文件）。
#[derive(Debug)]
pub struct Selected {
    pub value: serde_json::Value,
    pub file: PathBuf,
    pub publish_seq: u64,
    pub meta: ServiceCheckpointMeta,
    /// 因撕裂/哈希损坏跳过的份数（不含身份/格式硬错误——那些直接中止）。
    pub skipped_broken: usize,
    /// 因不满足恢复下限（v2 目录中的 v1 旧份或锚点约束过滤）跳过的份数。
    pub skipped_ineligible: usize,
}

/// 恢复下限（effects 锚点）：恢复选择只允许锚点本身或携带同一屏障
/// 身份、命令消费水位不低于下限的后继完整版本；无满足者即停服条件。
#[derive(Clone, Debug)]
pub struct RecoveryFloor {
    pub checkpoint_seq: u64,
    pub barrier_id: String,
    pub inbox_applied: BTreeMap<String, u64>,
}

/// 无日志恢复选择：按发布序号降序取首个完整且身份一致的服务检查点。
/// 服务身份逐项核对目录身份与引擎载荷（文件名不是身份真相源）。
/// S1 无外部耐久确认，允许退到较旧完整版本（丢失窗口如实上报）。
pub fn select_latest(
    dir: &Path,
    identity: &ServiceIdentity,
    floor: Option<&RecoveryFloor>,
) -> anyhow::Result<Selected> {
    let mut seqs: Vec<u64> = list_seqs(dir)?.into_iter().collect();
    seqs.sort_unstable();
    seqs.reverse();
    anyhow::ensure!(
        !seqs.is_empty(),
        "服务检查点目录 {} 没有规范命名的文件",
        dir.display()
    );
    let mut skipped = 0usize;
    let mut skipped_ineligible = 0usize;
    for seq in seqs {
        let file = dir.join(checkpoint_file_name(seq));
        let value = match kk_storage::envelope::read_json(&file, SPEC) {
            Ok(value) => value,
            // 撕裂/哈希损坏：记录并跳过次新。
            Err(ReadError::Broken(_)) => {
                skipped += 1;
                continue;
            }
            // I/O 故障、超限、未来格式/版本或不可识别结构：无法判断语义，
            // 必须停止。
            Err(ReadError::Fatal(error)) => {
                anyhow::bail!("候选 {} 选择中止: {error}", file.display())
            }
        };
        let meta = metadata::decode(&value)
            .with_context(|| format!("候选 {} 服务元数据核验", file.display()))?;
        anyhow::ensure!(
            meta.publish_seq == seq,
            "候选 {} 的载荷序号 {} 与文件名不一致",
            file.display(),
            meta.publish_seq
        );
        anyhow::ensure!(
            meta.world_id == identity.world_id,
            "候选 {} 的 world_id 与目录身份不一致（{} ≠ {}）；同目录不能混放两个世界",
            file.display(),
            meta.world_id,
            identity.world_id
        );
        anyhow::ensure!(
            meta.world_seed == identity.world_seed,
            "候选 {} 的世界种子与目录身份不一致",
            file.display()
        );
        anyhow::ensure!(
            meta.scenario_sha256 == identity.scenario_sha256,
            "候选 {} 的种子内容身份与目录身份不一致",
            file.display()
        );
        if meta.service_format_version != identity.service_format_version {
            // v2 目录中启用线之前的 v1 旧份：不可选（锚点约束天然排
            // 除），跳过继续找；未来版本（含不完整的伪形状）是必须停止
            // 的外来内容，不能当旧份静默跳过。
            if floor.is_some() && meta.service_format_version == super::identity::SERVICE_FORMAT_V1
            {
                skipped_ineligible += 1;
                continue;
            }
            anyhow::bail!(
                "候选 {} 的服务格式版本 {} 与目录身份 {} 不一致",
                file.display(),
                meta.service_format_version,
                identity.service_format_version
            );
        }
        if let Some(floor) = floor {
            let eligible = meta.is_v2_complete()
                && meta.publish_seq >= floor.checkpoint_seq
                && meta.barrier_id.as_deref() == Some(floor.barrier_id.as_str())
                && floor.inbox_applied.iter().all(|(stream, applied)| {
                    meta.inbox_applied
                        .as_ref()
                        .and_then(|applied_map| applied_map.get(stream))
                        .copied()
                        .unwrap_or(0)
                        >= *applied
                });
            if !eligible {
                skipped_ineligible += 1;
                continue;
            }
        }
        let tick = value
            .pointer("/engine/world/time")
            .and_then(|v| v.as_u64())
            .context("检查点载荷缺少世界时间")?;
        anyhow::ensure!(
            tick == meta.tick,
            "候选 {} 的服务元数据 tick {} 与引擎载荷 tick {tick} 不一致",
            file.display(),
            meta.tick
        );
        // 引擎实际使用的种子是载荷内的值（restore 收养它）；冗余的服务
        // 元数据种子必须与之一致，防止重哈希载荷带不同随机流域通过外层门。
        let seed = value
            .pointer("/engine/world/seed")
            .and_then(|v| v.as_u64())
            .context("检查点载荷缺少世界种子")?;
        anyhow::ensure!(
            seed == meta.world_seed,
            "候选 {} 的服务元数据种子 {} 与引擎载荷种子 {seed} 不一致",
            file.display(),
            meta.world_seed
        );
        return Ok(Selected {
            value,
            file,
            publish_seq: seq,
            meta,
            skipped_broken: skipped,
            skipped_ineligible,
        });
    }
    if skipped_ineligible > 0 {
        anyhow::bail!(
            "目录存在 {skipped} 份损坏与 {skipped_ineligible} 份不满足恢复下限的候选，没有锚点及之后的完整版本；锚点检查点须完整保留，不能回退到锚点之前（停服处置）"
        );
    }
    anyhow::bail!(
        "服务检查点目录全部 {skipped} 份候选损坏（撕裂/校验值不匹配），没有可恢复的完整版本"
    )
}

/// 保留策略执行结果；单份删除失败不致命（残留旧文件不影响正确性）。
#[derive(Default)]
pub struct KeepReport {
    pub deleted: Vec<u64>,
    pub failed: Vec<String>,
}

/// 只在新版本耐久成功之后调用：保留最近 `keep` 个**完整版本**（信封校
/// 验可读）与显式保护集（S2 的恢复锚点）的并集——保护集不占用 `keep`
/// 名额，损坏份不占名额但也不在窗口内删除（删除会释放其占位序号）。
/// 只删除严格早于保留窗最旧完整版本且不在保护集内的名字；目录中没有
/// 任何完整版本时不清理（保诊断现场）。keep=0 表示不清理。
pub fn enforce_keep(dir: &Path, keep: u32, protected: &[u64]) -> anyhow::Result<KeepReport> {
    if keep == 0 {
        return Ok(KeepReport::default());
    }
    let mut seqs: Vec<u64> = list_seqs(dir)?.into_iter().collect();
    seqs.sort_unstable();
    let protected: BTreeSet<u64> = protected.iter().copied().collect();
    // 自新到旧找 keep 个完整版本；损坏份跳过不计数。
    let mut complete = 0u32;
    let mut window_min: Option<u64> = None;
    for &seq in seqs.iter().rev() {
        if complete >= keep {
            break;
        }
        if protected.contains(&seq) {
            continue;
        }
        let file = dir.join(checkpoint_file_name(seq));
        if kk_storage::envelope::read_json(&file, SPEC).is_ok() {
            complete += 1;
            window_min = Some(seq);
        }
    }
    let mut report = KeepReport::default();
    if let Some(window_min) = window_min {
        for seq in &seqs {
            if *seq >= window_min || protected.contains(seq) {
                continue;
            }
            let file = dir.join(checkpoint_file_name(*seq));
            match std::fs::remove_file(&file) {
                Ok(()) => report.deleted.push(*seq),
                Err(error) => report
                    .failed
                    .push(format!("删除 {}: {error}", file.display())),
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests;
