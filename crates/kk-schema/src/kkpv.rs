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
//! 恰被一个目录项引用）、零行桶拒绝（类型桶至少承载一行）、
//! 桶结构恰好消费、键 UTF-8 与升序（字段目录；
//! 嵌套记录保持载荷顺序、只拒绝重复键）、位图互斥
//! 与越行尾位为零、浮点有限性、bool 字节域、变长描述符 arena 边界
//! 及各项限额；字段值内的字符串在读取时校验 UTF-8。编码端执行同源
//! 限额与值域检查，超限时不得产出数据。
//!
//! 嵌套记录的条目顺序与 JSON 参考通道的插入序一致（preserve_order
//! 构建下 serde_json::Map 保插入序）：store、spawning 等嵌套对象的
//! 键序对玩家可见（如 Object.keys(creep.store)），编码不重排。
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
    /// 记录的键值对；编码保持条目顺序并拒绝重复键。
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
    /// 类型桶行数为零；编码端只按实际存在的对象分桶，零行桶无合法产出方。
    EmptyBucket {
        type_key: String,
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
    /// 束框架分帧不符（块数目录、块边界或块间版本一致性）。
    Malformed(String),
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
            Self::EmptyBucket { type_key } => {
                write!(f, "类型 {type_key} 的桶行数为零，桶须至少承载一行")
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
            Self::Malformed(reason) => write!(f, "束框架不合法: {reason}"),
        }
    }
}

impl std::error::Error for KkpvError {}

mod encode;
mod merge;
mod view;
mod wire;

pub use encode::{encode, encode_bundle, validate_bundle};
/// 头部 audience_hash 字段的字节偏移（32 字节宽）。见 [`patch_block_audience`]。
pub const AUDIENCE_HASH_OFFSET: usize = OFF_AUDIENCE_HASH;
/// 把块的 audience_hash 改写为给定值。与以目标 audience 直接编码的产物
/// 逐字节一致——除该 32 字节外不触及其余布局；供宿主在共享缓存字节上
/// 按查看者装配私有束。本模块不解释 audience 语义（见模块注释）。
///
/// # Errors
/// 块短于头部 audience 字段范围时返回错误。
pub fn patch_block_audience(block: &mut [u8], audience: &[u8; 32]) -> Result<(), KkpvError> {
    let end = AUDIENCE_HASH_OFFSET + 32;
    if block.len() < end {
        return Err(KkpvError::Malformed("块短于 audience_hash 字段".into()));
    }
    block[AUDIENCE_HASH_OFFSET..end].copy_from_slice(audience);
    Ok(())
}
pub use merge::{MergedView, merge};
pub use view::KkpvView;
use wire::*;

#[cfg(test)]
mod tests;
