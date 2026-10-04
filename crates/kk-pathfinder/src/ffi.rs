//! Rust 与 native/kk_ffi.h 的 C ABI 绑定，集中管理裸指针、回调和结果释放。
//!
//! 输入切片与闭包指针只在同步调用期间使用。矩阵由 C++ 直接借用，必须持续到
//! 搜索返回；地形则在载入时复制。lib.rs 的读写锁防止搜索期间替换全局地形。
//! C++ 输出的路径由 malloc 分配，这里复制到 Vec 后调用 kk_pf_free_result。
//!
//! Rust 回调通过 trampoline 捕获可展开的 panic，转成返回码 3。此机制依赖
//! panic=unwind；不捕获进程主动终止，也不保证 panic 载荷析构再次 panic 时恢复。
//! C++ 的两个入口用完整函数的 try/catch 将分配或其他异常转为非零返回码。
//! C ABI 状态与生命周期约定以 native/kk_ffi.h 为准。

use std::ffi::c_void;

use crate::{HostRoomDecision, RoomDecision, SearchHost};

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

// 返回码见 native/kk_ffi.h。矩阵指针和真实长度通过两个输出参数传回，
// 2500 字节的长度判断由 native/pf.cc 的 room_index_from_pos 完成。
pub(super) type RoomCallback = unsafe extern "C" fn(
    user_data: *mut c_void,
    xx: u8,
    yy: u8,
    out_matrix: *mut *const u8,
    out_len: *mut usize,
) -> i32;

#[repr(C)]
struct SearchResultRaw {
    status: i32,
    js_error_message: *const std::os::raw::c_char,
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
        cancel: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
        options: *const Options,
        out_result: *mut SearchResultRaw,
    ) -> i32;

    fn kk_pf_free_result(result: *mut SearchResultRaw);

    fn kk_pf_load_terrain(map_xx: u8, map_yy: u8, bits: *const u8) -> i32;
}

/// 将 C 回调转回具体类型的 Rust 闭包，捕获可展开的 panic 后返回码 3。
/// panic hook 仍会运行；捕获的载荷在这里丢弃，不带回错误消息。
///
/// # Safety
/// user_data 须指向本次 ffi_search 期间有效、可独占借用的 F；调用不能并发或
/// 重入同一个 F。out_matrix/out_len 须为可写且不相互重叠的有效输出地址。
/// 返回矩阵时，其字节须保持不变并存活至整次搜索返回。
unsafe extern "C" fn room_callback_trampoline<'a, F>(
    user_data: *mut c_void,
    xx: u8,
    yy: u8,
    out_matrix: *mut *const u8,
    out_len: *mut usize,
) -> i32
where
    F: FnMut(u8, u8) -> RoomDecision<'a>,
{
    // SAFETY: search_with_room_callback 传入本次调用的 &mut F，C++ 同步串行调用它。
    // 嵌套搜索使用另一个调用帧和回调指针，不重入当前 F。
    let callback = unsafe { &mut *(user_data as *mut F) };
    // 闭包可能在 panic 前修改捕获的状态；捕获只阻止跨 ABI 展开，不回滚副作用。
    // 返回 3 后 C++ 停止本次搜索，不再调用这个闭包。
    let decision = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback(xx, yy)));
    match decision {
        Err(_payload) => 3,
        Ok(RoomDecision::None) => 0,
        Ok(RoomDecision::Blocked) => 1,
        Ok(RoomDecision::CostMatrix(matrix)) => {
            // SAFETY: native/compat/nan.h 的 Call 提供两个栈上输出地址；
            // matrix 的 'a 借用覆盖搜索调用，写出的缓冲指针不会提前失效。
            unsafe {
                *out_matrix = matrix.raw().as_ptr();
                *out_len = matrix.raw().len();
            }
            2
        }
    }
}

pub(super) fn trampoline<'a, F>() -> RoomCallback
where
    F: FnMut(u8, u8) -> RoomDecision<'a>,
{
    room_callback_trampoline::<'a, F>
}

/// 已转为 Rust 所有权的 C++ 结果，路径内存不再依赖 C++ 分配。
pub(super) struct RawOutcome {
    pub status: i32,
    /// 状态 3 的可选消息，返回后立即复制；回调 panic 不提供消息。
    pub js_error_message: Option<String>,
    pub ops: u32,
    pub cost: u32,
    pub incomplete: bool,
    /// 从结果终点向起点排列，lib.rs 的 finish 再反转。
    pub path: Vec<(u32, u32)>,
}

