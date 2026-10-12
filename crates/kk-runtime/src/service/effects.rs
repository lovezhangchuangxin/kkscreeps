//! 恢复下限锚点（effects.json v1）：独立于可回滚游戏进度的最新值。
//!
//! 记录当前恢复锚点（屏障检查点的身份与命令消费水位）和各 sink 的
//! 已确认水位（字段在 v1 定义、S2.4 起填值，避免后续升版）。锚点只随
//! 更高屏障的耐久确认单调前进；锚点文件损坏或身份不符必须停服——回退
//! 到旧锚点等于把已对外的确认当作未发生。本文件不随游戏检查点回滚。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// effects.json 线格式版本。
pub const EFFECTS_FORMAT_VERSION: u32 = 1;

const EFFECTS_SPEC: kk_storage::EnvelopeSpec = kk_storage::EnvelopeSpec {
    format: "kkscreeps-service-effects",
    version: EFFECTS_FORMAT_VERSION,
    max_bytes: 1024 * 1024,
};

/// 当前恢复锚点：恢复选择只允许锚点本身或其携带同一屏障身份、消费
/// 水位不低于下限的后继完整版本。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryAnchor {
    /// 锚点屏障检查点的发布序号。
    pub checkpoint_seq: u64,
    /// 屏障身份；后继普通检查点继承同值。
    pub barrier_id: String,
    /// 锚点确认的命令消费水位（streamId→已应用 clientSeq 上界）；
    /// 恢复候选的 inbox_applied 不得低于该值。
    pub inbox_applied: BTreeMap<String, u64>,
}

/// effects.json 载荷；读取方按严格格式整体解码并核对世界身份。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectsState {
    pub effects_format_version: u32,
    pub world_id: String,
    pub anchor: RecoveryAnchor,
    /// 各具名 sink 的已确认输出水位（outputId 上界）；S2.1 恒空，
    /// S2.4 起由投递确认推进。
    pub sinks: BTreeMap<String, u64>,
}

impl EffectsState {
    /// enable-inbox 时的初始锚点：指向初始屏障检查点（消费水位为空）。
    pub fn initial(world_id: &str, checkpoint_seq: u64, barrier_id: &str) -> Self {
        EffectsState {
            effects_format_version: EFFECTS_FORMAT_VERSION,
            world_id: world_id.to_string(),
            anchor: RecoveryAnchor {
                checkpoint_seq,
                barrier_id: barrier_id.to_string(),
                inbox_applied: BTreeMap::new(),
            },
            sinks: BTreeMap::new(),
        }
    }
}

/// 以发布协议原子写出 effects；锚点推进的最后一原子步。
pub fn write(path: &Path, state: &EffectsState) -> anyhow::Result<()> {
    validate(state)?;
    kk_storage::publish_json(
        path,
        EFFECTS_SPEC,
        &serde_json::to_value(state)?,
        |_| Ok(()),
    )
}

/// 读取并整体校验；任何失败都是停服条件（不降级、不重建锚点）。
pub fn load(path: &Path, world_id: &str) -> anyhow::Result<EffectsState> {
    let state: EffectsState = kk_storage::envelope::read_json(path, EFFECTS_SPEC)
        .map_err(|error| anyhow::anyhow!("读取恢复锚点 {}: {error}", path.display()))
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|error| anyhow::anyhow!("恢复锚点数据非法: {error}"))
        })?;
    validate(&state)?;
    anyhow::ensure!(
        state.world_id == world_id,
        "恢复锚点的世界身份 {} 与目录身份 {} 不一致",
        state.world_id,
        world_id
    );
    Ok(state)
}

fn validate(state: &EffectsState) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.effects_format_version == EFFECTS_FORMAT_VERSION,
        "effects 格式版本 {} 不受支持（期望 {EFFECTS_FORMAT_VERSION}）",
        state.effects_format_version
    );
    anyhow::ensure!(!state.anchor.barrier_id.is_empty(), "锚点屏障身份不能为空");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_anchor_roundtrip_and_identity_gate() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-effects-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("effects.json");
        let state = EffectsState::initial(&"a".repeat(32), 7, "b0");
        write(&path, &state)?;
        assert_eq!(load(&path, &"a".repeat(32))?.anchor.checkpoint_seq, 7);
        // 世界身份不符 = 停服条件。
        assert!(load(&path, &"b".repeat(32)).is_err());
        // 外来内容 = Fatal，不允许跳过。
        std::fs::write(&path, "{\"effects_format_version\": 9}")?;
        assert!(load(&path, &"a".repeat(32)).is_err());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
