//! KKPV 玩家数据协议版本 1 的编解码。
//!
//! 布局规格见 docs/designs/domain-projection.md §6：160 字节头、按 ID 升序的
//! 对象目录、按类型分桶的列式字段、以及集中存放全部变长数据的 arena。
//! 头部与四个段的范围相对整块起点；room_name 与变长列描述符相对 arena 起点。
//! 整数一律小端。本模块只做结构编解码与边界校验，不计算查看者权限，
//! 也不理解对象字段的业务含义；块内容必须由上游按权限裁切后交付。
//!
//! 缺失、null、零值与空集合分别编码：缺失和 null 记录在逐字段位图，
//! 同一行两个位同时置位视为非法。定宽列（bool/整数/浮点/引用）按行紧密
//! 排列；字符串、数组与记录以 arena 描述符指向递归标签编码的载荷，
//! 读取该字段时才解码。嵌套载荷内的值使用单字节标签自描述，
//! null 以标签 0 表达。
//!
//! 建立视图时校验魔数、版本、flags、长度、保留位、段边界与重叠
//! （零长度段的 offset 须为零）、目录有序性与句柄/行关系（每桶每行
//! 恰被一个目录项引用）、桶结构恰好消费、键 UTF-8 与升序、位图互斥
//! 与越行尾位为零、浮点有限性、bool 字节域、变长描述符 arena 边界
//! 及各项限额；字段值内的字符串在读取时校验 UTF-8，嵌套记录键须
//! 严格升序。编码端执行同源限额与值域检查，超限时不得产出数据。
//!
//! 公开块与私有块只能在相同 tick/generation/计划指纹下经 [`merge`] 合并。
//! audience_hash 由宿主生成，本模块不解释、不校验（merge 亦不比较它）；
//! 它不是授权凭证。设计上的一致性校验与缓存分组用途见
//! docs/designs/domain-projection.md §6，当前仓库内尚无对应实现。
//!
//! 全 null 字段的列类型编码为 bool（宽度最小）：位图已表达全部信息，
//! 列类型字节随后续 tick 出现首个非 null 值而改变——依赖列类型跨块
//! 稳定的消费者须以计划字段描述为准，不得缓存推断结果。

/// 块魔数，占头部前 4 字节。
pub const MAGIC: [u8; 4] = *b"KKPV";
/// 当前协议版本。
pub const VERSION: u16 = 1;
/// 头部长度固定值，也是全部段的起始边界。
pub const HEADER_LEN: usize = 160;
/// flags 中标记私有块的位；其余位写入零，读取时非零即拒绝。
pub const FLAG_PRIVATE: u16 = 1;

/// 块内一个对象的待编码形态。字段序任意，编码按键排序并拒绝重复；
/// 对象 ID 只进入目录，`fields` 中的 `id` 键会被拒绝。
#[derive(Clone, Debug, PartialEq)]
pub struct KkpvObject {
    pub id: u64,
    pub type_key: String,
    pub fields: Vec<(String, KkpvValue)>,
}

/// 一个块的待编码形态。`terrain` 为 None 时不写地形段。
#[derive(Clone, Debug, PartialEq)]
pub struct KkpvBlockInput {
    pub private: bool,
    pub tick: u64,
    pub generation: u64,
    pub plan_hash: [u8; 32],
    /// 宿主定义的公开受众组或私有查看者摘要；本模块不解释其内容。
    pub audience_hash: [u8; 32],
    pub room_name: String,
    pub terrain: Option<Vec<u8>>,
    pub objects: Vec<KkpvObject>,
}

/// 块内值的表示模型。`Ref` 携带对象 ID 的 u64 原值，字符串形式由
/// 调用方在边界转换；`Uint` 与 `Int` 是两种独立列类型，不得互相改写。
#[derive(Clone, Debug, PartialEq)]
pub enum KkpvValue {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Ref(u64),
    Str(String),
    Array(Vec<KkpvValue>),
    /// 记录的键值对；编码按键排序并拒绝重复。
    Object(Vec<(String, KkpvValue)>),
}

/// 编码与解码共用的显式限额。超限时编码拒绝产出、解码拒绝建立视图，
/// 不存在静默截断。
#[derive(Clone, Debug, PartialEq)]
pub struct KkpvLimits {
    pub max_bytes: usize,
    pub max_objects: usize,
    pub max_types: usize,
    pub max_fields_per_type: usize,
    pub max_arena_bytes: usize,
    pub max_terrain_bytes: usize,
    pub max_string_bytes: usize,
    pub max_array_len: usize,
    pub max_object_entries: usize,
    pub max_value_depth: usize,
}

impl Default for KkpvLimits {
    fn default() -> Self {
        Self {
            max_bytes: 8 << 20,
            max_objects: 4096,
            max_types: 256,
            max_fields_per_type: 128,
            max_arena_bytes: 4 << 20,
            max_terrain_bytes: 4096,
            max_string_bytes: 8192,
            max_array_len: 512,
            max_object_entries: 256,
            max_value_depth: 16,
        }
    }
}

/// 编码、解码或合并失败的统一形态。
#[derive(Debug, Clone, PartialEq)]
pub enum KkpvError {
    BadMagic([u8; 4]),
    UnsupportedVersion(u16),
    UnknownFlags(u16),
    HeaderLen(u32),
    ByteLen {
        declared: u32,
        actual: usize,
    },
    ReservedNonZero,
    SegmentOverlap(&'static str),
    SegmentOutOfBounds {
        segment: &'static str,
        offset: usize,
        length: usize,
    },
    DirectoryLength {
        expected: usize,
        actual: usize,
    },
    DuplicateObjectId(u64),
    DirectoryNotSorted,
    UnknownTypeHandle(u32),
    RowOutOfRange {
        handle: u32,
        row: u32,
        rows: u32,
    },
    BucketWalk {
        consumed: usize,
        segment: usize,
    },
    ArenaBounds {
        offset: usize,
        length: usize,
    },
    ArenaMissing,
    RoomNameMissing,
    InvalidUtf8,
    InvalidBool(u8),
    NonFiniteFloat,
    UnknownKind(u8),
    UnknownTag(u8),
    AmbiguousBitmap,
    BitmapTailBits,
    KeyOrder,
    DuplicateRowRef {
        handle: u32,
        row: u32,
    },
    UnreferencedRow {
        handle: u32,
        row: u32,
    },
    LimitExceeded(&'static str),
    DuplicateField {
        type_key: String,
        field: String,
    },
    KindMismatch {
        type_key: String,
        field: String,
    },
    DepthExceeded,
    MergeTickMismatch {
        base: u64,
        overlay: u64,
    },
    MergeGenerationMismatch {
        base: u64,
        overlay: u64,
    },
    MergePlanMismatch,
    MergeRoomMismatch {
        base: String,
        overlay: String,
    },
    MergeFlags {
        base: u16,
        overlay: u16,
    },
    MergeObjectConflict(u64),
}

impl std::fmt::Display for KkpvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic(m) => write!(f, "KKPV 魔数不符: {m:?}"),
            Self::UnsupportedVersion(v) => write!(f, "不支持的 KKPV 版本: {v}"),
            Self::UnknownFlags(fl) => write!(f, "未知 flags 位: {fl:#x}"),
            Self::HeaderLen(h) => write!(f, "头部长度须为 {HEADER_LEN}: {h}"),
            Self::ByteLen { declared, actual } => {
                write!(f, "byte_len 声明 {declared} 与实际 {actual} 不符")
            }
            Self::ReservedNonZero => write!(f, "保留字节须为零"),
            Self::SegmentOverlap(s) => write!(f, "段 {s} 与其他段重叠"),
            Self::SegmentOutOfBounds {
                segment,
                offset,
                length,
            } => {
                write!(f, "段 {segment} 越界: {offset}+{length}")
            }
            Self::DirectoryLength { expected, actual } => {
                write!(f, "对象目录长度应为 {expected}: {actual}")
            }
            Self::DuplicateObjectId(id) => write!(f, "对象 ID 重复: {id}"),
            Self::DirectoryNotSorted => write!(f, "对象目录未按 ID 严格升序"),
            Self::UnknownTypeHandle(h) => write!(f, "未知类型句柄: {h}"),
            Self::RowOutOfRange { handle, row, rows } => {
                write!(f, "行 {row} 越出类型句柄 {handle} 的行数 {rows}")
            }
            Self::BucketWalk { consumed, segment } => {
                write!(f, "类型桶消费 {consumed} 字节不等于段长 {segment}")
            }
            Self::ArenaBounds { offset, length } => {
                write!(f, "arena 描述符越界: {offset}+{length}")
            }
            Self::ArenaMissing => write!(f, "arena 段缺失"),
            Self::RoomNameMissing => write!(f, "房名为空"),
            Self::InvalidUtf8 => write!(f, "字符串非 UTF-8"),
            Self::InvalidBool(b) => write!(f, "非法 bool 字节: {b}"),
            Self::NonFiniteFloat => write!(f, "浮点值为 NaN 或 Infinity"),
            Self::UnknownKind(k) => write!(f, "未知列类型字节: {k}"),
            Self::UnknownTag(t) => write!(f, "未知嵌套值标签: {t}"),
            Self::AmbiguousBitmap => write!(f, "缺失位与 null 位同时置位"),
            Self::BitmapTailBits => write!(f, "位图越行尾位非零"),
            Self::DuplicateRowRef { handle, row } => {
                write!(f, "类型句柄 {handle} 的行 {row} 被多个目录项引用")
            }
            Self::UnreferencedRow { handle, row } => {
                write!(f, "类型句柄 {handle} 的行 {row} 无目录项引用")
            }
            Self::KeyOrder => write!(f, "键未按字典序严格升序"),
            Self::LimitExceeded(what) => write!(f, "超出限额: {what}"),
            Self::DuplicateField { type_key, field } => {
                write!(f, "类型 {type_key} 内字段 {field} 重复")
            }
            Self::KindMismatch { type_key, field } => {
                write!(f, "类型 {type_key} 内字段 {field} 的值编码不一致")
            }
            Self::DepthExceeded => write!(f, "嵌套深度超限"),
            Self::MergeTickMismatch { base, overlay } => {
                write!(f, "合并块 tick 不符: {base} 与 {overlay}")
            }
            Self::MergeGenerationMismatch { base, overlay } => {
                write!(f, "合并块 generation 不符: {base} 与 {overlay}")
            }
            Self::MergePlanMismatch => write!(f, "合并块计划指纹不符"),
            Self::MergeRoomMismatch { base, overlay } => {
                write!(f, "合并块房名不符: {base} 与 {overlay}")
            }
            Self::MergeFlags { base, overlay } => {
                write!(
                    f,
                    "合并须为一个公开块与一个私有块: flags {base:#x} 与 {overlay:#x}"
                )
            }
            Self::MergeObjectConflict(id) => write!(f, "合并块对象 ID 冲突: {id}"),
        }
    }
}

