//! 有界信封读取与错误分类：发布协议写出的文件按校验值整读。
//!
//! 读取错误分两类，供恢复选择采取不同处置：
//! [`ReadError::Broken`]（JSON 撕裂/截断或载荷校验值不匹配）可记录并跳过
//! 该份；[`ReadError::Fatal`]（I/O 故障、超限、不可识别格式或版本、结构
//! 不可识别的合法 JSON）必须停止，不能以“坏文件”为由静默退到其他版本。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// 发布/读取共用的信封规格：格式名、线格式版本与单文件字节上限。
#[derive(Clone, Copy)]
pub struct EnvelopeSpec {
    pub format: &'static str,
    pub version: u32,
    pub max_bytes: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    sha256: String,
    payload: String,
}

/// 信封读取失败的分类；跳过或中止的处置政策由恢复选择方决定。
#[derive(Debug)]
pub enum ReadError {
    /// JSON 撕裂/截断，或载荷校验值不匹配：该份不可用，可跳过次新。
    Broken(String),
    /// 权限或 I/O 故障、超限、格式/版本不可识别，或合法 JSON 但结构
    /// 不符合本线格式（未知/缺失字段、类型错误——撕裂截断不可能产出
    /// 合法 JSON，此类只可能来自未来格式或外来内容）：必须停止。
    Fatal(String),
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadError::Broken(detail) => write!(f, "信封损坏（撕裂或校验值不匹配）: {detail}"),
            ReadError::Fatal(detail) => {
                write!(f, "信封不可读（I/O/超限/不可识别格式或版本）: {detail}")
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// 读取信封、校验格式/版本/哈希后解码载荷 JSON 值。
pub fn read_json(path: &Path, spec: EnvelopeSpec) -> Result<Value, ReadError> {
    let payload = read_payload(path, spec)?;
    serde_json::from_str(&payload)
        .map_err(|error| ReadError::Broken(format!("解析载荷 JSON: {error}")))
}

/// 读取信封并返回原始载荷字符串（供调用方自行解释载荷结构）。
pub fn read_payload(path: &Path, spec: EnvelopeSpec) -> Result<String, ReadError> {
    let io =
        |error: std::io::Error| ReadError::Fatal(format!("打开/读取 {}: {error}", path.display()));
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(io)?
        // saturating：max_bytes=u64::MAX 的规格退化为无上限读取，随后的
        // 长度判定仍以 max_bytes 为准，不会误放行。
        .take(spec.max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > spec.max_bytes {
        return Err(ReadError::Fatal(format!(
            "{} 超过 {} 字节上限",
            path.display(),
            spec.max_bytes
        )));
    }
    let envelope: Envelope = serde_json::from_slice(&bytes).map_err(classify_parse_error)?;
    if envelope.format != spec.format {
        return Err(ReadError::Fatal(format!(
            "信封格式 {format:?} 与期望 {expected:?} 不符",
            format = envelope.format,
            expected = spec.format
        )));
    }
    if envelope.version != spec.version {
        return Err(ReadError::Fatal(format!(
            "信封版本 {} 与期望 {} 不一致；尚未提供迁移",
            envelope.version, spec.version
        )));
    }
    let actual = format!("{:x}", Sha256::digest(envelope.payload.as_bytes()));
    if actual != envelope.sha256 {
        return Err(ReadError::Broken(format!(
            "载荷校验值不匹配（期望 {}，实际 {actual}）",
            envelope.sha256
        )));
    }
    Ok(envelope.payload)
}

/// 按错误类别拆分信封解析失败：语法/EOF（撕裂截断）归 Broken；数据类
/// （未知字段、缺失字段、类型不符——合法 JSON 但不符合本线格式）归
/// Fatal，防止未来格式/外来内容被当作坏份静默跳过。已知残留形态：完
/// 整信封后跟尾部垃圾/第二份 JSON 报 Syntax 归 Broken——该形态不可能
/// 由本发布协议产生（整文件临时写+rename），仅外部篡改可造，按坏份
/// 跳过不违反恢复语义。
fn classify_parse_error(error: serde_json::Error) -> ReadError {
    use serde_json::error::Category;
    match error.classify() {
        Category::Syntax | Category::Eof => {
            ReadError::Broken(format!("信封 JSON 撕裂/畸形: {error}"))
        }
        Category::Data | Category::Io => {
            ReadError::Fatal(format!("信封结构不可识别（未来格式或外来内容）: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SPEC: EnvelopeSpec = EnvelopeSpec {
        format: "test-envelope",
        version: 1,
        max_bytes: 1024,
    };

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn classifies_broken_vs_fatal() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-envelope-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        // 截断（撕裂）与哈希不匹配属于 Broken：可跳过。
        write(&path, b"{\"format\":");
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Broken(_))
        ));
        let payload = serde_json::to_string(&json!({"tick": 3}))?;
        let envelope = json!({
            "format": SPEC.format, "version": SPEC.version,
            "sha256": "0".repeat(64), "payload": payload,
        });
        write(&path, &serde_json::to_vec(&envelope)?);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Broken(_))
        ));
        // 未来版本、异格式、超限与 I/O 类故障属于 Fatal：必须停止。
        let mut future = envelope.clone();
        future["version"] = json!(2);
        write(&path, &serde_json::to_vec(&future)?);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Fatal(_))
        ));
        let mut foreign = envelope;
        foreign["format"] = json!("other-format");
        write(&path, &serde_json::to_vec(&foreign)?);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Fatal(_))
        ));
        let big = EnvelopeSpec {
            max_bytes: 8,
            ..SPEC
        };
        write(
            &path,
            &serde_json::to_vec(&json!({
                "format": SPEC.format, "version": SPEC.version,
                "sha256": "0".repeat(64), "payload": "1234567890",
            }))?,
        );
        assert!(matches!(read_payload(&path, big), Err(ReadError::Fatal(_))));
        assert!(matches!(
            read_payload(&dir.join("absent.json"), SPEC),
            Err(ReadError::Fatal(_))
        ));
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn valid_json_with_foreign_shape_is_fatal_not_broken() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-envelope-foreign-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        // 合法 JSON 但含未来字段（即使同时提升 version）：结构不可识别，
        // 不能按“坏份”跳过退到旧版本。
        let future_field = json!({
            "format": SPEC.format, "version": SPEC.version + 1,
            "sha256": "0".repeat(64), "payload": "1", "compression": "zstd",
        });
        write(&path, &serde_json::to_vec(&future_field)?);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Fatal(_))
        ));
        // 完全外来的 JSON 内容与缺失字段同样 Fatal。
        write(&path, br#"{"hello":"world"}"#);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Fatal(_))
        ));
        write(&path, br#"{"format":"test-envelope"}"#);
        assert!(matches!(
            read_payload(&path, SPEC),
            Err(ReadError::Fatal(_))
        ));
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn roundtrip_returns_payload_value() -> anyhow::Result<()> {
        let dir = std::env::temp_dir().join(format!("kk-envelope-rt-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("state.json");
        let payload = json!({"tick": 12, "nested": {"a": [1, 2]}});
        let text = serde_json::to_string(&payload)?;
        let envelope = json!({
            "format": SPEC.format, "version": SPEC.version,
            "sha256": format!("{:x}", Sha256::digest(text.as_bytes())),
            "payload": text,
        });
        write(&path, &serde_json::to_vec(&envelope)?);
        assert_eq!(read_json(&path, SPEC)?, payload);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
