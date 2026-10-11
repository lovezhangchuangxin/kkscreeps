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
/// 脏位段（逐桶行脏位图，桶秩序拼接）的段偏移对；FLAG_DIRTY 置位时
/// 存在，此时段是块内第五段（arena 之后）。
pub(crate) const OFF_DIRTY: usize = 144;
/// 脏位基准拍号（u64）：脏位所比对的上一拍编码块的 tick。解码端只有
/// 自身上一拍解码 tick 与之一致时才信任干净行；FLAG_DIRTY 未置位时
/// 保留为零。
pub(crate) const OFF_DIRTY_BASE_TICK: usize = 152;

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

pub(super) fn kind_of_val(value: &KkpvVal<'_>) -> Option<u8> {
    match value {
        KkpvVal::Null => None,
        KkpvVal::Bool(_) => Some(KIND_BOOL),
        KkpvVal::Int(_) => Some(KIND_INT),
        KkpvVal::Uint(_) => Some(KIND_UINT),
        KkpvVal::Float(_) => Some(KIND_FLOAT),
        KkpvVal::Ref(_) => Some(KIND_REF),
        KkpvVal::Str(_) => Some(KIND_STR),
        KkpvVal::Array(_) => Some(KIND_ARRAY),
        KkpvVal::Object(_) => Some(KIND_OBJECT),
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

/// 嵌套标签编码的唯一实现：owned 输入经 [`KkpvValue::as_val`] 桥接后
/// 共用本函数，防止两套编码漂移。`seen` 是调用方提供的复用查重
/// scratch：对象键查重用它做栈式线性扫描（base..len 为当前对象作用
/// 域），取代逐嵌套对象分配 BTreeSet；首个重复键的检测点与插入序
/// 版一致。
pub(super) fn encode_nested_val<'o>(
    value: &'o KkpvVal<'o>,
    limits: &KkpvLimits,
    depth: usize,
    out: &mut Vec<u8>,
    seen: &mut Vec<&'o str>,
) -> Result<(), KkpvError> {
    if depth > limits.max_value_depth.min(NESTING_HARD_CAP) {
        return Err(KkpvError::DepthExceeded);
    }
    match value {
        KkpvVal::Null => out.push(TAG_NULL),
        KkpvVal::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(*b as u8);
        }
        KkpvVal::Int(v) => {
            out.push(TAG_INT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvVal::Uint(v) => {
            out.push(TAG_UINT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvVal::Float(v) => {
            if !v.is_finite() {
                return Err(KkpvError::NonFiniteFloat);
            }
            out.push(TAG_FLOAT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvVal::Ref(v) => {
            out.push(TAG_REF);
            out.extend_from_slice(&v.to_le_bytes());
        }
        KkpvVal::Str(s) => {
            if s.len() > limits.max_string_bytes {
                return Err(KkpvError::LimitExceeded("max_string_bytes"));
            }
            out.push(TAG_STR);
            let length =
                u32::try_from(s.len()).map_err(|_| KkpvError::LimitExceeded("max_string_bytes"))?;
            out.extend_from_slice(&length.to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        KkpvVal::Array(items) => {
            if items.len() > limits.max_array_len {
                return Err(KkpvError::LimitExceeded("max_array_len"));
            }
            out.push(TAG_ARRAY);
            let length = u32::try_from(items.len())
                .map_err(|_| KkpvError::LimitExceeded("max_array_len"))?;
            out.extend_from_slice(&length.to_le_bytes());
            for item in items {
                encode_nested_val(item, limits, depth + 1, out, seen)?;
            }
        }
        KkpvVal::Object(entries) => {
            if entries.len() > limits.max_object_entries {
                return Err(KkpvError::LimitExceeded("max_object_entries"));
            }
            // 嵌套记录保持条目顺序：JSON 参考通道的嵌套对象（store、
            // spawning 等）在 preserve_order 构建下是插入序，玩家可见的
            // 键序（如 Object.keys(creep.store)）不得因二进制通道重排。
            // 只拒绝重复键；字节确定性由输入条目序决定。查重用调用方
            // scratch 的栈式线性扫描（对象键数受 max_object_entries
            // 上限约束，线性扫描与旧 BTreeSet 检出同一首个重复键）。
            let base = seen.len();
            for (key, _) in entries {
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                if seen[base..].contains(key) {
                    return Err(KkpvError::DuplicateField {
                        type_key: "嵌套记录".into(),
                        field: (*key).into(),
                    });
                }
                seen.push(key);
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
                encode_nested_val(value, limits, depth + 1, out, seen)?;
            }
            seen.truncate(base);
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
