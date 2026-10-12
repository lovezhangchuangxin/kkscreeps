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
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    /// v2 起存在（S2 耐久命令面）；v2 目录的合法候选必须全部 Some。
    /// 发布时刻继承的屏障身份：屏障检查点建立新身份，普通检查点继承。
    #[serde(default)]
    pub barrier_id: Option<String>,
    /// 命令流已应用水位（streamId→已消费 clientSeq）；S2.1 接收面恒为
    /// 启用初值，真实消费自 S2.2 起推进。
    #[serde(default)]
    pub inbox_applied: Option<BTreeMap<String, u64>>,
    /// 玩家模块版本（S2.2 代码边界事务的 CAS 基准）；启用初值空。
    #[serde(default)]
    pub code_versions: Option<BTreeMap<String, u64>>,
    /// 输出身份分配游标（S2.4 outbox）；启用初值 0。
    #[serde(default)]
    pub outbox_cursor: Option<u64>,
}

impl ServiceCheckpointMeta {
    /// v2 目录的合法性：四项 v2 字段须齐备（缺任一即 v1 形状候选，
    /// 不能当 v2 空状态使用）。
    pub fn is_v2_complete(&self) -> bool {
        self.barrier_id.is_some()
            && self.inbox_applied.is_some()
            && self.code_versions.is_some()
            && self.outbox_cursor.is_some()
    }
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

/// 新建 v1 服务元数据；publish 路径使用，字段一致性由调用方保证。
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
        barrier_id: None,
        inbox_applied: None,
        code_versions: None,
        outbox_cursor: None,
    }
}

/// 新建 v2 服务元数据；v2 目录的每次发布（含 enable 的初始屏障检查点）
/// 都携带完整 v2 字段，水位类字段继承自恢复版本，不在各调用点重复拼装。
/// 参数随字段集增长（与 ServiceHost::new 同例，构造器聚合处允许）。
#[allow(clippy::too_many_arguments)]
pub fn new_meta_v2(
    world_id: &str,
    publish_seq: u64,
    tick: u64,
    world_seed: u64,
    scenario_sha256: &str,
    entries: &BTreeMap<String, String>,
    barrier_id: &str,
    inbox_applied: &BTreeMap<String, u64>,
    code_versions: &BTreeMap<String, u64>,
    outbox_cursor: u64,
) -> ServiceCheckpointMeta {
    ServiceCheckpointMeta {
        service_format_version: super::identity::SERVICE_FORMAT_V2,
        world_id: world_id.to_string(),
        publish_seq,
        tick,
        world_seed,
        scenario_sha256: scenario_sha256.to_string(),
        entries: entries.clone(),
        barrier_id: Some(barrier_id.to_string()),
        inbox_applied: Some(inbox_applied.clone()),
        code_versions: Some(code_versions.clone()),
        outbox_cursor: Some(outbox_cursor),
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
