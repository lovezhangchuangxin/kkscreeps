//! G30.d 移动仲裁同功能默认参考：原生 check_impl 的仲裁吞吐微基准。
//!
//! 基准在 release 构建下有统计意义（`cargo test --release -p kk-engine
//! --test settle_tests movement_benchmark -- --ignored --nocapture`），输出每房仲裁
//! 耗时不作门禁，只作为 G01 前后与 JS 策略对照的同功能参考；debug
//! 构建仅跑通功能不断言时限。

use super::*;
use kk_engine::settle::movement;
use kk_world::{ObjId, RoomIndex, RoomState};

/// 构造 8 个竞争格 × 每格 4 竞争者的登记矩阵：每格从左上三个邻格与
/// 左侧一格同时挤入。
fn contested_movement(room: &mut RoomState) -> movement::Movement {
    let mut m = movement::Movement::new();
    let mut next = 1;
    for cell in 0..8u8 {
        let (gx, gy) = (10 + cell * 2, 15u8);
        for (dx, dy) in [(1i32, 1i32), (0, 1), (-1, 1), (1, 0)] {
            let (x, y) = ((gx as i32 - dx) as u8, (gy as i32 - dy) as u8);
            let id = ObjId(next);
            next += 1;
            room.objects.insert(
                id,
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x,
                    y,
                    fatigue: 0,
                    age_time: u64::MAX / 2,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: format!("c{cell}-{x}-{y}"),
                    owner: "p1".into(),
                    body: body(&[BodyKind::Move]),
                    store: kk_schema::room::Store {
                        energy: 0,
                        ghodium: 0,
                    },
                    store_extra: Vec::new(),
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            );
            m.add(id, (x, y), dx, dy);
        }
    }
    m
}

#[test]
#[ignore = "手动基准：cargo test --release -p kk-engine --test settle_tests movement_benchmark -- --ignored --nocapture"]
fn movement_benchmark_native_arbitration() {
    let mut room = RoomState::new("W1N1", terrain_with(&[]));
    let movement = contested_movement(&mut room);
    let index = RoomIndex::build(&room.objects);
    let old_fatigue = BTreeMap::new();
    let iterations = if cfg!(debug_assertions) { 10 } else { 2000 };
    // 预热与正确性检查不计入窗口。
    let mut probe = movement.clone();
    probe.check_impl(&room, &index, &old_fatigue, None, &BTreeMap::new());
    assert_eq!(probe.winners().len(), 8, "每个竞争格恰一胜者");
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let mut m = movement.clone();
        m.check_impl(&room, &index, &old_fatigue, None, &BTreeMap::new());
        debug_assert_eq!(m.winners().len(), 8);
    }
    let elapsed = start.elapsed();
    println!(
        "movement check_impl 默认参考：8 竞争格 ×4 竞争者 ×{iterations} 次 = {:.2} µs/房",
        elapsed.as_nanos() as f64 / iterations as f64 / 1000.0,
    );
}
