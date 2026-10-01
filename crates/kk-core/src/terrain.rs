//! 单房间的 50×50 地形，按行保存为字节数组。
//!
//! 每格使用 wall=1、swamp=2、lava=4 的位掩码，可以组合；索引为 `y * 50 + x`。
//! 构造与补齐四角墙是两个操作，构造函数不会自动调用 [`Terrain::normalize`]。
//! 来源：Screeps engine `src/utils.js:333-335` 的 `checkTerrain`，以及 common
//! `lib/constants.js` 的 `TERRAIN_MASK_*` 定义。Rust 读取接口额外检查每轴边界。

/// 墙地形位；与上游 `TERRAIN_MASK_WALL` 对应。
pub const TERRAIN_MASK_WALL: u8 = 1;
/// 沼泽地形位；与上游 `TERRAIN_MASK_SWAMP` 对应。
pub const TERRAIN_MASK_SWAMP: u8 = 2;
/// 熔岩地形位；与上游 `TERRAIN_MASK_LAVA` 对应，不单独决定地形名称。
pub const TERRAIN_MASK_LAVA: u8 = 4;

/// 全部合法位的并集；之外的位值视为非法输入。
const KNOWN_MASKS: u8 = TERRAIN_MASK_WALL | TERRAIN_MASK_SWAMP | TERRAIN_MASK_LAVA;

/// 房间每轴的格子数，有效坐标为 0..ROOM_SIZE。
pub const ROOM_SIZE: usize = 50;
/// 一个房间的地形字节数，每格一个字节。
pub const TERRAIN_LEN: usize = ROOM_SIZE * ROOM_SIZE;

/// 一个房间的地形，以 Arc 共享固定长度的字节数组。
///
/// 克隆只共享数组，不复制 2500 字节。修改要求独占底层 Arc；持有其他克隆时，
/// [`Self::normalize`] 和 [`Self::as_mut_slice`] 会 panic，不会复制或修改共享数据。
/// 地图应在共享前完成构造。是否允许在游戏运行中替换地形由世界层控制，
/// 本类型不跟踪 tick，也不保证四角已经补墙或位码始终有效。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrain(std::sync::Arc<[u8; TERRAIN_LEN]>);

impl Terrain {
    /// 创建所有位码为 0 的地形，包括四角；需要四角墙时另调 [`Self::normalize`]。
    pub fn plains() -> Self {
        Self(std::sync::Arc::new([0; TERRAIN_LEN]))
    }

    /// 校验并复制按行排列的地形字节，保留原位码，不自动补四角墙。
    ///
    /// # Errors
    /// 长度不等于 [`TERRAIN_LEN`]，或任一格包含 wall/swamp/lava 之外的位时返回错误。
    pub fn from_codes(codes: &[u8]) -> Result<Self, String> {
        if codes.len() != TERRAIN_LEN {
            return Err(format!(
                "地形长度必须为 {TERRAIN_LEN}，实际 {}",
                codes.len()
            ));
        }
        if let Some(i) = codes.iter().position(|c| c & !KNOWN_MASKS != 0) {
            return Err(format!(
                "地形值含未知位: 位置 {i} 值 {}（合法位 wall|swamp|lava）",
                codes[i]
            ));
        }
        let mut arr = [0u8; TERRAIN_LEN];
        arr.copy_from_slice(codes);
        Ok(Self(std::sync::Arc::new(arr)))
    }

    /// 复制固定长度、由调用方保证位码合法的数组，不自动补四角墙。
    /// 未验证的输入应使用 [`Self::from_codes`]；本方法不提供运行期校验错误。
    ///
    /// # Panics
    /// 开启调试断言时，未知位会触发 panic；关闭调试断言后不检查位码。
    pub fn from_slice(codes: &[u8; TERRAIN_LEN]) -> Self {
        debug_assert!(
            codes.iter().all(|c| c & !KNOWN_MASKS == 0),
            "from_slice 收到含未知位的地形字节"
        );
        let mut arr = [0u8; TERRAIN_LEN];
        arr.copy_from_slice(codes);
        Self(std::sync::Arc::new(arr))
    }

