//! search 行为测试：期望值全部由官方 pf.cc 语义手工推导（推导写在各用例注释），
//! 并由 tests/official_golden.rs 的官方校验和背书（编译产物 == 官方算法）。
//!
//! 并行纪律：每个用例使用独立房间（全局地形表共享，见 lib.rs 的锁说明）。

use kk_pathfinder::{
    CostMatrix, Goal, RoomDecision, SearchOptions, SearchResult, TERRAIN_PLAIN, TERRAIN_SWAMP,
    TERRAIN_WALL, WorldPosition, generate_room_name, load_terrain, pack_terrain, search,
    search_with_room_callback,
};

fn pos(room: &str, x: u32, y: u32) -> WorldPosition {
    WorldPosition::from_room(room, x, y).unwrap()
}

/// 结果路径 → 房内坐标序（限定单房用例；跨房用例单独断言）。
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

// =====================================================================
// 1. 无障碍直线（E0N0）
// =====================================================================
//
// 期望推导（官方默认选项：plain=1、swamp=5、hw=1.2、maxOps=2000）：
// - origin (25,25) → goal (25,30)（range 0）。astar 首 expands 8 邻居
//   （pf.cc:523、152-189），f = 1 + trunc(h*1.2)：(24,26)/(25,26)/(26,26) 均为
//   h=4 → f=5 的三方平局；(25,24)/(26,24)/(24,24) f=8，(26,25)/(24,25) f=7。
// - 堆平局裁决（pf.h:301-330）：insert 用 bubble_up，比较为 `<=`——后插入的等价
//   节点会上浮越过先插入者。astar 按 TOP..TOP_LEFT 枚举（pf.h:69），BOTTOM_LEFT
//   (24,26) 第 6 个插入 → 平局链的最上方 → 首个出堆。
// - (24,26) 的 jps（斜向左下，pf.cc:304-437）因左侧墙面... 无墙，纯地形均匀：
//   jump_x 沿行扫到房间边（h 在该行永不为 0），jump_y 到 (24,30)（h=1），
//   对角 jump 到 (23,27)。出堆序 (24,26)→(23,27)→(25,26)：各耗 1 op。
// - (25,26)（直向下方）的 jps：jump_y 从 (25,27) 直扫到 (25,30)（heuristic==0
//   中断，pf.cc:227-228），一步入堆 f=5。
// - 第 4 次出堆 (25,30) → h=0 收束（pf.cc:539-543）。
// ⇒ 路径 = 直列 [(25,26)..(25,30)]，cost = 5，ops = 3（3 次 jps 展开）。
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

// =====================================================================
// 2. 绕墙 tie-breaking（E1N0）：两侧等价路径，官方取左侧
// =====================================================================
//
// 布置：origin (25,25)、goal (25,27)、墙 (25,26)。两条等价最优路径：
// [(24,26),(25,27)] 与 [(26,26),(25,27)]，均 cost 2——官方取**左侧**。
//
// 期望推导：
// - astar 7 个可通行邻居（BOTTOM 是墙）。BOTTOM_RIGHT (26,26) 第 4 个插入、
//   BOTTOM_LEFT (24,26) 第 5 个插入，f 均 = 1 + trunc(1*1.2) = 2。
// - 堆 insert/bubble_up 的 `<=` 比较（pf.h:322-330）使**后插入的等价节点**
//   上浮到堆顶：5 个插入后堆顶 = (24,26)（左侧）——这是方向枚举序
//   （pf.h:69 TOP..TOP_LEFT）× 堆平局规则共同决定的确定性偏置。
// - jps((24,26))（dx=-1, dy=1）：look((25,26)) = 墙 ≠ cost → 触发 forced
//   neighbor 规则（pf.cc:415-417），对 (25,27) 做对角 jump：jump_xy 首检
//   heuristic((25,27)) == 0 → 立即返回（pf.cc:259）→ (25,27) 以 g=2 入堆。
// - 下一轮出堆 (25,27) → h=0 收束 ⇒ 路径 [(24,26),(25,27)]，cost 2，ops 1。
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

