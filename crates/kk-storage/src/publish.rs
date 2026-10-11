//! 原子文件发布：同目录新临时文件写入、同步、rename 与父目录同步。
//!
//! 发布成功返回前父目录已同步（rename 的目录项已耐久）；任一阶段失败
//! 清理临时文件并返回错误。rename 后的父目录同步失败归为耐久未知，
//! 由调用方按持久化故障处置，不能假设旧文件一定还在而盲目重试。
use crate::envelope::EnvelopeSpec;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Serialize)]
struct Envelope<'a> {
    format: &'a str,
    version: u32,
    sha256: String,
    payload: &'a str,
}

/// 发布推进的阶段；阶段回调用于故障注入测试（进程退出或模拟中断）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishStage {
    /// 临时文件已完整写入（尚未同步）。
    Written,
    /// 临时文件已同步（尚未 rename）。
    Synced,
    /// 已 rename 到目标名（父目录尚未同步）。
    Replaced,
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 以信封发布 JSON 载荷：校验值按载荷字符串字节计算，读取方按同规格
/// 整体校验。发布本身不改写已存在的目标之外的任何文件。
pub fn publish_json(
    path: &Path,
    spec: EnvelopeSpec,
    payload: &Value,
    stage: impl FnMut(PublishStage) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    publish(path, spec, &serde_json::to_string(payload)?, stage)
}

/// 发布原始载荷字符串（信封内 payload 即该字符串）。
pub fn publish(
    path: &Path,
    spec: EnvelopeSpec,
    payload: &str,
    mut stage: impl FnMut(PublishStage) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let envelope = Envelope {
        format: spec.format,
        version: spec.version,
        sha256: format!("{:x}", Sha256::digest(payload.as_bytes())),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    anyhow::ensure!(
        bytes.len() as u64 <= spec.max_bytes,
        "发布载荷超过 {} 字节上限",
        spec.max_bytes
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // 目录须事先存在，避免把多层目录的耐久性误算作一次父目录 fsync。
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow::anyhow!("发布路径必须指向文件: {}", path.display()))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        stage(PublishStage::Written)?;
        file.sync_all()?;
        stage(PublishStage::Synced)?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        stage(PublishStage::Replaced)?;
        std::fs::File::open(parent)?.sync_all().map_err(|error| {
            anyhow::anyhow!("已替换，但父目录同步失败（{error}）；发布耐久性未确认")
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// 清点并删除发布协议临时命名（`.<name>.<pid>.<seq>.tmp`）的残留文件。
/// 只能在持有该目录单写者租约时调用——否则可能删除并发发布者的临时
/// 文件。单个文件删除失败只计入 `failed`，不阻塞其余清理。
pub fn remove_stale_temporaries(dir: &Path) -> anyhow::Result<StaleCleanup> {
    let mut cleanup = StaleCleanup::default();
    for entry in std::fs::read_dir(dir)
        .map_err(|error| anyhow::anyhow!("清点目录 {}: {error}", dir.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                cleanup.failed.push(format!("枚举目录项: {error}"));
                continue;
            }
        };
        let name = entry.file_name().into_string().unwrap_or_default();
        if !is_temporary_name(&name) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => cleanup.removed += 1,
            Err(error) => cleanup.failed.push(format!(
                "删除 {name}: {error}（残留临时文件不影响选择，可稍后重试）"
            )),
        }
    }
    Ok(cleanup)
}

/// 临时文件清点结果：`failed` 中的失败不构成发布或恢复的硬错误。
#[derive(Default)]
pub struct StaleCleanup {
    pub removed: usize,
    pub failed: Vec<String>,
}

/// 判定 `.<name>.<pid>.<seq>.tmp` 形态：以点起头、`.tmp` 结尾，且去掉
/// 两端后至少有三段、末两段为非空纯数字（进程号与序号）。目录初始化
/// 校验用它区分“发布协议崩溃残留（可清理后重试）”与“来源不明内容”。
pub fn is_temporary_name(name: &str) -> bool {
    let Some(stripped) = name.strip_prefix('.').and_then(|n| n.strip_suffix(".tmp")) else {
        return false;
    };
    let parts: Vec<&str> = stripped.split('.').collect();
    let digits = |part: &str| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit());
    parts.len() >= 3 && digits(parts[parts.len() - 1]) && digits(parts[parts.len() - 2])
}

/// 打开目录并同步其目录项（新建目录的父目录同步由调用方组合本函数完成）。
pub fn sync_directory(dir: &Path) -> anyhow::Result<()> {
    std::fs::File::open(dir)?
        .sync_all()
        .map_err(|error| anyhow::anyhow!("同步目录 {}: {error}", dir.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::read_json;
    use std::path::PathBuf;

    const SPEC: EnvelopeSpec = EnvelopeSpec {
        format: "test-publish",
        version: 1,
        max_bytes: 4096,
    };

    #[test]
    fn interruption_leaves_previous_version_intact() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-publish-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        for stop in [
            PublishStage::Written,
            PublishStage::Synced,
            PublishStage::Replaced,
        ] {
            publish_json(&path, SPEC, &serde_json::json!({"v": 1}), |_| Ok(()))?;
            let outcome = publish_json(&path, SPEC, &serde_json::json!({"v": 2}), |at| {
                if at == stop {
                    anyhow::bail!("模拟发布中断")
                }
                Ok(())
            });
            assert!(outcome.is_err(), "应在 {stop:?} 阶段失败");
            let expected = if stop == PublishStage::Replaced { 2 } else { 1 };
            assert_eq!(read_json(&path, SPEC)?["v"], expected);
            // 失败路径清理临时文件（Replaced 后 rename 已完成，无临时残留）。
            if stop != PublishStage::Replaced {
                assert!(stale_temporaries(&dir).is_empty(), "中断后无临时残留");
            }
        }
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    fn stale_temporaries(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| is_temporary_name(&entry.file_name().into_string().unwrap_or_default()))
            .map(|entry| entry.path())
            .collect()
    }

    #[test]
    fn removes_only_protocol_temporaries() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-publish-stale-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".state.json.123.4.tmp"), b"torn")?;
        std::fs::write(dir.join(".cp-0001.json.9.12.tmp"), b"torn")?;
        std::fs::write(dir.join(".noise.tmp"), b"keep")?;
        std::fs::write(dir.join("plain.json"), b"keep")?;
        std::fs::write(dir.join(".hidden-not-tmp.1.2.txt"), b"keep")?;
        // 空数字段（`.` 连续出现）不是协议形态，不清理。
        std::fs::write(dir.join(".a.1..tmp"), b"keep")?;
        let cleanup = remove_stale_temporaries(&dir)?;
        assert_eq!(cleanup.removed, 2);
        assert!(cleanup.failed.is_empty());
        for kept in [
            ".noise.tmp",
            "plain.json",
            ".hidden-not-tmp.1.2.txt",
            ".a.1..tmp",
        ] {
            assert!(dir.join(kept).exists(), "{kept} 不应被清理");
        }
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
