//! 使用 Screeps 的 C++ 寻路算法，提供房间坐标、地形、代价矩阵和同步搜索接口。
//!
//! `native/pf.cc` 与 `native/pf.h` 保留上游源码；本项目维护的 Nan 兼容层和 C ABI
//! 负责构造输入、调用 Rust 回调及转换结果。复用算法保留其方向枚举和堆的平局
//! 规则，但不表示 Rust API 与 JavaScript 的所有输入转换、异常行为都相同。
//!
//! 地形在进程内全局共享，搜索持有读锁，载入地形持有写锁。回调可同步嵌套搜索，
//! 回调返回的矩阵须存活到搜索结束；指针生命周期与释放责任见 ffi.rs。
//!
//! 来源：Screeps driver cf63d8adf902663e2ebddd7f8c5b7baa425dc928 的
//! `native/src/pf.cc`、`native/src/pf.h`、`native/src/main.cc` 和 `lib/path-finder.js`；
//! CostMatrix 对照 Screeps engine a90d26fc9eb943147799e65d0bf9d4d0d79a2722 的
//! `src/game/path-finder.js`。下文的 driver/engine 路径相对这两个仓库。

mod ffi;

use std::cell::Cell;
use std::fmt;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

/// 平原的两位地形码；1 为墙，2 为沼泽，3 也按障碍处理。
/// 来源：native/pf.cc 的 look 与 search（只为 `look_table[0]`、`look_table[2]` 设置可通行代价）。
pub const TERRAIN_PLAIN: u8 = 0;
pub const TERRAIN_WALL: u8 = 1;
pub const TERRAIN_SWAMP: u8 = 2;

/// 保护 C++ 静态地形表，允许同线程递归持读锁。
/// 每线程的最外层搜索计为一个读者，内层搜索只增加 READ_DEPTH；写者等待全部
/// 读者退出。因此等待中的写者不会阻塞回调里的嵌套搜索。
///
/// 本锁只用于 TERRAIN_LOCK：线程本地深度不区分锁实例。
/// load_terrain 拒绝本线程持读锁时再申请写锁；调用方仍须避免在回调中等待
/// 另一个正在载入地形的线程，否则会形成跨线程等待环。
/// 新读者不让位于等待中的写者，持续搜索可能推迟地形更新。
struct TerrainLock {
    state: Mutex<TerrainLockState>,
    readers_done: Condvar,
}

struct TerrainLockState {
    readers: usize,
    writer: bool,
}

// 本线程尚未结束的搜索层数；只在从 0 到 1、从 1 到 0 时修改全局读者数。
thread_local! {
    static READ_DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl TerrainLock {
    fn read(&'static self) -> TerrainReadGuard<'static> {
        let depth = READ_DEPTH.with(Cell::get);
        if depth == 0 {
            let mut state = recover(&self.state);
            while state.writer {
                state = self
                    .readers_done
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
            }
            state.readers += 1;
        }
        READ_DEPTH.with(|d| d.set(depth + 1));
        TerrainReadGuard { lock: self }
    }

    // 调用前须确认 READ_DEPTH == 0；否则写锁会等待本线程自己的读锁。
    fn write(&'static self) -> TerrainWriteGuard<'static> {
        let mut state = recover(&self.state);
        while state.writer || state.readers > 0 {
            state = self
                .readers_done
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.writer = true;
        TerrainWriteGuard { lock: self }
    }
}

// 临界区只维护计数与标志，不运行用户回调；中毒时继续使用受保护的状态。
fn recover<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct TerrainReadGuard<'a> {
    lock: &'a TerrainLock,
}

impl Drop for TerrainReadGuard<'_> {
    fn drop(&mut self) {
        let depth = READ_DEPTH.with(Cell::get) - 1;
        READ_DEPTH.with(|d| d.set(depth));
        if depth == 0 {
            let mut state = recover(&self.lock.state);
            state.readers -= 1;
            if state.readers == 0 {
                self.lock.readers_done.notify_all();
            }
        }
    }
}

struct TerrainWriteGuard<'a> {
    lock: &'a TerrainLock,
}

impl Drop for TerrainWriteGuard<'_> {
    fn drop(&mut self) {
        let mut state = recover(&self.lock.state);
        state.writer = false;
        self.lock.readers_done.notify_all();
    }
}

static TERRAIN_LOCK: TerrainLock = TerrainLock {
    state: Mutex::new(TerrainLockState {
        readers: 0,
        writer: false,
    }),
    readers_done: Condvar::new(),
};

