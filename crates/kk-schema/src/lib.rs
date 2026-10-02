//! 房间二进制快照的类型声明、顺序读写和版本识别。
//!
//! 字段按声明顺序紧密排列，没有对齐填充。整数为小端编码，bool 为 0 或 1；
//! String 使用 u32 字节长度加 UTF-8 内容，Vec 使用 u32 元素数量加元素序列，
//! Option 使用单字节存在标志（0=None，1=Some 后接载荷），定长字节数组直接写入内容。
//!
//! [`macro@kk_schema!`] 生成结构体和读写实现，并为连续的原始类型字段前缀生成
//! 编码偏移。偏移相对结构体载荷起点，不是 Rust 内存布局；嵌套或变长字段后
//! 不再生成偏移，即使嵌套类型自身定长。
//!
//! [`macro@kk_room_object!`] 生成对象枚举、访问器和单字节标签编解码。
//! [`room`] 提供完整快照的魔数、版本及尾部字节检查；业务字段的合法性由调用方检查。

/// 二进制读写错误的统一形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    UnexpectedEof,
    InvalidUtf8,
    InvalidBool(u8),
    InvalidOptionTag(u8),
    InvalidVariantTag(u8),
    /// 版本为 0、比当前版本更新，或无法升级为当前版本。
    UnsupportedVersion(u16),
    BadMagic(u32),
    /// 完整快照解码后剩余的字节数；单个 Schema 值不要求读完整个游标。
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

/// 从借用的字节切片顺序读取，不复制底层数据。
/// 固定长度读取失败时位置不变；组合类型解码失败可能已消费部分字段。
#[derive(Debug)]
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// 返回尚未消费的字节数。
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// 借用接下来的 n 个字节并推进位置；n 为 0 时返回空切片。
    ///
    /// # Errors
    /// 剩余字节不足时返回 UnexpectedEof，位置保持不变。
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

/// 将一个值追加到字节缓冲区，或从当前位置解码一个值。
///
/// 解码不要求消费全部输入，也不会在失败时回退游标；完整快照检查见
/// [`room::decode_snapshot`]。内置字符串和列表用 u32 存长度，编码调用方须
/// 保证长度能由 u32 表示；解码不设置独立的内存或元素数量上限。
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

/// 按声明顺序生成结构体、[`Schema`] 实现和定长前缀的编码偏移常量。
///
/// 每个字段用括号包裹。支持 u8/u16/u32/u64/i32/i64、bool、`str`、`vec T`、`opt T`、
/// `[u8; N]` 和实现 Schema 的 Rust 类型；`str`、`vec str`、`opt str` 为 String 简写。
/// 嵌套容器可使用 Rust 类型写法，如 `Vec<Option<u32>>`。
///
/// `OFFSET_<field>` 相对本结构体编码起点，只有开头连续的整数、bool 和
/// 字节数组字段有该常量。具名嵌套类型也会终止偏移生成，不按其内存大小推算。
///
/// ```
/// kk_schema::kk_schema! {
///     pub Store { (pub energy: u32) }
/// }
/// assert_eq!(Store::OFFSET_energy, 0);
/// ```
#[macro_export]
macro_rules! kk_schema {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident { $($fields:tt)* }
    ) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [] $($fields)* }
        impl $crate::Schema for $name {
            fn encode_into(&self, _out: &mut Vec<u8>) {
                $crate::kk_schema!(@enc self _out [] $($fields)*);
            }
            fn decode_from(_cur: &mut $crate::Cursor<'_>) -> Result<Self, $crate::SchemaError> {
                $crate::kk_schema!(@dec _cur [] [] $($fields)*)
            }
        }
        impl $name {
            $crate::kk_schema! { @off [0] [] $($fields)* }
        }
    };

    // 累积 Rust 字段后一次生成结构体；item 位置的递归调用使用花括号。
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]) => {
        #[derive(Clone, Debug, PartialEq)]
        $(#[$meta])*
        $vis struct $name { $($acc)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u8) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u8,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u16) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u16,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u32) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u32,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : u64) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: u64,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : i32) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: i32,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : i64) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: i64,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : bool) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: bool,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : str) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: String,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : vec str) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Vec<String>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : vec $t:ty) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Vec<$t>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : opt str) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Option<String>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : opt $t:ty) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: Option<$t>,] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : [u8; $n:literal]) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: [u8; $n],] $($r)* }
    };
    (@struct $(#[$meta:meta])* $vis:vis $name:ident [$($acc:tt)*]
        ($fv:vis $f:ident : $t:ty) $($r:tt)*) => {
        $crate::kk_schema! { @struct $(#[$meta])* $vis $name [$($acc)* $fv $f: $t,] $($r)* }
    };

    // 按声明顺序累积编码语句，各字段委托给自身的 Schema 实现。
    (@enc $self:ident $out:ident [$($acc:tt)*]) => { $($acc)* };
    (@enc $self:ident $out:ident [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        $crate::kk_schema!(@enc $self $out [$($acc)* $crate::Schema::encode_into(&$self.$f, $out);] $($r)*);
    };

    // 先累积字段读取语句，递归终点返回构造 Self 的块表达式。
    (@dec $cur:ident [$($l:tt)*] [$($ids:ident)*]) => { { $($l)* Ok(Self { $($ids),* }) } };
    (@dec $cur:ident [$($l:tt)*] [$($ids:ident)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        $crate::kk_schema!(@dec $cur [$($l)* let $f = $crate::Schema::decode_from($cur)?;] [$($ids)* $f] $($r)*)
    };

    // 按编码宽度累加偏移，不使用 Rust 的 size_of 或内存对齐。
    (@off [$($sum:tt)*] [$($acc:tt)*]) => { $crate::__paste_item! { $($acc)* } };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u8) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 1] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u16) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 2] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u32) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 4] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : u64) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 8] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : i32) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 4] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : i64) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 8] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : bool) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + 1] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : [u8; $n:literal]) $($r:tt)*) => {
        $crate::kk_schema! { @off [$($sum)* + $n] [$($acc)* #[allow(non_upper_case_globals)]
            pub const [<OFFSET_ $f>]: usize = ($($sum)*);] $($r)* }
    };
    // 遇到嵌套或变长字段后停止计算，@offdone 只跳过后续字段并发出已有常量。
    (@off [$($sum:tt)*] [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        $crate::kk_schema! { @offdone [$($acc)*] $($r)* }
    };
    (@offdone [$($acc:tt)*]) => { $crate::__paste_item! { $($acc)* } };
    (@offdone [$($acc:tt)*] ($fv:vis $f:ident : $($t:tt)*) $($r:tt)*) => {
        $crate::kk_schema! { @offdone [$($acc)*] $($r)* }
    };
}