impl std::error::Error for KkpvError {}

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
const DESCRIPTOR: usize = 8;

// 列类型字节；定宽 0..=4，变长 5..=7。
const KIND_BOOL: u8 = 0;
const KIND_INT: u8 = 1;
const KIND_UINT: u8 = 2;
const KIND_FLOAT: u8 = 3;
const KIND_REF: u8 = 4;
const KIND_STR: u8 = 5;
const KIND_ARRAY: u8 = 6;
const KIND_OBJECT: u8 = 7;

fn column_width(kind: u8) -> usize {
    match kind {
        KIND_BOOL => 1,
        KIND_INT | KIND_UINT | KIND_FLOAT | KIND_REF => 8,
        _ => DESCRIPTOR,
    }
}

fn is_variable(kind: u8) -> bool {
    matches!(kind, KIND_STR | KIND_ARRAY | KIND_OBJECT)
}

fn kind_of(value: &KkpvValue) -> Option<u8> {
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
const TAG_NULL: u8 = 0;
const TAG_BOOL: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_UINT: u8 = 3;
const TAG_FLOAT: u8 = 4;
const TAG_REF: u8 = 5;
const TAG_STR: u8 = 6;
const TAG_ARRAY: u8 = 7;
const TAG_OBJECT: u8 = 8;

// —— 编码 —————————————————————————————————————————————————

/// 校验值域并递归编码为嵌套标签形态。
fn encode_nested(
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
            let mut sorted = entries.clone();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            for (a, b) in sorted.iter().zip(sorted.iter().skip(1)) {
                if a.0 == b.0 {
                    return Err(KkpvError::DuplicateField {
                        type_key: "嵌套记录".into(),
                        field: a.0.clone(),
                    });
                }
            }
            out.push(TAG_OBJECT);
            let length = u32::try_from(sorted.len())
                .map_err(|_| KkpvError::LimitExceeded("max_object_entries"))?;
            out.extend_from_slice(&length.to_le_bytes());
            for (key, value) in sorted {
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                let key_len = u32::try_from(key.len())
                    .map_err(|_| KkpvError::LimitExceeded("max_string_bytes"))?;
                out.extend_from_slice(&key_len.to_le_bytes());
                out.extend_from_slice(key.as_bytes());
                encode_nested(&value, limits, depth + 1, out)?;
            }
        }
    }
    Ok(())
}

struct ArenaBuilder {
    bytes: Vec<u8>,
}

