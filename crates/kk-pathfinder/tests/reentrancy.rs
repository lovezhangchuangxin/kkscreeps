use kk_pathfinder::{
    Error, Goal, RoomDecision, SearchOptions, TERRAIN_PLAIN, WorldPosition, load_terrain,
    pack_terrain, search, search_with_room_callback,
};

fn pos(room: &str, x: u32, y: u32) -> WorldPosition {
    WorldPosition::from_room(room, x, y).unwrap()
}

#[test]
fn handled_inner_errors_do_not_abort_outer_search() {
    for room in ["W50N0", "W49N0"] {
        load_terrain(room, &pack_terrain(&[[TERRAIN_PLAIN; 50]; 50])).unwrap();
    }
    let origin = pos("W50N0", 10, 10);
    let goals = [Goal::pos(pos("W50N0", 20, 20))];
    let options = SearchOptions::default();
    let baseline = search(origin, &goals, &options).unwrap();

    for panic_in_callback in [false, true] {
        let mut calls = 0;
        let result = search_with_room_callback(origin, &goals, &options, |_, _| {
            calls += 1;
            let inner = if panic_in_callback {
                search_with_room_callback(
                    pos("W49N0", 10, 10),
                    &[Goal::pos(pos("W49N0", 20, 20))],
                    &options,
                    |_, _| panic!("inner callback"),
                )
            } else {
                // W48N0 不载入：内层搜索会返回缺少地形的错误。
                search(
                    pos("W49N0", 45, 25),
                    &[Goal::pos(pos("W48N0", 10, 25))],
                    &options,
                )
            };
            assert!(matches!(inner, Err(Error::JSError(_))));
            // 回调已处理内层错误，外层应继续使用地形搜索。
            RoomDecision::None
        });
        assert_eq!(result.unwrap(), baseline);
        assert_eq!(calls, 1);
    }
}

#[test]
fn recursion_can_exceed_the_two_cached_instances() {
    load_terrain("W47N0", &pack_terrain(&[[TERRAIN_PLAIN; 50]; 50])).unwrap();
    fn nested(depth: usize) {
        let result = search_with_room_callback(
            pos("W47N0", 10, 10),
            &[Goal::pos(pos("W47N0", 20, 20))],
            &SearchOptions::default(),
            |_, _| {
                if depth > 0 {
                    nested(depth - 1);
                }
                RoomDecision::None
            },
        )
        .unwrap();
        assert!(!result.incomplete);
        assert_eq!(result.path.last(), Some(&pos("W47N0", 20, 20)));
    }
    nested(3);
}
