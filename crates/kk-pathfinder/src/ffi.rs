//! C ABI 绑定：本 crate 的 unsafe 集中地（其余模块均为安全代码）。
//!
//! # 安全性说明（对照 native/kk_ffi.h）
//!
//! - `kk_pf_search`：
//!   - `goals` 指针仅在调用期内被读取（C++ 侧逐元素构造成 arena 节点后不再引用），
//!     调用方保证切片存活于调用期——由安全封装以引用传入，编译期即成立；
//!   - 回调 `user_data` 必须在整次调用内有效且可变借用——安全封装把闭包放在自身
//!     栈帧上传入，借用检查保证不存在别名；
//!   - 回调写出的 `*out_matrix` 指向的 CostMatrix 必须存活到 search 返回
//!     （pf.cc:54-57 直接引用该缓冲、不拷贝）——安全封装用 `search_with_room_callback`
//!     的生命周期参数把该约束交给借用检查；
//!   - 返回的 `result.path` 是 C 侧 malloc 分配的数组，安全封装拷贝进 `Vec` 后
//!     立即 `kk_pf_free_result`，无泄漏、无悬垂。
//! - `kk_pf_load_terrain`：`bits` 仅在调用期内读取（pf.cc:606-612 内部 memcpy 到
//!   自有分配后不再引用调用方缓冲）。
//! - 地形是 C++ 侧静态全局（pf.h:359），`load_terrain` 与 `search` 的并发访问由
//!   lib.rs 的全局读写锁串行化（对照官方：地形在进程启动时一次性载入，之后只读）。

use std::ffi::c_void;

use crate::RoomDecision;

#[repr(C)]
#[derive(Clone, Copy)]
struct WorldPos {
    xx: u32,
    yy: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Goal {
    xx: u32,
    yy: u32,
    range: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Options {
    plain_cost: u32,
    swamp_cost: u32,
    max_ops: u32,
    max_cost: u32,
    max_rooms: u32,
    flee: u32,
    heuristic_weight: f64,
}

impl Options {
    pub(super) fn new(
        plain_cost: u32,
        swamp_cost: u32,
        max_ops: u32,
        max_cost: u32,
        max_rooms: u32,
        flee: bool,
        heuristic_weight: f64,
    ) -> Self {
        Options {
            plain_cost,
            swamp_cost,
            max_ops,
            max_cost,
            max_rooms,
            flee: u32::from(flee),
            heuristic_weight,
        }
    }
}

/// 回调返回码：0=undefined（无矩阵）；1=false（封锁房间）；2=矩阵经 out 提供。
pub(super) type RoomCallback =
    unsafe extern "C" fn(user_data: *mut c_void, xx: u8, yy: u8, out_matrix: *mut *const u8) -> i32;

#[repr(C)]
struct SearchResultRaw {
    status: i32,
    ops: u32,
    cost: u32,
    incomplete: u32,
    path_len: u32,
    path: *mut u32,
}

unsafe extern "C" {
    fn kk_pf_search(
        origin: WorldPos,
        goals: *const Goal,
        goals_len: usize,
        callback: Option<RoomCallback>,
        callback_user_data: *mut c_void,
        options: *const Options,
        out_result: *mut SearchResultRaw,
    ) -> i32;

    fn kk_pf_free_result(result: *mut SearchResultRaw);

    fn kk_pf_load_terrain(map_xx: u8, map_yy: u8, bits: *const u8) -> i32;
}

/// 回调 trampoline：把 C 函数指针形态转回 Rust 闭包（按 F 单态化）。
///
/// # Safety
/// `user_data` 必须是存活于整次 `kk_pf_search` 调用期的 `&mut F`；闭包返回
/// `RoomDecision::CostMatrix(m)` 时 `m` 必须存活到 search 返回——两条约束均由
/// `search_with_room_callback` 的签名与调用结构静态保证（闭包与其引用的
/// CostMatrix 都在调用方栈帧或更外层存活）。
unsafe extern "C" fn room_callback_trampoline<'a, F>(
    user_data: *mut c_void,
    xx: u8,
    yy: u8,
    out_matrix: *mut *const u8,
) -> i32
where
    F: FnMut(u8, u8) -> RoomDecision<'a>,
{
    let callback = unsafe { &mut *(user_data as *mut F) };
    match callback(xx, yy) {
        RoomDecision::None => 0,
        RoomDecision::Blocked => 1,
        RoomDecision::CostMatrix(matrix) => {
            unsafe { *out_matrix = matrix.raw().as_ptr() };
            2
        }
    }
}

/// 取 F 对应的 trampoline 函数指针（生命周期参数在单态化后擦除，仅用于约束检查）。
pub(super) fn trampoline<'a, F>() -> RoomCallback
where
    F: FnMut(u8, u8) -> RoomDecision<'a>,
{
    room_callback_trampoline::<'a, F>
}

/// 一次 search 的原始结果拷贝（path 已转入 `Vec`，C 内存已释放）。
pub(super) struct RawOutcome {
    pub status: i32,
    pub ops: u32,
    pub cost: u32,
    pub incomplete: bool,
    /// native 原序（goal→origin），安全层负责 reverse（driver/lib/path-finder.js:101）
    pub path: Vec<(u32, u32)>,
}

/// # Safety（调用方 = 本 crate 安全层，约束已静态满足）
/// - `goals` 切片调用期内有效；
/// - `user_data`（非空时）指向的闭包及其返回的 CostMatrix 引用存活至调用返回；
/// - 调用期间无并发 `kk_pf_load_terrain`（安全层以地形读锁保证）。
pub(super) unsafe fn ffi_search(
    origin: (u32, u32),
    goals: &[(u32, u32, u32)],
    options: Options,
    callback: Option<RoomCallback>,
    user_data: *mut c_void,
) -> Option<RawOutcome> {
    let c_goals: Vec<Goal> = goals
        .iter()
        .map(|g| Goal {
            xx: g.0,
            yy: g.1,
            range: g.2,
        })
        .collect();
    let mut raw = SearchResultRaw {
        status: 0,
        ops: 0,
        cost: 0,
        incomplete: 0,
        path_len: 0,
        path: std::ptr::null_mut(),
    };
    let rc = unsafe {
        kk_pf_search(
            WorldPos {
                xx: origin.0,
                yy: origin.1,
            },
            c_goals.as_ptr(),
            c_goals.len(),
            callback,
            user_data,
            &options,
            &mut raw,
        )
    };
    if rc != 0 {
        return None;
    }
    let mut outcome = RawOutcome {
        status: raw.status,
        ops: raw.ops,
        cost: raw.cost,
        incomplete: raw.incomplete != 0,
        path: Vec::new(),
    };
    // path 在 path_len==0 时可能为哑指针：长度为 0 时不解引用
    if raw.path_len > 0 && !raw.path.is_null() {
        let len = raw.path_len as usize * 2;
        // Safety: C 侧保证 path 指向 path_len*2 个 u32 的 malloc 分配
        let slice = unsafe { std::slice::from_raw_parts(raw.path, len) };
        outcome.path = slice
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[xx, yy]| (xx, yy))
            .collect();
    }
    unsafe { kk_pf_free_result(&mut raw) };
    Some(outcome)
}

/// 载入一个房间的地形。`bits` 仅调用期内读取。
///
/// # Safety
/// 调用期间不得并发调用其他 kk_pf_* 入口（安全层以全局写锁保证）。
pub(super) unsafe fn ffi_load_terrain(map_xx: u8, map_yy: u8, bits: &[u8; 625]) -> bool {
    unsafe { kk_pf_load_terrain(map_xx, map_yy, bits.as_ptr()) == 0 }
}
