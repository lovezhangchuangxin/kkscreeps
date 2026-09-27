//! 房间快照的编码格式：声明宏 + 版本升级 + 读写器。
//!
//! 设计参照 xxscreeps 的 schema 包（声明式 format → 编译期 layout → 二进制读写），
//! 在 Rust 用 `kk_schema!` 声明宏实现 M1 所需子集：
//!
//! - 定长原语（u8/u16/u32/u64/i32/i64/bool）小端内联；
//! - `str` → u32 长度前缀 + UTF-8 字节；`vec T` → u32 长度前缀 + 元素序列；
//! - `opt T` → u8 存在标志 + 载荷；`[u8; N]` → 原始字节；
//! - 具名结构字段 → 递归编码（目标类型需实现 [`Schema`]）。
//!
//! 布局常量：`OFFSET_<field>` 覆盖声明中**原始类型（含 [u8; N]）的连续前缀**
//! （变长/嵌套字段中断链，其后字段不再产出偏移）——M1 schema 尚未冻结，
//! 完整偏移面在 M2 共享内存直读定稿时交付。
//!
//! 多态对象（`RoomObject` 等 variant 枚举）M1 采用手写 tag 前缀编解码（见 room.rs），
//! 宏支持随需求出现再评估。

/// 二进制读写错误的统一形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    UnexpectedEof,
    InvalidUtf8,
    InvalidBool(u8),
    InvalidOptionTag(u8),
    InvalidVariantTag(u8),
    /// 编码版本不识别（无可用升级链）。
    UnsupportedVersion(u16),
    BadMagic(u32),
    /// 载荷解码结束后仍有剩余字节（拼接/追加损坏）。
    TrailingBytes(usize),
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "数据不足"),
            Self::InvalidUtf8 => write!(f, "字符串非 UTF-8"),
            Self::InvalidBool(b) => write!(f, "非法 bool 字节: {b}"),
            Self::InvalidOptionTag(b) => write!(f, "非法 option 标志: {b}"),
            Self::InvalidVariantTag(b) => write!(f, "非法 variant 标签: {b}"),
            Self::UnsupportedVersion(v) => write!(f, "不支持的快照版本: {v}"),
            Self::BadMagic(m) => write!(f, "快照魔数不符: {m:#x}"),
            Self::TrailingBytes(n) => write!(f, "载荷尾部多余字节: {n}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// 顺序读取游标。
#[derive(Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], SchemaError> {
        if self.remaining() < n {
            return Err(SchemaError::UnexpectedEof);
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub fn read_u8(&mut self) -> Result<u8, SchemaError> {
        Ok(self.read_bytes(1)?[0])
    }

    pub fn read_u16(&mut self) -> Result<u16, SchemaError> {
        let b = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn read_u32(&mut self) -> Result<u32, SchemaError> {
        let b = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_u64(&mut self) -> Result<u64, SchemaError> {
        let b = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    pub fn read_i32(&mut self) -> Result<i32, SchemaError> {
        let b = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_i64(&mut self) -> Result<i64, SchemaError> {
        let b = self.read_bytes(8)?;
        Ok(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}

#[doc(hidden)]
pub use paste::item as __paste_item;

/// schema 化类型：编码进 Vec、从 Cursor 解码。
pub trait Schema: Sized {
    fn encode_into(&self, out: &mut Vec<u8>);
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError>;
}

macro_rules! impl_schema_int {
    ($($t:ty),*) => {$(
        impl Schema for $t {
            fn encode_into(&self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
            fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
                let b = cur.read_bytes(std::mem::size_of::<$t>())?;
                let mut arr = [0u8; std::mem::size_of::<$t>()];
                arr.copy_from_slice(b);
                Ok(<$t>::from_le_bytes(arr))
            }
        }
    )*};
}
impl_schema_int!(u8, u16, u32, u64, i32, i64);

impl Schema for bool {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.push(*self as u8);
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        match cur.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            b => Err(SchemaError::InvalidBool(b)),
        }
    }
}

impl Schema for String {
    fn encode_into(&self, out: &mut Vec<u8>) {
        (self.len() as u32).encode_into(out);
        out.extend_from_slice(self.as_bytes());
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        let len = u32::decode_from(cur)? as usize;
        let bytes = cur.read_bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SchemaError::InvalidUtf8)
    }
}

impl<T: Schema> Schema for Vec<T> {
    fn encode_into(&self, out: &mut Vec<u8>) {
        (self.len() as u32).encode_into(out);
        for item in self {
            item.encode_into(out);
        }
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        let len = u32::decode_from(cur)? as usize;
        (0..len).map(|_| T::decode_from(cur)).collect()
    }
}

impl<T: Schema> Schema for Option<T> {
    fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            None => out.push(0),
            Some(v) => {
                out.push(1);
                v.encode_into(out);
            }
        }
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        match cur.read_u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::decode_from(cur)?)),
            b => Err(SchemaError::InvalidOptionTag(b)),
        }
    }
}

