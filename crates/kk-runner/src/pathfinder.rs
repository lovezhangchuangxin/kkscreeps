//! 玩家面 PathFinder.search 的原生搜索接线：地形打包、进程级地形表与搜索 op。
//!
//! 玩家沙箱内的 path-finder.js 按 driver/lib/path-finder.js 逐字清洗参数后，
//! 经 `op_kk_pathfinder_search` 把纯数值送进 kk-pathfinder 的同步搜索，再把
//! 世界坐标路径映射回 RoomPosition。地形是世界公共数据（不可见房也投影
//! 名称与地形），两条注入通道在 begin_tick_plan 汇入本模块的 TickTerrain：
//!
//! - JSON 通道：`world.rooms[room].terrain` 为 `{rev, payload}`（行序 2500
//!   字节 base64，kk-world terrain_json）或宿主手搓夹具的裸数组形态。
//! - KKPV 通道：公开地形束（`SharedKkpvBundle`），首次搜索时懒解码。
//!
//! 官方 driver 在启动时载入全部分片房地形（driver/lib/path-finder.js 的
//! init），搜索触达任何合法房都有地形；本仓世界只含注册房，未注册房以
//! 全墙环（注册房 8 邻域减注册房）补位，原生对无地形房抛
//! `Could not load terrain data` 的路径只可能发生在墙环之外。
//! 边界语义与分歧登记见 docs/designs/pathfinder-terrain.md。

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deno_core::{OpState, op2, v8};
use deno_error::JsErrorBox;
use serde::{Deserialize, Serialize};

/// 全墙房间的 625 字节地形：每格两位均为 0b01（wall）。
const ALL_WALL: [u8; 625] = [0b0101_0101; 625];

/// 一 tick 内可用的寻路地形：房名 → kk-pathfinder 列序 625 字节。
///
/// 由行序 2500 字节（`tiles[y][x]`，kk-core 位掩码）经
/// `kk_pathfinder::pack_terrain` 转换；位值取低两位（wall|swamp=3 按障碍，
/// lava=4 按平原），契约在 kk-pathfinder 的 `pack_terrain` 文档。
#[derive(Debug, PartialEq, Eq)]
pub struct TerrainPack {
    pub rooms: BTreeMap<String, [u8; 625]>,
}

impl TerrainPack {
    /// 从行序 2500 字节逐房打包；长度不符的房间跳过（该房搜索将以原生
    /// 缺地形错误显形，不静默产出错位地形）。
    pub fn from_room_rows<I: IntoIterator<Item = (String, Vec<u8>)>>(rows: I) -> Self {
        let mut rooms = BTreeMap::new();
        for (name, row) in rows {
            let Some(bits) = pack_row_terrain(&row) else {
                continue;
            };
            rooms.insert(name, bits);
        }
        TerrainPack { rooms }
    }

    /// JSON 通道世界投影提取：`terrain` 为 `{payload}` base64（生产路径，
    /// kk-world terrain_json）或 2500 元素裸数组（宿主手搓夹具，game.js
    /// beginTick 同样双形态接收）。无 rooms 键或单房形态不符时跳过该房。
    pub fn from_world_json(world: &serde_json::Value) -> Self {
        let mut rows = Vec::new();
        if let Some(rooms) = world.get("rooms").and_then(|v| v.as_object()) {
            for (name, entry) in rooms {
                let Some(terrain) = entry.get("terrain") else {
                    continue;
                };
                if let Some(payload) = terrain.get("payload").and_then(|v| v.as_str()) {
                    if let Some(row) = base64_decode(payload) {
                        rows.push((name.clone(), row));
                    }
                } else if let Some(array) = terrain.as_array() {
                    let mut row = Vec::with_capacity(array.len());
                    for cell in array {
                        // 位掩码值 0..=7 只取低两位，与 pack_terrain 输入契约一致。
                        match cell.as_u64() {
                            Some(v) => row.push((v & 0x03) as u8),
                            None => {
                                row.clear();
                                break;
                            }
                        }
                    }
                    if !row.is_empty() {
                        rows.push((name.clone(), row));
                    }
                }
            }
        }
        TerrainPack::from_room_rows(rows)
    }