// =====================================================================
// 3. range > 0 的目标域（E2N0）
// =====================================================================
//
// 期望推导：origin (10,10) → goal (20,20) range 3。heuristic = Chebyshev 距离
// 减 range（pf.cc:137-146），h==0 即到达 ⇒ 终点在距离恰为 3 的格上。
// - astar 邻居中 (11,11) 是唯一最低 f（h = 9-3 = 6 → f = 1+7 = 8；直向邻居
//   h=7 → f=9）→ 先出堆。
// - 均匀场上对角 jump_xy 的正交跳检查立即中断（pf.cc:263-276：两侧 jump_x/
//   jump_y 均非空 → break），对角只能逐格推进；直向 jump 可长跳但 h 沿途不为
//   0（range 域边界不在直线上）。
// - 对角节点 (11,11)...(16,16) 依次出堆（每段一次 jps 展开），(17,17)（h=0，
//   heuristic 中断处，pf.cc:259）出堆即收束 ⇒ 7 步纯对角，cost 7，
//   ops 6（6 次展开；收束轮不展开）。
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

// origin 已在目标域内：heuristic(origin)==0 → native 返回 undefined →
// {path:[], ops:0, cost:0, incomplete:false}（pf.cc:506-510 +
// driver/lib/path-finder.js:97-98）
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

// 空 goals：game 层早退形态（engine/src/game/path-finder.js:60-62）
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

// =====================================================================
// 4. CostMatrix 抬高某格后路径改变（E3N0，经 roomCallback 注入）
// =====================================================================
//
// 布置：origin (10,25) → goal (14,25)。矩阵把 (12,25) 抬到 3（其余 0 = 用地形）。
//
// 期望推导：
// - 无矩阵基线：直列 [(11,25),(12,25),(13,25),(14,25)]，cost 4（同用例 1 的
//   直线模式：astar 三方平局 (11,24)/(11,25)/(11,26) 中后插入的 (11,26) 先出堆，
//   其对角/横向 jump 均被更高 f 压制，(11,25) 的 jump_x 直达 goal 行）。
// - 有矩阵：(12,25) 代价 3 → 直列 cost = 1+3+1+1 = 6；绕行阶梯 cost 4
//   （对角步只计落格代价）。jps 在 (12,26) 处 jump_x 因 prev_cost_u =
//   look((12,25)) = 3 ≠ cost 触发横向中断（pf.cc:203-208），随后 (12,26) 的
//   直行 forced 规则（look((12,25)) ≠ cost，pf.cc:425-427）推入 (13,25)，
//   (13,25) 的 jump_x 首检 h==0 直达 (14,25) ⇒ 下侧阶梯
//   [(11,26),(12,26),(13,25),(14,25)]，cost 4。
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

// roomCallback 返回 false 封锁目标房间（pf.cc:49-52）——goal 房间被封锁后
// 初始方向可达的房间全部不可入，搜索返回 incomplete（maxOps 耗尽路径）。
// 本用例只验证「封锁后不再可达」的单房间形态：封锁唯一房间 →
// origin 房间也被封锁 → native 返回 -1 → incomplete:true（pf.cc:515-519 +
// driver/lib/path-finder.js:99-100）。
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

// =====================================================================
// 5. 对角线规则（E4N0）：允许贴角对角穿行（Screeps 语义，无角落限制）
// =====================================================================
//
// 布置：墙 (20,24) 与 (21,25)，origin (20,25)、goal (21,24)——两墙互为正交
// 相邻，goal 在其对角象限。多数网格寻路器禁止「穿角」，官方允许（对角步只计
// 落格代价与可通行性，pf.cc:183 look(neighbor)）。
//
// 期望推导：astar 推 (21,24)（TOP_RIGHT 方向）时 h=0 → f=1 全场最低，首轮
// 出堆即收束（无 jps 展开）⇒ 路径 [(21,24)]，cost 1，ops 0。
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

// 对角目标走纯对角线（E5N0，平原）：(10,10) → (13,13) ⇒ [(11,11),(12,12),(13,13)]
// cost 3。推导同用例 3（无 range）：对角邻居 f 唯一最低（h=2 → f=1+2=3 <
// 直向 f=1+2=3？——直向 (11,10) h=3 → f=1+3=4，对角 (11,11) h=2 → f=3 唯一低），
// jump_xy 扫到 (13,13) h=0 中断。
#[test]
fn diagonal_goal_straight_diagonal() {
    plain_room("E5N0");
    let result = do_search(pos("E5N0", 10, 10), &[Goal::pos(pos("E5N0", 13, 13))]);
    assert_eq!(path_xy(&result, "E5N0"), vec![(11, 11), (12, 12), (13, 13)]);
    assert_eq!(result.cost, 3);
}