/// 错误类型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// C++ 搜索报告的错误。缺少地形时携带消息；Rust 回调 panic 时为 None。
    /// 调用方负责将错误转换成自己的异常或处理结果，本 crate 不向 JS 自动抛错。
    JSError(Option<String>),
    /// 名字格式不符，或房间超出 W127N127 到 E127S127 的坐标范围。
    InvalidRoomName(String),
    /// 房内 x 或 y 不在 0..50 内。
    InvalidPosition { x: u32, y: u32 },
    /// 当前线程的搜索尚未结束时尝试载入地形；拒绝从读锁升级到写锁，避免死锁。
    ReentrantTerrainLoad,
    /// C ABI 返回失败，包括 C++ 异常、内存分配失败或结果结构不符合约定。
    NativeFailure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidRoomName(name) => write!(f, "invalid room name: {name}"),
            Error::InvalidPosition { x, y } => write!(f, "invalid room position: ({x},{y})"),
            Error::ReentrantTerrainLoad => write!(
                f,
                "load_terrain is not allowed inside a search callback (deadlock guard)"
            ),
            Error::NativeFailure => write!(f, "native pathfinder failure"),
            Error::JSError(Some(msg)) => write!(f, "native pathfinder threw: {msg}"),
            Error::JSError(None) => write!(f, "native pathfinder callback threw"),
        }
    }
}

impl std::error::Error for Error {}

/// 将大写房间名转换成 0..=255 的地图房间坐标，前导零可接受。
/// W0/N0 对应 127，E0/S0 对应 128；格式不符或任一方向数字大于 127 返回 None。
/// 此坐标与 kk-core 的有符号房间坐标不同，不能直接混用。
/// 来源：driver/lib/path-finder.js 的 parseRoomName。
pub fn parse_room_name(room_name: &str) -> Option<(u8, u8)> {
    let bytes = room_name.as_bytes();
    let parse_axis = |digits: &[u8]| -> Option<u32> {
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
        return None; // 保证下面的 1..ns_pos 切片顺序有效。
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

/// 将地图房间坐标转换成规范大写名字，不保留输入名字的前导零。
/// 来源：driver/lib/path-finder.js 的 generateRoomName。
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

/// 连续世界格子坐标，每轴有效范围为 0..12800。
/// 房内坐标加地图房间坐标乘 50 得到此值；字段公开，直接构造不执行范围检查。
/// 通常使用 [`Self::from_room`] 构造。来源：driver/lib/path-finder.js 的 toWorldPosition。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldPosition {
    pub xx: u32,
    pub yy: u32,
}

impl WorldPosition {
    /// 从房间名和房内坐标构造世界格子坐标。
    ///
    /// # Errors
    /// x/y 越界返回 InvalidPosition；名字或地图范围不合法返回 InvalidRoomName。
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

    /// 返回所在房间名；直接构造的越界坐标不会在此校验，房间编号会截为 u8。
    pub fn room_name(&self) -> String {
        generate_room_name((self.xx / 50) as u8, (self.yy / 50) as u8)
    }

    pub fn x(&self) -> u32 {
        self.xx % 50
    }

    pub fn y(&self) -> u32 {
        self.yy % 50
    }
}

/// 目标位置与允许的切比雪夫距离（横纵差的较大值）。
/// 普通搜索到任一目标的距离 <= range 即完成；flee 搜索须对每个目标都达到
/// 距离 >= range。来源：native/pf.cc 的 heuristic。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Goal {
    pub pos: WorldPosition,
    pub range: u32,
}

impl Goal {
    /// 构造必须到达指定格子的目标（range 为 0）。
    pub fn pos(pos: WorldPosition) -> Self {
        Goal { pos, range: 0 }
    }

    pub fn with_range(pos: WorldPosition, range: u32) -> Self {
        Goal { pos, range }
    }
}

