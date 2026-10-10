//! Generic 原始载荷与紧凑身份元数据。候选共享不可变载荷，写入整体替换；
//! 不常驻完整 JSON 树，不参与玩法默认值或权限判定。
use crate::{Cursor, Schema, SchemaError};
use std::sync::{Arc, OnceLock};

/// 仅缓存分类/归属和衰减读取门所需的少量字段，不限制动态 JSON 的其他字段。
#[derive(Debug)]
pub struct GenericIdentity {
    pub type_name: Option<Box<str>>,
    pub structure_type: Option<Box<str>>,
    pub owner: Option<Box<str>>,
    pub observe_room: Option<Box<str>>,
    /// structureType 存在但非字符串时仍遮蔽 type；移动类型读取沿用此语义。
    pub has_structure_type: bool,
    /// 衰减的只读门：只在真正到期或 hitsMax 需同步时构造可写 JSON 树。
    pub next_decay_time: u64,
    pub hits_max: i64,
}

#[derive(Debug)]
struct Payload {
    text: String,
    identity: OnceLock<Option<GenericIdentity>>,
}

/// 不可变 JSON 文本。克隆只共享载荷；替换字段自然建立新的缓存身份。
/// 二进制表示与原 String 完全相同，派生缓存不进入快照或检查点。
#[derive(Clone)]
pub struct GenericData(Arc<Payload>);

impl GenericData {
    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// 首次读取解析，之后借用紧凑结果。无效 JSON/非对象缓存为 None；
    /// 转义键、重复键及非字符串字段保持 serde_json::Value 的读取语义。
    pub fn identity(&self) -> Option<&GenericIdentity> {
        self.0
            .identity
            .get_or_init(|| {
                let value: serde_json::Value = serde_json::from_str(self.as_str()).ok()?;
                let fields = value.as_object()?;
                let string = |key: &str| {
                    fields
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(Box::from)
                };
                Some(GenericIdentity {
                    type_name: string("type"),
                    structure_type: string("structureType"),
                    owner: string("owner"),
                    observe_room: string("observeRoom"),
                    has_structure_type: fields.contains_key("structureType"),
                    next_decay_time: fields
                        .get("nextDecayTime")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    hits_max: fields
                        .get("hitsMax")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                })
            })
            .as_ref()
    }
}

impl From<String> for GenericData {
    fn from(text: String) -> Self {
        Self(Arc::new(Payload {
            text,
            identity: OnceLock::new(),
        }))
    }
}

impl From<&str> for GenericData {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

impl std::ops::Deref for GenericData {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Debug for GenericData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}

impl std::fmt::Display for GenericData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq for GenericData {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for GenericData {}

impl Schema for GenericData {
    fn encode_into(&self, out: &mut Vec<u8>) {
        (self.len() as u32).encode_into(out);
        out.extend_from_slice(self.as_bytes());
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        String::decode_from(cur).map(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_identity_matches_json_even_with_escaped_and_duplicate_keys() {
        for text in [
            r#"{"type":"structure","structureType":"rampart","owner":"p1"}"#,
            r#"{"\u0074ype":"powerCreep","\u006fwner":"p\u0031","structureType":null}"#,
            r#"{"owner":"p1","owner":null,"structureType":9,"type":"deposit"}"#,
            r#"{"owner":null,"owner":"p2","structureType":"observer","observeRoom":"W1N1"}"#,
            r#"{"hitsMax":300000000,"nextDecayTime":18446744073709551615}"#,
            r#"{"hitsMax":4.5,"nextDecayTime":-1,"owner":false}"#,
            "null",
            "[]",
            "{",
            r#"{"owner":"p1"} trailing"#,
        ] {
            let data = GenericData::from(text);
            let value = serde_json::from_str::<serde_json::Value>(text).ok();
            let expected = value.as_ref().and_then(serde_json::Value::as_object);
            assert_eq!(data.identity().is_some(), expected.is_some(), "{text}");
            if let Some(fields) = expected {
                let identity = data.identity().unwrap();
                for (key, got) in [
                    ("type", identity.type_name.as_deref()),
                    ("structureType", identity.structure_type.as_deref()),
                    ("owner", identity.owner.as_deref()),
                    ("observeRoom", identity.observe_room.as_deref()),
                ] {
                    assert_eq!(
                        got,
                        fields.get(key).and_then(serde_json::Value::as_str),
                        "{text}"
                    );
                }
                assert_eq!(
                    identity.has_structure_type,
                    fields.contains_key("structureType")
                );
                assert_eq!(
                    identity.next_decay_time,
                    fields
                        .get("nextDecayTime")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0)
                );
                assert_eq!(
                    identity.hits_max,
                    fields
                        .get("hitsMax")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0)
                );
            }
        }
    }

    #[test]
    fn candidate_replacement_keeps_old_payload_and_cache_is_not_persisted() {
        let original = GenericData::from(r#"{"owner":"p1","custom":{"nested":[1,2]}}"#);
        let identity = original.identity().unwrap();
        let mut candidate = original.clone();
        assert!(Arc::ptr_eq(&original.0, &candidate.0));
        assert!(std::ptr::eq(identity, candidate.identity().unwrap()));
        candidate = r#"{"owner":"p2","custom":{"nested":[3]}}"#.into();
        assert_eq!(original.identity().unwrap().owner.as_deref(), Some("p1"));
        assert_eq!(candidate.identity().unwrap().owner.as_deref(), Some("p2"));
        let mut actual = Vec::new();
        original.encode_into(&mut actual);
        let mut legacy = Vec::new();
        original.to_string().encode_into(&mut legacy);
        assert_eq!(actual, legacy);
        let restored = GenericData::decode_from(&mut Cursor::new(&actual)).unwrap();
        assert_eq!(restored, original);
        assert!(restored.0.identity.get().is_none());
        assert_eq!(restored.identity().unwrap().owner.as_deref(), Some("p1"));
    }
}