    /// KKPV 公开地形束解析：束框架（u32 块数 + 逐块 u32 长度 + 顺序拼接，
    /// kkpv.js forEachBlock 同构）逐块走 `KkpvView` 校验解码，取公开块的
    /// 房名与 2500 字节地形段。束是宿主产出的可信数据，解析失败按通道
    /// 装配错误整体报错（不产出半包地形）。
    pub fn from_public_bundle(bytes: &[u8]) -> Result<Self, String> {
        let read_u32 = |at: usize| -> Option<u32> {
            let end = at.checked_add(4)?;
            if end > bytes.len() {
                return None;
            }
            Some(u32::from_le_bytes(bytes[at..end].try_into().unwrap()))
        };
        let count = read_u32(0).ok_or("束短于框架头部")? as usize;
        let mut cursor = 4 + count.checked_mul(4).ok_or("块数溢出")?;
        let mut lens = Vec::with_capacity(count);
        for i in 0..count {
            lens.push(read_u32(4 + i * 4).ok_or("束短于长度目录")? as usize);
        }
        let limits = kk_schema::kkpv::KkpvLimits::default();
        let mut seen_rooms = std::collections::BTreeSet::new();
        let mut rows = Vec::with_capacity(count);
        for len in lens {
            let end = cursor.checked_add(len).ok_or("块长度溢出")?;
            if end > bytes.len() {
                return Err("块越界".into());
            }
            let block = &bytes[cursor..end];
            cursor = end;
            let view = kk_schema::kkpv::KkpvView::decode(block, &limits)
                .map_err(|e| format!("公开束块不合法：{e}"))?;
            if view.is_private() {
                return Err("公开束出现私有块".into());
            }
            // 公开块契约与沙箱解码器一致：不带对象记录、房名唯一
            //（prelude/30_game/kkpv.js 的 decodePublicBundle 同校验）。
            if view.object_count() != 0 {
                return Err("公开地形块不带对象".into());
            }
            let room = view.room_name().to_owned();
            if seen_rooms.contains(&room) {
                return Err(format!("公开束房间名重复：{room}"));
            }
            seen_rooms.insert(room.clone());
            let terrain = view.terrain().ok_or("公开块缺地形段")?;
            if terrain.len() != 2500 {
                return Err("公开块地形段须为 2500 字节".into());
            }
            rows.push((room, terrain.to_vec()));
        }
        if cursor != bytes.len() {
            return Err("束长度与块目录不符".into());
        }
        Ok(TerrainPack::from_room_rows(rows))
    }
}

/// 行序 2500 字节 → kk-pathfinder 列序 625 字节。转换复用
/// `pack_terrain`（`i = x*50 + y`），不另写位运算，方向错误无处可藏。
fn pack_row_terrain(row: &[u8]) -> Option<[u8; 625]> {
    if row.len() != 2500 {
        return None;
    }
    let mut tiles = [[0u8; 50]; 50];
    for (y, tile_row) in tiles.iter_mut().enumerate() {
        tile_row.copy_from_slice(&row[y * 50..(y + 1) * 50]);
    }
    Some(kk_pathfinder::pack_terrain(&tiles))
}