impl ArenaBuilder {
    /// arena 描述符是 u32 对；超出的长度必须报错而不是回绕。
    fn push_str(&mut self, s: &str) -> Result<(u32, u32), KkpvError> {
        self.push_bytes(s.as_bytes())
    }

    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(u32, u32), KkpvError> {
        let fits =
            |v: usize| u32::try_from(v).map_err(|_| KkpvError::LimitExceeded("max_arena_bytes"));
        let offset = fits(self.bytes.len())?;
        let length = fits(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        Ok((offset, length))
    }
}

fn bitmap_bytes(rows: u32) -> usize {
    (rows as usize).div_ceil(8)
}

fn bit_set(bitmap: &[u8], row: u32) -> bool {
    bitmap[row as usize / 8] & (1 << (row % 8)) != 0
}

/// 将输入编码为完整的 KKPV 块。对象按 ID 升序、类型桶与字段按键
/// 排序后写入，相同输入不依赖字段或对象的提交顺序得到相同字节。
///
/// # Errors
/// 值域或限额违规、对象 ID 或字段键重复、同键值编码不一致时返回错误，
/// 不产出部分数据。
pub fn encode(input: &KkpvBlockInput, limits: &KkpvLimits) -> Result<Vec<u8>, KkpvError> {
    if input.room_name.is_empty() {
        return Err(KkpvError::RoomNameMissing);
    }
    if input.room_name.len() > limits.max_string_bytes {
        return Err(KkpvError::LimitExceeded("max_string_bytes"));
    }
    if input.objects.len() > limits.max_objects {
        return Err(KkpvError::LimitExceeded("max_objects"));
    }
    let terrain = input.terrain.as_deref();
    if terrain.is_some_and(|t| t.len() > limits.max_terrain_bytes) {
        return Err(KkpvError::LimitExceeded("max_terrain_bytes"));
    }

    let mut objects = input.objects.clone();
    objects.sort_by_key(|o| o.id);
    for (a, b) in objects.iter().zip(objects.iter().skip(1)) {
        if a.id == b.id {
            return Err(KkpvError::DuplicateObjectId(a.id));
        }
    }

    // 类型分桶；BTreeMap 同时给出类型键的确定顺序与句柄分配。
    let mut buckets: std::collections::BTreeMap<&str, Vec<&KkpvObject>> =
        std::collections::BTreeMap::new();
    for object in &objects {
        buckets
            .entry(object.type_key.as_str())
            .or_default()
            .push(object);
    }
    if buckets.len() > limits.max_types {
        return Err(KkpvError::LimitExceeded("max_types"));
    }

    struct FieldPlan<'o> {
        key_desc: (u32, u32),
        kind: u8,
        column: Vec<Option<&'o KkpvValue>>,
    }
    struct BucketPlan<'o> {
        rows: u32,
        fields: Vec<FieldPlan<'o>>,
    }
    let mut arena = ArenaBuilder { bytes: Vec::new() };
    let room_desc = arena.push_str(&input.room_name)?;
    let mut type_descs = Vec::with_capacity(buckets.len());
    let mut plans = Vec::with_capacity(buckets.len());
    for (type_key, objects) in &buckets {
        if objects.len() > u32::MAX as usize {
            return Err(KkpvError::LimitExceeded("max_objects"));
        }
        if type_key.len() > limits.max_string_bytes {
            return Err(KkpvError::LimitExceeded("max_string_bytes"));
        }
        type_descs.push(arena.push_str(type_key)?);
        // 字段目录是全桶键并集；重复只指同一对象内出现两次同名键。
        let mut field_order: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for object in objects {
            let mut seen = std::collections::BTreeSet::new();
            for (key, _) in &object.fields {
                if key == "id" {
                    return Err(KkpvError::DuplicateField {
                        type_key: (*type_key).into(),
                        field: "id".into(),
                    });
                }
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                if !seen.insert(key.as_str()) {
                    return Err(KkpvError::DuplicateField {
                        type_key: (*type_key).into(),
                        field: key.clone(),
                    });
                }
                field_order.insert(key.as_str());
            }
        }
        if field_order.len() > limits.max_fields_per_type {
            return Err(KkpvError::LimitExceeded("max_fields_per_type"));
        }
        let mut fields = Vec::with_capacity(field_order.len());
        for key in field_order {
            let key_desc = arena.push_str(key)?;
            let mut kind: Option<u8> = None;
            let mut column = Vec::with_capacity(objects.len());
            for object in objects {
                let value = object
                    .fields
                    .iter()
                    .find_map(|(k, v)| (k == key).then_some(v));
                let Some(value) = value else {
                    column.push(None);
                    continue;
                };
                if let Some(value_kind) = kind_of(value) {
                    match kind {
                        None => kind = Some(value_kind),
                        Some(existing) if existing != value_kind => {
                            return Err(KkpvError::KindMismatch {
                                type_key: (*type_key).into(),
                                field: key.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
                column.push(Some(value));
            }
            // 全 null 字段没有可推断的列类型；bool 宽度最小，位图已表达全部信息。
            let kind = kind.unwrap_or(KIND_BOOL);
            fields.push(FieldPlan {
                key_desc,
                kind,
                column,
            });
        }
        plans.push(BucketPlan {
            rows: objects.len() as u32,
            fields,
        });
    }

    // 变长载荷在结构段之前进入 arena；描述符按桶/字段/行的确定顺序写入。
    let mut variable_cells: Vec<Vec<Vec<(u32, u32)>>> = Vec::with_capacity(plans.len());
    for plan in &plans {
        let mut per_field = Vec::with_capacity(plan.fields.len());
        for field in &plan.fields {
            let mut cells = Vec::with_capacity(field.column.len());
            if is_variable(field.kind) {
                for value in &field.column {
                    match value {
                        None | Some(KkpvValue::Null) => cells.push((0u32, 0u32)),
                        Some(value) => {
                            let mut payload = Vec::new();
                            encode_nested(value, limits, 0, &mut payload)?;
                            cells.push(arena.push_bytes(&payload)?);
                        }
                    }
                }
            }
            per_field.push(cells);
        }
        variable_cells.push(per_field);
    }
    if arena.bytes.len() > limits.max_arena_bytes {
        return Err(KkpvError::LimitExceeded("max_arena_bytes"));
    }

    let directory_len = objects.len() * DIRECTORY_ENTRY;
    let mut bucket_bytes = Vec::new();
    for (i, (plan, per_field)) in plans.iter().zip(&variable_cells).enumerate() {
        let type_desc = type_descs[i];
        bucket_bytes.extend_from_slice(&type_desc.0.to_le_bytes());
        bucket_bytes.extend_from_slice(&type_desc.1.to_le_bytes());
        bucket_bytes.extend_from_slice(&plan.rows.to_le_bytes());
        bucket_bytes.extend_from_slice(&(plan.fields.len() as u32).to_le_bytes());
        for field in &plan.fields {
            bucket_bytes.extend_from_slice(&field.key_desc.0.to_le_bytes());
            bucket_bytes.extend_from_slice(&field.key_desc.1.to_le_bytes());
            bucket_bytes.push(field.kind);
            bucket_bytes.extend_from_slice(&[0; 3]);
        }
        let bitmap_len = bitmap_bytes(plan.rows);
        for field in &plan.fields {
            let mut bitmap = vec![0u8; bitmap_len];
            for (row, value) in field.column.iter().enumerate() {
                if value.is_none() {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for field in &plan.fields {
            let mut bitmap = vec![0u8; bitmap_len];
            for (row, value) in field.column.iter().enumerate() {
                if matches!(value, Some(KkpvValue::Null)) {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for (field, cells) in plan.fields.iter().zip(per_field) {
            let width = column_width(field.kind);
            for (row, value) in field.column.iter().enumerate() {
                match value {
                    None | Some(KkpvValue::Null) => {
                        bucket_bytes.extend(std::iter::repeat_n(0u8, width));
                    }
                    Some(value) => {
                        if is_variable(field.kind) {
                            let cell = cells[row];
                            bucket_bytes.extend_from_slice(&cell.0.to_le_bytes());
                            bucket_bytes.extend_from_slice(&cell.1.to_le_bytes());
                        } else {
                            match value {
                                KkpvValue::Bool(b) => bucket_bytes.push(*b as u8),
                                KkpvValue::Int(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Uint(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Float(v) => {
                                    if !v.is_finite() {
                                        return Err(KkpvError::NonFiniteFloat);
                                    }
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Ref(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                _ => unreachable!("定宽列的值类型已由 kind 校验"),
                            }
                        }
                    }
                }
            }
        }
    }

    let terrain_len = terrain.map_or(0, |t| t.len());
    let total = HEADER_LEN + terrain_len + directory_len + bucket_bytes.len() + arena.bytes.len();
    if total > limits.max_bytes {
        return Err(KkpvError::LimitExceeded("max_bytes"));
    }
    let fits = |value: usize| -> Result<u32, KkpvError> {
        u32::try_from(value).map_err(|_| KkpvError::LimitExceeded("max_bytes"))
    };
    // 零长度段写 (0, 0)：不存在的段不携带 offset，与解码端规范形一致。
    let seg_pair = |offset: usize, length: usize| -> Result<(u32, u32), KkpvError> {
        if length == 0 {
            Ok((0, 0))
        } else {
            Ok((fits(offset)?, fits(length)?))
        }
    };
    let terrain_off = HEADER_LEN;
    let directory_off = terrain_off + terrain_len;
    let buckets_off = directory_off + directory_len;
    let arena_off = buckets_off + bucket_bytes.len();

    let mut out = vec![0u8; total];
    out[..4].copy_from_slice(&MAGIC);
    out[OFF_VERSION..OFF_VERSION + 2].copy_from_slice(&VERSION.to_le_bytes());
    let flags = u16::from(input.private) * FLAG_PRIVATE;
    out[OFF_FLAGS..OFF_FLAGS + 2].copy_from_slice(&flags.to_le_bytes());
    out[OFF_BYTE_LEN..OFF_BYTE_LEN + 4].copy_from_slice(&fits(total)?.to_le_bytes());
    out[OFF_HEADER_LEN..OFF_HEADER_LEN + 4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    out[OFF_TICK..OFF_TICK + 8].copy_from_slice(&input.tick.to_le_bytes());
    out[OFF_GENERATION..OFF_GENERATION + 8].copy_from_slice(&input.generation.to_le_bytes());
    out[OFF_PLAN_HASH..OFF_PLAN_HASH + 32].copy_from_slice(&input.plan_hash);
    out[OFF_AUDIENCE_HASH..OFF_AUDIENCE_HASH + 32].copy_from_slice(&input.audience_hash);
    let (room_off, room_len) = room_desc;
    out[OFF_ROOM_NAME..OFF_ROOM_NAME + 4].copy_from_slice(&room_off.to_le_bytes());
    out[OFF_ROOM_NAME + 4..OFF_ROOM_NAME + 8].copy_from_slice(&room_len.to_le_bytes());
    out[OFF_OBJECT_COUNT..OFF_OBJECT_COUNT + 4]
        .copy_from_slice(&fits(objects.len())?.to_le_bytes());
    out[OFF_TYPE_COUNT..OFF_TYPE_COUNT + 4].copy_from_slice(&fits(plans.len())?.to_le_bytes());
    let (terrain_o, terrain_l) = seg_pair(terrain_off, terrain_len)?;
    out[OFF_TERRAIN..OFF_TERRAIN + 4].copy_from_slice(&terrain_o.to_le_bytes());
    out[OFF_TERRAIN + 4..OFF_TERRAIN + 8].copy_from_slice(&terrain_l.to_le_bytes());
    if let Some(terrain) = terrain {
        out[terrain_off..terrain_off + terrain_len].copy_from_slice(terrain);
    }
    let (directory_o, directory_l) = seg_pair(directory_off, directory_len)?;
    out[OFF_DIRECTORY..OFF_DIRECTORY + 4].copy_from_slice(&directory_o.to_le_bytes());
    out[OFF_DIRECTORY + 4..OFF_DIRECTORY + 8].copy_from_slice(&directory_l.to_le_bytes());
    let (buckets_o, buckets_l) = seg_pair(buckets_off, bucket_bytes.len())?;
    out[OFF_BUCKETS..OFF_BUCKETS + 4].copy_from_slice(&buckets_o.to_le_bytes());
    out[OFF_BUCKETS + 4..OFF_BUCKETS + 8].copy_from_slice(&buckets_l.to_le_bytes());
    let (arena_o, arena_l) = seg_pair(arena_off, arena.bytes.len())?;
    out[OFF_ARENA..OFF_ARENA + 4].copy_from_slice(&arena_o.to_le_bytes());
    out[OFF_ARENA + 4..OFF_ARENA + 8].copy_from_slice(&arena_l.to_le_bytes());
    // 目录按对象（已按 ID 升序）写入；句柄是桶在类型键序中的秩，
    // 行号是对象在其桶内按 ID 的序。
    let mut row_counters: std::collections::BTreeMap<&str, (u32, u32)> = buckets
        .keys()
        .enumerate()
        .map(|(rank, key)| (*key, (rank as u32, 0)))
        .collect();
    for (i, object) in objects.iter().enumerate() {
        let counter = row_counters
            .get_mut(object.type_key.as_str())
            .expect("对象必属已建桶");
        let at = directory_off + i * DIRECTORY_ENTRY;
        out[at..at + 8].copy_from_slice(&object.id.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&counter.0.to_le_bytes());
        out[at + 12..at + 16].copy_from_slice(&counter.1.to_le_bytes());
        counter.1 += 1;
    }
    out[buckets_off..buckets_off + bucket_bytes.len()].copy_from_slice(&bucket_bytes);
    out[arena_off..arena_off + arena.bytes.len()].copy_from_slice(&arena.bytes);
    Ok(out)
}

// —— 解码视图 ——————————————————————————————————————————————

struct FieldView<'a> {
    key: &'a str,
    kind: u8,
    missing: &'a [u8],
    null: &'a [u8],
    column: &'a [u8],
}

struct BucketView<'a> {
    type_key: &'a str,
    rows: u32,
    fields: Vec<FieldView<'a>>,
}

struct DirectoryEntry {
    id: u64,
    handle: u32,
    row: u32,
}

/// 已通过结构校验的块视图。字段值在读取时解码；变长值内的字符串
/// 在读取时校验 UTF-8。视图借用块字节，块本身不可变。
pub struct KkpvView<'a> {
    data: &'a [u8],
    tick: u64,
    generation: u64,
    plan_hash: [u8; 32],
    audience_hash: [u8; 32],
    flags: u16,
    room_name: &'a str,
    terrain: Option<&'a [u8]>,
    arena: &'a [u8],
    limits: KkpvLimits,
    objects: Vec<DirectoryEntry>,
    buckets: Vec<BucketView<'a>>,
}

fn read_u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_u64_at(data: &[u8], offset: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&data[offset..offset + 8]);
    u64::from_le_bytes(bytes)
}

/// 检查段表：各段完整落在块内、不与头部及彼此重叠；零长度段豁免重叠检查。
fn check_segments(
    data_len: usize,
    segments: [(&'static str, usize, usize); 4],
) -> Result<(), KkpvError> {
    // 零长度段不存在内容，offset 必须为零；与头部保留字节同一规范形。
    if segments
        .iter()
        .any(|(_, offset, length)| *length == 0 && *offset != 0)
    {
        return Err(KkpvError::SegmentOutOfBounds {
            segment: segments
                .iter()
                .find(|(_, offset, length)| *length == 0 && *offset != 0)
                .map(|(s, _, _)| *s)
                .unwrap_or("arena"),
            offset: 0,
            length: 0,
        });
    }
    let mut present: Vec<(&'static str, usize, usize)> = segments
        .into_iter()
        .filter(|(_, _, length)| *length > 0)
        .collect();
    for (segment, offset, length) in &present {
        let end = offset.checked_add(*length);
        if !matches!(end, Some(end) if *offset >= HEADER_LEN && end <= data_len) {
            return Err(KkpvError::SegmentOutOfBounds {
                segment,
                offset: *offset,
                length: *length,
            });
        }
    }
    present.sort_by_key(|(_, offset, _)| *offset);
    for (a, b) in present.iter().zip(present.iter().skip(1)) {
        if a.1 + a.2 > b.1 {
            return Err(KkpvError::SegmentOverlap(b.0));
        }
    }
    Ok(())
}

/// 嵌套深度的结构硬上限：编码与解码共用，配置不得越过。
/// 上限取嵌套载荷的保守上界——每层至少消费 1 字节，而块受
/// max_bytes 约束；1024 足以覆盖任何合法嵌套并防栈溢出。
const NESTING_HARD_CAP: usize = 1024;

fn decode_nested(
    limits: &KkpvLimits,
    depth: usize,
    bytes: &mut &[u8],
) -> Result<KkpvValue, KkpvError> {
    if depth > limits.max_value_depth.min(NESTING_HARD_CAP) {
        return Err(KkpvError::DepthExceeded);
    }
    let (&tag, rest) = bytes.split_first().ok_or(KkpvError::UnknownTag(0))?;
    *bytes = rest;
    fn take<'b>(bytes: &mut &'b [u8], n: usize) -> Result<&'b [u8], KkpvError> {
        if bytes.len() < n {
            return Err(KkpvError::ArenaBounds {
                offset: 0,
                length: n,
            });
        }
        let (head, tail) = bytes.split_at(n);
        *bytes = tail;
        Ok(head)
    }
    match tag {
        TAG_NULL => Ok(KkpvValue::Null),
        TAG_BOOL => Ok(KkpvValue::Bool(match take(bytes, 1)?[0] {
            0 => false,
            1 => true,
            b => return Err(KkpvError::InvalidBool(b)),
        })),
        TAG_INT => Ok(KkpvValue::Int(i64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_UINT => Ok(KkpvValue::Uint(u64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_FLOAT => {
            let v = f64::from_le_bytes(take(bytes, 8)?.try_into().unwrap());
            if !v.is_finite() {
                return Err(KkpvError::NonFiniteFloat);
            }
            Ok(KkpvValue::Float(v))
        }
        TAG_REF => Ok(KkpvValue::Ref(u64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_STR => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_string_bytes {
                return Err(KkpvError::LimitExceeded("max_string_bytes"));
            }
            let raw = take(bytes, len)?;
            let s = std::str::from_utf8(raw).map_err(|_| KkpvError::InvalidUtf8)?;
            Ok(KkpvValue::Str(s.into()))
        }
        TAG_ARRAY => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_array_len {
                return Err(KkpvError::LimitExceeded("max_array_len"));
            }
            let mut items = Vec::with_capacity(len.min(bytes.len()));
            for _ in 0..len {
                items.push(decode_nested(limits, depth + 1, bytes)?);
            }
            Ok(KkpvValue::Array(items))
        }
        TAG_OBJECT => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_object_entries {
                return Err(KkpvError::LimitExceeded("max_object_entries"));
            }
            // 记录键严格升序是编码侧的规范形态；解码镜像校验，
            // 重复键的取舍不由消费者偶然决定。
            let mut entries = Vec::with_capacity(len.min(bytes.len()));
            let mut last_key: Option<&str> = None;
            for _ in 0..len {
                let key_len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
                if key_len > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                let key = std::str::from_utf8(take(bytes, key_len)?)
                    .map_err(|_| KkpvError::InvalidUtf8)?;
                if last_key.is_some_and(|k| key <= k) {
                    return Err(KkpvError::KeyOrder);
                }
                last_key = Some(key);
                let value = decode_nested(limits, depth + 1, bytes)?;
                entries.push((key.into(), value));
            }
            Ok(KkpvValue::Object(entries))
        }
        _ => Err(KkpvError::UnknownTag(tag)),
    }
}

impl<'a> KkpvView<'a> {
    /// 解码并建立块视图，执行模块文档列举的全部结构校验。
    ///
    /// # Errors
    /// 结构非法、限额超限或段关系不成立时返回错误，不产生部分视图。
    pub fn decode(data: &'a [u8], limits: &KkpvLimits) -> Result<Self, KkpvError> {
        if data.len() < HEADER_LEN {
            return Err(KkpvError::ByteLen {
                declared: 0,
                actual: data.len(),
            });
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[..4]);
        if magic != MAGIC {
            return Err(KkpvError::BadMagic(magic));
        }
        let version = u16::from_le_bytes([data[OFF_VERSION], data[OFF_VERSION + 1]]);
        if version != VERSION {
            return Err(KkpvError::UnsupportedVersion(version));
        }
        let flags = u16::from_le_bytes([data[OFF_FLAGS], data[OFF_FLAGS + 1]]);
        if flags & !FLAG_PRIVATE != 0 {
            return Err(KkpvError::UnknownFlags(flags));
        }
        let declared_len = read_u32_at(data, OFF_BYTE_LEN);
        if declared_len as usize != data.len() {
            return Err(KkpvError::ByteLen {
                declared: declared_len,
                actual: data.len(),
            });
        }
        let header_len = read_u32_at(data, OFF_HEADER_LEN);
        if header_len as usize != HEADER_LEN {
            return Err(KkpvError::HeaderLen(header_len));
        }
        if data[OFF_RESERVED..HEADER_LEN].iter().any(|&b| b != 0) {
            return Err(KkpvError::ReservedNonZero);
        }
        let object_count = read_u32_at(data, OFF_OBJECT_COUNT) as usize;
        let type_count = read_u32_at(data, OFF_TYPE_COUNT) as usize;
        if object_count > limits.max_objects {
            return Err(KkpvError::LimitExceeded("max_objects"));
        }
        if type_count > limits.max_types {
            return Err(KkpvError::LimitExceeded("max_types"));
        }
        let segment = |offset_pos: usize| {
            (
                read_u32_at(data, offset_pos) as usize,
                read_u32_at(data, offset_pos + 4) as usize,
            )
        };
        let (terrain_off, terrain_len) = segment(OFF_TERRAIN);
        let (directory_off, directory_len) = segment(OFF_DIRECTORY);
        let (buckets_off, buckets_len) = segment(OFF_BUCKETS);
        let (arena_off, arena_len) = segment(OFF_ARENA);
        check_segments(
            data.len(),
            [
                ("terrain", terrain_off, terrain_len),
                ("directory", directory_off, directory_len),
                ("buckets", buckets_off, buckets_len),
                ("arena", arena_off, arena_len),
            ],
        )?;
        if arena_len == 0 {
            return Err(KkpvError::ArenaMissing);
        }
        if data.len() > limits.max_bytes {
            return Err(KkpvError::LimitExceeded("max_bytes"));
        }
        if arena_len > limits.max_arena_bytes {
            return Err(KkpvError::LimitExceeded("max_arena_bytes"));
        }
        if terrain_len > limits.max_terrain_bytes {
            return Err(KkpvError::LimitExceeded("max_terrain_bytes"));
        }
        let arena = &data[arena_off..arena_off + arena_len];
        let arena_str = |desc: (usize, usize)| -> Result<&'a str, KkpvError> {
            let end = desc.0.checked_add(desc.1).ok_or(KkpvError::ArenaBounds {
                offset: desc.0,
                length: desc.1,
            })?;
            if end > arena.len() || desc.1 > limits.max_string_bytes {
                return Err(KkpvError::ArenaBounds {
                    offset: desc.0,
                    length: desc.1,
                });
            }
            std::str::from_utf8(&arena[desc.0..end]).map_err(|_| KkpvError::InvalidUtf8)
        };
        let room_name = arena_str(segment(OFF_ROOM_NAME))?;
        if room_name.is_empty() {
            return Err(KkpvError::RoomNameMissing);
        }

        if directory_len != object_count * DIRECTORY_ENTRY {
            return Err(KkpvError::DirectoryLength {
                expected: object_count * DIRECTORY_ENTRY,
                actual: directory_len,
            });
        }
        let mut objects: Vec<DirectoryEntry> = Vec::with_capacity(object_count);
        for i in 0..object_count {
            let at = directory_off + i * DIRECTORY_ENTRY;
            let id = read_u64_at(data, at);
            let handle = read_u32_at(data, at + 8);
            let row = read_u32_at(data, at + 12);
            if let Some(last) = objects.last()
                && id <= last.id
            {
                return Err(if id == last.id {
                    KkpvError::DuplicateObjectId(id)
                } else {
                    KkpvError::DirectoryNotSorted
                });
            }
            if handle as usize >= type_count {
                return Err(KkpvError::UnknownTypeHandle(handle));
            }
            objects.push(DirectoryEntry { id, handle, row });
        }

        let mut walk = &data[buckets_off..buckets_off + buckets_len];
        let mut buckets: Vec<BucketView> = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            let step = |walk: &mut &[u8], n: usize| -> Result<(), KkpvError> {
                if walk.len() < n {
                    return Err(KkpvError::BucketWalk {
                        consumed: buckets_len - walk.len(),
                        segment: buckets_len,
                    });
                }
                *walk = &walk[n..];
                Ok(())
            };
            if walk.len() < BUCKET_HEADER {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let type_key =
                arena_str((read_u32_at(walk, 0) as usize, read_u32_at(walk, 4) as usize))?;
            let rows = read_u32_at(walk, 8);
            let field_count = read_u32_at(walk, 12) as usize;
            if field_count > limits.max_fields_per_type {
                return Err(KkpvError::LimitExceeded("max_fields_per_type"));
            }
            if rows as usize > limits.max_objects {
                return Err(KkpvError::LimitExceeded("max_objects"));
            }
            if let Some(last) = buckets.last()
                && type_key <= last.type_key
            {
                return Err(KkpvError::KeyOrder);
            }
            step(&mut walk, BUCKET_HEADER)?;
            if walk.len() < field_count * FIELD_ENTRY {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let mut kinds = Vec::with_capacity(field_count);
            let mut keys = Vec::with_capacity(field_count);
            let mut last_key: Option<&str> = None;
            for i in 0..field_count {
                let at = i * FIELD_ENTRY;
                let key = arena_str((
                    read_u32_at(walk, at) as usize,
                    read_u32_at(walk, at + 4) as usize,
                ))?;
                if key == "id" {
                    return Err(KkpvError::DuplicateField {
                        type_key: type_key.into(),
                        field: "id".into(),
                    });
                }
                if last_key.is_some_and(|k| key <= k) {
                    return Err(KkpvError::KeyOrder);
                }
                last_key = Some(key);
                keys.push(key);
                let kind = walk[at + 8];
                if kind > KIND_OBJECT {
                    return Err(KkpvError::UnknownKind(kind));
                }
                if walk[at + 9..at + FIELD_ENTRY].iter().any(|&b| b != 0) {
                    return Err(KkpvError::ReservedNonZero);
                }
                kinds.push(kind);
            }
            step(&mut walk, field_count * FIELD_ENTRY)?;
            let bitmap_len = bitmap_bytes(rows);
            let bitmaps_needed = bitmap_len
                .checked_mul(field_count)
                .and_then(|v| v.checked_mul(2));
            let bitmaps_needed = match bitmaps_needed {
                Some(v) if v <= walk.len() => v,
                _ => {
                    return Err(KkpvError::BucketWalk {
                        consumed: buckets_len - walk.len(),
                        segment: buckets_len,
                    });
                }
            };
            let missing_all = &walk[..bitmap_len * field_count];
            let null_all = &walk[bitmap_len * field_count..bitmaps_needed];
            // 位图在全部已声明行上检查：越行尾位必须为零，
            // 缺失与 null 不得同置。
            let valid_bits: u8 = if rows.is_multiple_of(8) {
                0xFF
            } else {
                (1u8 << (rows % 8)) - 1
            };
            for i in 0..field_count {
                let missing = &missing_all[i * bitmap_len..(i + 1) * bitmap_len];
                let null = &null_all[i * bitmap_len..(i + 1) * bitmap_len];
                for (m, n) in missing.iter().zip(null.iter()) {
                    if m & n != 0 {
                        return Err(KkpvError::AmbiguousBitmap);
                    }
                }
                if missing[bitmap_len - 1] & !valid_bits != 0
                    || null[bitmap_len - 1] & !valid_bits != 0
                {
                    return Err(KkpvError::BitmapTailBits);
                }
            }
            step(&mut walk, bitmaps_needed)?;
            let mut total_width = 0usize;
            let mut widths = Vec::with_capacity(field_count);
            for &kind in &kinds {
                let width = column_width(kind).checked_mul(rows as usize);
                let width = match width {
                    Some(w) => w,
                    None => return Err(KkpvError::LimitExceeded("max_objects")),
                };
                widths.push(width);
                total_width += width;
            }
            if walk.len() < total_width {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let mut fields = Vec::with_capacity(field_count);
            let mut column_data = walk;
            for (i, &kind) in kinds.iter().enumerate() {
                let width = widths[i];
                let column = &column_data[..width];
                column_data = &column_data[width..];
                // 定宽列的值域在建立视图时完成检查。
                if kind == KIND_BOOL
                    && let Some(&b) = column.iter().find(|&&b| b > 1)
                {
                    return Err(KkpvError::InvalidBool(b));
                }
                if kind == KIND_FLOAT {
                    for chunk in column.as_chunks::<8>().0 {
                        let v = f64::from_le_bytes(*chunk);
                        if !v.is_finite() {
                            return Err(KkpvError::NonFiniteFloat);
                        }
                    }
                }
                if is_variable(kind) {
                    for chunk in column.as_chunks::<DESCRIPTOR>().0 {
                        let offset = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize;
                        let length = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as usize;
                        let end = offset.checked_add(length);
                        if !matches!(end, Some(end) if end <= arena.len()) {
                            return Err(KkpvError::ArenaBounds { offset, length });
                        }
                    }
                }
                fields.push(FieldView {
                    key: keys[i],
                    kind,
                    missing: &missing_all[i * bitmap_len..(i + 1) * bitmap_len],
                    null: &null_all[i * bitmap_len..(i + 1) * bitmap_len],
                    column,
                });
            }
            step(&mut walk, total_width)?;
            buckets.push(BucketView {
                type_key,
                rows,
                fields,
            });
        }
        if !walk.is_empty() {
            return Err(KkpvError::BucketWalk {
                consumed: buckets_len - walk.len(),
                segment: buckets_len,
            });
        }
        // 行占用不变式：每桶的每行恰被一个目录项引用。重复引用使
        // 一行数据以多个 ID 显形，未引用行是幻影数据，两者都拒绝。
        let mut occupied: Vec<Vec<bool>> = buckets
            .iter()
            .map(|b| vec![false; b.rows as usize])
            .collect();
        for entry in &objects {
            let bucket = &buckets[entry.handle as usize];
            if entry.row >= bucket.rows {
                return Err(KkpvError::RowOutOfRange {
                    handle: entry.handle,
                    row: entry.row,
                    rows: bucket.rows,
                });
            }
            let slot = &mut occupied[entry.handle as usize][entry.row as usize];
            if *slot {
                return Err(KkpvError::DuplicateRowRef {
                    handle: entry.handle,
                    row: entry.row,
                });
            }
            *slot = true;
        }
        for (handle, rows) in occupied.iter().enumerate() {
            if let Some(row) = rows.iter().position(|free| !free) {
                return Err(KkpvError::UnreferencedRow {
                    handle: handle as u32,
                    row: row as u32,
                });
            }
        }
        let mut meta_plan = [0u8; 32];
        meta_plan.copy_from_slice(&data[OFF_PLAN_HASH..OFF_PLAN_HASH + 32]);
        let mut meta_audience = [0u8; 32];
        meta_audience.copy_from_slice(&data[OFF_AUDIENCE_HASH..OFF_AUDIENCE_HASH + 32]);
        Ok(Self {
            data,
            tick: read_u64_at(data, OFF_TICK),
            generation: read_u64_at(data, OFF_GENERATION),
            plan_hash: meta_plan,
            audience_hash: meta_audience,
            flags,
            room_name,
            terrain: (terrain_len > 0).then(|| &data[terrain_off..terrain_off + terrain_len]),
            arena,
            limits: limits.clone(),
            objects,
            buckets,
        })
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn plan_hash(&self) -> [u8; 32] {
        self.plan_hash
    }

    pub fn audience_hash(&self) -> [u8; 32] {
        self.audience_hash
    }

    pub fn is_private(&self) -> bool {
        self.flags & FLAG_PRIVATE != 0
    }

    pub fn room_name(&self) -> &str {
        self.room_name
    }

    pub fn terrain(&self) -> Option<&[u8]> {
        self.terrain
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn object_id(&self, index: usize) -> u64 {
        self.objects[index].id
    }

    /// 对象的类型稳定键。
    ///
    /// # Panics
    /// index 越界时 panic；调用方先以 [`Self::object_count`] 约束。
    pub fn object_type(&self, index: usize) -> &str {
        self.buckets[self.objects[index].handle as usize].type_key
    }

    /// 按 ID 定位对象；二分依赖目录升序不变式。
    pub fn object_index_by_id(&self, id: u64) -> Option<usize> {
        self.objects.binary_search_by_key(&id, |e| e.id).ok()
    }

    /// 读取对象的字段值。外层 None 表示字段缺失（未声明于该行），
    /// `Some(KkpvValue::Null)` 表示显式 null。
    ///
    /// # Errors
    /// 变长值解码失败（UTF-8、标签、深度、限额）时返回错误。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field(&self, index: usize, key: &str) -> Result<Option<KkpvValue>, KkpvError> {
        let entry = &self.objects[index];
        let bucket = &self.buckets[entry.handle as usize];
        let Ok(position) = bucket.fields.binary_search_by(|f| f.key.cmp(key)) else {
            return Ok(None);
        };
        let field = &bucket.fields[position];
        if bit_set(field.missing, entry.row) {
            return Ok(None);
        }
        if bit_set(field.null, entry.row) {
            return Ok(Some(KkpvValue::Null));
        }
        let width = column_width(field.kind);
        let cell = &field.column[entry.row as usize * width..][..width];
        let value = match field.kind {
            KIND_BOOL => KkpvValue::Bool(cell[0] == 1),
            KIND_INT => KkpvValue::Int(i64::from_le_bytes(cell.try_into().unwrap())),
            KIND_UINT => KkpvValue::Uint(u64::from_le_bytes(cell.try_into().unwrap())),
            KIND_FLOAT => {
                let v = f64::from_le_bytes(cell.try_into().unwrap());
                if !v.is_finite() {
                    return Err(KkpvError::NonFiniteFloat);
                }
                KkpvValue::Float(v)
            }
            KIND_REF => KkpvValue::Ref(u64::from_le_bytes(cell.try_into().unwrap())),
            _ => {
                let offset = u32::from_le_bytes(cell[..4].try_into().unwrap()) as usize;
                let length = u32::from_le_bytes(cell[4..].try_into().unwrap()) as usize;
                let end = offset.checked_add(length);
                let end = match end {
                    Some(end) if end <= self.arena.len() => end,
                    _ => return Err(KkpvError::ArenaBounds { offset, length }),
                };
                let mut payload = &self.arena[offset..end];
                let value = decode_nested(&self.limits, 0, &mut payload)?;
                if !payload.is_empty() {
                    return Err(KkpvError::ArenaBounds { offset, length });
                }
                value
            }
        };
        Ok(Some(value))
    }

    /// 列出对象上已声明且未缺失的字段键；不含由目录承担的 `id`。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field_keys(&self, index: usize) -> Vec<&str> {
        let entry = &self.objects[index];
        let bucket = &self.buckets[entry.handle as usize];
        bucket
            .fields
            .iter()
            .filter(|field| !bit_set(field.missing, entry.row))
            .map(|field| field.key)
            .collect()
    }

    /// 原始块字节。
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}

// —— 公开/私有合并 ——————————————————————————————————————————

/// 公开块与私有 overlay 的合并视图。对象集合取并集并保持 ID 升序；
/// 地形取公开块。两个块须来自相同 tick/generation/计划与房间。
pub struct MergedView<'v> {
    public: &'v KkpvView<'v>,
    overlay: &'v KkpvView<'v>,
    entries: Vec<(u64, bool, usize)>,
}

/// 合并同一世界版本下的公开块与私有 overlay。
///
/// # Errors
/// 块身份（tick/generation/计划/房间）不一致、双方 flags 不是
/// 一公开一私有，或对象 ID 同时出现在两个块中时返回错误。
pub fn merge<'v>(
    public: &'v KkpvView<'v>,
    overlay: &'v KkpvView<'v>,
) -> Result<MergedView<'v>, KkpvError> {
    if public.is_private() || !overlay.is_private() {
        return Err(KkpvError::MergeFlags {
            base: if public.is_private() { FLAG_PRIVATE } else { 0 },
            overlay: if overlay.is_private() {
                FLAG_PRIVATE
            } else {
                0
            },
        });
    }
    if public.tick != overlay.tick {
        return Err(KkpvError::MergeTickMismatch {
            base: public.tick,
            overlay: overlay.tick,
        });
    }
    if public.generation != overlay.generation {
        return Err(KkpvError::MergeGenerationMismatch {
            base: public.generation,
            overlay: overlay.generation,
        });
    }
    if public.plan_hash != overlay.plan_hash {
        return Err(KkpvError::MergePlanMismatch);
    }
    if public.room_name != overlay.room_name {
        return Err(KkpvError::MergeRoomMismatch {
            base: public.room_name.into(),
            overlay: overlay.room_name.into(),
        });
    }
    let mut entries = Vec::with_capacity(public.objects.len() + overlay.objects.len());
    let mut i = 0;
    let mut j = 0;
    while i < public.objects.len() && j < overlay.objects.len() {
        let a = public.objects[i].id;
        let b = overlay.objects[j].id;
        match a.cmp(&b) {
            std::cmp::Ordering::Less => {
                entries.push((a, false, i));
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                entries.push((b, true, j));
                j += 1;
            }
            std::cmp::Ordering::Equal => return Err(KkpvError::MergeObjectConflict(a)),
        }
    }
    for (i, entry) in public.objects.iter().enumerate().skip(i) {
        entries.push((entry.id, false, i));
    }
    for (j, entry) in overlay.objects.iter().enumerate().skip(j) {
        entries.push((entry.id, true, j));
    }
    Ok(MergedView {
        public,
        overlay,
        entries,
    })
}

impl MergedView<'_> {
    pub fn object_count(&self) -> usize {
        self.entries.len()
    }

    pub fn object_id(&self, index: usize) -> u64 {
        self.entries[index].0
    }

    /// 对象来源：false 为公开块，true 为私有 overlay。
    pub fn from_overlay(&self, index: usize) -> bool {
        self.entries[index].1
    }

    pub fn tick(&self) -> u64 {
        self.public.tick
    }

    pub fn generation(&self) -> u64 {
        self.public.generation
    }

    pub fn plan_hash(&self) -> [u8; 32] {
        self.public.plan_hash
    }

    pub fn room_name(&self) -> &str {
        self.public.room_name
    }

    pub fn terrain(&self) -> Option<&[u8]> {
        self.public.terrain
    }

    /// 读取对象的字段值，语义与 [`KkpvView::field`] 一致。
    ///
    /// # Errors
    /// 变长值解码失败时返回错误。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field(&self, index: usize, key: &str) -> Result<Option<KkpvValue>, KkpvError> {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.field(source, key)
        } else {
            self.public.field(source, key)
        }
    }

    /// 列出对象的未缺失字段键。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field_keys(&self, index: usize) -> Vec<&str> {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.field_keys(source)
        } else {
            self.public.field_keys(source)
        }
    }

    pub fn object_type(&self, index: usize) -> &str {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.object_type(source)
        } else {
            self.public.object_type(source)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(id: u64, type_key: &str, fields: &[(&str, KkpvValue)]) -> KkpvObject {
        KkpvObject {
            id,
            type_key: type_key.into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn input(objects: Vec<KkpvObject>) -> KkpvBlockInput {
        KkpvBlockInput {
            private: false,
            tick: 7,
            generation: 3,
            plan_hash: [7; 32],
            audience_hash: [9; 32],
            room_name: "W1N1".into(),
            terrain: None,
            objects,
        }
    }

    fn enc(input: &KkpvBlockInput) -> Vec<u8> {
        encode(input, &KkpvLimits::default()).expect("编码成功")
    }

    fn dec(data: &[u8]) -> KkpvView<'_> {
        KkpvView::decode(data, &KkpvLimits::default()).expect("解码成功")
    }

    fn dec_err(data: &[u8]) -> KkpvError {
        match KkpvView::decode(data, &KkpvLimits::default()) {
            Err(e) => e,
            Ok(_) => panic!("期望解码失败"),
        }
    }

    fn put_u16(data: &mut [u8], at: usize, value: u16) {
        data[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(data: &mut [u8], at: usize, value: u32) {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// 单类型单字段单对象的最小块及其结构偏移；字段类型由调用方指定。
    fn minimal_block(kind_value: KkpvValue) -> (Vec<u8>, usize, usize, usize) {
        let bytes = enc(&input(vec![obj(1, "t:a", &[("k", kind_value)])]));
        let buckets_off = read_u32_at(&bytes, OFF_BUCKETS) as usize;
        let bitmap_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY;
        let column_off = bitmap_off + 2;
        (bytes, buckets_off, bitmap_off, column_off)
    }

    #[test]
    fn header_metadata_round_trip() {
        let mut spec = input(vec![]);
        spec.private = true;
        spec.tick = 42;
        spec.generation = 9;
        spec.plan_hash = [3; 32];
        spec.audience_hash = [4; 32];
        spec.terrain = Some(vec![7u8; 64]);
        let bytes = enc(&spec);
        let view = dec(&bytes);
        assert!(view.is_private());
        assert_eq!(view.tick(), 42);
        assert_eq!(view.generation(), 9);
        assert_eq!(view.plan_hash(), [3; 32]);
        assert_eq!(view.audience_hash(), [4; 32]);
        assert_eq!(view.room_name(), "W1N1");
        assert_eq!(view.terrain(), Some(&[7u8; 64][..]));
        assert_eq!(view.object_count(), 0);
    }

    #[test]
    fn all_kinds_round_trip() {
        let fields: &[(&str, KkpvValue)] = &[
            ("b", KkpvValue::Bool(true)),
            ("i", KkpvValue::Int(-12)),
            ("u", KkpvValue::Uint(u64::MAX)),
            ("f", KkpvValue::Float(2.5)),
            ("r", KkpvValue::Ref(0xabc)),
            ("s", KkpvValue::Str("结构值".into())),
            (
                "arr",
                KkpvValue::Array(vec![
                    KkpvValue::Null,
                    KkpvValue::Int(1),
                    KkpvValue::Array(Vec::new()),
                ]),
            ),
            (
                "rec",
                KkpvValue::Object(vec![
                    ("zero".into(), KkpvValue::Uint(0)),
                    ("嵌套".into(), KkpvValue::Str("v".into())),
                ]),
            ),
        ];
        let bytes = enc(&input(vec![obj(5, "t:a", fields)]));
        let view = dec(&bytes);
        assert_eq!(view.object_count(), 1);
        assert_eq!(view.object_id(0), 5);
        assert_eq!(view.object_type(0), "t:a");
        for (key, value) in fields {
            assert_eq!(view.field(0, key), Ok(Some(value.clone())), "字段 {key}");
        }
        assert_eq!(view.field(0, "不存在"), Ok(None));
        let mut expected: Vec<&str> = fields.iter().map(|(k, _)| *k).collect();
        expected.sort_unstable();
        assert_eq!(view.field_keys(0), expected);
        assert_eq!(view.object_index_by_id(5), Some(0));
        assert_eq!(view.object_index_by_id(6), None);
    }

    /// 缺失、null、零与空集合在解码后保持互异。
    #[test]
    fn missing_null_zero_empty_distinct() {
        // 三行分别为：缺失、null、有值；三列分别为 Int/Str/Array。
        let objects: Vec<KkpvObject> = vec![
            obj(1, "t:a", &[]),
            obj(
                2,
                "t:a",
                &[
                    ("n", KkpvValue::Null),
                    ("s", KkpvValue::Null),
                    ("a", KkpvValue::Null),
                ],
            ),
            obj(
                3,
                "t:a",
                &[
                    ("n", KkpvValue::Int(0)),
                    ("s", KkpvValue::Str(String::new())),
                    ("a", KkpvValue::Array(Vec::new())),
                ],
            ),
        ];
        let bytes = enc(&input(objects));
        let view = dec(&bytes);
        // 行 1：全部缺失；行 2：全部 null；行 3：零/空字符串/空数组。
        assert_eq!(view.field_keys(0), Vec::<&str>::new());
        for key in ["n", "s", "a"] {
            assert_eq!(view.field(1, key), Ok(Some(KkpvValue::Null)), "行 2 {key}");
        }
        assert_eq!(view.field(2, "n"), Ok(Some(KkpvValue::Int(0))));
        assert_eq!(view.field(2, "s"), Ok(Some(KkpvValue::Str(String::new()))));
        assert_eq!(view.field(2, "a"), Ok(Some(KkpvValue::Array(Vec::new()))));
        assert_eq!(view.field(0, "n"), Ok(None));
    }

    /// 对象、字段与记录键的提交顺序不影响编码字节。
    #[test]
    fn deterministic_bytes_regardless_of_order() {
        let fields_a = vec![
            ("x".to_string(), KkpvValue::Int(1)),
            ("a".to_string(), KkpvValue::Str("v".into())),
        ];
        let fields_b = vec![("a".to_string(), KkpvValue::Str("v".into()))];
        let straight = KkpvBlockInput {
            objects: vec![
                KkpvObject {
                    id: 1,
                    type_key: "t:a".into(),
                    fields: fields_a.clone(),
                },
                KkpvObject {
                    id: 2,
                    type_key: "t:b".into(),
                    fields: Vec::new(),
                },
                KkpvObject {
                    id: 3,
                    type_key: "t:a".into(),
                    fields: fields_b.clone(),
                },
            ],
            ..input(vec![])
        };
        let shuffled = KkpvBlockInput {
            objects: vec![
                KkpvObject {
                    id: 3,
                    type_key: "t:a".into(),
                    fields: fields_b.clone(),
                },
                KkpvObject {
                    id: 2,
                    type_key: "t:b".into(),
                    fields: Vec::new(),
                },
                KkpvObject {
                    id: 1,
                    type_key: "t:a".into(),
                    fields: vec![fields_a[1].clone(), fields_a[0].clone()],
                },
            ],
            ..input(vec![])
        };
        assert_eq!(enc(&straight), enc(&shuffled));
        let mut other = straight.clone();
        other.audience_hash = [1; 32];
        assert_ne!(enc(&straight), enc(&other), "受众摘要参与字节");
    }

    /// 目录在类型交错时仍按 ID 全局升序，类型与字段归属不串行。
    #[test]
    fn directory_sorted_across_types() {
        let objects = vec![
            obj(1, "t:b", &[("v", KkpvValue::Int(10))]),
            obj(2, "t:a", &[("w", KkpvValue::Int(20))]),
            obj(3, "t:b", &[("v", KkpvValue::Int(30))]),
            obj(4, "t:a", &[("w", KkpvValue::Int(40))]),
        ];
        let bytes = enc(&input(objects));
        let view = dec(&bytes);
        assert_eq!(
            (0..4).map(|i| view.object_id(i)).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(view.object_type(0), "t:b");
        assert_eq!(view.object_type(1), "t:a");
        assert_eq!(view.field(0, "v"), Ok(Some(KkpvValue::Int(10))));
        assert_eq!(view.field(1, "w"), Ok(Some(KkpvValue::Int(20))));
        assert_eq!(view.field(2, "v"), Ok(Some(KkpvValue::Int(30))));
        assert_eq!(view.field(3, "w"), Ok(Some(KkpvValue::Int(40))));
        assert_eq!(view.field(0, "w"), Ok(None), "他桶字段不串行");
    }

    #[test]
    fn terrain_round_trip() {
        let mut spec = input(vec![]);
        spec.terrain = Some(vec![0u8; 2500]);
        let bytes = enc(&spec);
        let view = dec(&bytes);
        assert_eq!(view.terrain().map(|t| t.len()), Some(2500));
    }

    // —— 编码拒绝 ——

    #[test]
    fn encode_rejects_invalid_inputs() {
        let limits = KkpvLimits::default();
        let dup_id = input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(1, "t:a", &[("k", KkpvValue::Int(2))]),
        ]);
        assert_eq!(
            encode(&dup_id, &limits),
            Err(KkpvError::DuplicateObjectId(1))
        );
        let dup_field = input(vec![obj(
            1,
            "t:a",
            &[("k", KkpvValue::Int(1)), ("k", KkpvValue::Int(2))],
        )]);
        assert!(matches!(
            encode(&dup_field, &limits),
            Err(KkpvError::DuplicateField { .. })
        ));
        let kind_mismatch = input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(2, "t:a", &[("k", KkpvValue::Bool(true))]),
        ]);
        assert!(matches!(
            encode(&kind_mismatch, &limits),
            Err(KkpvError::KindMismatch { .. })
        ));
        let id_field = input(vec![obj(1, "t:a", &[("id", KkpvValue::Int(1))])]);
        assert!(matches!(
            encode(&id_field, &limits),
            Err(KkpvError::DuplicateField { field, .. }) if field == "id"
        ));
        let nan = input(vec![obj(1, "t:a", &[("k", KkpvValue::Float(f64::NAN))])]);
        assert_eq!(encode(&nan, &limits), Err(KkpvError::NonFiniteFloat));
        let mut nested = KkpvValue::Int(0);
        for _ in 0..20 {
            nested = KkpvValue::Array(vec![nested]);
        }
        let deep = input(vec![obj(1, "t:a", &[("k", nested)])]);
        assert_eq!(encode(&deep, &limits), Err(KkpvError::DepthExceeded));
        let mut record_dup = input(vec![obj(1, "t:a", &[("k", KkpvValue::Null)])]);
        record_dup.objects[0].fields[0].1 = KkpvValue::Object(vec![
            ("a".into(), KkpvValue::Int(1)),
            ("a".into(), KkpvValue::Int(2)),
        ]);
        assert!(matches!(
            encode(&record_dup, &limits),
            Err(KkpvError::DuplicateField { .. })
        ));
        let mut empty_room = input(vec![]);
        empty_room.room_name = String::new();
        assert_eq!(
            encode(&empty_room, &limits),
            Err(KkpvError::RoomNameMissing)
        );
        let two = input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
        ]);
        let mut tight = limits.clone();
        tight.max_objects = 1;
        assert_eq!(
            encode(&two, &tight),
            Err(KkpvError::LimitExceeded("max_objects"))
        );
        let mut long_str = input(vec![obj(1, "t:a", &[("k", KkpvValue::Str(String::new()))])]);
        long_str.objects[0].fields[0].1 = KkpvValue::Str("x".repeat(8193));
        assert_eq!(
            encode(&long_str, &limits),
            Err(KkpvError::LimitExceeded("max_string_bytes"))
        );
        // 键超限同样在编码端拒绝：字段键与类型键超出限额的块无法通过
        // 自身解码，不得产出。
        let mut long_key = input(vec![obj(1, "t:a", &[])]);
        long_key.objects[0]
            .fields
            .push(("k".repeat(8193), KkpvValue::Int(1)));
        assert_eq!(
            encode(&long_key, &limits),
            Err(KkpvError::LimitExceeded("max_string_bytes"))
        );
        let mut long_type = input(vec![obj(1, "t:a", &[])]);
        long_type.objects[0].type_key = "t".repeat(8193);
        assert_eq!(
            encode(&long_type, &limits),
            Err(KkpvError::LimitExceeded("max_string_bytes"))
        );
    }

    // —— 解码拒绝 ——

    #[test]
    fn decode_rejects_header_mutation() {
        let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
        let mut bad_magic = base.clone();
        bad_magic[0] = b'X';
        assert!(matches!(dec_err(&bad_magic), KkpvError::BadMagic(_)));
        let mut v2 = base.clone();
        put_u16(&mut v2, OFF_VERSION, 2);
        assert_eq!(dec_err(&v2), KkpvError::UnsupportedVersion(2));
        let mut flags = base.clone();
        put_u16(&mut flags, OFF_FLAGS, 2);
        assert_eq!(dec_err(&flags), KkpvError::UnknownFlags(2));
        let mut byte_len = base.clone();
        put_u32(&mut byte_len, OFF_BYTE_LEN, base.len() as u32 + 1);
        assert!(matches!(dec_err(&byte_len), KkpvError::ByteLen { .. }));
        let mut header_len = base.clone();
        put_u32(&mut header_len, OFF_HEADER_LEN, 32);
        assert_eq!(dec_err(&header_len), KkpvError::HeaderLen(32));
        let mut reserved = base.clone();
        reserved[OFF_RESERVED] = 1;
        assert_eq!(dec_err(&reserved), KkpvError::ReservedNonZero);
        let short = &base[..base.len() - 1];
        assert!(matches!(dec_err(short), KkpvError::ByteLen { .. }))
    }

    #[test]
    fn decode_rejects_directory_mutation() {
        let base = enc(&input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(5))]),
            obj(2, "t:b", &[("k", KkpvValue::Int(6))]),
        ]));
        let directory_off = read_u32_at(&base, OFF_DIRECTORY) as usize;
        let mut swapped = base.clone();
        let (first, second) = (
            base[directory_off..directory_off + 16].to_vec(),
            base[directory_off + 16..directory_off + 32].to_vec(),
        );
        swapped[directory_off..directory_off + 16].copy_from_slice(&second);
        swapped[directory_off + 16..directory_off + 32].copy_from_slice(&first);
        assert_eq!(dec_err(&swapped), KkpvError::DirectoryNotSorted);
        let mut dup = base.clone();
        put_u64(&mut dup, directory_off + 16, 1);
        assert_eq!(dec_err(&dup), KkpvError::DuplicateObjectId(1));
        let mut handle = base.clone();
        put_u32(&mut handle, directory_off + 8, 9);
        assert_eq!(dec_err(&handle), KkpvError::UnknownTypeHandle(9));
        let mut row = base.clone();
        put_u32(&mut row, directory_off + 12, 5);
        assert_eq!(
            dec_err(&row),
            KkpvError::RowOutOfRange {
                handle: 0,
                row: 5,
                rows: 1
            }
        );
        let mut count = base.clone();
        put_u32(&mut count, OFF_OBJECT_COUNT, 3);
        assert!(matches!(dec_err(&count), KkpvError::DirectoryLength { .. }));
        let tight_limits = KkpvLimits {
            max_objects: 1,
            ..KkpvLimits::default()
        };
        assert_eq!(
            KkpvView::decode(&base, &tight_limits).err().unwrap(),
            KkpvError::LimitExceeded("max_objects")
        );
    }

    fn put_u64(data: &mut [u8], at: usize, value: u64) {
        data[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn decode_rejects_segment_and_bucket_mutation() {
        // 带地形块：扩大地形长度使其与对象目录重叠。
        let mut with_terrain = input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]);
        with_terrain.terrain = Some(vec![0u8; 8]);
        let base = enc(&with_terrain);
        let mut overlap = base.clone();
        let terrain_len = read_u32_at(&base, OFF_TERRAIN + 4);
        put_u32(&mut overlap, OFF_TERRAIN + 4, terrain_len + 32);
        assert!(matches!(dec_err(&overlap), KkpvError::SegmentOverlap(_)));
        let mut oob = base.clone();
        put_u32(&mut oob, OFF_ARENA, base.len() as u32 - 2);
        assert!(matches!(
            dec_err(&oob),
            KkpvError::SegmentOutOfBounds { .. }
        ));
        let mut shrunk = base.clone();
        let buckets_len = read_u32_at(&base, OFF_BUCKETS + 4);
        put_u32(&mut shrunk, OFF_BUCKETS + 4, buckets_len - 1);
        assert!(matches!(dec_err(&shrunk), KkpvError::BucketWalk { .. }));

        // 单类型单字段块的结构偏移由布局常量推得。
        let (kind_base, buckets_off, _bitmap_off, _column_off) = minimal_block(KkpvValue::Int(5));
        let mut kind = kind_base.clone();
        kind[buckets_off + BUCKET_HEADER + 8] = 9;
        assert_eq!(dec_err(&kind), KkpvError::UnknownKind(9));
        let (null_base, _, null_bitmap, _) = minimal_block(KkpvValue::Null);
        assert_eq!(null_base[null_bitmap], 0, "缺失位图原为零");
        assert_eq!(null_base[null_bitmap + 1], 1, "null 位图原为一");
        let mut both_bits = null_base.clone();
        both_bits[null_bitmap] = 1;
        assert_eq!(dec_err(&both_bits), KkpvError::AmbiguousBitmap);
        let (float_base, _, _, float_column) = minimal_block(KkpvValue::Float(2.5));
        let mut nan = float_base.clone();
        nan[float_column..float_column + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert_eq!(dec_err(&nan), KkpvError::NonFiniteFloat);
        let (bool_base, _, bool_bitmap, bool_column) = minimal_block(KkpvValue::Bool(true));
        let mut bad_bool = bool_base.clone();
        bad_bool[bool_column] = 2;
        assert_eq!(dec_err(&bad_bool), KkpvError::InvalidBool(2));
        assert_eq!(bool_base[bool_bitmap], 0, "有值行两个位图都为零");
    }

    #[test]
    fn decode_rejects_key_order_and_arena_mutation() {
        // 两个字段键交换描述符后不再升序。
        let two_fields = enc(&input(vec![obj(
            1,
            "t:a",
            &[("a", KkpvValue::Int(1)), ("b", KkpvValue::Int(2))],
        )]));
        let buckets_off = read_u32_at(&two_fields, OFF_BUCKETS) as usize;
        let first_entry = buckets_off + BUCKET_HEADER;
        let mut swapped = two_fields.clone();
        let desc_a = two_fields[first_entry..first_entry + 8].to_vec();
        let desc_b = two_fields[first_entry + FIELD_ENTRY..first_entry + FIELD_ENTRY + 8].to_vec();
        swapped[first_entry..first_entry + 8].copy_from_slice(&desc_b);
        swapped[first_entry + FIELD_ENTRY..first_entry + FIELD_ENTRY + 8].copy_from_slice(&desc_a);
        assert_eq!(dec_err(&swapped), KkpvError::KeyOrder);

        // 两个同形桶交换类型键描述符后不再升序。
        let two_types = enc(&input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(2, "t:b", &[("k", KkpvValue::Int(2))]),
        ]));
        let buckets_off = read_u32_at(&two_types, OFF_BUCKETS) as usize;
        let bucket_len = 16 + FIELD_ENTRY + 2 + 8;
        let mut swapped_types = two_types.clone();
        let desc_a = two_types[buckets_off..buckets_off + 8].to_vec();
        let desc_b = two_types[buckets_off + bucket_len..buckets_off + bucket_len + 8].to_vec();
        swapped_types[buckets_off..buckets_off + 8].copy_from_slice(&desc_b);
        swapped_types[buckets_off + bucket_len..buckets_off + bucket_len + 8]
            .copy_from_slice(&desc_a);
        assert_eq!(dec_err(&swapped_types), KkpvError::KeyOrder);

        // 房名是非 UTF-8 时建立视图即失败。
        let mut room_utf8 = two_types.clone();
        let arena_off = read_u32_at(&two_types, OFF_ARENA) as usize;
        room_utf8[arena_off] = 0xFF;
        assert_eq!(dec_err(&room_utf8), KkpvError::InvalidUtf8);

        // 变长描述符越界在建立视图时被结构检查拦截。
        let (str_base, _, _, column_off) = minimal_block(KkpvValue::Str("hi".into()));
        let mut desc_oob = str_base.clone();
        let arena_len = read_u32_at(&str_base, OFF_ARENA + 4);
        put_u32(&mut desc_oob, column_off, arena_len);
        assert!(matches!(dec_err(&desc_oob), KkpvError::ArenaBounds { .. }));

        // 变长值内的坏字符串/坏标签在读取该字段时暴露。
        let mut lazy_utf8 = str_base.clone();
        let payload_off = read_u32_at(&str_base, OFF_ARENA) as usize
            + read_u32_at(&str_base, column_off) as usize;
        lazy_utf8[payload_off + 5] = 0xFF;
        let view = dec(&lazy_utf8);
        assert_eq!(
            view.field(0, "k").unwrap_err(),
            KkpvError::InvalidUtf8,
            "载荷字符串 UTF-8 在读取时校验"
        );
        let mut bad_tag = str_base.clone();
        bad_tag[payload_off] = 0xFF;
        let view = dec(&bad_tag);
        assert!(matches!(view.field(0, "k"), Err(KkpvError::UnknownTag(_))));
    }

    // —— 清偿后补充的钉子 ——

    /// 解码端执行字节/arena/地形三项限额：超限块不得建立视图。
    #[test]
    fn decode_enforces_byte_arena_terrain_limits() {
        let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
        let tiny = |what: &str, value: usize| KkpvLimits {
            max_bytes: if what == "max_bytes" {
                value
            } else {
                usize::MAX
            },
            max_arena_bytes: if what == "max_arena_bytes" {
                value
            } else {
                usize::MAX
            },
            max_terrain_bytes: if what == "max_terrain_bytes" {
                value
            } else {
                usize::MAX
            },
            ..KkpvLimits::default()
        };
        assert_eq!(
            KkpvView::decode(&base, &tiny("max_bytes", 100)).err(),
            Some(KkpvError::LimitExceeded("max_bytes"))
        );
        assert_eq!(
            KkpvView::decode(&base, &tiny("max_arena_bytes", 2)).err(),
            Some(KkpvError::LimitExceeded("max_arena_bytes"))
        );
        let mut with_terrain = input(vec![]);
        with_terrain.terrain = Some(vec![0u8; 64]);
        let terrain_block = enc(&with_terrain);
        assert_eq!(
            KkpvView::decode(&terrain_block, &tiny("max_terrain_bytes", 4)).err(),
            Some(KkpvError::LimitExceeded("max_terrain_bytes"))
        );
    }

    /// 嵌套记录的重复键在读取时以键序错误拒绝，不产出编码必拒的值。
    #[test]
    fn nested_duplicate_record_key_rejected_on_read() {
        // 记录含 "a"、"b" 两键；把第二个键名字节改为 'a' 构成重复键。
        let bytes = enc(&input(vec![obj(
            1,
            "t:a",
            &[(
                "k",
                KkpvValue::Object(vec![
                    ("a".to_string(), KkpvValue::Int(1)),
                    ("b".to_string(), KkpvValue::Int(2)),
                ]),
            )],
        )]));
        // 嵌套载荷：TAG_OBJECT + count=2 + (4,"a",TAG_INT,8B) + (4,"b",...)。
        // 'b' 的绝对位置 = 载荷起点 + 1 + 4 + 14 + 4。
        let arena_off = read_u32_at(&bytes, OFF_ARENA) as usize;
        let buckets_off = read_u32_at(&bytes, OFF_BUCKETS) as usize;
        let column_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY + 2;
        let payload_off = arena_off + read_u32_at(&bytes, column_off) as usize;
        let b_key_at = payload_off + 1 + 4 + 14 + 4;
        let mut dup = bytes.clone();
        assert_eq!(dup[b_key_at], b'b', "定位到第二个键名字节");
        dup[b_key_at] = b'a';
        let view = dec(&dup);
        assert_eq!(
            view.field(0, "k"),
            Err(KkpvError::KeyOrder),
            "重复键须在读取时拒绝"
        );
    }

    /// 变长描述符吞入额外字节后，载荷非恰好消费完即拒绝。
    #[test]
    fn variable_payload_trailing_bytes_rejected() {
        // 两个同类型对象、各一个 Str 字段：两个载荷相邻，改长第一个
        // 描述符仍留在 arena 内，尾字节检查因此可单独命中。
        let base = enc(&input(vec![
            obj(1, "t:a", &[("s", KkpvValue::Str("ab".into()))]),
            obj(2, "t:a", &[("s", KkpvValue::Str("cd".into()))]),
        ]));
        let buckets_off = read_u32_at(&base, OFF_BUCKETS) as usize;
        // 列起始于桶头 16 + 字段条目 12 + 位图 1 + 1。
        let first_column = buckets_off + BUCKET_HEADER + FIELD_ENTRY + 2;
        let mut stretched = base.clone();
        let len = read_u32_at(&base, first_column + 4);
        put_u32(&mut stretched, first_column + 4, len + 1);
        let view = dec(&stretched);
        assert!(
            matches!(view.field(0, "s"), Err(KkpvError::ArenaBounds { .. })),
            "多读的尾字节使载荷非恰好消费"
        );
    }

    /// arena 段缺失与零长度段非零 offset 都拒绝。
    #[test]
    fn arena_missing_and_zero_segment_offset_rejected() {
        let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
        let mut no_arena = base.clone();
        let arena_off = read_u32_at(&base, OFF_ARENA) as usize;
        let arena_len = read_u32_at(&base, OFF_ARENA + 4) as usize;
        put_u32(&mut no_arena, OFF_ARENA, 0);
        put_u32(&mut no_arena, OFF_ARENA + 4, 0);
        // 把 arena 内容挪进 terrain 段位置以保持总长不变，段表合法但 arena 空。
        let _ = (arena_off, arena_len);
        assert_eq!(dec_err(&no_arena), KkpvError::ArenaMissing);
        // 本块无地形段（len=0）：offset 非零即拒绝。
        let mut bad_offset = base.clone();
        put_u32(&mut bad_offset, OFF_TERRAIN, 99);
        assert!(matches!(
            dec_err(&bad_offset),
            KkpvError::SegmentOutOfBounds { .. }
        ));
    }

    /// 字段条目保留字节与位图越行尾位非零都拒绝；目录行重复引用拒绝。
    #[test]
    fn reserved_entry_bytes_bitmap_tail_and_row_alias_rejected() {
        let (kind_base, buckets_off, _, _) = minimal_block(KkpvValue::Int(5));
        let mut reserved = kind_base.clone();
        reserved[buckets_off + BUCKET_HEADER + 9] = 0xAA;
        assert_eq!(dec_err(&reserved), KkpvError::ReservedNonZero);

        let two = enc(&input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
        ]));
        let buckets_off = read_u32_at(&two, OFF_BUCKETS) as usize;
        let bitmap_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY;
        let mut tail = two.clone();
        tail[bitmap_off] = 0b1111_1100;
        assert_eq!(dec_err(&tail), KkpvError::BitmapTailBits);

        let directory_off = read_u32_at(&two, OFF_DIRECTORY) as usize;
        let mut aliased = two.clone();
        put_u32(&mut aliased, directory_off + DIRECTORY_ENTRY + 12, 0);
        assert_eq!(
            dec_err(&aliased),
            KkpvError::DuplicateRowRef { handle: 0, row: 0 },
            "两对象引用同行使一行数据以两个 ID 显形"
        );
    }

    // —— 合并 ——

    #[test]
    fn merge_public_and_private() {
        let public = input(vec![
            obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
            obj(4, "t:a", &[("k", KkpvValue::Int(4))]),
        ]);
        let mut overlay = input(vec![
            obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
            obj(3, "t:a", &[("k", KkpvValue::Int(3))]),
        ]);
        overlay.private = true;
        overlay.audience_hash = [5; 32];
        let public_bytes = enc(&public);
        let overlay_bytes = enc(&overlay);
        let public_view = dec(&public_bytes);
        let overlay_view = dec(&overlay_bytes);
        let merged = merge(&public_view, &overlay_view).expect("同版本块可合并");
        assert_eq!(merged.object_count(), 4);
        assert_eq!(
            (0..merged.object_count())
                .map(|i| merged.object_id(i))
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            (0..merged.object_count())
                .map(|i| merged.from_overlay(i))
                .collect::<Vec<_>>(),
            vec![true, false, true, false]
        );
        assert_eq!(
            merged.field(0, "k"),
            Ok(Some(KkpvValue::Int(1))),
            "overlay 字段经合并视图读取"
        );
        assert_eq!(merged.room_name(), "W1N1");
    }

    #[test]
    fn merge_rejects_identity_and_conflicts() {
        let public_bytes = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(1))])]));
        let mk_overlay = |mut spec: KkpvBlockInput| {
            spec.private = true;
            enc(&spec)
        };
        let conflict_bytes = mk_overlay(input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(9))])]));
        let pv = dec(&public_bytes);
        let cv = dec(&conflict_bytes);
        assert_eq!(
            merge(&pv, &cv).err().unwrap(),
            KkpvError::MergeObjectConflict(1)
        );
        let mut tick_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
        tick_spec.tick = 8;
        let tick_bytes = mk_overlay(tick_spec);
        let tv = dec(&tick_bytes);
        assert!(matches!(
            merge(&pv, &tv).err().unwrap(),
            KkpvError::MergeTickMismatch { .. }
        ));
        let mut gen_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
        gen_spec.generation = 4;
        let gen_bytes = mk_overlay(gen_spec);
        let gv = dec(&gen_bytes);
        assert!(matches!(
            merge(&pv, &gv).err().unwrap(),
            KkpvError::MergeGenerationMismatch { .. }
        ));
        let mut plan_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
        plan_spec.plan_hash = [1; 32];
        let plan_bytes = mk_overlay(plan_spec);
        let plv = dec(&plan_bytes);
        assert_eq!(
            merge(&pv, &plv).err().unwrap(),
            KkpvError::MergePlanMismatch
        );
        let mut room_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
        room_spec.room_name = "E2S2".into();
        let room_bytes = mk_overlay(room_spec);
        let rv = dec(&room_bytes);
        assert!(matches!(
            merge(&pv, &rv).err().unwrap(),
            KkpvError::MergeRoomMismatch { .. }
        ));
        let both_public_bytes = enc(&input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]));
        let both_public = dec(&both_public_bytes);
        assert!(matches!(
            merge(&pv, &both_public).err().unwrap(),
            KkpvError::MergeFlags { .. }
        ));
    }
}
