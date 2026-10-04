//! 布局常量、列类型与嵌套编码的共享操作。
use super::*;

// —— 布局常量 ——————————————————————————————————————————————

// 头部字段偏移；全部范围相对整块起点。
pub(crate) const OFF_VERSION: usize = 4;
pub(crate) const OFF_FLAGS: usize = 6;
pub(crate) const OFF_BYTE_LEN: usize = 8;
pub(crate) const OFF_HEADER_LEN: usize = 12;
pub(crate) const OFF_TICK: usize = 16;
pub(crate) const OFF_GENERATION: usize = 24;
pub(crate) const OFF_PLAN_HASH: usize = 32;
pub(crate) const OFF_AUDIENCE_HASH: usize = 64;
pub(crate) const OFF_ROOM_NAME: usize = 96;
pub(crate) const OFF_OBJECT_COUNT: usize = 104;
pub(crate) const OFF_TYPE_COUNT: usize = 108;
pub(crate) const OFF_TERRAIN: usize = 112;
pub(crate) const OFF_DIRECTORY: usize = 120;
pub(crate) const OFF_BUCKETS: usize = 128;
pub(crate) const OFF_ARENA: usize = 136;
pub(crate) const OFF_RESERVED: usize = 144;

pub(crate) const DIRECTORY_ENTRY: usize = 16;
pub(crate) const BUCKET_HEADER: usize = 16;
pub(crate) const FIELD_ENTRY: usize = 12;
pub(super) const DESCRIPTOR: usize = 8;

// 列类型字节；定宽 0..=4，变长 5..=7。
pub(super) const KIND_BOOL: u8 = 0;
pub(super) const KIND_INT: u8 = 1;
pub(super) const KIND_UINT: u8 = 2;
pub(super) const KIND_FLOAT: u8 = 3;
pub(super) const KIND_REF: u8 = 4;
pub(super) const KIND_STR: u8 = 5;
pub(super) const KIND_ARRAY: u8 = 6;
pub(super) const KIND_OBJECT: u8 = 7;

pub(super) fn column_width(kind: u8) -> usize {
    match kind {
        KIND_BOOL => 1,
        KIND_INT | KIND_UINT | KIND_FLOAT | KIND_REF => 8,
        _ => DESCRIPTOR,
    }
}

pub(super) fn is_variable(kind: u8) -> bool {
    matches!(kind, KIND_STR | KIND_ARRAY | KIND_OBJECT)
}

pub(super) fn kind_of(value: &KkpvValue) -> Option<u8> {
    match value {
        KkpvValue::Null => None,
        KkpvValue::Bool(_) => Some(KIND_BOOL),
        KkpvValue::Int(_) => Some(KIND_INT),
        KkpvValue::Uint(_) => Some(KIND_UINT),
        KkpvValue::Float(_) => Some(KIND_FLOAT),
        KkpvValue::Ref(_) => Some(KIND_REF),
        KkpvValue::Str(_) => Some(KIND_STR),
        KkpvValue::Array(_) => Some(KIND_ARRAY),
        KkpvValue::Object(_) => Some(KIND_OBJECT),
    }
}

// 嵌套载荷内的值标签；null 只以标签 0 出现，不占载荷字节。
pub(super) const TAG_NULL: u8 = 0;
pub(super) const TAG_BOOL: u8 = 1;
pub(super) const TAG_INT: u8 = 2;
pub(super) const TAG_UINT: u8 = 3;
pub(super) const TAG_FLOAT: u8 = 4;
pub(super) const TAG_REF: u8 = 5;
pub(super) const TAG_STR: u8 = 6;
pub(super) const TAG_ARRAY: u8 = 7;
pub(super) const TAG_OBJECT: u8 = 8;

// —— 编码 —————————————————————————————————————————————————

