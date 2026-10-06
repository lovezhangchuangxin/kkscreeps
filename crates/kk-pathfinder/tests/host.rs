//! 显式宿主桥的取消/异常不能变成成功空路径，嵌套帧不能污染外层。
mod support;
use kk_pathfinder::{Error, Goal, HostRoomDecision, SearchHost, SearchOptions, WorldPosition};
use support::{load_terrain, search, search_with_host};

struct Host {
    cancel: bool,
    error: bool,
    polls: usize,
}
// SAFETY: 不返回矩阵或共享指针，调用只访问本栈帧的宿主。
unsafe impl SearchHost for Host {
    fn room(&mut self, _: u8, _: u8) -> HostRoomDecision {
        if self.error {
            HostRoomDecision::Error
        } else {
            HostRoomDecision::None
        }
    }
    fn is_cancelled(&mut self) -> bool {
        self.polls += 1;
        self.cancel
    }
}

#[test]
fn explicit_host_cancel_and_error_release_pool() {
    load_terrain("E80S80", &[0; 625]).unwrap();
    let origin = WorldPosition::from_room("E80S80", 5, 25).unwrap();
    let goals = [Goal::pos(
        WorldPosition::from_room("E80S80", 40, 25).unwrap(),
    )];
    let options = SearchOptions {
        max_rooms: 1,
        ..SearchOptions::default()
    };
    let mut host = Host {
        cancel: true,
        error: false,
        polls: 0,
    };
    assert_eq!(
        search_with_host(origin, &goals, &options, &mut host),
        Err(Error::Cancelled)
    );
    assert!(host.polls > 0);
    host.cancel = false;
    host.error = true;
    assert_eq!(
        search_with_host(origin, &goals, &options, &mut host),
        Err(Error::JSError(None))
    );
    host.error = false;
    let recovered = search_with_host(origin, &goals, &options, &mut host).unwrap();
    assert_eq!(recovered, search(origin, &goals, &options).unwrap());
}

struct NestedHost {
    origin: WorldPosition,
    goals: [Goal; 1],
    inner_cancelled: bool,
}
// SAFETY: 不返回矩阵；内层搜索有独立宿主，外层仅在内层返回后继续。
unsafe impl SearchHost for NestedHost {
    fn room(&mut self, _: u8, _: u8) -> HostRoomDecision {
        let mut host = Host {
            cancel: true,
            error: false,
            polls: 0,
        };
        self.inner_cancelled = search_with_host(
            self.origin,
            &self.goals,
            &SearchOptions::default(),
            &mut host,
        ) == Err(Error::Cancelled);
        HostRoomDecision::None
    }
}
#[test]
fn nested_cancel_restores_outer_native_frame() {
    load_terrain("E81S80", &[0; 625]).unwrap();
    let origin = WorldPosition::from_room("E81S80", 5, 25).unwrap();
    let goals = [Goal::pos(
        WorldPosition::from_room("E81S80", 40, 25).unwrap(),
    )];
    let mut host = NestedHost {
        origin,
        goals,
        inner_cancelled: false,
    };
    let result = search_with_host(
        origin,
        &goals,
        &SearchOptions {
            max_rooms: 1,
            ..SearchOptions::default()
        },
        &mut host,
    )
    .unwrap();
    assert!(host.inner_cancelled);
    assert!(!result.incomplete);
    assert_eq!(result.cost, 35);
}