impl<const N: usize> Schema for [u8; N] {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self);
    }
    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        let bytes = cur.read_bytes(N)?;
        let mut arr = [0u8; N];
        arr.copy_from_slice(bytes);
        Ok(arr)
    }
}

/// 声明一个 schema 结构体：生成定义（derive Clone/Debug/PartialEq）、
/// [`Schema`] 实现与原始字段前缀的 `OFFSET_<field>` 常量。
///
/// 字段用括号组声明，类型词汇：`u8..u64/i32/i64/bool`、`str`（String）、
/// `vec T`（Vec<T>）、`opt T`（Option<T>）、`[u8; N]`、具名结构（该类型自身实现 Schema）。
///
/// ```ignore
/// kk_schema! {
///     pub Store { (pub energy: u32) }
/// }
/// ```
#[macro_export]
macro_rules! kk_schema {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident { $($fields:tt)* }
    ) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [] $($fields)* }
        impl $crate::Schema for $name {
            fn encode_into(&self, _out: &mut Vec<u8>) {
                kk_schema!(@enc self _out [] $($fields)*);
            }
            fn decode_from(_cur: &mut $crate::Cursor<'_>) -> Result<Self, $crate::SchemaError> {
                kk_schema!(@dec _cur [] [] $($fields)*)
            }
        }
        impl $name {
            kk_schema! { @off [0] [] $($fields)* }
        }
    };

    // ---- 结构体定义：类型词汇 → Rust 类型（嵌套 item 宏须花括号定界） ----
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]) => {
        #[derive(Clone, Debug, PartialEq)]
        $(#[$meta])*
        $vis struct $name { $($acc)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u8) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u8,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u16) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u16,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u32) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u32,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u64) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u64,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : i32) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: i32,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : i64) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: i64,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : bool) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: bool,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : str) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: String,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : vec str) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Vec<String>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : vec $t:ty) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Vec<$t>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : opt str) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Option<String>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : opt $t:ty) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Option<$t>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : [u8; $n:literal]) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: [u8; $n],] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : $t:ty) $($r:tt)*) => {
        kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: $t,] $($r)* }
    };

    // ---- 编码：全部字段统一走 Schema::encode_into（语句位置） ----
    (@enc $self:ident $out:ident [$($acc:tt)*]) => { $($acc)* };
    (@enc $self:ident $out:ident [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        kk_schema!(@enc $self $out [$($acc)* $crate::Schema::encode_into(&$self.$f, $out);] $($r)*);
    };

    // ---- 解码：统一走 Schema::decode_from；基臂产出块表达式（尾表达式链，无分号） ----
    (@dec $cur:ident [$($l:tt)*] [$($ids:ident)*]) => { { $($l)* Ok(Self { $($ids),* }) } };
    (@dec $cur:ident [$($l:tt)*] [$($ids:ident)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        kk_schema!(@dec $cur [$($l)* let $f = $crate::Schema::decode_from($cur)?;] [$($ids)* $f] $($r)*)
    };

    // ---- 偏移常量：原始类型连续前缀（变长/嵌套字段之后的字段不产出偏移） ----
    (@off [$($sum:tt)*] [$($acc:tt)*]) => { $crate::__paste_item! { $($acc)* } };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u8) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 1] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u16) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 2] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u32) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 4] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u64) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 8] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : i32) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 4] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : i64) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 8] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : bool) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + 1] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : [u8; $n:literal]) $($r:tt)*) => {
        kk_schema! { @off [$($sum)* + $n] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    // 变长/嵌套字段：终止偏移链——其后的字段一律不再产出偏移（绝对偏移仅在
    // 变长字段之前的原始字段上有定义，与模块文档一致）。累积器随 @offdone
    // 带到终点统一发出
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        kk_schema! { @offdone [$($acc)*] $($r)* }
    };
    (@offdone [$($acc:tt)*]) => { $crate::__paste_item! { $($acc)* } };
    (@offdone [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        kk_schema! { @offdone [$($acc)*] $($r)* }
    };
}

pub mod room;