/// 搜索预算和移动代价，调用搜索时按范围限制，无须预先调整字段。
/// 默认值见 Default 实现。道路不是单独的地形码，其代价通过 CostMatrix 提供。
/// 整数使用 Rust 的 u32 输入，不执行 JS 的有符号 int32 强制转换。
/// 来源：driver/lib/path-finder.js 的 search 选项处理。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchOptions {
    /// 平原移动代价，限制为 1..=254；0 使用默认值 1。
    pub plain_cost: u32,
    /// 沼泽移动代价，限制为 1..=254；0 使用默认值 5。
    pub swamp_cost: u32,
    /// 启发式权重，限制为 1..=9；0/NaN 使用 1.2，高于 1 时不保证最短路径。
    pub heuristic_weight: f64,
    /// 主搜索循环的展开次数预算；0 使用 2000，不限制回调的运行时间。
    pub max_ops: u32,
    /// 累计移动代价的搜索阈值；0 使用 u32::MAX，不作为返回 cost 的硬上限。
    pub max_cost: u32,
    /// 可纳入搜索的房间数，限制为 1..=64；0 使用 16。
    pub max_rooms: u32,
    /// 从所有目标的 range 范围中撤离，而非接近任一目标。
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
    // 先应用零值/NaN 缺省，再限制范围；不能先将零值 clamp 成 1。
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

/// 搜索首次触达一个可载入房间时，回调为该房间选择地形或代价矩阵。
/// 来源：native/pf.cc 的 room_index_from_pos。
pub enum RoomDecision<'a> {
    /// 使用已载入地形的代价。
    None,
    /// 不允许搜索进入该房间。
    Blocked,
    /// 借用矩阵直到搜索返回，C++ 不复制字节。
    /// 非 2500 字节的矩阵按 None 处理，由 native/pf.cc 的长度检查决定。
    CostMatrix(&'a CostMatrix),
}

/// 搜索结果；未完成时仍可能包含一段向目标接近的路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// 从起点向结果终点排列的步进位置，不包含起点，终点不一定是目标格。
    pub path: Vec<WorldPosition>,
    /// 主搜索循环的展开次数，不计首次 A* 展开或回调耗时。
    pub ops: u32,
    /// C++ 返回的累计移动代价，不是路径步数或 CPU 时间。
    pub cost: u32,
    /// 是否尚未达到目标条件；不表示发生错误。
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

/// 按列排列的格子代价，索引为 x * 50 + y，与 kk-core 地形的行顺序不同。
/// 0 使用地形代价；1..=254 覆盖地形代价（包括墙格）；255 为不可通行。
/// 来源：engine/src/game/path-finder.js 的 CostMatrix，native/pf.cc 的 look。
///
/// 为保留上游下标行为，set/get 只检查平铺索引，不逐轴检查 0..50。
/// 因而 (0, 50) 与 (1, 0) 访问同一字节；索引溢出或越出实际缓冲才算越界。
/// deserialize 可生成非标准长度，只有 2500 字节的矩阵会在搜索中生效。
/// new 与 deserialize 都保证长度为 4 的倍数，serialize 可完整按小端 u32 分组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostMatrix(Vec<u8>);

impl Default for CostMatrix {
    fn default() -> Self {
        Self::new()
    }
}

impl CostMatrix {
    pub fn new() -> Self {
        CostMatrix(vec![0u8; 2500])
    }

    /// 写入代价，上限限制为 255；平铺索引越界或计算溢出时不修改矩阵。
    /// Rust 输入不表示 JS 的负数、NaN 或小数，相关转换须在 JS 接口层完成。
    pub fn set(&mut self, x: usize, y: usize, val: u32) {
        let Some(idx) = x.checked_mul(50).and_then(|v| v.checked_add(y)) else {
            return;
        };
        if idx >= self.0.len() {
            return;
        }
        self.0[idx] = val.min(255) as u8;
    }

    /// 按平铺索引读取代价；越界或计算溢出返回 None，边界规则见类型文档。
    pub fn get(&self, x: usize, y: usize) -> Option<u32> {
        let idx = x.checked_mul(50).and_then(|v| v.checked_add(y))?;
        self.0.get(idx).map(|&b| u32::from(b))
    }

    /// 每四字节按小端编码为一个 u32；标准矩阵得到 625 个值。
    /// 这对应上游 CostMatrix.serialize 在小端平台的输出，不改变实际长度。
    pub fn serialize(&self) -> Vec<u32> {
        self.0
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[b0, b1, b2, b3]| u32::from_le_bytes([b0, b1, b2, b3]))
            .collect()
    }

    /// 按小端展开每个 u32，接受空数组或非 625 个元素的数组。
    /// 非标准长度矩阵仍可读取和再序列化，但搜索会忽略它。
    pub fn deserialize(data: &[u32]) -> Self {
        let mut bits = Vec::with_capacity(data.len() * 4);
        for word in data {
            bits.extend_from_slice(&word.to_le_bytes());
        }
        CostMatrix(bits)
    }

    fn raw(&self) -> &[u8] {
        &self.0
    }
}

