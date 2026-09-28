//! # kk-pathfinder
//!
//! 寻路：FFI 直接复用官方 C++ 源码（`native/pf.cc` / `native/pf.h`，官方
//! `driver/native/src/` 的原样拷贝，sha256 一致），保证路径**逐点一致**——包括
//! 堆内平局裁决（tie-breaking）——不自行实现算法（ADR-004）。
//!
//! 分层：
//! - `native/compat/nan.h`：最小 Nan/v8 兼容层（自有文件），让官方源码零改动编译；
//! - `native/kk_ffi.cc`：C ABI 包装，只做数据搬运与结果分类，不含寻路逻辑；
//! - [`ffi`] 模块：unsafe 绑定集中地（安全性说明见模块文档）；
//! - 本文件的公开 API：官方 JS 调用面的 Rust 等价物。
//!
//! 官方调用面对照（本 crate 等价于 driver 层的 `driver.pathFinder.search`，
//! 经 driver/lib/runtime/runtime-driver.js:124-125 注入引擎）：
//! - 参数形态：`driver/lib/path-finder.js:62-95`（origin/goals 世界坐标、九个选项）；
//! - 返回形态：`driver/lib/path-finder.js:96-102`（path/ops/cost/incomplete 及
//!   undefined / -1 两种特殊返回的映射）；
//! - CostMatrix 序列化：`engine/src/game/path-finder.js:40-48`（2500 字节按
//!   little-endian uint32×625 重解释）；
//! - 地形 packed bits：`driver/lib/path-finder.js:20-27`（每格 2bit，下标 xx*50+yy）。

mod ffi;

use std::fmt;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 格值：平原（terrain 数组取值 0..=3；0=plain、1=wall、2=swamp，3 亦按障碍处理
/// ——pf.h:351 look_table 初值全 obstacle，pf.cc:495-496 只覆写 0/2 两项）。
pub const TERRAIN_PLAIN: u8 = 0;
pub const TERRAIN_WALL: u8 = 1;
pub const TERRAIN_SWAMP: u8 = 2;

/// 地形全局表的并发守卫。官方形态：地形在进程启动时一次性载入、之后只读
/// （driver/lib/path-finder.js:12-30 的 init）；本 crate 允许多次 `load_terrain`
/// （测试需要），用全局读写锁把写入与搜索串行化，避免 C++ 静态表（pf.h:359）
/// 的数据竞争。
static TERRAIN_LOCK: RwLock<()> = RwLock::new(());

fn terrain_read() -> RwLockReadGuard<'static, ()> {
    TERRAIN_LOCK.read().unwrap_or_else(|e| e.into_inner())
}

fn terrain_write() -> RwLockWriteGuard<'static, ()> {
    TERRAIN_LOCK.write().unwrap_or_else(|e| e.into_inner())
}

/// 错误类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 房间名不合法（对照官方 `Invalid room name`，driver/lib/path-finder.js:8-9）
    InvalidRoomName(String),
    /// 房内坐标越界（0..50，对照 `Invalid room position`，driver/lib/path-finder.js:36-37）
    InvalidPosition { x: u32, y: u32 },
    /// CostMatrix 反序列化长度错误（官方 JS 侧无长度检查，错误长度在 native 侧被
    /// 静默忽略——pf.cc:55 的 `length() == 2500` 检查；本 API 提前拒绝：错误矩阵
    /// 本就不会生效，玩家可见结果不变）
    CostMatrixLength(usize),
    /// C++ 侧异常逃逸或内存不足（正常输入不可达，见 kk_ffi.cc 的兜底说明）
    NativeFailure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidRoomName(name) => write!(f, "invalid room name: {name}"),
            Error::InvalidPosition { x, y } => write!(f, "invalid room position: ({x},{y})"),
            Error::CostMatrixLength(len) => write!(f, "cost matrix length != 625: {len}"),
            Error::NativeFailure => write!(f, "native pathfinder failure"),
        }
    }
}

impl std::error::Error for Error {}

