use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use kk_pathfinder::{
    CostMatrix, Error, Goal, RoomDecision, SearchOptions, SearchResult, TERRAIN_PLAIN,
    WorldPosition, load_terrain, pack_terrain, search, search_with_room_callback,
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

// 2496 字节矩阵仍能读到被改的格子，但搜索只接受 2500 字节。
// 先用完整矩阵确认路径变化，再用短矩阵检查回到无矩阵的基线路径。
#[test]
fn cost_matrix_wrong_length_ignored_at_search() {
    load_plain("E3N0");
    let goal = Goal::pos(pos("E3N0", 14, 25));

    // 基线（无矩阵）：直列（同 tests/search.rs 用例 4 推导）
    let baseline = search(
        pos("E3N0", 10, 25),
        std::slice::from_ref(&goal),
        &SearchOptions::default(),
    )
    .unwrap();
    assert_eq!(
        path_xy(&baseline, "E3N0"),
        vec![(11, 25), (12, 25), (13, 25), (14, 25)]
    );

    // 对照组：2500 字节矩阵生效（绕行阶梯，证明 deserialize 通道本身工作）
    let mut words = vec![0u32; 625];
    words[156] = 3 << 8; // 字节 625 = (12,25) ← LE word 156 的 byte 1
    let full = CostMatrix::deserialize(&words);
    assert_eq!(
        full.get(12, 25),
        Some(3),
        "(12,25) 字节应在 2500 字节矩阵界内"
    );
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

// 首房回调 panic 应返回 JSError(None)，不能变成正常空结果。
#[test]
fn room_callback_panic_first_room_maps_to_official_throw() {
    load_plain("W56N0");
    let err = search_with_room_callback(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 40, 40))],
        &SearchOptions::default(),
        |_xx, _yy| panic!("player roomCallback exploded"),
    )
    .unwrap_err();
    assert!(
        matches!(err, kk_pathfinder::Error::JSError(None)),
        "回调 panic 映射 JSError(None) 上抛: {err:?}"
    );
}

// 首房正常、跨房后 panic，也应终止整次搜索并返回错误。
#[test]
fn room_callback_panic_second_room_maps_to_official_throw() {
    load_plain("W57N0");
    load_plain("W57S0");
    let err = search_with_room_callback(
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
    .unwrap_err();
    assert!(
        matches!(err, kk_pathfinder::Error::JSError(None)),
        "第二房 panic 同一 js_error 通道: {err:?}"
    );
}

// 同线程发生回调错误后，实例池和本层异常状态仍须允许后续搜索。
#[test]
fn search_works_after_callback_panic() {
    load_plain("W56N0");
    // 触发 panic 路径（官方上抛 → Err）
    let err = search_with_room_callback(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 40, 40))],
        &SearchOptions::default(),
        |_xx, _yy| panic!("first: reset instance under js_error"),
    )
    .unwrap_err();
    assert!(matches!(err, kk_pathfinder::Error::JSError(None)));
    let after = search(
        pos("W56N0", 10, 10),
        &[Goal::pos(pos("W56N0", 20, 20))],
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(!after.path.is_empty());
    assert!(!after.incomplete);
}

// 回调持读锁时不能申请地形写锁。用 channel 超时检测阻塞，
// 错误处理后外层搜索仍应完成。
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

#[test]
fn load_terrain_outside_callback_still_works() {
    load_plain("W53N0");
    let result = search(
        pos("W53N0", 10, 10),
        &[Goal::pos(pos("W53N0", 20, 20))],
        &SearchOptions::default(),
    )
    .unwrap();
    assert!(!result.path.is_empty());
}

// 内外层搜索同时存活，须使用不同的 C++ 实例，并复用本线程的地形读锁。
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
