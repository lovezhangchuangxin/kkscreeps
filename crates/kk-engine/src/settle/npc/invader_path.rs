//! 普通 Invader 的 findClosestByPath、moveTo 与 flee；缓存随房间候选保存。
use super::path::{self, Options};
use crate::settle::{SettleError, movement};
use kk_world::{
    ObjId, RoomState,
    domain::{invader_memory::InvaderMemory, keeper_memory::KeeperPath, npc::INVADER_OWNER},
};

pub(super) fn range(a: (u8, u8), b: (u8, u8)) -> u8 {
    a.0.abs_diff(b.0).max(a.1.abs_diff(b.1))
}

pub(super) fn closest(
    room: &RoomState,
    origin: (u8, u8),
    targets: &[ObjId],
    opts: Options,
) -> Result<Option<ObjId>, SettleError> {
    if targets.is_empty() {
        return Ok(None);
    }
    if let Some(id) = targets.iter().find(|id| room.objects[id].pos() == origin) {
        return Ok(Some(*id));
    }
    let goals: Vec<_> = targets
        .iter()
        .map(|id| (room.objects[id].pos(), 1))
        .collect();
    let route = path::search(room, INVADER_OWNER, origin, &goals, opts)?;
    let end = route.last().map(|p| (p[0], p[1])).unwrap_or(origin);
    // 官方即使 incomplete 也按路径末格扫描，最后一个符合范围的对象覆盖。
    Ok(targets
        .iter()
        .rev()
        .find(|id| range(end, room.objects[id].pos()) <= 1)
        .copied())
}

pub(super) fn move_to(
    room: &RoomState,
    origin: (u8, u8),
    destination: (u8, u8),
    range_to: u32,
    opts: Options,
    time: u64,
    memory: &mut InvaderMemory,
) -> Result<Option<u8>, SettleError> {
    if u32::from(range(origin, destination)) <= range_to {
        return Ok(None);
    }
    let dest = [destination.0, destination.1];
    if memory.move_cache.as_ref().is_none_or(|cache| {
        cache.time == 0 || cache.destination != dest || time > cache.time.saturating_add(5)
    }) {
        memory.move_cache = Some(KeeperPath {
            destination: dest,
            time,
            path: path::search(
                room,
                INVADER_OWNER,
                origin,
                &[(destination, range_to)],
                opts,
            )?,
        });
    }
    let route = &memory.move_cache.as_ref().expect("缓存已写入").path;
    let index = route.iter().position(|p| *p == [origin.0, origin.1]);
    let next = match index {
        Some(i) => route.get(i + 1),
        None => route.first(),
    };
    let Some(next) = next else { return Ok(None) };
    if range(origin, (next[0], next[1])) > 1 {
        return Ok(None);
    }
    let direction = movement::direction_from_delta(
        i32::from(next[0]) - i32::from(origin.0),
        i32::from(next[1]) - i32::from(origin.1),
    );
    if direction == 0 {
        return Ok(None);
    }
    memory.last_move = Some(time);
    Ok(Some(direction))
}

pub(super) fn flee(
    room: &RoomState,
    origin: (u8, u8),
    targets: &[ObjId],
    range_to: u8,
) -> Result<Option<u8>, SettleError> {
    let goals: Vec<_> = targets
        .iter()
        .filter_map(|id| {
            let pos = room.objects[id].pos();
            (range(origin, pos) < range_to).then_some((pos, u32::from(range_to)))
        })
        .collect();
    if goals.is_empty() {
        return Ok(None);
    }
    let route = path::search(
        room,
        INVADER_OWNER,
        origin,
        &goals,
        Options {
            flee: true,
            ..Default::default()
        },
    )?;
    Ok(route
        .first()
        .map(|p| {
            movement::direction_from_delta(
                i32::from(p[0]) - i32::from(origin.0),
                i32::from(p[1]) - i32::from(origin.1),
            )
        })
        .filter(|d| *d != 0))
}
