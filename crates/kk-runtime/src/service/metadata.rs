//! 检查点载荷内的服务元数据（`metadata.service` v1）。
//!
//! 引擎检查点的 metadata 域对引擎不透明；服务域在此严格解码。载荷必须
//! 包含世界身份、发布序号、世界时间、服务格式与种子/内容身份——文件名
//! 不是身份真相源。新字段升级须提升服务格式版本并整体迁移，不把缺字段
//! 当作空状态。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use super::identity::SERVICE_FORMAT_VERSION;

/// `metadata.service` v1 载荷；与目录身份同版核验。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCheckpointMeta {
    pub service_format_version: u32,
    pub world_id: String,
    pub publish_seq: u64,
    /// 保存时刻的世界时间；供选择器在解码引擎载荷前核对一致性。
    pub tick: u64,
    pub world_seed: u64,
    pub scenario_sha256: String,
    /// 每玩家入口模块名：保存 init 实际 entry，运行/恢复不假设 main。
    pub entries: BTreeMap<String, String>,
}

/// 从完整检查点载荷严格解码服务元数据；缺失或畸形即错误（该文件由本
/// 服务发布，缺域说明不是合法服务检查点）。
pub fn decode(checkpoint: &Value) -> anyhow::Result<ServiceCheckpointMeta> {
    let meta = checkpoint
        .pointer("/metadata/service")
        .ok_or_else(|| anyhow::anyhow!("检查点缺少 metadata.service 服务元数据"))?;
    serde_json::from_value(meta.clone()).map_err(|error| anyhow::anyhow!("服务元数据畸形: {error}"))
}

/// 组装传给 `EmbedEngine::save_checkpoint` 的 metadata 值。
pub fn engine_metadata(meta: &ServiceCheckpointMeta) -> Value {
    serde_json::to_value(meta)
        .map(|value| serde_json::json!({ "service": value }))
        .expect("服务元数据编码不可失败")
}

/// 新建服务元数据；publish 路径使用，字段一致性由调用方保证。
pub fn new_meta(
    world_id: &str,
    publish_seq: u64,
    tick: u64,
    world_seed: u64,
    scenario_sha256: &str,
    entries: &BTreeMap<String, String>,
) -> ServiceCheckpointMeta {
    ServiceCheckpointMeta {
        service_format_version: SERVICE_FORMAT_VERSION,
        world_id: world_id.to_string(),
        publish_seq,
        tick,
        world_seed,
        scenario_sha256: scenario_sha256.to_string(),
        entries: entries.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_is_strict_about_shape_and_location() {
        let meta = new_meta(
            &"w".repeat(32),
            3,
            7,
            11,
            &"a".repeat(64),
            &Default::default(),
        );
        let value = json!({"metadata": {"service": meta}});
        assert_eq!(decode(&value).unwrap().publish_seq, 3);
        // 位置错误与未知字段都拒绝。
        assert!(decode(&json!({"service": value["metadata"]["service"]})).is_err());
        let mut extra = value.clone();
        extra["metadata"]["service"]["surprise"] = json!(1);
        assert!(decode(&extra).is_err());
    }
}
