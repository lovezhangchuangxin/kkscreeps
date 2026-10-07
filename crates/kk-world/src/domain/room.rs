//! 房间与地形派生数值，JSON/KKPV 和查询快照共用。
use kk_core::constants as c;
use kk_schema::room::RoomObject;

/// 根据地形计算道路最大 hits，沼泽和墙系数分别相乘。
/// 两种投影共用此计算；不处理上游另有的同格 swamp、wall 对象。
/// 参考：engine/src/processor/intents/creeps/build.js 的 road 分支。
pub(crate) fn road_hits_max(terrain: &kk_core::Terrain, x: u8, y: u8) -> i64 {
    let mut hits = c::ROAD_HITS;
    if terrain.check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_SWAMP) {
        hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO;
    }
    if terrain.check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_WALL) {
        hits *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO;
    }
    hits
}

/// 按对象 ID 顺序读取首个控制器等级，无控制器时返回 0。
pub(crate) fn room_rcl(state: &crate::world::RoomState) -> u8 {
    state
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(ctl) => Some(ctl.level),
            _ => None,
        })
        .unwrap_or(0)
}

/// 汇总非 off 的 spawn、extension 能量与容量，有 viewer 时只计己方对象。
/// spawn 容量固定，extension 容量按房间 RCL 查表。结算所用的全房聚合不在此处。
/// 玩家统计参考：engine/src/game/game.js 的房间 energyAvailable 计算。
pub(crate) fn room_energy(state: &crate::world::RoomState, viewer: Option<&str>) -> (i64, i64) {
    let rcl = room_rcl(state);
    let mut available = 0i64;
    let mut capacity = 0i64;
    for o in state.objects.values() {
        let (energy, cap) = match o {
            RoomObject::Spawn(s) if !s.off => (s.store.energy as i64, c::SPAWN_ENERGY_CAPACITY),
            RoomObject::Extension(e) if !e.off => (
                e.store.energy as i64,
                crate::types::extension_energy_capacity(rcl),
            ),
            _ => continue,
        };
        if viewer.is_none_or(|v| crate::domain::object_owner(o).as_deref() == Some(v)) {
            available += energy;
            capacity += cap;
        }
    }
    (available, capacity)
}