/// 标准 base64 解码（字母表与 kk-world `base64_encode` 一致，含 '=' 填充）。
/// 输入含非法字符或长度不符返回 None，由调用方按缺地形处置。
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        // 填充只出现在末组：组内尾部 '=' 计数即本组填充数；中间 '='
        // 会落入 value() 的非法字符分支而整体失败。
        let pad = chunk.iter().rev().take_while(|&&b| b == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n: u32 = 0;
        for (i, &b) in chunk.iter().enumerate() {
            let v = if i >= 4 - pad { 0 } else { value(b)? };
            n = (n << 6) | v;
        }
        // 数据字符数 = 4 - pad：2 字符出 1 字节、3 字符出 2 字节、4 字符出 3 字节。
        out.push((n >> 16) as u8);
        if pad <= 1 {
            out.push((n >> 8) as u8);
        }
        if pad == 0 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// 进程级已载入地形表：房名 → 最后一次 load_terrain 的字节。
///
/// C++ 地形表进程全局且 `load_terrain` 覆盖同名房；这里按**内容比对**判断
/// 是否需要重载——set_terrain 换地形、createShard 换世界后，差异房与
/// 期望全墙的墙环房都会被重新载入，陈旧表项不残留。
static LOADED: Mutex<BTreeMap<String, [u8; 625]>> = Mutex::new(BTreeMap::new());

/// 把 pack 的真实地形与墙环地形同步进 C++ 表。
///
/// 比对与载入分两段持锁：比对段取出差异后立即放锁，再逐房
/// `load_terrain`（写锁会等待其他线程在途搜索），最后回填。并发下两个
/// 线程可能重复载入同一房（同世界字节相同，仅多一次 C++ 缓冲分配，有界
/// 无害；同进程多世界的并发载入顺序可倒置，属设计文档登记的串地形边界）；
/// 换来的约束是未来回调内嵌套搜索不会因本锁与地形写锁互等成环。
fn ensure_loaded(pack: &TerrainPack) -> Result<(), kk_pathfinder::Error> {
    let mut pending: Vec<(String, [u8; 625])> = Vec::new();
    {
        let loaded = LOADED.lock().unwrap();
        for (room, bits) in &pack.rooms {
            if loaded.get(room) != Some(bits) {
                pending.push((room.clone(), *bits));
            }
        }
        for room in ring_rooms(&pack.rooms) {
            if loaded.get(&room) != Some(&ALL_WALL) {
                pending.push((room, ALL_WALL));
            }
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    for (room, bits) in &pending {
        kk_pathfinder::load_terrain(room, bits)?;
    }
    let mut loaded = LOADED.lock().unwrap();
    for (room, bits) in pending {
        loaded.insert(room, bits);
    }
    Ok(())
}

/// 注册房 8 邻域中不在世界内的房间（全墙补位）。房名解析/生成复用
/// kk-pathfinder 的官方坐标语义；世界内出现不可解析房名时该房不参与
/// 环计算（搜索触达由原生缺地形错误显形）。
fn ring_rooms(rooms: &BTreeMap<String, [u8; 625]>) -> Vec<String> {
    let owned: BTreeSet<(u8, u8)> = rooms
        .keys()
        .filter_map(|name| kk_pathfinder::parse_room_name(name))
        .collect();
    let mut ring = BTreeSet::new();
    for &(xx, yy) in &owned {
        for dx in -1i16..=1 {
            for dy in -1i16..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = i16::from(xx) + dx;
                let ny = i16::from(yy) + dy;
                if !(0..=255).contains(&nx) || !(0..=255).contains(&ny) {
                    continue;
                }
                let name = kk_pathfinder::generate_room_name(nx as u8, ny as u8);
                if !owned.contains(&(nx as u8, ny as u8)) {
                    ring.insert(name);
                }
            }
        }
    }
    ring.into_iter().collect()
}

/// 本 tick 的寻路地形来源，begin_tick_plan 注入 OpState，每 tick 整体替换。
pub struct TickTerrain {
    source: TickTerrainSource,
    /// 懒物化缓存：KKPV 首次搜索解码一次；JSON 通道注入前已物化。
    pack: Mutex<Option<Arc<TerrainPack>>>,
}

enum TickTerrainSource {
    Json(Arc<TerrainPack>),
    Kkpv(crate::shared_bundle::SharedKkpvBundle),
}

impl TickTerrain {
    pub fn json(pack: Arc<TerrainPack>) -> Self {
        TickTerrain {
            source: TickTerrainSource::Json(pack),
            pack: Mutex::new(None),
        }
    }

    pub fn kkpv(bundle: crate::shared_bundle::SharedKkpvBundle) -> Self {
        TickTerrain {
            source: TickTerrainSource::Kkpv(bundle),
            pack: Mutex::new(None),
        }
    }

    /// 当前 tick 的地形包；来源不可用（理论上 begin_tick 必注入）返回 None。
    pub fn pack(&self) -> Option<Arc<TerrainPack>> {
        if let Some(cached) = self.pack.lock().unwrap().clone() {
            return Some(cached);
        }
        let materialized = match &self.source {
            TickTerrainSource::Json(pack) => Some(pack.clone()),
            TickTerrainSource::Kkpv(bundle) => TerrainPack::from_public_bundle(bundle.bytes())
                .ok()
                .map(Arc::new),
        };
        if let Some(pack) = &materialized {
            *self.pack.lock().unwrap() = Some(pack.clone());
        }
        materialized
    }
}

/// path-finder.js 清洗后的搜索参数：坐标为世界格坐标（房内坐标 + 房间
/// 偏移 ×50），数值均已按 driver/lib/path-finder.js:86-96 的 JS 语义钳制。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct PathfinderArgs {
    origin: (u32, u32),
    goals: Vec<(u32, u32, u32)>,
    plain_cost: u32,
    swamp_cost: u32,
    heuristic_weight: f64,
    max_ops: u32,
    max_cost: u32,
    max_rooms: u32,
    flee: bool,
}

/// 原生结果的原样回传：路径为世界坐标行走序（不含起点），JS 侧映射回
/// RoomPosition；两种空结果以 incomplete 区分（driver 对原生
/// undefined/-1 的两个四键形状）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
struct PathfinderOutcome {
    path: Vec<(u32, u32)>,
    ops: u32,
    cost: u32,
    incomplete: bool,
}

/// 原生错误 → JS 异常文案。C++ Nan::ThrowError 的消息（如缺地形房的
/// `Could not load terrain data`）在官方 driver 原样传播给玩家，因此
/// JSError 携带消息时不再加前缀；其余错误用 Display 描述。
fn native_error_message(error: &kk_pathfinder::Error) -> String {
    match error {
        kk_pathfinder::Error::JSError(Some(message)) => message.clone(),
        other => other.to_string(),
    }
}

/// 同步原生搜索。地形缺失或原生错误以 op 错误抛回 JS，由玩家面按官方
/// 异常传播形态处理（如缺地形房的 `Could not load terrain data`）。
// 回调可执行任意玩家 JS（包括同 op 的嵌套搜索）；不得跨回调借用 OpState。
#[op2(reentrant)]
#[serde]
pub(crate) fn op_kk_pathfinder_search(
    scope: &mut v8::PinScope,
    state: Rc<RefCell<OpState>>,
    #[serde] args: PathfinderArgs,
    callback: v8::Local<v8::Value>,
) -> Result<PathfinderOutcome, JsErrorBox> {
    // try_borrow：begin_tick 前的调用（如 Mod 玩家模块安装期顶层代码）缺
    // TickTerrain 时给出 op 错误而不是 OpState 缺类型 panic。
    let pack = state
        .borrow()
        .try_borrow::<TickTerrain>()
        .and_then(|terrain| terrain.pack());
    // 不跨 OpState 借用执行载入与搜索；后续嵌套搜索（回调通道）不受影响。
    let Some(pack) = pack else {
        return Err(JsErrorBox::generic("寻路地形尚未注入（begin_tick 前）"));
    };
    ensure_loaded(&pack).map_err(|e| JsErrorBox::generic(e.to_string()))?;
    let goals: Vec<kk_pathfinder::Goal> = args
        .goals
        .iter()
        .map(|&(xx, yy, range)| {
            kk_pathfinder::Goal::with_range(kk_pathfinder::WorldPosition { xx, yy }, range)
        })
        .collect();
    let options = kk_pathfinder::SearchOptions {
        plain_cost: args.plain_cost,
        swamp_cost: args.swamp_cost,
        heuristic_weight: args.heuristic_weight,
        max_ops: args.max_ops,
        max_cost: args.max_cost,
        max_rooms: args.max_rooms,
        flee: args.flee,
    };
    let callback = callback
        .try_cast::<v8::Function>()
        .ok()
        .map(|function| v8::Global::new(scope, function));
    let mut host = PlayerSearchHost {
        scope,
        callback,
        matrices: Vec::new(),
        exception: None,
    };
    let result = kk_pathfinder::search_with_host(
        kk_pathfinder::WorldPosition {
            xx: args.origin.0,
            yy: args.origin.1,
        },
        &goals,
        &options,
        &mut host,
    );
    // 重新抛出原值（包括非 Error），不能格式化或用 op 错误替换异常身份。
    if let Some(exception) = host.exception.take() {
        let exception = v8::Local::new(host.scope, exception);
        host.scope.throw_exception(exception);
        return Ok(PathfinderOutcome {
            path: Vec::new(),
            ops: 0,
            cost: 0,
            incomplete: true,
        });
    }
    if host.scope.is_execution_terminating() {
        // 保留不可捕获的 V8 终止；不能用普通错误替换后让玩家继续执行。
        return Ok(PathfinderOutcome {
            path: Vec::new(),
            ops: 0,
            cost: 0,
            incomplete: true,
        });
    }
    let result = result.map_err(|e| JsErrorBox::generic(native_error_message(&e)))?;
    Ok(PathfinderOutcome {
        path: result.path.iter().map(|p| (p.xx, p.yy)).collect(),
        ops: result.ops,
        cost: result.cost,
        incomplete: result.incomplete,
    })
}

/// 保留返回时的 backing store 与偏移；detach 不会释放已被搜索持有的存储。
/// C++ 只读固定 Cell 缓冲，JS 回调期间同步内容，不借用可变 V8 字节为 &[u8]。
struct MatrixSnapshot {
    backing: v8::SharedRef<v8::BackingStore>,
    offset: usize,
    bytes: Box<[Cell<u8>; 2500]>,
}

impl MatrixSnapshot {
    fn refresh(&self) {
        let source = self.backing.get(self.offset..).unwrap_or_default();
        let copied = source.len().min(self.bytes.len());
        for (target, source) in self.bytes[..copied].iter().zip(&source[..copied]) {
            target.set(source.get());
        }
        // 可缩放 ArrayBuffer 可能在后续回调中收缩到原视图之外。不可读部分
        // 按地形默认值清零，既不越界读取，也不让旧障碍残留在固定副本中。
        for target in &self.bytes[copied..] {
            target.set(0);
        }
    }
}

struct PlayerSearchHost<'a, 's, 'i> {
    scope: &'a mut v8::PinScope<'s, 'i>,
    callback: Option<v8::Global<v8::Function>>,
    matrices: Vec<MatrixSnapshot>,
    exception: Option<v8::Global<v8::Value>>,
}

