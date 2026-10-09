//! Keeper fake-runtime moveTo 的同房原生寻路与 50 tick 路径复用。
use kk_core::constants as c;
use kk_pathfinder::{CostMatrix, Goal, RoomDecision, SearchOptions, TerrainSet, WorldPosition};
use kk_schema::room::RoomObject;
use kk_world::{
    RoomState,
    domain::{
        keeper::OWNER,
        keeper_memory::{KeeperMemory, KeeperPath},
    },
};

use crate::settle::{SettleError, movement};

fn search(
    room: &RoomState,
    origin: (u8, u8),
    destination: (u8, u8),
) -> Result<Vec<[u8; 2]>, SettleError> {
    let tiles = std::array::from_fn(|y| std::array::from_fn(|x| room.terrain().get(x, y)));
    let terrain = TerrainSet::new([(room.name.clone(), kk_pathfinder::pack_terrain(&tiles))])
        .map_err(|e| SettleError(e.to_string()))?;
    let mut costs = CostMatrix::new();
    // 按稳定对象顺序复现官方矩阵写入（道路可覆盖先前写入的 255）。
    // 来源：common/fake-runtime.js 的 defaultCostMatrix；自家障碍工地、
    // 非公有他人 rampart 与 portal 也阻挡，生物（含自身）按官方默认入矩阵。
    for o in room.objects.values() {
        let kind = movement::type_name_of(o);
        let mut blocked = c::OBSTACLE_OBJECT_TYPES.contains(&kind.as_ref()) || kind == "portal";
        if let RoomObject::Generic(g) = o {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            blocked |= kind == "rampart"
                && !data["isPublic"].as_bool().unwrap_or(false)
                && data["owner"].as_str() != Some(OWNER);
        }
        if let RoomObject::ConstructionSite(site) = o {
            blocked |= site.owner.as_deref() == Some(OWNER)
                && c::OBSTACLE_OBJECT_TYPES.contains(&site.structure_type.as_str());
        }
        if blocked {
            let (x, y) = o.pos();
            costs.set(x as usize, y as usize, 255);
        }
        if kind == "swamp" {
            let (x, y) = o.pos();
            if costs.get(x as usize, y as usize) == Some(0) {
                costs.set(x as usize, y as usize, 10);
            }
        }
        if matches!(o, RoomObject::Road(_)) {
            let (x, y) = o.pos();
            // 官方 Uint8 矩阵的 255 仍小于 Infinity，因此道路无条件覆盖。
            costs.set(x as usize, y as usize, 1);
        }
    }
    let pos = |p: (u8, u8)| {
        WorldPosition::from_room(&room.name, p.0 as u32, p.1 as u32)
            .map_err(|e| SettleError(e.to_string()))
    };
    let options = SearchOptions {
        plain_cost: 2,
        swamp_cost: 10,
        max_rooms: 1,
        ..Default::default()
    };
    let result = kk_pathfinder::search_with_room_callback(
        &terrain,
        pos(origin)?,
        &[Goal::with_range(pos(destination)?, 1)],
        &options,
        |_, _| RoomDecision::CostMatrix(&costs),
    )
    .map_err(|e| SettleError(e.to_string()))?;
    // 官方 moveTo 保留 incomplete 搜索返回的局部路径，并不提前拒绝。
    Ok(result
        .path
        .into_iter()
        .map(|p| [p.x() as u8, p.y() as u8])
        .collect())
}

pub(super) fn direction(
    room: &RoomState,
    origin: (u8, u8),
    destination: (u8, u8),
    time: u64,
    memory: &mut KeeperMemory,
) -> Result<Option<u8>, SettleError> {
    let dest = [destination.0, destination.1];
    if memory.move_cache.as_ref().is_none_or(|cache| {
        cache.time == 0 || cache.destination != dest || time > cache.time.saturating_add(50)
    }) {
        memory.move_cache = Some(KeeperPath {
            destination: dest,
            time,
            path: search(room, origin, destination)?,
        });
    }
    let cache = memory.move_cache.as_ref().expect("路径已装入");
    let index = cache.path.iter().position(|p| *p == [origin.0, origin.1]);
    let next = match index {
        Some(i) => cache.path.get(i + 1),
        None => cache.path.first(),
    };
    let Some(next) = next else { return Ok(None) };
    let dx = i32::from(next[0]) - i32::from(origin.0);
    let dy = i32::from(next[1]) - i32::from(origin.1);
    if dx.abs().max(dy.abs()) > 1 {
        return Ok(None);
    }
    let direction = movement::direction_from_delta(dx, dy);
    Ok((direction != 0).then_some(direction))
}
