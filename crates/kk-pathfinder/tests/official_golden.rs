//! 金标测试：用官方仓库的基准素材锁「与官方逐点一致」。
//!
//! 素材：`driver/native/sample-terrain.js`（143 个真实房间的地形）+
//! `driver/native/profile.js` 的全对搜索场景与其校验和 **11843305**
//! （profile.js:87-98，官方以此校验路径长度与 ops 的总量——任何 tie-breaking
//! 或代价计算的偏差都会改变该和）。
//!
//! 场景逐点对照 profile.js：
//! - 21 个出发点（profile.js:62-85 的 RoomPosition 列表）两两互搜（跳过自身），
//!   重复 5 轮；
//! - `range = ii % 3`（ii 为出发点下标）；选项 `1, 5, 16, 100000, 100000, 0,
//!   1.2`（profile.js:75，即 plain=1/swamp=5/maxRooms=16/maxOps=100000/
//!   maxCost=100000/flee=false/hw=1.2）；
//! - 回调语义复刻 profile.js:86-89——注意官方脚本有一处笔误
//!   `costMatrix = ii % 11`（应为 `costMatrix[ii] = ...`，profile.js:53-58），
//!   导致回调实际返回标量 2 而非 TypedArray：native 侧 TypedArrayContents
//!   长度检查（pf.cc:55）不过 → 等价于「无矩阵」。本测试用 `None` 复刻该行为
//!   （`false` 封锁分支保留：`jj % 5 == 0 && ii % 2 == 0` 时 `Blocked`）；
//! - checksum += path.len() + ops（profile.js:94-97，ret 恒真值对象；native -1
//!   时 path/ops 按 0 计——driver 映射后同为 0）。

use kk_pathfinder::{
    Goal, RoomDecision, SearchOptions, WorldPosition, load_terrain, search_with_room_callback,
};

/// sample-terrain.bin：[u32 LE 房间数] + [每房 u8 xx, u8 yy] + [每房 625 字节]。
fn load_sample_terrain() {
    let data = include_bytes!("fixtures/sample-terrain.bin");
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    assert_eq!(data.len(), 4 + count * 2 + count * 625);
    for ii in 0..count {
        let xx = data[4 + ii * 2];
        let yy = data[4 + ii * 2 + 1];
        let start = 4 + count * 2 + ii * 625;
        let bits: &[u8; 625] = data[start..start + 625].try_into().unwrap();
        load_terrain(&kk_pathfinder::generate_room_name(xx, yy), bits).unwrap();
    }
}

/// profile.js:62-85 的 21 个位置（roomName, x, y）。
const POSITIONS: &[(&str, u32, u32)] = &[
    ("W5N3", 20, 39),
    ("W5N4", 31, 20),
    ("W6N5", 15, 30),
    ("W7N5", 14, 18),
    ("W8N5", 16, 41),
    ("W6N2", 35, 7),
    ("W5N2", 3, 25),
    ("W4N1", 4, 40),
    ("W3N1", 11, 36),
    ("W2N2", 33, 29),
    ("W3N3", 45, 5),
    ("W4N4", 31, 12),
    ("W5N5", 1, 27),
    ("W6N6", 17, 14),
    ("W7N7", 22, 14),
    ("W8N8", 21, 20),
    ("W9N8", 25, 33),
    ("W10N10", 29, 21),
    ("W1N1", 32, 26),
    ("W2N2", 32, 33),
    ("W3N8", 41, 35),
    ("W3N7", 20, 34),
    ("W4N8", 44, 33),
    ("W3N7", 44, 32),
];

#[test]
fn official_profile_checksum() {
    load_sample_terrain();
    let positions: Vec<WorldPosition> = POSITIONS
        .iter()
        .map(|&(room, x, y)| WorldPosition::from_room(room, x, y).unwrap())
        .collect();
    // profile.js:75：plain=1, swamp=5, maxRooms=16, maxOps=100000, maxCost=100000,
    // flee=false, heuristicWeight=1.2
    let options = SearchOptions {
        max_ops: 100000,
        max_cost: 100000,
        ..SearchOptions::default()
    };
    let mut checksum: u64 = 0;
    for _round in 0..5 {
        for ii in 0..positions.len() {
            for jj in 0..positions.len() {
                if ii == jj {
                    continue;
                }
                let goals = [Goal::with_range(positions[jj], (ii % 3) as u32)];
                let origin_blocked_pair = jj % 5 == 0 && ii % 2 == 0;
                let result =
                    search_with_room_callback(positions[ii], &goals, &options, |_xx, _yy| {
                        if origin_blocked_pair {
                            RoomDecision::Blocked
                        } else {
                            // 官方脚本笔误后回调返回标量 → 等价无矩阵
                            RoomDecision::None
                        }
                    })
                    .unwrap();
                checksum += u64::from(result.path.len() as u32) + u64::from(result.ops);
            }
        }
    }
    // profile.js:96-99：官方校验和
    assert_eq!(checksum, 11843305, "与官方 profile.js 校验和不一致");
}