// SAFETY: 矩阵各自拥有固定 Box<Cell>，Vec 增长不移动 Box 的存储。所有写入
// 仅在 room 回调内（C++ 暂停）发生，不持不可变字节借用；同步搜索结束后才
// 释放。嵌套 op 建立独立宿主，外层矩阵在内层返回后重新同步。
unsafe impl kk_pathfinder::SearchHost for PlayerSearchHost<'_, '_, '_> {
    fn room(&mut self, xx: u8, yy: u8) -> kk_pathfinder::HostRoomDecision {
        use kk_pathfinder::HostRoomDecision as Decision;
        let Some(callback) = &self.callback else {
            return Decision::None;
        };
        v8::tc_scope!(let tc, self.scope);
        let Some(name) = v8::String::new(tc, &kk_pathfinder::generate_room_name(xx, yy)) else {
            return Decision::Error;
        };
        let recv = v8::undefined(tc).into();
        let ret = callback.open(tc).call(tc, recv, &[name.into()]);
        let Some(ret) = ret else {
            if tc.has_terminated() {
                // TryCatch 的析构会清除已捕获状态；必须重挂不可捕获的终止，
                // 否则 native 的异常占位会被包装成普通错误，让玩家 catch 后续跑。
                tc.rethrow();
                return Decision::Error;
            }
            if let Some(exception) = tc.exception() {
                self.exception = Some(v8::Global::new(tc, exception));
            }
            return Decision::Error;
        };
        // 包装器已经读取 _bits（含 getter）；native 按 TypedArray 字节长度裁决。
        // 每次回调后同步此前所有矩阵，包含共享 backing、嵌套搜索与返回 false。
        for matrix in &self.matrices {
            matrix.refresh();
        }
        if ret.is_false() {
            return Decision::Blocked;
        }
        let Ok(array) = ret.try_cast::<v8::TypedArray>() else {
            return Decision::None;
        };
        if array.byte_length() != 2500 {
            return Decision::None;
        }
        let Some(backing) = array.get_backing_store() else {
            return Decision::None;
        };
        let matrix = MatrixSnapshot {
            backing,
            offset: array.byte_offset(),
            bytes: Box::new(std::array::from_fn(|_| Cell::new(0))),
        };
        matrix.refresh();
        let data = matrix.bytes.as_ptr().cast::<u8>();
        self.matrices.push(matrix);
        Decision::Matrix { data, len: 2500 }
    }

    fn is_cancelled(&mut self) -> bool {
        self.scope.is_execution_terminating()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_terrain(cells: &[(usize, usize, u8)]) -> Vec<u8> {
        let mut row = vec![0u8; 2500];
        for &(x, y, code) in cells {
            row[y * 50 + x] = code;
        }
        row
    }

    /// 行序 (x,y) → 列序 ii=x*50+y 的单点验证：与 kk-pathfinder
    /// terrain_packing 测试同锚（driver/lib/path-finder.js 的 init 打包）。
    #[test]
    fn pack_row_matches_column_layout() {
        let row = row_terrain(&[(20, 10, 1), (4, 3, 2)]);
        let bits = pack_row_terrain(&row).unwrap();
        let ii = 20 * 50 + 10;
        assert_eq!(bits[ii / 4] >> (ii % 4 * 2) & 0x03, 1);
        let ii2 = 4 * 50 + 3;
        assert_eq!(bits[ii2 / 4] >> (ii2 % 4 * 2) & 0x03, 2);
        assert_eq!(bits.iter().filter(|&&b| b != 0).count(), 2);
    }

    /// kk-core 位掩码：wall|swamp(3) 保留低两位（原生按障碍），lava(4)
    /// 低两位为 0（按平原）——pack_terrain 输入契约。
    #[test]
    fn pack_row_keeps_low_two_bits() {
        let row = row_terrain(&[(1, 1, 3), (2, 2, 4)]);
        let bits = pack_row_terrain(&row).unwrap();
        let ii = 50 + 1;
        assert_eq!(bits[ii / 4] >> (ii % 4 * 2) & 0x03, 3);
        let ii2 = 100 + 2;
        assert_eq!(bits[ii2 / 4] >> (ii2 % 4 * 2) & 0x03, 0);
    }

    #[test]
    fn pack_row_rejects_wrong_length() {
        assert!(pack_row_terrain(&[0; 2499]).is_none());
        assert!(pack_row_terrain(&[0; 2501]).is_none());
        assert!(pack_row_terrain(&[0; 2500]).is_some());
    }

    /// base64 与 kk-world 生产编码互逆（含 1/2 字节尾组与全部位组合）。
    #[test]
    fn base64_roundtrip_against_reference_alphabet() {
        // kk-world base64_encode 为私有函数；此处按同一字母表锚定已知向量。
        assert_eq!(base64_decode("AAAA"), Some(vec![0, 0, 0]));
        assert_eq!(base64_decode("AAAAAA=="), Some(vec![0, 0, 0, 0]));
        assert_eq!(base64_decode("/w=="), Some(vec![255]));
        assert_eq!(base64_decode("//8="), Some(vec![255, 255]));
        assert_eq!(base64_decode("//8B"), Some(vec![255, 255, 1]));
        // 非法输入
        assert_eq!(base64_decode("A"), None);
        assert_eq!(base64_decode("A==="), None);
        assert_eq!(base64_decode("===="), None);
        assert_eq!(base64_decode("AA=A"), None);
        let mut all = Vec::new();
        for b in 0..=255u8 {
            all.push(b);
        }
        // 256 字节 = 85.33 组：补 2 字节凑整三字节组后往返。
        all.extend_from_slice(&[0, 2]);
        let encoded = encode_reference(&all);
        assert_eq!(base64_decode(&encoded), Some(all));
    }

    /// 仅为测试提供的参考编码器：字母表逐字对齐 kk-world base64_encode。
    fn encode_reference(data: &[u8]) -> String {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            out.push(ALPHABET[(n >> 18) as usize & 0x3f] as char);
            out.push(ALPHABET[(n >> 12) as usize & 0x3f] as char);
            out.push(if chunk.len() > 1 {
                ALPHABET[(n >> 6) as usize & 0x3f] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                ALPHABET[n as usize & 0x3f] as char
            } else {
                '='
            });
        }
        out
    }

    /// JSON 通道双形态：b64 payload 与裸数组；坏房跳过、好房保留。
    #[test]
    fn from_world_json_accepts_both_terrain_shapes() {
        let row = row_terrain(&[(5, 6, 1)]);
        let payload = encode_reference(&row);
        let world = serde_json::json!({
            "rooms": {
                "W1N1": { "name": "W1N1", "terrain": { "rev": 3, "payload": payload } },
                "E2S2": { "name": "E2S2", "terrain": row.clone() },
                "E3S3": { "name": "E3S3", "terrain": "junk" },
            }
        });
        let pack = TerrainPack::from_world_json(&world);
        assert_eq!(pack.rooms.len(), 2);
        assert!(pack.rooms.contains_key("W1N1"));
        let expected = pack_row_terrain(&row).unwrap();
        assert_eq!(pack.rooms["E2S2"], expected);
        assert_eq!(pack.rooms["W1N1"], expected);
    }

    /// 墙环 = 8 邻域减世界内房间；名字经官方坐标语义生成。
    #[test]
    fn ring_rooms_eight_neighborhood_minus_owned() {
        let mut rooms = BTreeMap::new();
        rooms.insert("W1N1".to_string(), [0u8; 625]);
        rooms.insert("W0N1".to_string(), [0u8; 625]);
        let ring = ring_rooms(&rooms);
        // W1N1 的 8 邻域：W0N1(有)、W0N0、W0N2、W1N0、W1N2、W2N0、W2N1、W2N2
        // W0N1 的 8 邻域：W1N1(有)、W1N0、W1N2、W0N0、W0N2、E0N0、E0N1、E0N2
        //（W0 的东邻按官方坐标语义是 E0，无越界房）
        assert!(ring.contains(&"W1N0".to_string()));
        assert!(ring.contains(&"W2N1".to_string()));
        assert!(ring.contains(&"W0N2".to_string()));
        assert!(!ring.contains(&"W1N1".to_string()));
        assert!(!ring.contains(&"W0N1".to_string()));
        assert!(ring.contains(&"E0N1".to_string()));
        assert_eq!(ring.len(), 10);
    }

    /// 单房世界的墙环恰好 8 房且全墙。
    #[test]
    fn ring_single_room_is_eight_walls() {
        let mut rooms = BTreeMap::new();
        rooms.insert("E10S10".to_string(), [0u8; 625]);
        let ring = ring_rooms(&rooms);
        assert_eq!(ring.len(), 8);
        for room in ring {
            assert!(kk_pathfinder::parse_room_name(&room).is_some());
        }
    }

    /// 内容比对的载入语义：同字节零动作、异字节重载、真实房撤出世界后
    /// 以墙环身份被覆盖回全墙。直接驱动 kk-pathfinder 原生表（进程级状态）；
    /// 本测试是本二进制内唯一触碰者，其他集成测试各自独立进程。
    #[test]
    fn ensure_loaded_reloads_on_content_change_and_walls_ring() {
        fn loaded_snapshot() -> BTreeMap<String, [u8; 625]> {
            LOADED.lock().unwrap().clone()
        }
        let plain_row = vec![0u8; 2500];
        let mut rooms = BTreeMap::new();
        rooms.insert("E10S10".to_string(), pack_row_terrain(&plain_row).unwrap());
        let pack = TerrainPack { rooms };
        ensure_loaded(&pack).unwrap();
        let first = loaded_snapshot();
        assert_eq!(first.get("E10S10"), pack.rooms.get("E10S10"));
        assert_eq!(first.get("E9S10"), Some(&ALL_WALL));

        // 同内容再入：无重载（快照一致即可，重复载入无观察差）。
        ensure_loaded(&pack).unwrap();
        assert_eq!(loaded_snapshot(), first);

        // 地形变化：该房重载为混合地形（与全墙位级不同——全 1 行打包后
        // 与 ALL_WALL 相同，撤房回墙的断言会空洞）。
        let mut mixed_row = vec![1u8; 2500];
        mixed_row[0] = 0; // (0,0) 平原
        let mixed_bits = pack_row_terrain(&mixed_row).unwrap();
        assert_ne!(mixed_bits, ALL_WALL);
        let mut changed = BTreeMap::new();
        changed.insert("E10S10".to_string(), mixed_bits);
        ensure_loaded(&TerrainPack { rooms: changed }).unwrap();
        let second = loaded_snapshot();
        assert_eq!(second.get("E10S10"), Some(&mixed_bits));
        assert_eq!(second.get("E9S10"), Some(&ALL_WALL));

        // 跨分片残留：换世界后旧真实房（E10S10）落入新房间的墙环——期望
        // 全墙，与残留的 mixed_bits 不同，覆盖重载真实发生（空世界的环为空，
        // 不触碰任何房，同名场景须以邻房世界构造）。
        let mut next_world = BTreeMap::new();
        next_world.insert(
            "E9S10".to_string(),
            pack_row_terrain(&vec![0u8; 2500]).unwrap(),
        );
        ensure_loaded(&TerrainPack { rooms: next_world }).unwrap();
        assert_eq!(loaded_snapshot().get("E10S10"), Some(&ALL_WALL));
    }

    /// KKPV 公开束解析对照 kk-world 参考编码：房名与行序地形逐房相等。
    #[test]
    fn from_public_bundle_matches_kk_world_reference() {
        let terrain = kk_core::Terrain::from_codes(&row_terrain(&[(7, 8, 1), (9, 8, 2)])).unwrap();
        let mut world = kk_world::World::default();
        world.register_room("W5N5", terrain.clone());
        world.register_room("W5N6", kk_core::Terrain::plains());
        let blocks = world.kkpv_public_terrain_blocks([0; 32], [0; 32]);
        let bundle =
            kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                .unwrap();
        let pack = TerrainPack::from_public_bundle(&bundle).unwrap();
        assert_eq!(pack.rooms.len(), 2);
        assert_eq!(
            pack.rooms["W5N5"],
            pack_row_terrain(terrain.as_slice()).unwrap()
        );
        assert_eq!(
            pack.rooms["W5N6"],
            pack_row_terrain(kk_core::Terrain::plains().as_slice()).unwrap()
        );
    }

    /// 束框架与块头校验：截断、垃圾输入整体报错。
    #[test]
    fn from_public_bundle_rejects_malformed() {
        assert!(TerrainPack::from_public_bundle(&[]).is_err());
        assert!(TerrainPack::from_public_bundle(&[0, 0, 0, 5, 9, 9]).is_err());
        let mut world = kk_world::World::default();
        world.register_room("W5N5", kk_core::Terrain::plains());
        let blocks = world.kkpv_public_terrain_blocks([0; 32], [0; 32]);
        let bundle =
            kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                .unwrap();
        assert!(TerrainPack::from_public_bundle(&bundle[..bundle.len() - 1]).is_err());
    }

    /// 懒物化：JSON 通道直接返回；KKPV 通道解码一次并缓存。
    #[test]
    fn tick_terrain_materializes_once_for_kkpv() {
        let terrain = kk_core::Terrain::from_codes(&row_terrain(&[])).unwrap();
        let mut world = kk_world::World::default();
        world.register_room("W5N5", terrain);
        let blocks = world.kkpv_public_terrain_blocks([0; 32], [0; 32]);
        let bundle =
            kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                .unwrap();
        let tick = TickTerrain::kkpv(crate::shared_bundle::SharedKkpvBundle::from_vec(bundle));
        let a = tick.pack().unwrap();
        let b = tick.pack().unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert!(a.rooms.contains_key("W5N5"));

        let json_pack = Arc::new(TerrainPack::from_world_json(&serde_json::json!({})));
        let tick_json = TickTerrain::json(json_pack.clone());
        assert!(Arc::ptr_eq(&tick_json.pack().unwrap(), &json_pack));
    }
}