/// 解析房间名 → 世界网格坐标（map position）。
///
/// 对照 `parseRoomName`（driver/lib/path-finder.js:6-19）：形如
/// `/^([WE])([0-9]+)([NS])([0-9]+)$/`；`rx = 127 + (W ? -n : n+1)`，
/// `ry = 127 + (N ? -n : n+1)`，越界（>255）报错。与 JS `Number()` 一致，
/// 接受前导零（"E01N0" == "E1N0"）。
pub fn parse_room_name(room_name: &str) -> Option<(u8, u8)> {
    let bytes = room_name.as_bytes();
    let parse_axis = |digits: &[u8]| -> Option<u32> {
        // `[0-9]+`；u32 溢出按越界处理（JS 中同样落入范围检查失败）
        if digits.is_empty() {
            return None;
        }
        let mut value: u64 = 0;
        for &b in digits {
            if !b.is_ascii_digit() {
                return None;
            }
            value = value * 10 + u64::from(b - b'0');
            if value > u32::MAX.into() {
                return None;
            }
        }
        u32::try_from(value).ok()
    };
    let ns_pos = bytes.iter().position(|&b| b == b'N' || b == b'S')?;
    if ns_pos == 0 {
        return None; // 缺 [WE] 段（如 "N0"），避免 1..0 反向切片
    }
    let h_num = parse_axis(&bytes[1..ns_pos])?;
    let v_num = parse_axis(&bytes[ns_pos + 1..])?;
    let rx = match bytes.first() {
        Some(b'W') => 127i64 - i64::from(h_num),
        Some(b'E') => 127 + i64::from(h_num) + 1,
        _ => return None,
    };
    let ry = match bytes[ns_pos] {
        b'N' => 127i64 - i64::from(v_num),
        b'S' => 127 + i64::from(v_num) + 1,
        _ => return None,
    };
    if !(0..=255).contains(&rx) || !(0..=255).contains(&ry) {
        return None;
    }
    Some((rx as u8, ry as u8))
}

/// 世界网格坐标 → 房间名。对照 `generateRoomName`（driver/lib/path-finder.js:23-27）。
pub fn generate_room_name(xx: u8, yy: u8) -> String {
    let horizontal = if xx <= 127 {
        format!("W{}", 127 - i32::from(xx))
    } else {
        format!("E{}", i32::from(xx) - 128)
    };
    let vertical = if yy <= 127 {
        format!("N{}", 127 - i32::from(yy))
    } else {
        format!("S{}", i32::from(yy) - 128)
    };
    format!("{horizontal}{vertical}")
}

/// 世界连续平面坐标（对照 pf.h:59-81 `world_position_t`；JS 侧换算见
/// driver/lib/path-finder.js:34-44 `toWorldPosition`：`xx = x + offset.xx * 50`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldPosition {
    pub xx: u32,
    pub yy: u32,
}

impl WorldPosition {
    /// 房间名 + 房内坐标 → 世界坐标。x/y 必须 < 50（driver/lib/path-finder.js:35-37）。
    pub fn from_room(room_name: &str, x: u32, y: u32) -> Result<Self, Error> {
        if x >= 50 || y >= 50 {
            return Err(Error::InvalidPosition { x, y });
        }
        let (ox, oy) = parse_room_name(room_name)
            .ok_or_else(|| Error::InvalidRoomName(room_name.to_owned()))?;
        Ok(WorldPosition {
            xx: x + u32::from(ox) * 50,
            yy: y + u32::from(oy) * 50,
        })
    }

    /// 所在房间名（driver/lib/path-finder.js:21-27 `fromWorldPosition`）。
    pub fn room_name(&self) -> String {
        generate_room_name((self.xx / 50) as u8, (self.yy / 50) as u8)
    }

    /// 房内 x（`wp[0] % 50`，driver/lib/path-finder.js:17-19）。
    pub fn x(&self) -> u32 {
        self.xx % 50
    }