/// 校验值域并递归编码为嵌套标签形态。
pub(super) fn encode_nested(
    value: &KkpvValue,
    limits: &KkpvLimits,
    depth: usize,
    out: &mut Vec<u8>,
) -> Result<(), KkpvError> {
    if depth > limits.max_value_depth.min(NESTING_HARD_CAP) {
        return Err(KkpvError::DepthExceeded);
    }
    match value {
        KkpvValue::Null => out.push(TAG_NULL),
        KkpvValue::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(*b as u8);
        }
        KkpvValue::Int(v) => {
            out.push(TAG_INT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvValue::Uint(v) => {
            out.push(TAG_UINT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvValue::Float(v) => {
            if !v.is_finite() {
                return Err(KkpvError::NonFiniteFloat);
            }
            out.push(TAG_FLOAT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvValue::Ref(v) => {
            out.push(TAG_REF);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvValue::Str(s) => {
            if s.len() > limits.max_string_bytes {
                return Err(KkpvError::LimitExceeded("max_string_bytes"));
            }
            out.push(TAG_STR);
            let length =
                u32::try_from(s.len()).map_err(|_| KkpvError::LimitExceeded("max_string_bytes"))?;
            out.extend_from_slice(&length.to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        KkpvValue::Array(items) => {
            if items.len() > limits.max_array_len {
                return Err(KkpvError::LimitExceeded("max_array_len"));
            }
            out.push(TAG_ARRAY);
            let length = u32::try_from(items.len())
                .map_err(|_| KkpvError::LimitExceeded("max_array_len"))?;
            out.extend_from_slice(&length.to_le_bytes());
            for item in items {
                encode_nested(item, limits, depth + 1, out)?;
            }
        }
        KkpvValue::Object(entries) => {
            if entries.len() > limits.max_object_entries {
                return Err(KkpvError::LimitExceeded("max_object_entries"));
            }
            // 嵌套记录保持条目顺序：JSON 参考通道的嵌套对象（store、
            // spawning 等）在 preserve_order 构建下是插入序，玩家可见的
            // 键序（如 Object.keys(creep.store)）不得因二进制通道重排。
            // 只拒绝重复键；字节确定性由输入条目序决定。
            let mut seen = std::collections::BTreeSet::new();
            for (key, _) in entries {
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                if !seen.insert(key.as_str()) {
                    return Err(KkpvError::DuplicateField {
                        type_key: "嵌套记录".into(),
                        field: key.clone(),
                    });
                }
            }
            out.push(TAG_OBJECT);
            let length = u32::try_from(entries.len())
                .map_err(|_| KkpvError::LimitExceeded("max_object_entries"))?;
            out.extend_from_slice(&length.to_le_bytes());
            for (key, value) in entries {
                let key_len = u32::try_from(key.len())
                    .map_err(|_| KkpvError::LimitExceeded("max_string_bytes"))?;
                out.extend_from_slice(&key_len.to_le_bytes());
                out.extend_from_slice(key.as_bytes());
                encode_nested(value, limits, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

pub(super) struct ArenaBuilder {
    pub(super) bytes: Vec<u8>,
}

impl ArenaBuilder {
    /// arena 描述符是 u32 对；超出的长度必须报错而不是回绕。
    pub(super) fn push_str(&mut self, s: &str) -> Result<(u32, u32), KkpvError> {
        self.push_bytes(s.as_bytes())
    }

    pub(super) fn push_bytes(&mut self, bytes: &[u8]) -> Result<(u32, u32), KkpvError> {
        let fits =
            |v: usize| u32::try_from(v).map_err(|_| KkpvError::LimitExceeded("max_arena_bytes"));
        let offset = fits(self.bytes.len())?;
        let length = fits(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok((offset, length))
    }
}

pub(super) fn bitmap_bytes(rows: u32) -> usize {
    (rows as usize).div_ceil(8)
}

pub(super) fn bit_set(bitmap: &[u8], row: u32) -> bool {
    bitmap[row as usize / 8] & (1 << (row % 8)) != 0
}

pub(super) fn read_u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

pub(super) fn read_u64_at(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

/// 编码与解码共用的递归深度硬上限；实际限额取配置值与此值的较小者。
pub(super) const NESTING_HARD_CAP: usize = 1024;