    /// 解码按行排列的数字字符串，每个 ASCII 字符对应一格地形。
    /// 格式对应上游 `checkTerrain` 的字符串读取分支，不自动补四角墙。
    ///
    /// # Errors
    /// 字符不是 ASCII 数字、长度不是 2500，或数字含未知位时返回错误；
    /// 因此合法字符为 0..=7，8 和 9 虽是数字也不能作为地形位码。
    pub fn from_digit_string(s: &str) -> Result<Self, String> {
        let codes: Result<Vec<u8>, _> = s
            .chars()
            .map(|c| {
                c.to_digit(10)
                    .map(|d| d as u8)
                    .ok_or_else(|| format!("非法地形字符: {c:?}"))
            })
            .collect();
        Self::from_codes(&codes?)
    }

    /// 给四角添加墙位，保留这些格子原有的其他位；其余格子不变。
    /// 世界创建和地形设置入口用此方法补齐地图边界要求，重复调用结果不变。
    ///
    /// # Panics
    /// 底层 Arc 不满足独占修改条件时 panic，包括还有其他 Terrain 克隆的情况。
    pub fn normalize(&mut self) {
        let idx = |x: usize, y: usize| y * ROOM_SIZE + x;
        let arr = std::sync::Arc::get_mut(&mut self.0).expect("地形归一化要求独占引用");
        arr[idx(0, 0)] |= TERRAIN_MASK_WALL;
        arr[idx(ROOM_SIZE - 1, 0)] |= TERRAIN_MASK_WALL;
        arr[idx(0, ROOM_SIZE - 1)] |= TERRAIN_MASK_WALL;
        arr[idx(ROOM_SIZE - 1, ROOM_SIZE - 1)] |= TERRAIN_MASK_WALL;
    }

    /// 读取一格的原始位码，按 `y * ROOM_SIZE + x` 定位。
    ///
    /// # Panics
    /// x 或 y 不在 0..ROOM_SIZE 内时 panic；越界 x 不得被解释为下一行的格子。
    pub fn get(&self, x: usize, y: usize) -> u8 {
        assert!(
            x < ROOM_SIZE && y < ROOM_SIZE,
            "地形坐标越界: ({x}, {y})，每轴必须小于 {ROOM_SIZE}"
        );
        self.0[y * ROOM_SIZE + x]
    }

    /// 判断格子是否包含 mask 中的任一位；组合掩码不是要求全部位都存在。
    /// 掩码为 0 时返回 false，坐标边界要求与 [`Self::get`] 相同。
    /// 位判断对应上游 `src/utils.js` 的 `checkTerrain`。
    pub fn check_mask(&self, x: usize, y: usize, mask: u8) -> bool {
        self.get(x, y) & mask > 0
    }

    /// 判断墙位是否存在，坐标边界要求与 [`Self::get`] 相同。
    pub fn is_wall(&self, x: usize, y: usize) -> bool {
        self.check_mask(x, y, TERRAIN_MASK_WALL)
    }

    /// 判断沼泽位是否存在；墙和沼泽位可以同时为真。
    /// 坐标边界要求与 [`Self::get`] 相同。
    pub fn is_swamp(&self, x: usize, y: usize) -> bool {
        self.check_mask(x, y, TERRAIN_MASK_SWAMP)
    }

