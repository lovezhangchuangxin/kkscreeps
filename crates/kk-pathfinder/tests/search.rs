mod support;
use kk_pathfinder::{
    CostMatrix, Goal, RoomDecision, SearchOptions, SearchResult, TERRAIN_PLAIN, TERRAIN_SWAMP,
    TERRAIN_WALL, WorldPosition, generate_room_name, pack_terrain,
};
use support::{load_terrain, search, search_with_room_callback};

fn pos(room: &str, x: u32, y: u32) -> WorldPosition {
    WorldPosition::from_room(room, x, y).unwrap()
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

fn load_room(room: &str, tiles: [[u8; 50]; 50]) {
    load_terrain(room, &pack_terrain(&tiles)).unwrap();
}

fn plain_room(room: &str) {
    load_room(room, [[TERRAIN_PLAIN; 50]; 50]);
}

fn do_search(origin: WorldPosition, goals: &[Goal]) -> SearchResult {
    search(origin, goals, &SearchOptions::default()).unwrap()
}

// ops 只计主循环的 JPS 展开，不计初始 A* 或到达目标时的出堆。
#[test]
fn straight_line_no_obstacle() {
    plain_room("E0N0");
    let result = do_search(pos("E0N0", 25, 25), &[Goal::pos(pos("E0N0", 25, 30))]);
    assert_eq!(
        path_xy(&result, "E0N0"),
        vec![(25, 26), (25, 27), (25, 28), (25, 29), (25, 30)]
    );
    assert_eq!(result.cost, 5);
    assert_eq!(result.ops, 3);
    assert!(!result.incomplete);
}

// 左右绕墙代价相同，方向插入顺序与 heap_t::bubble_up 的 <= 比较决定取左。
// 来源：native/pf.h 的 direction_t、heap_t；native/pf.cc 的 astar。
#[test]
fn wall_detour_tie_breaking_takes_left() {
    let mut tiles = [[TERRAIN_PLAIN; 50]; 50];
    tiles[26][25] = TERRAIN_WALL; // (x=25, y=26)
    load_room("E1N0", tiles);
    let result = do_search(pos("E1N0", 25, 25), &[Goal::pos(pos("E1N0", 25, 27))]);
    assert_eq!(path_xy(&result, "E1N0"), vec![(24, 26), (25, 27)]);
    assert_eq!(result.cost, 2);
    assert_eq!(result.ops, 1);
    assert!(!result.incomplete);
}

// 到目标的切比雪夫距离不大于 range 即完成，路径不必走到目标格。
#[test]
fn range_goal_domain() {
    plain_room("E2N0");
    let result = do_search(
        pos("E2N0", 10, 10),
        &[Goal::with_range(pos("E2N0", 20, 20), 3)],
    );
    let expect: Vec<(u32, u32)> = (1..=7).map(|i| (10 + i, 10 + i)).collect();
    assert_eq!(path_xy(&result, "E2N0"), expect);
    assert_eq!(result.cost, 7);
    assert_eq!(result.ops, 6);
    assert!(!result.incomplete);
    // 终点恰好落在距离 == range 的域边界（Chebyshev）
    let last = result.path.last().unwrap();
    let dist = (20i32 - last.x() as i32)
        .abs()
        .max((20i32 - last.y() as i32).abs());
    assert_eq!(dist, 3);
}

// 起点已满足目标条件时不展开搜索，返回已完成的空路径。
#[test]
fn range_goal_already_in_range() {
    plain_room("E2N0");
    let result = do_search(
        pos("E2N0", 19, 20),
        &[Goal::with_range(pos("E2N0", 20, 20), 3)],
    );
    assert_eq!(
        result,
        SearchResult {
            path: vec![],
            ops: 0,
            cost: 0,
            incomplete: false
        }
    );
}

#[test]
fn empty_goals() {
    plain_room("E2N0");
    let result = do_search(pos("E2N0", 10, 10), &[]);
    assert_eq!(
        result,
        SearchResult {
            path: vec![],
            ops: 0,
            cost: 0,
            incomplete: false
        }
    );
}

// 把直线路径中间格设为较高代价，确认矩阵改变路径而不是仅被读取。
#[test]
fn cost_matrix_changes_path() {
    // 平原基线地形；矩阵差异经 roomCallback 注入（不重载地形）
    load_room("E3N0", [[TERRAIN_PLAIN; 50]; 50]);

    // 基线（无矩阵）
    let goal = Goal::pos(pos("E3N0", 14, 25));
    let baseline = do_search(pos("E3N0", 10, 25), &[goal]);
    assert_eq!(
        path_xy(&baseline, "E3N0"),
        vec![(11, 25), (12, 25), (13, 25), (14, 25)]
    );
    assert_eq!(baseline.cost, 4);

    // (12,25) 抬到 3
    let mut matrix = CostMatrix::new();
    matrix.set(12, 25, 3);
    let result = search_with_room_callback(
        pos("E3N0", 10, 25),
        std::slice::from_ref(&goal),
        &SearchOptions::default(),
        |_xx, _yy| RoomDecision::CostMatrix(&matrix),
    )
    .unwrap();
    let path = path_xy(&result, "E3N0");
    assert_eq!(
        path,
        vec![(11, 26), (12, 26), (13, 25), (14, 25)],
        "CostMatrix 应改变路径"
    );
    assert_eq!(result.cost, 4);
    assert!(!result.incomplete);
    assert!(!path.contains(&(12, 25)));
}

// 起点房间被回调封锁时，native 返回 -1；Rust 转为未完成的空结果。
#[test]
fn room_callback_block_returns_incomplete() {
    load_room("E3N0", [[TERRAIN_PLAIN; 50]; 50]);
    let result = search_with_room_callback(
        pos("E3N0", 10, 10),
        &[Goal::pos(pos("E3N0", 40, 40))],
        &SearchOptions::default(),
        |_xx, _yy| RoomDecision::Blocked,
    )
    .unwrap();
    assert_eq!(
        result,
        SearchResult {
            path: vec![],
            ops: 0,
            cost: 0,
            incomplete: true
        }
    );
}

// Screeps 只检查对角落格能否通行，不要求旁边两个正交格都可通行。
#[test]
fn diagonal_corner_cutting_allowed() {
    let mut tiles = [[TERRAIN_PLAIN; 50]; 50];
    tiles[24][20] = TERRAIN_WALL; // (20,24)
    tiles[25][21] = TERRAIN_WALL; // (21,25)
    load_room("E4N0", tiles);
    let result = do_search(pos("E4N0", 20, 25), &[Goal::pos(pos("E4N0", 21, 24))]);
    assert_eq!(path_xy(&result, "E4N0"), vec![(21, 24)]);
    assert_eq!(result.cost, 1);
    assert_eq!(result.ops, 0);
}

#[test]
fn diagonal_goal_straight_diagonal() {
    plain_room("E5N0");
    let result = do_search(pos("E5N0", 10, 10), &[Goal::pos(pos("E5N0", 13, 13))]);
    assert_eq!(path_xy(&result, "E5N0"), vec![(11, 11), (12, 12), (13, 13)]);
    assert_eq!(result.cost, 3);
}

// 墙列仅留一个沼泽入口，强制路径依次支付沼泽和平原代价。
#[test]
fn plain_and_swamp_costs() {
    let mut tiles = [[TERRAIN_PLAIN; 50]; 50];
    for row in &mut tiles {
        row[11] = TERRAIN_WALL;
    }
    tiles[10][11] = TERRAIN_SWAMP;
    load_room("E6N0", tiles);
    let origin = pos("E6N0", 10, 10);
    let goal = Goal::pos(pos("E6N0", 12, 10));

    // 默认 swamp=5：cost = 5 + 1
    let r = do_search(origin, &[goal]);
    assert_eq!(path_xy(&r, "E6N0"), vec![(11, 10), (12, 10)]);
    assert_eq!(r.cost, 6);
    // swampCost=2：cost = 2 + 1
    let r = search(
        origin,
        std::slice::from_ref(&goal),
        &SearchOptions {
            swamp_cost: 2,
            ..SearchOptions::default()
        },
    )
    .unwrap();
    assert_eq!(r.cost, 3);
    // plainCost=3：cost = 5 + 3
    let r = search(
        origin,
        std::slice::from_ref(&goal),
        &SearchOptions {
            plain_cost: 3,
            ..SearchOptions::default()
        },
    )
    .unwrap();
    assert_eq!(r.cost, 8);
    // plainCost=0 → 钳制为 1（driver/lib/path-finder.js:64）：cost = 5 + 1
    let r = search(
        origin,
        std::slice::from_ref(&goal),
        &SearchOptions {
            plain_cost: 0,
            ..SearchOptions::default()
        },
    )
    .unwrap();
    assert_eq!(r.cost, 6);
}

// 等代价路径的具体形状由堆序决定；房境处只能直跨，不能斜跨。
// 来源：native/pf.cc 的 astar（portal 邻居过滤）。
#[test]
fn cross_room_straight() {
    plain_room("E7N0");
    plain_room("E7S0");
    let result = do_search(pos("E7N0", 25, 47), &[Goal::pos(pos("E7S0", 25, 2))]);
    let path: Vec<(String, u32, u32)> = result
        .path
        .iter()
        .map(|p| (p.room_name(), p.x(), p.y()))
        .collect();
    assert_eq!(
        path,
        vec![
            ("E7N0".to_string(), 24, 48),
            ("E7N0".to_string(), 23, 49),
            ("E7S0".to_string(), 23, 0),
            ("E7S0".to_string(), 24, 1),
            ("E7S0".to_string(), 25, 2),
        ]
    );
    assert_eq!(result.cost, 5);
    assert!(!result.incomplete);
}

// 房间外圈补墙，防止搜索缺少地形的邻房；只观察目标被围住后的未完成路径。
#[test]
fn unreachable_goal_incomplete() {
    let mut tiles = [[TERRAIN_PLAIN; 50]; 50];
    for (y, row) in tiles.iter_mut().enumerate() {
        row[0] = TERRAIN_WALL;
        row[49] = TERRAIN_WALL;
        if y == 0 || y == 49 {
            row.fill(TERRAIN_WALL);
        }
    }
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx != 0 || dy != 0 {
                tiles[(30 + dy) as usize][(30 + dx) as usize] = TERRAIN_WALL;
            }
        }
    }
    load_room("W62N0", tiles);
    let goal = Goal::pos(pos("W62N0", 30, 30));
    let result = do_search(pos("W62N0", 10, 10), &[goal]);
    assert!(result.incomplete);
    assert!(!result.path.is_empty());
    // 路径合法性：首步邻接 origin，其余逐步邻接（国王步），末端 h==2
    let mut prev = pos("W62N0", 10, 10);
    for step in &result.path {
        assert_eq!(step.room_name(), "W62N0");
        let d = (step.xx as i64 - prev.xx as i64)
            .abs()
            .max((step.yy as i64 - prev.yy as i64).abs());
        assert_eq!(d, 1, "路径必须逐步邻接: {prev:?} -> {step:?}");
        prev = *step;
    }
    let last = result.path.last().unwrap();
    let dist = (30i32 - last.x() as i32)
        .abs()
        .max((30i32 - last.y() as i32).abs());
    assert_eq!(dist, 2);
    // cost == 逐步平原代价和（本房无沼泽/矩阵，plain=1）
    assert_eq!(result.cost, result.path.len() as u32);
}

