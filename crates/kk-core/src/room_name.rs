//! 房间名与带方向的整数坐标转换。
//!
//! W/N 方向为负并减一：W0/N0 对应 -1；E0/S0 对应 0。
//! 解析沿用上游按水平数值定位垂直方向的算法，但额外检查方向与数值长度。
//! 它不是完整的房间名格式校验，也不限制房间是否位于实际世界范围内。
//! 来源：Screeps engine `src/utils.js` 的 `roomNameToXY`、`getRoomNameFromXY`
//! （核对版本 a90d26fc9eb943147799e65d0bf9d4d0d79a2722）。

use std::fmt;

/// 房间的水平和垂直坐标，使用 W/N 为负、E/S 非负的表示。
/// 字段公开且可直接构造，因此本类型不保证原始名字格式或世界范围已验证。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoomName {
    /// 水平房间坐标，W0 = -1，E0 = 0；不是房间内的格子坐标。
    pub xx: i32,
    /// 垂直房间坐标，N0 = -1，S0 = 0；不是房间内的格子坐标。
    pub yy: i32,
}

impl RoomName {
    /// 解析房间名，返回带方向的整数坐标。
    ///
    /// 方向字母大小写均可。数值按十进制前缀读取，垂直数值可带符号和
    /// 前导空白，尾部非数字不参与解析，例如 `E1N-5` 得到 `(1, 4)`。
    /// 垂直方向的位置由水平数值决定，不是搜索 N/S 字母：规范名字的水平
    /// 数字须为一至三位且没有多余前导零，否则可能被拒绝。
    ///
    /// # Errors
    /// 方向位不是 W/E 或 N/S、数值前缀缺失或任一数值超过九位数字时返回错误。
    /// 本函数不保证全串符合 `[WE][0-9]+[NS][0-9]+`，也不检查世界坐标范围。
    pub fn parse(name: &str) -> Result<Self, String> {
        let bytes = name.as_bytes();
        if bytes.len() < 3 {
            return Err(format!("房间名过短: {name:?}"));
        }
        let horizontal_dir = bytes[0];
        if !matches!(horizontal_dir, b'W' | b'w' | b'E' | b'e') {
            return Err(format!("房间名水平方向位非法: {name:?}"));
        }
        // 首字节已确认是 ASCII 方向字母，因此从字节 1 切片不会截断 UTF-8。
        let xx =
            parse_leading_int(&name[1..]).ok_or_else(|| format!("房间名水平数值缺失: {name:?}"))?;
        // 上游按数值大小确定方向位置，而不是按原字符串的数字长度。
        let vertical_pos = if xx >= 100 {
            4
        } else if xx >= 10 {
            3
        } else {
            2
        };
        // 固定位置可能越界或落在 UTF-8 字符中间，按缺少数值处理，不让切片 panic。
        let rest = name.get(vertical_pos as usize + 1..).unwrap_or("");
        let vertical_dir = *bytes
            .get(vertical_pos as usize)
            .ok_or_else(|| format!("房间名垂直方向位缺失: {name:?}"))?;
        let yy = parse_leading_int(rest).ok_or_else(|| format!("房间名垂直数值缺失: {name:?}"))?;
        let mut out_x = xx;
        let mut out_y = yy;
        if matches!(horizontal_dir, b'W' | b'w') {
            out_x = -out_x - 1;
        }
        if matches!(vertical_dir, b'N' | b'n') {
            out_y = -out_y - 1;
        } else if !matches!(vertical_dir, b'S' | b's') {
            return Err(format!("房间名垂直方向位非法: {name:?}"));
        }
        Ok(Self {
            xx: out_x,
            yy: out_y,
        })
    }

    /// 用水平和垂直坐标构造值，不执行名字解析或世界范围检查。
    ///
    /// 生成的规范名字要能被 [`Self::parse`] 读回，水平坐标须在 -1000..=999，
    /// 垂直坐标须在 -1_000_000_000..=999_999_999；否则不保证往返。
    ///
    /// # Panics
    /// 开启调试断言时，任一坐标的绝对值达到 10^9 会触发构造范围检查。
    /// 该检查仅限制极端数值，不保证名字能够往返；关闭调试断言后不执行。
    pub fn from_xy(xx: i32, yy: i32) -> Self {
        debug_assert!(
            (xx as i64).abs() < 1_000_000_000 && (yy as i64).abs() < 1_000_000_000,
            "from_xy 坐标超出调试构造范围 |v| < 10^9"
        );
        Self { xx, yy }
    }

    /// 生成大写方向、不带前导零的房间名，支持全部 i32 坐标。
    /// 生成成功不代表 [`Self::parse`] 能读回；其水平定位和数字长度仍有限制。
    pub fn as_str(&self) -> String {
        let x = if self.xx < 0 {
            // 先加一再取负，避免对 i32::MIN 直接取负溢出。
            format!("W{}", -(self.xx + 1))
        } else {
            format!("E{}", self.xx)
        };
        let y = if self.yy < 0 {
            format!("N{}", -(self.yy + 1))
        } else {
            format!("S{}", self.yy)
        };
        format!("{x}{y}")
    }
}