// =====================================================================
// 6. plain/swamp 代价选项（E6N0，强制走廊）
// =====================================================================
//
// 布置：x=11 整列除 (11,10) 外全墙；(11,10) 为沼泽。origin (10,10)、
// goal (12,10)——墙列强制路径必经 (11,10)，最优路径唯一：
// [(11,10),(12,10)]。cost = swamp_cost + plain_cost（进 (11,10) 计沼泽、
// 进 (12,10) 计平原，pf.cc:106-123 look 按落格地形取 look_table）。
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

// =====================================================================
// 7. 跨房寻路（E7N0 + E7S0）：直线穿房境
// =====================================================================
//
// origin (25,47)@E7N0 → goal (25,2)@E7S0（E7N0 的 y=49 与 E7S0 的 y=0 相接）。
//
// 期望推导：Chebyshev 度量下最优路径**不唯一**——对角步与直步同价（均计落格
// 代价），凡单调向下的 5 步阶梯（dy=5、dx 净位移 0）cost 均 = 5；实际形状由
// 官方堆序裁决（astar 方向枚举 + bubble_up `<=` 平局规则，见用例 1/2 的机制
// 注释）。本用例锁定的阶梯形状记录自官方算法运行结果（由官方校验和测试
// tests/official_golden.rs 背书）。
// 关键断言：房境处**无跨界斜跳**——(23,49)@N0 → (23,0)@S0 为直落，对照
// astar 的 portal 过滤规则（pf.cc:168-180：边缘格的纵向邻居若 x 有偏移则丢弃）。
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

// =====================================================================
// 8. 目标不可达：incomplete 语义（W62N0）
// =====================================================================
//
// goal (30,30) 被一圈墙（Chebyshev 距离 1）围死；房间四周再围整圈墙（x/y 边界
// 全墙），使搜索无法越出已载入的房间（否则触达未载房间会走 js_error →
// undefined 映射，见用例 10）。堆耗尽后 min_node 为最接近目标者（h ≥ 2，墙外）
// ⇒ incomplete=true、路径非空且逐步合法、末端距 goal 的 Chebyshev 距离 == 2、
// cost == 路径逐步代价和（pf.cc:544-548,599）。
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

// =====================================================================
// 9. flee（E9N0）
// =====================================================================
//
// origin (25,25)、goal (25,25) range 10、flee=true：求距目标 ≥ 10 的位置
// （heuristic 的 flee 分支 pf.cc:127-135）。
//
// 期望推导：
// - astar 的 8 个邻居 f 全平局（f = 1 + trunc(9*1.2) = 11）；bubble_up 的 `<=`
//   使后插入者越过先插入者上浮（pf.h:322-330），第 8 个插入的 TOP_LEFT (24,24)
//   （方向序 pf.h:69）位于堆顶 → 首轮出堆。
// - jps((24,24))（dx=-1,dy=-1）push 三个节点：x 直跳 → (15,24)（h=0 中断，
//   pf.cc:196-199）；y 直跳 → (24,15)（同）；对角 jump_xy 因均匀场的正交跳
//   检查**立即中断**（pf.cc:271-276）只推 (23,23)（f=11）。
// - 两个 f=10 节点按 push 序入堆：(15,24) 先、(24,15) 后 → 后者经 `<=` 平局
//   上浮到堆顶 → 第二轮出堆，h=0 收束 ⇒ **正上方直列**，cost 10，ops 1
//   （仅 (24,24) 被展开）。
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

// =====================================================================
// 10. 目标方向房间缺地形（W60N0 已载、W59N0 未载）
// =====================================================================
//
// 官方行为：进入未载入地形的房间时 native 抛 js_error → 内部捕获 → 返回
// undefined → {path:[], ops:0, cost:0, incomplete:false}
// （pf.cc:31-35 抛出、pf.cc:563-567 捕获、driver/lib/path-finder.js:97-98 映射）。
// 注：真实服务器地形恒已载入，此用例锁定 JS bridge 映射边界的确定性。
#[test]
fn missing_terrain_returns_empty_not_incomplete() {
    plain_room("W60N0"); // W59N0（+x 方向邻房）不载入
    let result = do_search(pos("W60N0", 45, 25), &[Goal::pos(pos("W59N0", 25, 25))]);
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

// =====================================================================
// 11. 回调每房至多触发一次 + 地图坐标正确（W58N0）
// =====================================================================
//
// pf.cc:22-64：房间首次触达时经回调取矩阵（reverse_room_table 命中则不再调）。
// 单房搜索应恰好回调 1 次，参数为该房的地图坐标（parseRoomName 的 xx/yy）。
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