/// 将行顺序的 `tiles[y][x]` 转为寻路器按列存储的 625 字节地形。
/// 每格占两位，列顺序索引 i = x * 50 + y，存入 bits[i/4] 的第 (i%4)*2 位。
/// 输入应使用 0..=3 的地形码，本实现对更大值只保留低两位。
/// 来源：driver/lib/path-finder.js 的 init；低两位截取是本封装的输入处理。
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

/// 将一个房间的地形复制到进程共享的 C++ 地形表，影响之后的所有搜索。
/// 同名房间重复载入会覆盖表项；上游分配的旧缓冲不会释放，宜在搜索前统一加载。
/// 来源：native/pf.cc 的 load_terrain。写锁等待正在执行的搜索结束。
///
/// # Errors
/// 名字不合法、当前线程尚在搜索中，或 C ABI 失败时返回错误。
/// 同线程回调内载入返回 ReentrantTerrainLoad。回调若等待另一线程载入完成，
/// 仍会相互等待；应在搜索外协调更新。
pub fn load_terrain(room_name: &str, bits: &[u8; 625]) -> Result<(), Error> {
    let (xx, yy) =
        parse_room_name(room_name).ok_or_else(|| Error::InvalidRoomName(room_name.to_owned()))?;
    if READ_DEPTH.with(Cell::get) > 0 {
        return Err(Error::ReentrantTerrainLoad);
    }
    let _guard = TERRAIN_LOCK.write();
    // SAFETY: 写锁排除其他搜索或载入；固定长度 bits 在同步调用期间存活。
    if !unsafe { ffi::ffi_load_terrain(xx, yy, bits) } {
        return Err(Error::NativeFailure);
    }
    Ok(())
}

