//! 回调安全与静默语义测试（C2 批锁值）：
//!
//! 1. CostMatrix 错误长度在 search 侧被**官方代码**静默按「无矩阵」处理
//!    （长度经回调真实透传，pf.cc:54-57 裁决——与官方 JS bridge 无条件交出
//!    `ret._bits` 一致，driver/lib/path-finder.js:125-127）；
//! 2. 回调 panic 映射到官方 roomCallback throw 的同一条链：
//!    pf.cc:43-46 → pf.cc:563-567 → `{path:[], ops:0, cost:0, incomplete:false}`
//!    （driver/lib/path-finder.js:135-136），进程不 abort；
//! 3. 回调内 load_terrain 得 [`Error::ReentrantTerrainLoad`] 而非死锁
//!    （官方无此面，我方防死锁——带线程+超时保护，回退为死锁时测试报错而非挂起）；
//! 4. 嵌套 search 工作（官方 main.cc:9-14 双实例池 + TerrainLock 重入读）。
//!
//! 并行纪律：每个用例使用独立房间（全局地形表共享，见 lib.rs 的锁说明）。

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use kk_pathfinder::{
    CostMatrix, Error, Goal, RoomDecision, SearchOptions, SearchResult, TERRAIN_PLAIN, WorldPosition,
    load_terrain, pack_terrain, search, search_with_room_callback,
};

fn pos(room: &str, x: u32, y: u32) -> WorldPosition {
    WorldPosition::from_room(room, x, y).unwrap()
}

fn load_plain(room: &str) {
    load_terrain(room, &pack_terrain(&[[TERRAIN_PLAIN; 50]; 50])).unwrap();
}

fn path_xy(result: &SearchResult, room: &str) -> Vec<(u32, u32)> {
    result
        .path
        .iter()
        .map(|p| {
            assert_eq!(p.room_name(), room);
            (p.x(), p.y())
        })
        .collect()
}

fn empty_complete() -> SearchResult {
    SearchResult {
        path: vec![],
        ops: 0,
        cost: 0,
        incomplete: false,
    }
}

// =====================================================================
// 1. 错误长度矩阵：官方静默忽略（pf.cc:54-57 长度门经真实长度透传触发）
// =====================================================================
//
// 场景复用 tests/search.rs::cost_matrix_changes_path 的布置：E3N0 平原，
// origin (10,25) → goal (14,25)，矩阵把 (12,25)（字节 625）抬到 3。
// - deserialize 自 625 词（2500 字节）：矩阵生效 → 绕行阶梯（对照组）；
// - deserialize 自 624 词（2496 字节，(12,25) 字节 625 仍在界内可读！）：
//   官方 pf.cc:54-57 长度检查不过 → cost_matrix = null → **纯地形基线直列**。
//   这正是官方 JS bridge 的行为：错误长度的 _bits 照样交给 native、照样被忽略。
#[test]
fn cost_matrix_wrong_length_ignored_at_search() {
    load_plain("E3N0");
    let goal = Goal::pos(pos("E3N0", 14, 25));

    // 基线（无矩阵）：直列（同 tests/search.rs 用例 4 推导）
    let baseline = search(pos("E3N0", 10, 25), std::slice::from_ref(&goal), &SearchOptions::default())
        .unwrap();
    assert_eq!(
        path_xy(&baseline, "E3N0"),
        vec![(11, 25), (12, 25), (13, 25), (14, 25)]
    );

    // 对照组：2500 字节矩阵生效（绕行阶梯，证明 deserialize 通道本身工作）
    let mut words = vec![0u32; 625];
    words[156] = 3 << 8; // 字节 625 = (12,25) ← LE word 156 的 byte 1
    let full = CostMatrix::deserialize(&words);
    assert_eq!(full.get(12, 25), Some(3), "(12,25) 字节应在 2500 字节矩阵界内");
    let r = search_with_room_callback(
        pos("E3N0", 10, 25),
        std::slice::from_ref(&goal),
        &SearchOptions::default(),
        |_xx, _yy| RoomDecision::CostMatrix(&full),
    )
    .unwrap();
    assert_eq!(
        path_xy(&r, "E3N0"),
        vec![(11, 26), (12, 26), (13, 25), (14, 25)],
        "2500 字节矩阵应改变路径（对照组）"
    );

    // 主断言：2496 字节（(12,25) 仍可读）→ pf.cc:54-57 静默忽略 → 基线直列
    let short = CostMatrix::deserialize(&words[..624]);
    assert_eq!(
        short.get(12, 25),
        Some(3),
        "短矩阵的 (12,25) 字节 625 < 2496 应可读（官方 get 按实际字节工作）"
    );
    let r = search_with_room_callback(
        pos("E3N0", 10, 25),
        std::slice::from_ref(&goal),
        &SearchOptions::default(),
        |_xx, _yy| RoomDecision::CostMatrix(&short),
    )
    .unwrap();
    assert_eq!(
        path_xy(&r, "E3N0"),
        vec![(11, 25), (12, 25), (13, 25), (14, 25)],
        "非 2500 字节矩阵必须被官方长度门静默忽略（= 基线）"
    );
    assert_eq!(r.cost, 4);
}