/// 生成对象枚举、id/坐标访问器和单字节标签编解码。
///
/// 每个变体显式指定 u8 标签常量路径，标签必须互不相同，否则编译失败。
/// 编码为标签加载荷；解码遇到未知标签返回 InvalidVariantTag。
///
/// 载荷须实现 [`Schema`]，并提供可访问的 `id: u64`、`x: u8`、`y: u8` 字段。
/// 宏不要求 id 是编码首字段，也不验证 id 唯一性或坐标范围；这些由调用方负责。
/// 标签一经用于保存的快照，就属于版本格式，修改必须同时处理旧版本读取。
///
/// ```
/// kk_schema::kk_schema! {
///     pub WallObj { (pub id: u64) (pub x: u8) (pub y: u8) }
/// }
/// mod tags { pub const WALL: u8 = 5; }
/// kk_schema::kk_room_object! {
///     pub enum MyObject { Wall(WallObj) = tags::WALL }
/// }
/// let wall = MyObject::Wall(WallObj { id: 1, x: 2, y: 3 });
/// assert_eq!(wall.pos(), (2, 3));
/// ```
#[macro_export]
macro_rules! kk_room_object {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident ( $payload:ty ) = $tag:path
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant($payload),
            )*
        }

        impl $name {
            /// 返回载荷的对象 id。
            pub fn id(&self) -> u64 {
                match self {
                    $(Self::$variant(o) => o.id,)*
                }
            }

            /// 设置对象 id；调用方负责维护世界中的唯一性和索引。
            pub fn set_id(&mut self, id: u64) {
                match self {
                    $(Self::$variant(o) => o.id = id,)*
                }
            }

            /// 返回房间内的格子坐标；本访问器不检查范围。
            pub fn pos(&self) -> (u8, u8) {
                match self {
                    $(Self::$variant(o) => (o.x, o.y),)*
                }
            }

            /// 设置房间内坐标；不检查范围或同步世界索引。
            pub fn set_pos(&mut self, x: u8, y: u8) {
                match self {
                    $(Self::$variant(o) => {
                        o.x = x;
                        o.y = y;
                    })*
                }
            }

            fn tag(&self) -> u8 {
                match self {
                    $(Self::$variant(_) => $tag,)*
                }
            }
        }

        impl $crate::Schema for $name {
            fn encode_into(&self, out: &mut Vec<u8>) {
                out.push(self.tag());
                match self {
                    $(Self::$variant(o) => $crate::Schema::encode_into(o, out),)*
                }
            }

            fn decode_from(cur: &mut $crate::Cursor<'_>) -> Result<Self, $crate::SchemaError> {
                match cur.read_u8()? {
                    $($tag => Ok(Self::$variant(<$payload as $crate::Schema>::decode_from(cur)?)),)*
                    b => Err($crate::SchemaError::InvalidVariantTag(b)),
                }
            }
        }

        // 重复标签会使解码只命中第一个变体，而编码仍能写出两种载荷；编译期拒绝。
        const _: () = {
            let __tags = [$($tag),*];
            let mut __i = 0;
            while __i < __tags.len() {
                let mut __j = __i + 1;
                while __j < __tags.len() {
                    assert!(__tags[__i] != __tags[__j], "variant tag 值互异被破坏");
                    __j += 1;
                }
                __i += 1;
            }
        };
    };
}

pub mod kkpv;
pub mod room;
