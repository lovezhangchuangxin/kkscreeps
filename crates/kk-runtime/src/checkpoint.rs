//! 单文件检查点发布：完整载荷与校验值同版写入，落盘后原子替换旧文件。
//! 只接受显式版本，不把诊断投影、VM 堆或失败轮候选当作持久状态。
use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const CHECKPOINT_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_BYTES: u64 = 256 * 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    sha256: String,
    payload: String,
}

/// 管理入口读取未执行任何玩家/Mod 代码的载荷，缺包时亦可用于只读导出。
pub fn read_checkpoint(path: &Path) -> anyhow::Result<serde_json::Value> {
    let file = std::fs::File::open(path).context("打开检查点")?;
    let mut bytes = Vec::new();
    file.take(MAX_CHECKPOINT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CHECKPOINT_BYTES,
        "检查点超过 256 MiB 限额"
    );
    let envelope: Envelope = serde_json::from_slice(&bytes).context("解析检查点信封")?;
    anyhow::ensure!(
        envelope.format == "kkscreeps-checkpoint" && envelope.version == CHECKPOINT_VERSION,
        "不支持的检查点格式/版本 {}",
        envelope.version
    );
    anyhow::ensure!(
        envelope.sha256 == format!("{:x}", Sha256::digest(envelope.payload.as_bytes())),
        "检查点校验值不匹配"
    );
    serde_json::from_str(&envelope.payload).context("解析检查点载荷")
}

pub(crate) fn write_checkpoint(path: &Path, payload: &serde_json::Value) -> anyhow::Result<()> {
    publish(path, payload, |_| Ok(()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublishStage {
    Written,
    Synced,
    Replaced,
}

fn publish(
    path: &Path,
    payload: &serde_json::Value,
    mut stage: impl FnMut(PublishStage) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let payload = serde_json::to_string(payload)?;
    let envelope = Envelope {
        format: "kkscreeps-checkpoint".into(),
        version: CHECKPOINT_VERSION,
        sha256: format!("{:x}", Sha256::digest(payload.as_bytes())),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_CHECKPOINT_BYTES,
        "检查点超过 256 MiB 限额"
    );
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // 目录须事先存在，避免把多层目录的耐久性误算作一次父目录 fsync。
    let name = path
        .file_name()
        .context("检查点路径必须指向文件")?
        .to_string_lossy();
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
        std::fs::File::open(parent)?
            .sync_all()
            .context("检查点已替换，但父目录同步失败；发布耐久性未确认")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[ignore = "仅由 abrupt_process_exit_recovers_a_complete_version 子进程调用"]
    fn publication_crash_child() -> anyhow::Result<()> {
        let path = std::env::var("KK_CHECKPOINT_CRASH_PATH")?;
        let stop = std::env::var("KK_CHECKPOINT_CRASH_STAGE")?;
        publish(
            Path::new(&path),
            &json!({"tick": 8, "memory": 8}),
            |stage| {
                if format!("{stage:?}") == stop {
                    // exit 不执行 Rust Drop 或临时文件清理，模拟进程在发布中消失。
                    std::process::exit(23);
                }
                Ok(())
            },
        )
    }

    #[test]
    fn abrupt_process_exit_recovers_a_complete_version() -> anyhow::Result<()> {
        let directory =
            std::env::temp_dir().join(format!("kk-checkpoint-crash-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("state.json");
        for stop in [
            PublishStage::Written,
            PublishStage::Synced,
            PublishStage::Replaced,
        ] {
            write_checkpoint(&path, &json!({"tick": 7, "memory": 7}))?;
            let output = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--ignored",
                    "--exact",
                    "checkpoint::tests::publication_crash_child",
                ])
                .env("KK_CHECKPOINT_CRASH_PATH", &path)
                .env("KK_CHECKPOINT_CRASH_STAGE", format!("{stop:?}"))
                .output()?;
            assert_eq!(
                output.status.code(),
                Some(23),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let selected = if stop == PublishStage::Replaced { 8 } else { 7 };
            assert_eq!(
                read_checkpoint(&path)?,
                json!({"tick": selected, "memory": selected})
            );
        }
        assert!(
            std::fs::read_dir(&directory)?.count() > 1,
            "退出遗留临时文件不能影响完整版本选择"
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn interruption_selects_only_a_complete_version() -> anyhow::Result<()> {
        let directory =
            std::env::temp_dir().join(format!("kk-checkpoint-publish-{}", std::process::id()));
        std::fs::create_dir_all(&directory)?;
        let path = directory.join("state.json");
        for stop in [
            PublishStage::Written,
            PublishStage::Synced,
            PublishStage::Replaced,
        ] {
            write_checkpoint(&path, &json!({"tick": 7, "memory": 7}))?;
            assert!(
                publish(&path, &json!({"tick": 8, "memory": 8}), |at| {
                    if at == stop {
                        anyhow::bail!("模拟发布中断")
                    }
                    Ok(())
                })
                .is_err()
            );
            let selected = if stop == PublishStage::Replaced { 8 } else { 7 };
            assert_eq!(
                read_checkpoint(&path)?,
                json!({"tick": selected, "memory": selected})
            );
        }
        std::fs::write(&path, b"{\"format\":")?;
        assert!(read_checkpoint(&path).is_err());
        write_checkpoint(&path, &json!({"tick": 7}))?;
        let mut envelope: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        envelope["payload"] = json!("{}");
        std::fs::write(&path, serde_json::to_vec(&envelope)?)?;
        assert!(
            read_checkpoint(&path)
                .unwrap_err()
                .to_string()
                .contains("校验")
        );
        envelope["version"] = json!(2);
        std::fs::write(&path, serde_json::to_vec(&envelope)?)?;
        assert!(
            read_checkpoint(&path)
                .unwrap_err()
                .to_string()
                .contains("版本")
        );
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
