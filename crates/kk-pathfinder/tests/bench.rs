//! 简单基准（手动计时，不引 criterion）。运行方式：
//! `cargo test -p kk-pathfinder --release --test bench -- --ignored --nocapture`
//!
//! 场景对照官方 profile.js（driver/native/native/profile.js:62-103）：
//! 官方在同一硬件量级下用该场景做 PGO 校验（checksum 11843305）。

use std::time::Instant;

use kk_pathfinder::{Goal, SearchOptions, WorldPosition, load_terrain, search};

fn load_sample_terrain() {
    let data = include_bytes!("fixtures/sample-terrain.bin");
    let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    for ii in 0..count {
        let xx = data[4 + ii * 2];
        let yy = data[4 + ii * 2 + 1];
        let start = 4 + count * 2 + ii * 625;
        let bits: &[u8; 625] = data[start..start + 625].try_into().unwrap();
        load_terrain(&kk_pathfinder::generate_room_name(xx, yy), bits).unwrap();
    }
}

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

/// 单次 search 量级（短程）：同房直线 ~5 步。
#[test]
#[ignore]
fn bench_short_search() {
    load_sample_terrain();
    let origin = WorldPosition::from_room("W5N3", 20, 39).unwrap();
    let goal = Goal::pos(WorldPosition::from_room("W5N3", 25, 44).unwrap());
    search(origin, &[goal], &SearchOptions::default()).unwrap(); // 预热
    let rounds = 100_000;
    let start = Instant::now();
    for _ in 0..rounds {
        search(origin, &[goal], &SearchOptions::default()).unwrap();
    }
    let elapsed = start.elapsed();
    println!(
        "短程 search：{} 次 / {:.2?} = {:.2} µs/次",
        rounds,
        elapsed,
        elapsed.as_micros() as f64 / rounds as f64
    );
}

/// 单次 search 量级：真实地形跨房远征（W5N3 → W8N5，maxOps 100000）。
#[test]
#[ignore]
fn bench_single_search() {
    load_sample_terrain();
    let options = SearchOptions {
        max_ops: 100000,
        max_cost: 100000,
        ..SearchOptions::default()
    };
    let origin = WorldPosition::from_room("W5N3", 20, 39).unwrap();
    let goal = Goal::with_range(WorldPosition::from_room("W8N5", 16, 41).unwrap(), 0);
    // 预热（首搜含线程本地实例初始化）
    search(origin, &[goal], &options).unwrap();
    let rounds = 1000;
    let start = Instant::now();
    let mut total_ops = 0u64;
    for _ in 0..rounds {
        total_ops += u64::from(search(origin, &[goal], &options).unwrap().ops);
    }
    let elapsed = start.elapsed();
    println!(
        "单次跨房 search：{} 次 / {:.2?} = {:.1} µs/次（平均 ops/次 = {}）",
        rounds,
        elapsed,
        elapsed.as_micros() as f64 / rounds as f64,
        total_ops / rounds
    );
}

/// 官方 profile 场景（全对 × 5 轮，含 range 与缺省回调），报告总吞吐。
#[test]
#[ignore]
fn bench_profile_scenario() {
    load_sample_terrain();
    let positions: Vec<WorldPosition> = POSITIONS
        .iter()
        .map(|&(r, x, y)| WorldPosition::from_room(r, x, y).unwrap())
        .collect();
    let options = SearchOptions {
        max_ops: 100000,
        max_cost: 100000,
        ..SearchOptions::default()
    };
    search(positions[0], &[Goal::pos(positions[1])], &options).unwrap(); // 预热
    let start = Instant::now();
    let mut searches = 0u64;
    for _round in 0..5 {
        for ii in 0..positions.len() {
            for jj in 0..positions.len() {
                if ii == jj {
                    continue;
                }
                search(
                    positions[ii],
                    &[Goal::with_range(positions[jj], (ii % 3) as u32)],
                    &options,
                )
                .unwrap();
                searches += 1;
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "profile 场景：{} 次 / {:.2?} = {:.1} µs/次",
        searches,
        elapsed,
        elapsed.as_micros() as f64 / searches as f64
    );
}
