//! 单文件检查点发布：完整载荷与校验值同版写入，落盘后原子替换旧文件。
//! 只接受显式版本，不把诊断投影、VM 堆或失败轮候选当作持久状态。
//!
//! 发布与信封机制位于 kk-storage；本模块保留检查点线格式常量与公共
//! 读取入口，服务目录的选择器经 [`SPEC`] 复用同一错误分类。
use kk_storage::envelope::EnvelopeSpec;
use serde_json::Value;
use std::path::Path;

pub const CHECKPOINT_VERSION: u32 = 1;
pub const MAX_CHECKPOINT_BYTES: u64 = 256 * 1024 * 1024;

/// 检查点信封规格：格式名、线格式版本与 256 MiB 单文件上限。
pub const SPEC: EnvelopeSpec = EnvelopeSpec {
    format: "kkscreeps-checkpoint",
    version: CHECKPOINT_VERSION,
    max_bytes: MAX_CHECKPOINT_BYTES,
};

/// 管理入口读取未执行任何玩家/Mod 代码的载荷，缺包时亦可用于只读导出。
pub fn read_checkpoint(path: &Path) -> anyhow::Result<Value> {
    kk_storage::envelope::read_json(path, SPEC)
        .map_err(|error| anyhow::anyhow!("读取检查点 {}: {error}", path.display()))
}

pub(crate) fn write_checkpoint(path: &Path, payload: &Value) -> anyhow::Result<()> {
    kk_storage::publish_json(path, SPEC, payload, |_| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_storage::publish::{self as storage_publish, PublishStage};
    use serde_json::json;

    #[test]
    #[ignore = "仅由 abrupt_process_exit_recovers_a_complete_version 子进程调用"]
    fn publication_crash_child() -> anyhow::Result<()> {
        let path = std::env::var("KK_CHECKPOINT_CRASH_PATH")?;
        let stop = std::env::var("KK_CHECKPOINT_CRASH_STAGE")?;
        storage_publish::publish(
            Path::new(&path),
            SPEC,
            &serde_json::to_string(&json!({"tick": 8, "memory": 8}))?,
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
                storage_publish::publish(
                    Path::new(&path),
                    SPEC,
                    &serde_json::to_string(&json!({"tick": 8, "memory": 8}))?,
                    |at| {
                        if at == stop {
                            anyhow::bail!("模拟发布中断")
                        }
                        Ok(())
                    },
                )
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