// flee 须达到 range 距离，直跳与斜跳的堆顺序决定本场景的终点。
// 来源：native/pf.cc 的 heuristic、jump_x/jump_y/jump_xy；native/pf.h 的 heap_t。
#[test]
fn flee_takes_straight_up() {
    plain_room("E9N0");
    let result = search(
        pos("E9N0", 25, 25),
        &[Goal::with_range(pos("E9N0", 25, 25), 10)],
        &SearchOptions {
            flee: true,
            ..SearchOptions::default()
        },
    )
    .unwrap();
    let expect: Vec<(u32, u32)> = (0..10).map(|i| (24, 24 - i)).collect();
    assert_eq!(path_xy(&result, "E9N0"), expect);
    assert_eq!(result.cost, 10);
    assert_eq!(result.ops, 1);
    assert!(!result.incomplete);
}

// W59N0 不载入；触达它应返回带消息的错误，不能当作正常空路径。
#[test]
fn missing_terrain_throws_not_empty() {
    plain_room("W60N0"); // W59N0（+x 方向邻房）不载入
    let err = search(
        pos("W60N0", 45, 25),
        &[Goal::pos(pos("W59N0", 25, 25))],
        &SearchOptions::default(),
    )
    .expect_err("地形缺失官方上抛");
    assert!(
        matches!(&err, kk_pathfinder::Error::JSError(Some(m)) if m == "Could not load terrain data"),
        "地形缺失映射 JSError + 官方文案: {err:?}"
    );
}

// 房间在本次搜索中首次触达时调回调，缓存命中后不重复调用。
// 来源：native/pf.cc 的 room_index_from_pos。
#[test]
fn room_callback_invoked_once_per_room() {
    plain_room("W58N0");
    let (map_xx, map_yy) = kk_pathfinder::parse_room_name("W58N0").unwrap();
    let mut calls = Vec::new();
    let result = search_with_room_callback(
        pos("W58N0", 10, 10),
        &[Goal::pos(pos("W58N0", 20, 20))],
        &SearchOptions::default(),
        |xx, yy| {
            calls.push((xx, yy));
            assert_eq!(generate_room_name(xx, yy), "W58N0");
            RoomDecision::None
        },
    )
    .unwrap();
    assert_eq!(calls, vec![(map_xx, map_yy)]);
    assert!(!result.path.is_empty());
}