    /// 返回地形名称：wall 优先于 swamp，其余返回 plain，包括单独的 lava 位。
    /// 坐标边界要求与 [`Self::get`] 相同。
    pub fn terrain_name(&self, x: usize, y: usize) -> &'static str {
        let c = self.get(x, y);
        if c & TERRAIN_MASK_WALL != 0 {
            "wall"
        } else if c & TERRAIN_MASK_SWAMP != 0 {
            "swamp"
        } else {
            "plain"
        }
    }

    /// 借用按行排列的完整数组，不复制地形数据。
    pub fn as_slice(&self) -> &[u8; TERRAIN_LEN] {
        &self.0
    }

    /// 独占借用完整数组，用于地图构造或测试；写入值不经过校验。
    /// 调用方负责保持位码合法，并在需要时重新补齐四角墙。
    ///
    /// # Panics
    /// 底层 Arc 不满足独占修改条件时 panic；不会自动复制共享数组。
    pub fn as_mut_slice(&mut self) -> &mut [u8; TERRAIN_LEN] {
        std::sync::Arc::get_mut(&mut self.0).expect("可变访问要求独占引用")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 还有其他克隆时，补墙操作必须拒绝修改共享数组。
    #[test]
    #[should_panic(expected = "独占")]
    fn normalize_on_shared_terrain_panics() {
        let t = Terrain::plains();
        let holder = t.clone();
        let mut t = t;
        t.normalize();
        drop(holder);
    }

    #[test]
    #[should_panic(expected = "独占")]
    fn as_mut_slice_on_shared_terrain_panics() {
        let t = Terrain::plains();
        let holder = t.clone();
        let mut t = t;
        let _ = t.as_mut_slice();
        drop(holder);
    }

    #[test]
    fn validates_length_and_bits() {
        assert!(Terrain::from_codes(&[0; 2499]).is_err());
        assert!(Terrain::from_codes(&[0; 2501]).is_err());
        assert!(Terrain::from_codes(&[0; 2500]).is_ok());
        let mut bad = [0u8; 2500];
        bad[1234] = 0b1000; // 未知位 8
        let err = Terrain::from_codes(&bad).unwrap_err();
        assert!(err.contains("未知位"));
        // 合法位组合 3 = wall|swamp
        bad[1234] = 3;
        assert!(Terrain::from_codes(&bad).is_ok());
    }

    #[test]
    fn constructors_preserve_codes_and_masks_match_any_bit() {
        let mut codes = [0; TERRAIN_LEN];
        codes[1] = TERRAIN_MASK_LAVA;
        codes[2] = TERRAIN_MASK_WALL | TERRAIN_MASK_SWAMP | TERRAIN_MASK_LAVA;
        let t = Terrain::from_codes(&codes).unwrap();
        assert_eq!(t.get(0, 0), 0, "构造不会自动补四角墙");
        assert_eq!(t.terrain_name(1, 0), "plain", "lava 位不单独改变名称");
        assert_eq!(t.terrain_name(2, 0), "wall", "墙位优先于沼泽位");
        assert!(t.is_swamp(2, 0), "名称为 wall 不意味着沼泽位被清除");
        assert!(t.check_mask(1, 0, TERRAIN_MASK_WALL | TERRAIN_MASK_LAVA));
        assert!(!t.check_mask(1, 0, 0));
        assert!(Terrain::from_digit_string(&"8".repeat(TERRAIN_LEN)).is_err());
        assert!(Terrain::from_digit_string(&"１".repeat(TERRAIN_LEN)).is_err());
    }

    #[test]
    fn corner_walls_and_lookup() {
        let mut t = Terrain::plains();
        t.as_mut_slice()[7 * 50 + 7] = TERRAIN_MASK_SWAMP;
        t.normalize();
        assert!(t.is_wall(0, 0) && t.is_wall(49, 0) && t.is_wall(0, 49) && t.is_wall(49, 49));
        assert!(t.is_swamp(7, 7));
        assert_eq!(t.terrain_name(7, 7), "swamp");
        assert_eq!(t.terrain_name(1, 1), "plain");
        assert_eq!(t.terrain_name(0, 0), "wall");
        // normalize 用 |=：原有位保留
        t.as_mut_slice()[0] = TERRAIN_MASK_SWAMP;
        t.normalize();
        assert_eq!(t.0[0], TERRAIN_MASK_SWAMP | TERRAIN_MASK_WALL);
    }

    #[test]
    fn digit_string_roundtrip() {
        let mut codes = [0u8; 2500];
        codes[5] = 1;
        codes[6] = 2;
        let t = Terrain::from_codes(&codes).unwrap();
        let s: String = t.as_slice().iter().map(|c| (b'0' + c) as char).collect();
        assert_eq!(Terrain::from_digit_string(&s).unwrap(), t);
        assert!(Terrain::from_digit_string("9x").is_err());
    }

    #[test]
    fn rejects_coordinates_outside_each_axis() {
        let t = Terrain::plains();
        for (x, y) in [
            (ROOM_SIZE, 0),
            (ROOM_SIZE * 2, 0),
            (0, ROOM_SIZE),
            (usize::MAX, 1),
            (ROOM_SIZE, usize::MAX),
        ] {
            assert!(
                std::panic::catch_unwind(|| t.get(x, y)).is_err(),
                "越界坐标 ({x}, {y}) 必须被拒绝，不能读取另一格"
            );
        }
    }
}
