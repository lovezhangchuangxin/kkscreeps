//! 官方布防模板 bunker2 的静态结构表；聚合入口见本目录 mod.rs。
//! 来源：上游 common/lib/strongholds.js 的 'bunker2'（剥离核心条目，
//! 核心对象由放置归一建好、部署不重插）。
use super::super::stronghold::{StrongholdEntry, StrongholdTemplate};

pub(super) const TEMPLATE: StrongholdTemplate = StrongholdTemplate {
    name: "bunker2",
    reward_level: 2,
    core_level: 2,
    structures: &[
        StrongholdEntry {
            kind: "rampart",
            dx: 0,
            dy: 0,
        },
        StrongholdEntry {
            kind: "tower",
            dx: 1,
            dy: 1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: 1,
            dy: 1,
        },
        StrongholdEntry {
            kind: "tower",
            dx: -1,
            dy: -1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: -1,
            dy: -1,
        },
        StrongholdEntry {
            kind: "road",
            dx: 0,
            dy: -1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: 0,
            dy: -1,
        },
        StrongholdEntry {
            kind: "road",
            dx: 1,
            dy: -1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: 1,
            dy: -1,
        },
        StrongholdEntry {
            kind: "road",
            dx: -1,
            dy: 1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: -1,
            dy: 1,
        },
        StrongholdEntry {
            kind: "road",
            dx: 0,
            dy: 1,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: 0,
            dy: 1,
        },
        StrongholdEntry {
            kind: "container",
            dx: 1,
            dy: 0,
        },
        StrongholdEntry {
            kind: "road",
            dx: 1,
            dy: 0,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: 1,
            dy: 0,
        },
        StrongholdEntry {
            kind: "container",
            dx: -1,
            dy: 0,
        },
        StrongholdEntry {
            kind: "road",
            dx: -1,
            dy: 0,
        },
        StrongholdEntry {
            kind: "rampart",
            dx: -1,
            dy: 0,
        },
    ],
};