/// # Safety
/// callback/cancel 非空时，user_data 须符合两个回调的指针和独占调用约定。
/// 矩阵存储须稳定、可读并存活至返回；只能在 room 回调内（原生算法暂停）
/// 修改，且不能存在与修改冲突的 Rust 借用。不可变 RoomDecision 接口施加更严
/// 的只读约束。调用期间不能并发载入地形；lib.rs 的读锁排除地形写入，允许
/// 同线程使用独立宿主嵌套搜索。取消检查不能与房间回调并发或重入同一宿主。
pub(super) unsafe fn ffi_search(
    origin: (u32, u32),
    goals: &[(u32, u32, u32)],
    options: Options,
    callback: Option<RoomCallback>,
    user_data: *mut c_void,
    cancel: Option<unsafe extern "C" fn(*mut c_void) -> i32>,
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
        js_error_message: std::ptr::null(),
        ops: 0,
        cost: 0,
        incomplete: 0,
        path_len: 0,
        path: std::ptr::null_mut(),
    };
    // SAFETY: c_goals、options、raw 均在本栈帧存活；回调约束由调用方满足。
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
            cancel,
            &options,
            &mut raw,
        )
    };
    if rc != 0 {
        return None;
    }
    let mut outcome = RawOutcome {
        status: raw.status,
        js_error_message: if raw.js_error_message.is_null() {
            None
        } else {
            // SAFETY: C ABI 提供有效的 NUL 结尾消息，下一次调用前立即复制。
            Some(
                unsafe { std::ffi::CStr::from_ptr(raw.js_error_message) }
                    .to_string_lossy()
                    .into_owned(),
            )
        },
        ops: raw.ops,
        cost: raw.cost,
        incomplete: raw.incomplete != 0,
        path: Vec::new(),
    };
    // 空路径也可能有 malloc 分配；不构造零长度裸切片，但仍在末尾释放。
    if raw.path_len > 0 && !raw.path.is_null() {
        let len = raw.path_len as usize * 2;
        // SAFETY: C ABI 保证非空路径含 path_len 对 u32，按此长度借用后复制。
        let slice = unsafe { std::slice::from_raw_parts(raw.path, len) };
        outcome.path = slice
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[xx, yy]| (xx, yy))
            .collect();
    }
    // SAFETY: raw 来自成功的 kk_pf_search，尚未释放；复制后的 Rust 数据不引用它。
    unsafe { kk_pf_free_result(&mut raw) };
    Some(outcome)
}

/// # Safety
/// user_data 指向 search_with_host 的独占 H；输出指针由 C++ 栈提供。
pub(super) unsafe extern "C" fn host_room<H: SearchHost>(
    user_data: *mut c_void,
    xx: u8,
    yy: u8,
    out_matrix: *mut *const u8,
    out_len: *mut usize,
) -> i32 {
    // SAFETY: search_with_host 在本次同步调用内保持 H 存活，回调串行执行。
    let host = unsafe { &mut *user_data.cast::<H>() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.room(xx, yy))) {
        Ok(HostRoomDecision::None) => 0,
        Ok(HostRoomDecision::Blocked) => 1,
        Ok(HostRoomDecision::Matrix { data, len }) => {
            // SAFETY: 输出地址有效；data 的存活和可读范围由 SearchHost 保证。
            unsafe {
                *out_matrix = data;
                *out_len = len;
            }
            2
        }
        Ok(HostRoomDecision::Error) | Err(_) => 3,
    }
}

/// # Safety
/// user_data 与 host_room 同源，取消检查不与房间回调同时执行。
pub(super) unsafe extern "C" fn host_cancel<H: SearchHost>(user_data: *mut c_void) -> i32 {
    // SAFETY: H 由 search_with_host 保持独占、有效；嵌套调用另有 H。
    let host = unsafe { &mut *user_data.cast::<H>() };
    i32::from(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| host.is_cancelled()))
            .unwrap_or(true),
    )
}

/// 同步复制一个房间的 625 字节地形。
///
/// # Safety
/// 调用期间不得并发搜索或载入地形；lib.rs 的全局写锁满足此条件。
pub(super) unsafe fn ffi_load_terrain(map_xx: u8, map_yy: u8, bits: &[u8; 625]) -> bool {
    // SAFETY: bits 含 625 字节且调用期间存活；全局地形的互斥由调用方保证。
    unsafe { kk_pf_load_terrain(map_xx, map_yy, bits.as_ptr()) == 0 }
}