    /// 房内 y（`wp[1] % 50`）。
    pub fn y(&self) -> u32 {
        self.yy % 50
    }
}

/// 寻路目标：位置 + range（Chebyshev 距离域；`goal.range` 见
/// driver/lib/path-finder.js:74-80，heuristic 语义 pf.cc:126-148）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    pub pos: WorldPosition,
    pub range: u32,
}

impl Goal {
    /// range = 0 的目标（官方对 `{x,y,roomName}` 形态的默认，path-finder.js:71-73）。
    pub fn pos(pos: WorldPosition) -> Self {
        Goal { pos, range: 0 }
    }

    pub fn with_range(pos: WorldPosition, range: u32) -> Self {
        Goal { pos, range }
    }
}

/// 搜索选项。`Default` 与选项钳制均对照 driver/lib/path-finder.js:90-96：
/// `plainCost: min(254, max(1, x||1))`、`swampCost: … x||5`、
/// `heuristicWeight: min(9, max(1, x||1.2))`、`maxOps: max(1, x||2000)`、
/// `maxCost: max(1, x||0xffffffff)`、`maxRooms: min(64, max(1, x||16))`。
///
/// 注：官方 `PathFinder.search` 选项没有 roadCost——道路不是地形，其代价通过
/// roomCallback 返回的 CostMatrix 表达（官方引擎在 JS 侧生成矩阵，玩家侧同理）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchOptions {
    pub plain_cost: u32,
    pub swamp_cost: u32,
    pub heuristic_weight: f64,
    pub max_ops: u32,
    pub max_cost: u32,
    pub max_rooms: u32,
    pub flee: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            plain_cost: 1,
            swamp_cost: 5,
            heuristic_weight: 1.2,
            max_ops: 2000,
            max_cost: u32::MAX,
            max_rooms: 16,
            flee: false,
        }
    }
}

impl SearchOptions {
    /// 按 JS 语义钳制（`x || default` 的 0/NaN 回退 + min/max 夹逼，
    /// driver/lib/path-finder.js:90-96）。
    fn clamped(&self) -> SearchOptions {
        let or = |v: u32, d: u32| if v == 0 { d } else { v };
        let hw = if self.heuristic_weight == 0.0 || self.heuristic_weight.is_nan() {
            1.2
        } else {
            self.heuristic_weight
        };
        SearchOptions {
            plain_cost: or(self.plain_cost, 1).clamp(1, 254),
            swamp_cost: or(self.swamp_cost, 5).clamp(1, 254),
            heuristic_weight: hw.clamp(1.0, 9.0),
            max_ops: or(self.max_ops, 2000).max(1),
            max_cost: or(self.max_cost, u32::MAX).max(1),
            max_rooms: or(self.max_rooms, 16).clamp(1, 64),
            flee: self.flee,
        }
    }
}

/// roomCallback 的返回决策（对照官方 JS bridge 给 native 的返回形态，
/// driver/lib/path-finder.js:114-131 → pf.cc:47-58）。
pub enum RoomDecision<'a> {
    /// 返回 undefined / 非 TypedArray：该房间用纯地形（pf.cc:55 长度检查不过 →
    /// cost_matrix 保持 null）。
    None,
    /// 返回 false：封锁该房间（pf.cc:49-52 blocked_rooms）。
    Blocked,
    /// 返回 Uint8Array(2500)：该房间的 CostMatrix。**引用必须存活到 search 返回**
    /// （pf.cc 直接引用该缓冲，不拷贝）；2500 字节布局 `[xx*50 + yy]`
    /// （path-finder.js:25）。255 (0xff) 表示该格不可通行（pf.cc:115-116）。
    CostMatrix(&'a CostMatrix),
}

/// 搜索结果（对照 driver/lib/path-finder.js:96-102 的返回对象）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// 路径（**origin→goal 序**；native 产出为 goal→origin 序，JS 侧 reverse，
    /// driver/lib/path-finder.js:101，本层等价处理）。
    pub path: Vec<WorldPosition>,
    pub ops: u32,
    pub cost: u32,
    pub incomplete: bool,
}

