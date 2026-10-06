//! 同名房不同地形的并发、跨世界嵌套与资源释放。
use kk_pathfinder::{
    Goal, RoomDecision, SearchOptions, SearchResult, TerrainSet, WorldPosition, search,
    search_with_room_callback,
};
use std::sync::Arc;

fn worlds() -> (Arc<TerrainSet>, Arc<TerrainSet>) {
    let plain = Arc::new(TerrainSet::new([("E0S0".into(), [0; 625])]).unwrap());
    let mut tiles = [[0; 50]; 50];
    for row in &mut tiles {
        row[25] = 1;
    }
    tiles[30][25] = 0;
    let detour =
        Arc::new(TerrainSet::new([("E0S0".into(), kk_pathfinder::pack_terrain(&tiles))]).unwrap());
    (plain, detour)
}
fn query(terrain: &TerrainSet) -> SearchResult {
    search(
        terrain,
        WorldPosition::from_room("E0S0", 10, 25).unwrap(),
        &[Goal::pos(WorldPosition::from_room("E0S0", 40, 25).unwrap())],
        &SearchOptions {
            max_rooms: 1,
            ..SearchOptions::default()
        },
    )
    .unwrap()
}
#[test]
fn concurrent_worlds_and_repeated_switches_preserve_every_result_field() {
    let (plain, detour) = worlds();
    let a = query(&plain);
    let b = query(&detour);
    assert_ne!(a.path, b.path);
    assert_eq!(a.cost, 30);
    assert_eq!(b.cost, 30);
    std::thread::scope(|scope| {
        for i in 0..4 {
            let (plain, detour, a, b) = (&plain, &detour, &a, &b);
            scope.spawn(move || {
                for n in 0..1000 {
                    let (terrain, expected) = if (n + i) % 2 == 0 {
                        (plain, a)
                    } else {
                        (detour, b)
                    };
                    assert_eq!(&query(terrain), expected);
                }
            });
        }
    });
}
fn nested(worlds: &[Arc<TerrainSet>; 2], depth: usize) -> SearchResult {
    let terrain = &worlds[depth % 2];
    search_with_room_callback(
        terrain,
        WorldPosition::from_room("E0S0", 10, 25).unwrap(),
        &[Goal::pos(WorldPosition::from_room("E0S0", 40, 25).unwrap())],
        &SearchOptions {
            max_rooms: 1,
            ..SearchOptions::default()
        },
        |_, _| {
            if depth < 5 {
                assert_eq!(nested(worlds, depth + 1), query(&worlds[(depth + 1) % 2]));
            }
            RoomDecision::None
        },
    )
    .unwrap()
}
#[test]
fn cross_world_nesting_beyond_pool_depth_and_terrain_release() {
    let (plain, detour) = worlds();
    let weak = Arc::downgrade(&detour);
    let worlds = [plain, detour];
    for _ in 0..50 {
        assert_eq!(nested(&worlds, 0), query(&worlds[0]));
    }
    drop(worlds);
    assert!(weak.upgrade().is_none(), "搜索池不得持有世界地形所有权");
}

#[test]
#[ignore = "独立 release 长窗口，避免其他验证干扰 RSS"]
fn terrain_replacement_rss_plateaus() {
    fn rss() -> u64 {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse::<u64>()
            .unwrap()
            * 1024
    }
    for _ in 0..1000 {
        let (a, b) = worlds();
        query(&a);
        query(&b);
    }
    let mut trend = vec![rss()];
    for _ in 0..5 {
        for _ in 0..10000 {
            let (a, b) = worlds();
            query(&a);
            query(&b);
        }
        trend.push(rss());
    }
    println!("TERRAIN_RSS_TREND={trend:?}");
    let growth = trend.iter().max().unwrap() - trend.iter().min().unwrap();
    assert!(
        growth < 16 * 1024 * 1024,
        "预热后 100000 次换地形不得持续积累旧缓冲：{trend:?}"
    );
}
