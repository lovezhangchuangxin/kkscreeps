//! 玩家运行结果只修改对应的持久字段，不覆盖规则结算产生的 GCL 或余额。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::settle::{PlayerState, SettleError};

/// 候选已完成的子步，用于在部分候选写入后验证失败隔离。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritebackStep {
    Market,
    Cpu,
    RawMemory,
    Segments,
}

impl WritebackStep {
    pub fn label(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Cpu => "cpu",
            Self::RawMemory => "raw_memory",
            Self::Segments => "segments",
        }
    }
}

/// 一轮玩家执行产生的持久内存候选。由调用方绑定玩家身份并缓存供重试。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerWriteback {
    pub cpu: Option<kk_core::cpu::Bucket>,
    pub raw_memory: Option<String>,
    pub segments: Option<SegmentWriteback>,
    /// InterShardMemory 本地段候选：仅 setLocal 触发的拍携带（快照语义，
    /// 与 raw_memory 同门——写回失败轮不带，旧值保留）。
    pub ism_local: Option<String>,
}

/// 已过滤非法段 ID 的写回请求。缺省字段保留旧请求，显式 null 清除可选请求。
/// 字符串长度和活动段数量由 runner 检查，此处沿用已有宿主合并规则。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SegmentWriteback {
    writes: BTreeMap<u8, String>,
    active: Option<Vec<u8>>,
    public: Option<Vec<u8>>,
    default_public: Option<Option<u8>>,
    foreign: Option<Option<(String, Option<u8>)>>,
}

impl SegmentWriteback {
    /// 接收 runner 的 segmentsOut；过滤非法值，不截断超范围 ID。
    pub fn from_runner(out: &Value) -> Self {
        let seg_id = |v: &Value| -> Option<u8> {
            let n = v.as_u64()?;
            (n <= 99).then_some(n as u8)
        };
        let writes = out
            .get("writes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|pair| Some((pair.get(0).and_then(seg_id)?, pair.get(1)?.as_str()?.into())))
            .collect();
        Self {
            writes,
            active: out
                .get("active")
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(seg_id).collect()),
            public: out
                .get("public")
                .and_then(Value::as_array)
                .map(|ids| ids.iter().filter_map(seg_id).collect()),
            default_public: out.get("defaultPublic").map(seg_id),
            foreign: out.get("foreign").map(|v| {
                v.get("username")
                    .and_then(Value::as_str)
                    .map(|username| (username.into(), v.get("id").and_then(seg_id)))
            }),
        }
    }
}

impl PlayerWriteback {
    pub(in crate::settle) fn apply_with_checkpoints(
        self,
        state: &mut PlayerState,
        mut checkpoint: impl FnMut(WritebackStep) -> Result<(), SettleError>,
    ) -> Result<(), SettleError> {
        if let Some(cpu) = self.cpu {
            state.cpu = cpu;
        }
        checkpoint(WritebackStep::Cpu)?;
        if let Some(raw) = self.raw_memory {
            state.raw_memory = raw;
        }
        // ISM 写回与 RawMemory 同门提交：写回失败轮整体丢弃时两者一起回滚。
        if let Some(ism) = self.ism_local {
            state.ism_local = Some(ism);
        }
        checkpoint(WritebackStep::RawMemory)?;
        if let Some(segments) = self.segments {
            state.segments.extend(segments.writes);
            if let Some(active) = segments.active {
                state.active_request = active;
            }
            if let Some(public) = segments.public {
                state.public_ids = public;
            }
            if let Some(default) = segments.default_public {
                state.default_public = default;
            }
            if let Some(foreign) = segments.foreign {
                state.foreign_request = foreign;
            }
        }
        checkpoint(WritebackStep::Segments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn segment_patch_preserves_absent_fields_and_filters_ids_before_conversion() {
        let mut state = PlayerState {
            gcl: 12,
            money: 34,
            active_request: vec![1],
            public_ids: vec![2],
            default_public: Some(2),
            foreign_request: Some(("p2".into(), Some(3))),
            segments: BTreeMap::from([(0, "old".into()), (5, "keep".into())]),
            ..Default::default()
        };
        PlayerWriteback {
            cpu: None,
            ism_local: None,
            raw_memory: Some("new memory".into()),
            segments: Some(SegmentWriteback::from_runner(&json!({
                "writes": [[0, "first"], [256, "must not wrap"], [99, "valid"],
                           [-1, "invalid"], [0, "last"], [5, 123]],
                "active": [99, 100, -1, "1"], "defaultPublic": null, "foreign": null
            }))),
        }
        .apply_with_checkpoints(&mut state, |_| Ok(()))
        .unwrap();
        assert_eq!(state.raw_memory, "new memory");
        assert_eq!(
            state.segments,
            BTreeMap::from([(0, "last".into()), (5, "keep".into()), (99, "valid".into())])
        );
        assert_eq!(state.active_request, vec![99]);
        assert_eq!(state.public_ids, vec![2]);
        assert_eq!(state.default_public, None);
        assert_eq!(state.foreign_request, None);
        assert_eq!((state.gcl, state.money), (12, 34));
    }
}