impl SearchResult {
    fn empty(incomplete: bool) -> Self {
        SearchResult {
            path: Vec::new(),
            ops: 0,
            cost: 0,
            incomplete,
        }
    }
}

/// 2D 代价矩阵（对照 engine/src/game/path-finder.js:18-48 的 CostMatrix）。
///
/// 内存布局 `[xx*50 + yy]`（`this._bits[xx * 50 + yy]`，path-finder.js:25）；
/// `set` 钳制 0..=255（`Math.min(Math.max(0, val), 255)`，同出处）；
/// 255 = 不可通行（pf.cc:115-116）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostMatrix(Box<[u8; 2500]>);

impl Default for CostMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl CostMatrix {
    pub fn new() -> Self {
        CostMatrix(Box::new([0u8; 2500]))
    }

    /// 写入代价（钳制到 0..=255，对照 path-finder.js:22-26）。JS 侧 `val` 先经
    /// `|0` 数值化再 min/max 夹逼——本 API 取 u32，只需上界钳制。
    pub fn set(&mut self, x: usize, y: usize, val: u32) {
        assert!(x < 50 && y < 50, "cost matrix index out of range");
        self.0[x * 50 + y] = val.min(255) as u8;
    }

    /// 读取代价（对照 path-finder.js:28-32）。
    pub fn get(&self, x: usize, y: usize) -> u32 {
        assert!(x < 50 && y < 50, "cost matrix index out of range");
        u32::from(self.0[x * 50 + y])
    }

