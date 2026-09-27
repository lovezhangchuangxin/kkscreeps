//! 地形：50×50 平面字节数组 + 官方 `TERRAIN_MASK_*` 位语义。
//!
//! 官方读取语义（`engine/src/utils.js:333-337 checkTerrain`）：
//! `code = terrain[y*50+x]; (code & mask) > 0`。
//! 合法位：wall=1 / swamp=2 / lava=4（lava 仅官方地图生成器产出，编解码同样接受）。

/// 墙位掩码（官方 TERRAIN_MASK_WALL）。
pub const TERRAIN_MASK_WALL: u8 = 1;
/// 沼泽位掩码（官方 TERRAIN_MASK_SWAMP）。
pub const TERRAIN_MASK_SWAMP: u8 = 2;
/// 熔岩位掩码（官方 TERRAIN_MASK_LAVA）。
pub const TERRAIN_MASK_LAVA: u8 = 4;

/// 全部合法位的并集；之外的位值视为非法输入。
const KNOWN_MASKS: u8 = TERRAIN_MASK_WALL | TERRAIN_MASK_SWAMP | TERRAIN_MASK_LAVA;

pub const ROOM_SIZE: usize = 50;
pub const TERRAIN_LEN: usize = ROOM_SIZE * ROOM_SIZE;

/// 一个房间的地形。构造即校验（长度必须 2500、值域仅含已知位——非整数输入
/// 在 embed 边界层拒绝，字节层只能看到合法位），四角墙不变量（官方地图生成器
/// 不变量，所有正规地图成立）经 [`Terrain::normalize`] 强制。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terrain(Box<[u8; TERRAIN_LEN]>);

impl Terrain {
    /// 全平原地形。
    pub fn plains() -> Self {
        Self(Box::new([0; TERRAIN_LEN]))
    }

    /// 从官方数组形态构造（embed `setTerrain` 的 `number[]`）。
    /// 校验失败返回 Err（不改任何状态——调用方契约依赖"拒绝且无副作用"）。
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
        let mut arr = Box::new([0u8; TERRAIN_LEN]);
        arr.copy_from_slice(codes);
        Ok(Self(arr))
    }

    /// 从已验证的字节构造（快照解码等信任源；绕过校验但类型上仍是合法位）。
    pub fn from_slice(codes: &[u8; TERRAIN_LEN]) -> Self {
        let mut arr = Box::new([0u8; TERRAIN_LEN]);
        arr.copy_from_slice(codes);
        Self(arr)
    }

    /// 官方字符串形态解码（官方 DB 历史存数字串；`checkTerrain` 的
    /// `terrain.charAt(y*50+x)` 分支）：每字符一个数字。
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

    /// 四角强制为墙（官方地图生成器不变量——正规地图的 (0,0)/(49,0)/(0,49)/(49,49) 恒为墙）。
    pub fn normalize(&mut self) {
        let idx = |x: usize, y: usize| y * ROOM_SIZE + x;
        self.0[idx(0, 0)] |= TERRAIN_MASK_WALL;
        self.0[idx(ROOM_SIZE - 1, 0)] |= TERRAIN_MASK_WALL;
        self.0[idx(0, ROOM_SIZE - 1)] |= TERRAIN_MASK_WALL;
        self.0[idx(ROOM_SIZE - 1, ROOM_SIZE - 1)] |= TERRAIN_MASK_WALL;
    }

    /// 读取一格的原始位码（越界坐标 panic——结算层坐标已被钳制）。
    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.0[y * ROOM_SIZE + x]
    }

    /// 官方 `checkTerrain` 语义：`(code & mask) > 0`。
    pub fn check_mask(&self, x: usize, y: usize, mask: u8) -> bool {
        self.get(x, y) & mask > 0
    }

    pub fn is_wall(&self, x: usize, y: usize) -> bool {
        self.check_mask(x, y, TERRAIN_MASK_WALL)
    }

    pub fn is_swamp(&self, x: usize, y: usize) -> bool {
        self.check_mask(x, y, TERRAIN_MASK_SWAMP)
    }

    /// 玩家 API 形态的名字（官方 `getTerrainAt`：wall 优先、swamp 次之、其余 plain）。
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

    pub fn as_slice(&self) -> &[u8; TERRAIN_LEN] {
        &self.0
    }

    /// 可变访问（测试/地图构造用；值域约束由调用方自负——正式路径走 from_codes）。
    pub fn as_mut_slice(&mut self) -> &mut [u8; TERRAIN_LEN] {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn corner_walls_and_lookup() {
        let mut t = Terrain::plains();
        t.0[7 * 50 + 7] = TERRAIN_MASK_SWAMP;
        t.normalize();
        assert!(t.is_wall(0, 0) && t.is_wall(49, 0) && t.is_wall(0, 49) && t.is_wall(49, 49));
        assert!(t.is_swamp(7, 7));
        assert_eq!(t.terrain_name(7, 7), "swamp");
        assert_eq!(t.terrain_name(1, 1), "plain");
        assert_eq!(t.terrain_name(0, 0), "wall");
        // normalize 用 |=：原有位保留
        t.0[0] = TERRAIN_MASK_SWAMP;
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
}