// 状态码来自 native/kk_ffi.h；异常与正常的空结果必须分开处理。
fn finish(outcome: Option<ffi::RawOutcome>) -> Result<SearchResult, Error> {
    match outcome {
        None => Err(Error::NativeFailure),
        Some(raw) if raw.status == 3 => Err(Error::JSError(raw.js_error_message)),
        Some(raw) if raw.status == 2 => Ok(SearchResult::empty(false)),
        Some(raw) if raw.status == 1 => Ok(SearchResult::empty(true)),
        Some(raw) => {
            // C++ 从终点反向重建，公开结果按行走顺序排列。
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

/// 使用已载入的地形搜索，选项在进入 C++ 前按 SearchOptions 的规则调整。
/// 空目标列表直接返回完整的空 SearchResult，不读取地形。
/// 正常搜索须预先载入可能触达的房间；位置宜用 WorldPosition::from_room 构造。
///
/// # Errors
/// 触达未载入地形的房间时返回 JSError；C ABI 失败时返回 NativeFailure。
pub fn search(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
) -> Result<SearchResult, Error> {
    // C++ 没有空目标早退；此处避免无目标时消耗整个搜索预算。
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
    let _terrain = TERRAIN_LOCK.read();
    // SAFETY: 输入在同步调用期间有效；读锁防止地形被替换；没有回调指针。
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

/// 搜索首次触达一个已加载地形的房间时同步调用回调，每房间至多一次。
/// 回调参数是 0..=255 的地图房间坐标，可用 generate_room_name 转成名字。
/// C++ 借用返回的矩阵直到搜索结束，生命周期参数要求矩阵在此期间有效。
///
/// 回调可嵌套搜索：每线程缓存两个 C++ 实例，更多层临时分配实例，返回时释放。
/// 回调可处理内层搜索错误后继续，也可 panic 使外层返回错误。
/// 本 crate 不限制回调运行时间；max_ops 只限制搜索循环。
///
/// # Errors
/// 缺少地形或 C ABI 失败时与 search 相同。采用 panic=unwind 且普通 panic
/// 载荷析构不再次 panic 时，回调 panic 被捕获并返回 JSError(None)，不保留载荷。
/// panic=abort 或 panic hook 主动终止进程不在此捕获范围内。
/// 回调内 load_terrain 返回 ReentrantTerrainLoad，回调可自行处理该错误。
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
    let _terrain = TERRAIN_LOCK.read();
    // SAFETY: user_data 指向本栈帧的 callback，同步调用结束前不会移动或释放。
    // FnMut 的输出不能借用本次 &mut self，返回的矩阵借用须来自可覆盖整次搜索的
    // 外部存储；goals 同样存活至返回。读锁防止并发替换地形。
    // trampoline 在 C ABI 内捕获可展开的回调 panic；约束见 ffi.rs。
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

    // 零值先应用缺省，再限制范围，避免 max_ops=0 被误改成 1。
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

    #[test]
    fn cost_matrix_set_get_clamp() {
        let mut m = CostMatrix::new();
        assert_eq!(m.get(0, 0), Some(0));
        m.set(2, 3, 0xAB);
        assert_eq!(m.get(2, 3), Some(0xAB));
        m.set(5, 5, 999); // Math.min(Math.max(0, val), 255)
        assert_eq!(m.get(5, 5), Some(255));
    }

    // 只检查平铺索引：(0,50) 与 (1,0) 是同一字节，不能改成逐轴拒绝。
    #[test]
    fn cost_matrix_out_of_range_silent() {
        let mut m = CostMatrix::new();
        m.set(2, 3, 0xAB);
        m.set(50, 0, 1); // 平铺 2500 ≥ 2500 → 静默
        m.set(49, 50, 1); // 平铺 49*50+50 = 2500 → 静默
        m.set(usize::MAX, usize::MAX, 1); // 溢出 → 官方按越界静默
        m.set(49, 49, 7); // 平铺 2499 = 最大合法索引
        assert_eq!(m.get(50, 0), None);
        assert_eq!(m.get(49, 50), None);
        assert_eq!(m.get(usize::MAX, usize::MAX), None);
        assert_eq!(m.get(49, 49), Some(7));
        assert_eq!(m.get(2, 3), Some(0xAB), "越界 set 不得影响其他格子");
        // (0,50) 平铺 50 < 2500：官方照写（与 (1,0) 同字节——列主序别名）
        m.set(0, 50, 1);
        assert_eq!(m.get(0, 50), Some(1));
        m.set(1, 0, 9);
        assert_eq!(
            m.get(0, 50),
            Some(9),
            "(0,50) 与 (1,0) 必须与官方同字节别名"
        );
    }

    // 选取不同 word 的最高字节，检查列顺序索引与小端打包，而不只检查往返。
    #[test]
    fn cost_matrix_serialize_bytes() {
        // 逐字节对齐：_bits[xx*50+yy]（path-finder.js:25），serialize = LE u32
        let mut m = CostMatrix::new();
        m.set(0, 0, 1); // 字节 0
        m.set(2, 3, 0xAB); // 字节 2*50+3 = 103 → 第 25 个 u32 的最高字节
        m.set(49, 49, 2); // 字节 49*50+49 = 2499 = 624*4+3 → 第 624 个 u32 的最高字节
        let s = m.serialize();
        assert_eq!(s.len(), 625);
        assert_eq!(s[0], 1);
        assert_eq!(s[25], 0xAB << 24);
        assert_eq!(s[624], 2 << 24);
        // 往返（deserialize 任意长度均成功——官方无长度校验，path-finder.js:44-48）
        assert_eq!(CostMatrix::deserialize(&s), m);
    }

    // 非标准长度也保留实际数据；是否用于搜索由 C++ 的长度检查决定。
    #[test]
    fn cost_matrix_deserialize_any_length_silent() {
        // 空数据 → 0 字节矩阵：get 恒 None、serialize 为空（官方
        // `new Uint8Array(new Uint32Array([]).buffer)` 同为空）
        let empty = CostMatrix::deserialize(&[]);
        assert_eq!(empty.get(0, 0), None);
        assert!(empty.serialize().is_empty());

        // 短数据（624 词 = 2496 字节）：实际字节可读（(12,25) → 字节 625 在界内），
        // 界外 None；serialize 往返保持原长度
        let mut words = vec![0u32; 624];
        let m1 = CostMatrix::deserialize(&words);
        assert_eq!(m1.get(12, 25), Some(0));
        assert_eq!(m1.get(49, 49), None); // 字节 2499 越出 2496
        words[156] = 3 << 8; // 字节 625 = (12,25) 置 3（LE 低字节在 word 156 的 byte 1）
        let m2 = CostMatrix::deserialize(&words);
        assert_eq!(m2.get(12, 25), Some(3));
        assert_eq!(m2.serialize(), words);

        // 长数据（626 词）：静默接受，界内可读。注意字节序：word=1 的 LE 字节是
        // [01,00,00,00]——平铺 2499（= word 624 的 byte 3）读到 0，平铺 0 读到 1
        let long = CostMatrix::deserialize(&vec![1u32; 626]);
        assert_eq!(long.get(0, 0), Some(1));
        assert_eq!(long.get(49, 49), Some(0));
    }

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