    /// 序列化：2500 字节按 little-endian 重解释为 625 个 uint32
    /// （`Array.prototype.slice.apply(new Uint32Array(this._bits.buffer))`，
    /// path-finder.js:40-42；官方平台均为 LE，此处显式 LE 保证跨平台逐字节一致）。
    pub fn serialize(&self) -> Vec<u32> {
        self.0
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[b0, b1, b2, b3]| u32::from_le_bytes([b0, b1, b2, b3]))
            .collect()
    }

    /// 反序列化（`new Uint8Array(new Uint32Array(data).buffer)`，path-finder.js:44-48）。
    pub fn deserialize(data: &[u32]) -> Result<Self, Error> {
        if data.len() != 625 {
            return Err(Error::CostMatrixLength(data.len()));
        }
        let mut bits = Box::new([0u8; 2500]);
        for (ii, word) in data.iter().enumerate() {
            bits[ii * 4..ii * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        Ok(CostMatrix(bits))
    }

    fn raw(&self) -> &[u8; 2500] {
        &self.0
    }
}

/// 把 50×50 地形格打包成官方 625 字节 packed bits。
///
/// 输入 `tiles[y][x]`（行主序，与官方 `room.terrain` 字符串下标 `[yy*50 + xx]`
/// 一致，driver/lib/path-finder.js:21-22）；打包公式对照 :23：
/// `pack[ii/4] = pack[ii/4] & ~(0x03 << ii%4*2) | bit << ii%4*2`，其中 `ii = xx*50 + yy`。
/// 格值取 [`TERRAIN_PLAIN`]/[`TERRAIN_WALL`]/[`TERRAIN_SWAMP`]（>3 的值按官方
/// 位搬运语义截取低 2 位）。
pub fn pack_terrain(tiles: &[[u8; 50]; 50]) -> [u8; 625] {
    let mut bits = [0u8; 625];
    for (yy, row) in tiles.iter().enumerate() {
        for (xx, &tile) in row.iter().enumerate() {
            let ii = xx * 50 + yy;
            let shift = ii % 4 * 2;
            bits[ii / 4] = bits[ii / 4] & !(0x03 << shift) | ((tile & 0x03) << shift);
        }
    }
    bits
}

/// 载入一个房间的地形（可多次调用、不同房间；对照官方启动期 `loadTerrain`，
/// driver/lib/path-finder.js:12-30）。
///
/// 全局副作用：写入 C++ 静态地形表（pf.h:359）；与并发 search 用全局锁串行化。
/// 对同一房间重复载入会覆盖（旧 625 字节不回收——与官方多次 loadTerrain 行为
/// 一致，pf.cc:605-613）。
pub fn load_terrain(room_name: &str, bits: &[u8; 625]) -> Result<(), Error> {
    let (xx, yy) =
        parse_room_name(room_name).ok_or_else(|| Error::InvalidRoomName(room_name.to_owned()))?;
    let _guard = terrain_write();
    // Safety: 写锁持有期内无并发 kk_pf_* 调用；bits 在调用期内有效
    if !unsafe { ffi::ffi_load_terrain(xx, yy, bits) } {
        return Err(Error::NativeFailure);
    }
    Ok(())
}

/// 共同的收尾：native 三种返回 → 官方 JS bridge 的返回对象
/// （driver/lib/path-finder.js:96-102）。
fn finish(outcome: Option<ffi::RawOutcome>) -> Result<SearchResult, Error> {
    match outcome {
        None => Err(Error::NativeFailure),
        // native undefined → {path:[], ops:0, cost:0, incomplete:false}
        //（driver/lib/path-finder.js:97-98；触发条件 pf.cc:508-510 同格搜索、
        // pf.cc:563-567 JS 异常路径——含目标方向房间缺地形）
        Some(raw) if raw.status == 2 => Ok(SearchResult::empty(false)),
        // native -1 → {path:[], ops:0, cost:0, incomplete:true}
        //（driver/lib/path-finder.js:99-100；初始房间不可达 pf.cc:515-519）
        Some(raw) if raw.status == 1 => Ok(SearchResult::empty(true)),
        Some(raw) => {
            // reverse：native 为 goal→origin 序（pf.cc:574-594），JS 侧
            // `ret.path.map(fromWorldPosition).reverse()`（driver/lib/path-finder.js:101）
            let mut path: Vec<WorldPosition> = raw
                .path
                .iter()
                .map(|&(xx, yy)| WorldPosition { xx, yy })
                .collect();
            path.reverse();
            Ok(SearchResult {
                path,
                ops: raw.ops,
                cost: raw.cost,
                incomplete: raw.incomplete,
            })
        }
    }
}

/// 无 roomCallback 的搜索（官方 `options.roomCallback` 缺省形态，
/// driver/lib/path-finder.js:82-92 → pf.cc:488-492 room_callback=null）。
pub fn search(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
) -> Result<SearchResult, Error> {
    // 空 goals 早退：game 层 `PathFinder.search` 对空 goal 返回 {path:[], ops:0}
    // （engine/src/game/path-finder.js:60-62）；driver 层无此检查，但空 goals 会让
    // native 白烧满 max_ops（heuristic 恒为 max 永不命中），安全层提前返回，
    // 玩家可见行为一致。
    if goals.is_empty() {
        return Ok(SearchResult::empty(false));
    }
    let clamped = options.clamped();
    let c_options = ffi::Options::new(
        clamped.plain_cost,
        clamped.swamp_cost,
        clamped.max_ops,
        clamped.max_cost,
        clamped.max_rooms,
        clamped.flee,
        clamped.heuristic_weight,
    );
    let goal_triples: Vec<(u32, u32, u32)> = goals
        .iter()
        .map(|g| (g.pos.xx, g.pos.yy, g.range))
        .collect();
    let _terrain = terrain_read();
    // Safety: goals 切片与选项存活于调用期；读锁防并发写地形；无回调
    let outcome = unsafe {
        ffi::ffi_search(
            (origin.xx, origin.yy),
            &goal_triples,
            c_options,
            None,
            std::ptr::null_mut(),
        )
    };
    finish(outcome)
}

/// 带 roomCallback 的搜索。回调收 (地图 xx, 地图 yy)——对照官方 native 回调签名
/// `function(xx, yy)`（pf.cc:40-42）；JS bridge 才把地图坐标换算成房名交给玩家
/// （driver/lib/path-finder.js:85-89），本层直接给地图坐标，房名可用
/// [`generate_room_name`] 还原。
///
/// 回调在搜索按需触达房间时被调用（每房间至多一次，pf.cc:22-64；首房为
/// origin 所在房间）。注意：回调内不得调用 [`load_terrain`]（全局写锁会死锁；
/// 官方同样只允许启动期载入地形）。
pub fn search_with_room_callback<'a, F>(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
    mut callback: F,
) -> Result<SearchResult, Error>
where
    F: FnMut(u8, u8) -> RoomDecision<'a>,
{
    if goals.is_empty() {
        return Ok(SearchResult::empty(false));
    }
    let clamped = options.clamped();
    let c_options = ffi::Options::new(
        clamped.plain_cost,
        clamped.swamp_cost,
        clamped.max_ops,
        clamped.max_cost,
        clamped.max_rooms,
        clamped.flee,
        clamped.heuristic_weight,
    );
    let goal_triples: Vec<(u32, u32, u32)> = goals
        .iter()
        .map(|g| (g.pos.xx, g.pos.yy, g.range))
        .collect();
    let tramp = ffi::trampoline::<'a, F>();
    let user_data = (&mut callback) as *mut F as *mut std::ffi::c_void;
    let _terrain = terrain_read();
    // Safety: 闭包位于本栈帧（存活至调用结束）；其返回的 CostMatrix 引用只能
    // 溯源到闭包外层的所有者（FnMut Output 无法借用 &mut self），借用存活于整次
    // 调用；goals 切片调用期内有效；读锁防并发写。回调内 panic 会穿越 extern "C"
    // 边界（Rust ≥1.81 对 unwind 是 abort 而非 UB）；回调内 load_terrain 会死锁
    let outcome = unsafe {
        ffi::ffi_search(
            (origin.xx, origin.yy),
            &goal_triples,
            c_options,
            Some(tramp),
            user_data,
        )
    };
    finish(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 房名换算（对照 driver/lib/path-finder.js:6-27） =====

    #[test]
    fn room_name_parse() {
        // "E1N1" -> { xx: 129, yy: 126 }（pf.h:20 注释的官方例）
        assert_eq!(parse_room_name("E1N1"), Some((129, 126)));
        assert_eq!(parse_room_name("W10S3"), Some((117, 131)));
        assert_eq!(parse_room_name("W127N127"), Some((0, 0)));
        assert_eq!(parse_room_name("E127S127"), Some((255, 255)));
        // 前导零：JS Number("01") == 1
        assert_eq!(parse_room_name("E01N0"), parse_room_name("E1N0"));
        // 越界 / 形状错误
        assert_eq!(parse_room_name("E128N0"), None);
        assert_eq!(parse_room_name("E0"), None);
        assert_eq!(parse_room_name("e0n0"), None);
        assert_eq!(parse_room_name(""), None);
        assert_eq!(parse_room_name("E-1N0"), None);
        assert_eq!(parse_room_name("E0N0extra"), None);
    }

    #[test]
    fn room_name_generate() {
        assert_eq!(generate_room_name(129, 126), "E1N1");
        assert_eq!(generate_room_name(0, 0), "W127N127");
        assert_eq!(generate_room_name(255, 255), "E127S127");
        assert_eq!(generate_room_name(128, 127), "E0N0");
        assert_eq!(generate_room_name(128, 128), "E0S0");
        // 往返
        for &(xx, yy) in &[(0u8, 0u8), (64, 100), (128, 127), (200, 255)] {
            assert_eq!(parse_room_name(&generate_room_name(xx, yy)), Some((xx, yy)));
        }
    }

    #[test]
    fn world_position_conversion() {
        // toWorldPosition：xx = x + offset.xx*50（driver/lib/path-finder.js:34-44）
        let pos = WorldPosition::from_room("E1N1", 10, 20).unwrap();
        assert_eq!(
            pos,
            WorldPosition {
                xx: 10 + 129 * 50,
                yy: 20 + 126 * 50
            }
        );
        assert_eq!(pos.room_name(), "E1N1");
        assert_eq!((pos.x(), pos.y()), (10, 20));
        assert_eq!(
            WorldPosition::from_room("E1N1", 50, 0),
            Err(Error::InvalidPosition { x: 50, y: 0 })
        );
    }

    // ===== 选项钳制（对照 driver/lib/path-finder.js:90-96） =====

    #[test]
    fn options_clamping() {
        let d = SearchOptions::default().clamped();
        assert_eq!(
            (
                d.plain_cost,
                d.swamp_cost,
                d.max_ops,
                d.max_cost,
                d.max_rooms
            ),
            (1, 5, 2000, u32::MAX, 16)
        );
        assert_eq!(d.heuristic_weight, 1.2);
        assert!(!d.flee);
        let o = SearchOptions {
            plain_cost: 0,   // || 1
            swamp_cost: 999, // min(254, ...)
            heuristic_weight: 0.5,
            max_ops: 0, // || 2000
            max_cost: 3,
            max_rooms: 100, // min(64, ...)
            flee: true,
        }
        .clamped();
        assert_eq!(
            (o.plain_cost, o.swamp_cost, o.max_ops, o.max_rooms),
            (1, 254, 2000, 64)
        );
        assert_eq!(o.heuristic_weight, 1.0);
        assert!(o.flee);
        let o2 = SearchOptions {
            heuristic_weight: 100.0,
            ..SearchOptions::default()
        }
        .clamped();
        assert_eq!(o2.heuristic_weight, 9.0);
    }

    // ===== CostMatrix（对照 engine/src/game/path-finder.js:18-48） =====

    #[test]
    fn cost_matrix_set_get_clamp() {
        let mut m = CostMatrix::new();
        assert_eq!(m.get(0, 0), 0);
        m.set(2, 3, 0xAB);
        assert_eq!(m.get(2, 3), 0xAB);
        m.set(5, 5, 999); // Math.min(Math.max(0, val), 255)
        assert_eq!(m.get(5, 5), 255);
    }

    #[test]
    fn cost_matrix_serialize_bytes() {
        // 逐字节对齐：_bits[xx*50+yy]（path-finder.js:25），serialize = LE u32×625
        let mut m = CostMatrix::new();
        m.set(0, 0, 1); // 字节 0
        m.set(2, 3, 0xAB); // 字节 2*50+3 = 103 → 第 25 个 u32 的最高字节
        m.set(49, 49, 2); // 字节 49*50+49 = 2499 = 624*4+3 → 第 624 个 u32 的最高字节
        let s = m.serialize();
        assert_eq!(s.len(), 625);
        assert_eq!(s[0], 1);
        assert_eq!(s[25], 0xAB << 24);
        assert_eq!(s[624], 2 << 24);
        // 往返
        assert_eq!(CostMatrix::deserialize(&s).unwrap(), m);
        assert_eq!(
            CostMatrix::deserialize(&s[..100]),
            Err(Error::CostMatrixLength(100))
        );
    }

    // ===== 地形打包（对照 driver/lib/path-finder.js:20-27） =====

    #[test]
    fn terrain_packing() {
        let mut tiles = [[TERRAIN_PLAIN; 50]; 50];
        tiles[10][20] = TERRAIN_WALL; // x=20, y=10
        tiles[3][4] = TERRAIN_SWAMP; // x=4, y=3
        let bits = pack_terrain(&tiles);
        // ii = xx*50 + yy：(20,10) → ii=1010 → 字节 252 的第 4..5 位；
        // (4,3) → ii=203 → 字节 50 的第 6..7 位
        assert_eq!(bits[252] >> (1010 % 4 * 2) & 0x03, TERRAIN_WALL);
        assert_eq!(bits[50] >> (203 % 4 * 2) & 0x03, TERRAIN_SWAMP);
        // 其余全 0（plain）
        assert_eq!(bits.iter().filter(|&&b| b != 0).count(), 2);
    }
}