// =====================================================================
// 2. 回调 panic：官方 roomCallback throw 语义 + 进程不 abort
// =====================================================================
//
// 官方链路：玩家 cb throw → JS bridge 包装层传播（driver/lib/path-finder.js:120-129）
// → pf.cc:43-46 TryCatch 捕获 → ReThrow + throw js_error() → pf.cc:563-567
// 捕获并返回 Undefined → JS bridge 映射 {path:[], ops:0, cost:0, incomplete:false}
// （driver/lib/path-finder.js:135-136）。我方等价物：panic 在 FFI trampoline 被
// catch_unwind 拦截 → 返回码 3 → compat 置挂起异常 → 走**同一段官方 pf.cc 代码**。
// 本测试若 abort 则进程死、测试无法到达断言——能失败即证明没 abort。
#[test]
fn room_callback_panic_first_room_maps_to_official_empty() {
    load_plain("W56N0");
    let result = search_with_room_callback(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 40, 40))],
        &SearchOptions::default(),
        |_xx, _yy| panic!("player roomCallback exploded"),
    )
    .unwrap();
    assert_eq!(result, empty_complete());
}

// 第二房间才 panic（首房正常返回）：官方 js_error 链与异常发生时刻无关，
// 任何触达点抛错都汇入 pf.cc:563-567 的同一个 catch → 同一空结果。
// 布置：E7N0 → E7S0 跨房（房间触达序 = origin 房先、goal 房后）。
#[test]
fn room_callback_panic_second_room_maps_to_official_empty() {
    load_plain("W57N0");
    load_plain("W57S0");
    let result = search_with_room_callback(
        pos("W57N0", 25, 47),
        &[Goal::pos(pos("W57S0", 25, 2))],
        &SearchOptions::default(),
        |xx, yy| {
            if kk_pathfinder::generate_room_name(xx, yy) == "W57N0" {
                RoomDecision::None
            } else {
                panic!("boom in goal room");
            }
        },
    )
    .unwrap();
    assert_eq!(result, empty_complete());
}

// panic 后池完整性：官方 js_error 路径复位 _is_in_use（pf.cc:565），同一实例可
// 继续服务后续搜索。锁值：panic 之后的普通 search 结果正常。
#[test]
fn search_works_after_callback_panic() {
    load_plain("W56N0");
    // 触发 panic 路径
    let _ = search_with_room_callback(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 40, 40))],
        &SearchOptions::default(),
        |_xx, _yy| panic!("first: reset instance under js_error"),
    )
    .unwrap();
    // 同线程后续搜索正常（挂起异常标志由 kk_ffi.cc 的 ArenaGuard 在 FFI 入口复位）
    let after = search(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 20, 20))],
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(!after.path.is_empty());
    assert!(!after.incomplete);
}

