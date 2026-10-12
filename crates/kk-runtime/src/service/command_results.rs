//! 拍内命令的耐久结果窗口：与世界检查点同份发布，补全 inbox 存档缺口。
//! 独立版本树 `metadata.command_results` v1；旧 S2.2 检查点无该树迁移为
//! 空窗口（当时没有拍内命令）。仅保留尚未写回 inbox 的结果及最近屏障
//! 新结果，下一屏障按 inbox 终态清理，大小受 inbox/表达式结果预算限制。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    pub client_seq: u64,
    pub result: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResults {
    pub version: u32,
    pub streams: BTreeMap<String, CommandResult>,
}

impl Default for CommandResults {
    fn default() -> Self {
        Self {
            version: 1,
            streams: BTreeMap::new(),
        }
    }
}

impl CommandResults {
    /// 只允许旧版缺树迁移为空；存在的树缺字段/未来版本均拒绝恢复。
    pub fn decode(checkpoint: &Value) -> anyhow::Result<Self> {
        let Some(value) = checkpoint.pointer("/metadata/command_results") else {
            return Ok(Self::default());
        };
        let results: Self = serde_json::from_value(value.clone())?;
        anyhow::ensure!(results.version == 1, "command_results 版本不受支持");
        let meta = super::metadata::decode(checkpoint)?;
        for (stream, entry) in &results.streams {
            anyhow::ensure!(
                entry.client_seq > 0
                    && meta
                        .inbox_applied
                        .as_ref()
                        .and_then(|map| map.get(stream))
                        .copied()
                        .unwrap_or(0)
                        >= entry.client_seq,
                "command_results 水位不在已应用命令内"
            );
            anyhow::ensure!(
                serde_json::to_vec(&entry.result)?.len() <= 64 * 1024,
                "command_results 结果超预算"
            );
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn result_tree_migrates_legacy_and_rejects_invalid_or_future_shapes() {
        let meta = super::super::metadata::new_meta_v2(
            "w",
            1,
            0,
            5,
            "s",
            &Default::default(),
            "b",
            &BTreeMap::from([("s1".into(), 1)]),
            &Default::default(),
            0,
        );
        let mut value = json!({"metadata": {"service": meta}});
        assert!(CommandResults::decode(&value).unwrap().streams.is_empty());
        value["metadata"]["command_results"] =
            json!({"version":1, "streams":{"s1":{"client_seq":1,"result":{"ok":true,"value":1}}}});
        assert_eq!(
            CommandResults::decode(&value).unwrap().streams["s1"].result["value"],
            1
        );
        for bad in [
            json!({"version":2,"streams":{}}),
            json!({"version":1}),
            json!({"version":1,"streams":{},"unknown":true}),
            json!({"version":1,"streams":{"s1":{"client_seq":2,"result":1}}}),
            json!({"version":1,"streams":{"s1":{"client_seq":1,"result":"x".repeat(65537)}}}),
        ] {
            value["metadata"]["command_results"] = bad;
            assert!(CommandResults::decode(&value).is_err());
        }
    }
}
