//! 重放上游 driver/native/profile.js 的 24 个位置，两两搜索五轮并比较校验和。
//! 来源版本见 src/lib.rs；fixtures/sample-terrain.bin 含 143 个房间的压缩地形。
//! 校验和累计路径长度和 ops，只检查聚合量，不能发现等长异形路径的变化。
//!
//! profile.js 构造矩阵时将 costMatrix 变量赋为标量，而非更新数组元素，因此回调
//! 最终等价于无矩阵；这里保留该实际行为以重现 11843305，不修正上游测试意图。
//! origin_blocked_pair 为真时仍封锁房间，与原脚本的 false 返回一致。

use kk_pathfinder::{
    Goal, RoomDecision, SearchOptions, TerrainSet, WorldPosition, search_with_room_callback,
};

/// sample-terrain.bin：[u32 LE 房间数] + [每房 u8 xx, u8 yy] + [每房 625 字节]。
fn load_sample_terrain() -> TerrainSet {
    let mut rooms = Vec::new();
    let data = include_bytes!("fixtures/sample-terrain.bin");
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    assert_eq!(data.len(), 4 + count * 2 + count * 625);
    for ii in 0..count {
        let xx = data[4 + ii * 2];
        let yy = data[4 + ii * 2 + 1];
        let start = 4 + count * 2 + ii * 625;
        let bits: &[u8; 625] = data[start..start + 625].try_into().unwrap();
        rooms.push((kk_pathfinder::generate_room_name(xx, yy), *bits));
    }
    TerrainSet::new(rooms).unwrap()
}

// driver/native/profile.js 的位置列表，顺序影响 range 和封锁条件。
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
    let terrain = load_sample_terrain();
    let positions: Vec<WorldPosition> = POSITIONS
        .iter()
        .map(|&(room, x, y)| WorldPosition::from_room(room, x, y).unwrap())
        .collect();
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
                let result = search_with_room_callback(
                    &terrain,
                    positions[ii],
                    &goals,
                    &options,
                    |_xx, _yy| {
                        if origin_blocked_pair {
                            RoomDecision::Blocked
                        } else {
                            // 官方脚本笔误后回调返回标量 → 等价无矩阵
                            RoomDecision::None
                        }
                    },
                )
                .unwrap();
                checksum += u64::from(result.path.len() as u32) + u64::from(result.ops);
            }
        }
    }
    assert_eq!(checksum, 11843305, "与官方 profile.js 校验和不一致");
}