// =====================================================================
// 3. 回调内 load_terrain：重入拒绝（防死锁），带挂起保护
// =====================================================================
//
// 死锁机理（修复前）：search 持地形读份额 → 回调内 load_terrain 取写份额 →
// 写要等全部读者退出，而唯一读者正是本线程自己的栈帧 → 必死锁。
// 修复后：READ_DEPTH 线程本地深度检测 → Err(ReentrantTerrainLoad)。
// 官方无此面：官方地形只在启动期载入（driver/lib/path-finder.js:23-46），
// 搜索期不存在载入口——无官方语义可循，按防死锁拒绝（差异登记设计文档）。
//
// 挂起保护（macOS 无 timeout 命令）：搜索线程内经 channel 回报；主测试线程
// recv_timeout 限时等待——若实现回退为死锁，此处超时报错而不是测试进程挂死
// （泄漏的挂起工作线程随进程退出回收）。
#[test]
fn load_terrain_in_callback_rejected_not_deadlock() {
    load_plain("W55N0");
    let (tx, rx) = mpsc::channel::<Error>();
    let worker = thread::spawn(move || {
        let result = search_with_room_callback(
            pos("W55N0", 10, 10),
            &[Goal::pos(pos("W55N0", 40, 40))],
            &SearchOptions::default(),
            |_xx, _yy| {
                // 回调内重入 load_terrain：必须立即返回 Err 而非阻塞
                let inner = load_terrain("W54N0", &pack_terrain(&[[TERRAIN_PLAIN; 50]; 50]));
                tx.send(inner.unwrap_err()).ok();
                RoomDecision::None
            },
        );
        // 回调返回后搜索应正常完成（重入拒绝不影响外层）
        result.map(|r| (r.ops, r.incomplete))
    });
    let err = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("回调内 load_terrain 未返回——疑似死锁回归（READ_DEPTH 重入检测失效？）");
    assert_eq!(err, Error::ReentrantTerrainLoad);
    let (ops, incomplete) = worker
        .join()
        .expect("搜索线程 panic")
        .expect("外层搜索本应正常完成");
    assert!(ops > 0);
    assert!(!incomplete);
}

// 非回调路径的 load_terrain 不受影响（重入检测只拦本线程搜索栈内的调用）。
#[test]
fn load_terrain_outside_callback_still_works() {
    load_plain("W53N0"); // 拿不到锁的场合不存在——顺序调用必然成功
    let result = search(
        pos("W53N0", 10, 10),
        &[Goal::pos(pos("W53N0", 20, 20))],
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(!result.path.is_empty());
}

// =====================================================================
// 4. 嵌套 search：官方支持的两层递归
// =====================================================================
//
// 官方注释明示双实例池为递归 search 而设（driver/native/src/main.cc:9-14
// "recursive calls to the path finder are useful"）；我方 kk_ffi.cc 等价复刻，
// TerrainLock 的重入读保证同线程嵌套不阻塞。锁值：回调内的内层搜索与外层
// 结果均正常。
#[test]
fn nested_search_in_callback_works() {
    load_plain("W52N0"); // 外层
    load_plain("W51N0"); // 内层（回调中搜索）
    let outer_goal = Goal::pos(pos("W52N0", 40, 40));
    let mut inner_calls = 0u32;
    let result = search_with_room_callback(
        pos("W52N0", 10, 10),
        std::slice::from_ref(&outer_goal),
        &SearchOptions::default(),
        |_xx, _yy| {
            inner_calls += 1;
            let inner = search(
                pos("W51N0", 10, 10),
                &[Goal::pos(pos("W51N0", 20, 20))],
                &SearchOptions::default(),
            )
            .unwrap();
            assert!(!inner.path.is_empty(), "内层搜索应正常完成");
            assert!(!inner.incomplete);
            RoomDecision::None
        },
    )
    .unwrap();
    assert!(inner_calls >= 1);
    assert!(!result.path.is_empty());
    assert!(!result.incomplete);
}
