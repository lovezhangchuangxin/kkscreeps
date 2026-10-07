use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{FreezeError, PlanHash};

pub(crate) fn canonical_hash(value: &impl Serialize) -> Result<PlanHash, FreezeError> {
    let bytes = canonical_bytes(value)?;
    let mut digest = Sha256::new();
    digest.update(b"kkscreeps/contribution-plan/v1\0");
    digest.update(bytes);
    Ok(PlanHash(digest.finalize().into()))
}

/// 域分隔摘要：任意可序列化值经与 PlanHash 相同的键序规范化后哈希。
/// 供计划之外的身份（如执行策略身份）使用——域前缀防止跨身份的原像
/// 混用；输入面由调用方钉死并在其契约文档登记。
pub fn domain_digest(domain: &str, value: &impl Serialize) -> Result<[u8; 32], String> {
    if domain.is_empty()
        || !domain
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'/' || b == b'-')
    {
        return Err(format!("身份域 {domain:?} 须为小写字母数字、斜杠或连字符"));
    }
    let bytes = canonical_bytes(value).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    digest.update(domain.as_bytes());
    digest.update(b"\0");
    digest.update(bytes);
    Ok(digest.finalize().into())
}

fn canonical_bytes(value: &impl Serialize) -> Result<Vec<u8>, FreezeError> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
            other => other,
        }
    }
    let value =
        serde_json::to_value(value).map_err(|e| FreezeError::new("fingerprint", e.to_string()))?;
    serde_json::to_vec(&sorted(value)).map_err(|e| FreezeError::new("fingerprint", e.to_string()))
}
