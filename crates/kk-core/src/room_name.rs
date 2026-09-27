//! 房间名编解码：官方 `engine/src/utils.js:358-390` 的逐字移植。
//!
//! 官方坐标语义：W/N 方向取负并减一（W0 ↔ x=-1），E/S 方向即非负坐标本身。

use std::fmt;

/// 房间名（已验证合法）。合法形态：`[WE]\d+[NS]\d+`（大小写混认，与官方
/// `parseInt`/`charAt` 行为一致——官方解析对 `[we]`/`[ns]` 同样接受）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RoomName {
    /// 官方水平坐标：W 方向为负（W0 = -1），E 方向非负。
    pub xx: i32,
    /// 官方垂直坐标：N 方向为负（N0 = -1），S 方向非负。
    pub yy: i32,
}

impl RoomName {
    /// 解析房间名；非法名（NaN、缺方向位、乱序）返回 Err。
    ///
    /// 移植官方 `roomNameToXY`（utils.js:374-390）：
    /// - `parseInt(name.substr(1))`：从头方向位之后取前导数字（遇非数字停）；
    /// - 水平数值决定垂直方向位的位置（≥100 → 4，≥10 → 3，否则 2）；
    /// - `parseInt(name.substr(verticalPos + 1))` 取垂直数值；
    /// - W/N 把对应值取负再减一。
    pub fn parse(name: &str) -> Result<Self, String> {
        let bytes = name.as_bytes();
        if bytes.len() < 3 {
            return Err(format!("房间名过短: {name:?}"));
        }
        let horizontal_dir = bytes[0];
        if !matches!(horizontal_dir, b'W' | b'w' | b'E' | b'e') {
            return Err(format!("房间名水平方向位非法: {name:?}"));
        }
        // 官方 parseInt(name.substr(1))：跳过前导空白、取前导数字
        let xx =
            parse_leading_int(&name[1..]).ok_or_else(|| format!("房间名水平数值缺失: {name:?}"))?;
        let vertical_pos = if xx >= 100 {
            4
        } else if xx >= 10 {
            3
        } else {
            2
        };
        // 官方 substr(verticalPos + 1) 可能越界（substr 宽松出界得空串 → NaN）
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

    /// 从官方坐标生成房间名（官方 `getRoomNameFromXY`，utils.js:358-371）。
    ///
    /// 坐标应为 `parse` 产物或邻近房间的偏移结果（|值| < 10^9）；`i32::MIN`
    /// 会使 `as_str` 的取负减一溢出 panic（调用方不会自然产生该值，不设防）。
    pub fn from_xy(xx: i32, yy: i32) -> Self {
        Self { xx, yy }
    }

    /// 房间名字符串（官方形态，大写方向位）。
    pub fn as_str(&self) -> String {
        let x = if self.xx < 0 {
            format!("W{}", -self.xx - 1)
        } else {
            format!("E{}", self.xx)
        };
        let y = if self.yy < 0 {
            format!("N{}", -self.yy - 1)
        } else {
            format!("S{}", self.yy)
        };
        format!("{x}{y}")
    }
}

/// 官方 `parseInt` 语义的子集：跳过前导空白、可选符号、取前导十进制数字。
/// 空串/无数字 → None（对应 parseInt 的 NaN）。
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
        // 无数字或超 9 位：≥4 位水平数在官方的 verticalPos 定位（≤4）即落在数字
        // 位上得到 NaN 垃圾，同为"不可用"；9 位上限防 i32 溢出（parse 产物供
        // 偏移运算）。Unicode 数字（官方 parseInt 接受）同样拒绝——登记灰区
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

    /// 与官方宽松解析的有意差异（compatibility.md 灰区登记）：官方对方向位
    /// 只判 W/w 与 N/n、其余一律当 E/S，因此 "X1N1"→[1,-2]、"E1X1"→[1,1]、
    /// "W-100N5"→[99,0]（符号消费后方向位落在数字上）；官方 parseInt 还接受
    /// Unicode 数字（"E١N1"）。官方这些输入得到合法或错误坐标，我们一律 Err
    /// ——强类型构造即校验，不产出"静默别名到其他房间"的坐标。
    #[test]
    fn strict_rejects_official_loose_forms() {
        for bad in ["X1N1", "E1X1", "W-100N5", "E١N1", "E2147483648N1"] {
            assert!(RoomName::parse(bad).is_err(), "应拒绝官方宽松形态: {bad:?}");
        }
        // 对照：垂直数值的负号是官方我方一致的合法形态（parseInt 认 -）
        assert_eq!(RoomName::parse("E1N-5").unwrap(), RoomName { xx: 1, yy: 4 });
    }
}