/// 读取可选符号后的 ASCII 十进制前缀，尾部非数字不参与解析。
/// 无数字或数字多于九位时返回 None；前导空白使用 Rust `trim_start` 的字符集。
/// 这是受限的整数读取器，不实现 JavaScript parseInt 的全部空白或数值范围规则。
fn parse_leading_int(s: &str) -> Option<i32> {
    let t = s.trim_start();
    let bytes = t.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'+') || bytes.first() == Some(&b'-') {
        i = 1;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == start || bytes[start..i].len() > 9 {
        // 九位上限同时约束水平、垂直数值，留出取负减一等坐标运算的空间。
        return None;
    }
    let magnitude: i32 = t[start..i].parse().ok()?;
    Some(if bytes.first() == Some(&b'-') {
        -magnitude
    } else {
        magnitude
    })
}

impl fmt::Display for RoomName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_roundtrip() {
        // 官方 W/N 取负减一语义
        assert_eq!(
            RoomName::parse("W1N1").unwrap(),
            RoomName { xx: -2, yy: -2 }
        );
        assert_eq!(
            RoomName::parse("E10S0").unwrap(),
            RoomName { xx: 10, yy: 0 }
        );
        assert_eq!(
            RoomName::parse("W0N0").unwrap(),
            RoomName { xx: -1, yy: -1 }
        );
        // 小写方向位（官方 charAt 比较认小写）
        assert_eq!(
            RoomName::parse("w2n3").unwrap(),
            RoomName { xx: -3, yy: -4 }
        );
        // 三位数房间名：verticalPos=4
        assert_eq!(
            RoomName::parse("E100N5").unwrap(),
            RoomName { xx: 100, yy: -6 }
        );
        // 生成方向位
        assert_eq!(RoomName::from_xy(-2, -2).as_str(), "W1N1");
        assert_eq!(RoomName::from_xy(10, 0).as_str(), "E10S0");
        assert_eq!(RoomName::from_xy(-1, -1).as_str(), "W0N0");
        // 全象限往返
        for &(x, y) in &[(0i32, 0i32), (-5, 3), (7, -9), (-12, -34), (100, 100)] {
            let n = RoomName::from_xy(x, y);
            assert_eq!(RoomName::parse(&n.as_str()).unwrap(), n, "{x},{y}");
        }
    }

    #[test]
    fn rejects_malformed() {
        for bad in ["", "W", "W1", "X1N1", "W1X1", "WN1", "abc", "E1N"] {
            assert!(RoomName::parse(bad).is_err(), "应拒绝: {bad:?}");
        }
    }

    /// 拒绝非法方向、错位方向和过大的数值，避免上游宽松方向判断产生别名。
    /// 非 ASCII 数字两侧都无法作为十进制数字解析，不属于兼容性差异。
    #[test]
    fn rejects_invalid_directions_and_numbers() {
        for bad in ["X1N1", "E1X1", "W-100N5", "E١N1", "E2147483648N1"] {
            assert!(
                RoomName::parse(bad).is_err(),
                "应拒绝不可解析房间名: {bad:?}"
            );
        }
        // 垂直负数仍按前缀整数读取，再应用 N 方向的取负减一。
        assert_eq!(RoomName::parse("E1N-5").unwrap(), RoomName { xx: 1, yy: 4 });
    }

    #[test]
    fn formats_full_i32_coordinate_range() {
        let room = RoomName {
            xx: i32::MIN,
            yy: i32::MIN,
        };
        assert_eq!(room.as_str(), "W2147483647N2147483647");
        assert_eq!(room.to_string(), "W2147483647N2147483647");
        let room = RoomName {
            xx: i32::MAX,
            yy: i32::MAX,
        };
        assert_eq!(room.as_str(), "E2147483647S2147483647");
    }

    #[test]
    fn parsing_limits_and_prefixes() {
        for name in ["E1S2extra", "E1S+2", "E1S 2"] {
            assert_eq!(RoomName::parse(name).unwrap(), RoomName { xx: 1, yy: 2 });
        }
        assert_eq!(
            RoomName::parse("E999N999999999").unwrap(),
            RoomName {
                xx: 999,
                yy: -1_000_000_000,
            }
        );
        for name in ["E001N1", "E1000N5", "E1N1000000000", "E1N１２"] {
            assert!(RoomName::parse(name).is_err(), "解析限制: {name}");
        }
        for room in [
            RoomName::from_xy(-1000, 999_999_999),
            RoomName::from_xy(999, -999_999_999),
        ] {
            assert_eq!(RoomName::parse(&room.as_str()).unwrap(), room);
        }
    }
}
