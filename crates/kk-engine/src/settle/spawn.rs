//! 结算 spawn 动作、出生任务和免费能量，并按控制器配额标记可用能量结构。

use kk_core::constants as c;
use kk_schema::room::{CreepObj, RoomObject, SpawningJob};
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::Working;
use super::creep::{ACTION_HEALED, body_parts_kinds, set_action_log};
use super::movement::{self, offsets_by_direction};
use crate::intent::{IntentData, creep_cost};

/// 按控制器配额标记超额 spawn/extension 为 off，优先保留离控制器近的对象。
/// 等距时按 ObjId 顺序保留；这一标记阶段不按主人筛选。
pub fn calc_spawns(w: &mut Working) {
    let controller = w.controller().cloned();
    let rcl = controller.as_ref().map(|c| c.level).unwrap_or(0) as i64;
    let ctl_pos = controller.as_ref().map(|c| (c.x, c.y));

    let mut spawns: Vec<(ObjId, (u8, u8))> = Vec::new();
    let mut extensions: Vec<(ObjId, (u8, u8))> = Vec::new();
    for (id, obj) in &w.room.objects {
        match obj {
            RoomObject::Spawn(s) => spawns.push((*id, (s.x, s.y))),
            RoomObject::Extension(e) => extensions.push((*id, (e.x, e.y))),
            _ => {}
        }
    }
    mark_off(w, &mut spawns, "spawn", rcl, ctl_pos);
    mark_off(w, &mut extensions, "extension", rcl, ctl_pos);
}

fn allowed_count(w: &Working, structure_type: &str, rcl: i64) -> usize {
    super::structures::allowed_for(w, structure_type, rcl).max(0) as usize
}

fn mark_off(
    w: &mut Working,
    items: &mut [(ObjId, (u8, u8))],
    structure_type: &str,
    rcl: i64,
    ctl_pos: Option<(u8, u8)>,
) {
    let allowed = allowed_count(w, structure_type, rcl);
    if items.len() <= allowed {
        for (id, _) in items.iter() {
            set_off(w, *id, structure_type, false);
        }
        return;
    }
    let (cx, cy) = ctl_pos.unwrap_or((25, 25));
    items.sort_by(|a, b| {
        let da = chebyshev(a.1, (cx, cy));
        let db = chebyshev(b.1, (cx, cy));
        da.cmp(&db) // 稳定排序：平手保 id 序（BTreeMap 来源序）
    });
    for (index, (id, _)) in items.iter().enumerate() {
        set_off(w, *id, structure_type, index >= allowed);
    }
}

fn chebyshev(a: (u8, u8), b: (u8, u8)) -> i64 {
    (a.0 as i64 - b.0 as i64)
        .abs()
        .max((a.1 as i64 - b.1 as i64).abs())
}

fn set_off(w: &mut Working, id: ObjId, structure_type: &str, off: bool) {
    match (structure_type, w.room.objects.get_mut(&id)) {
        ("spawn", Some(RoomObject::Spawn(s))) => s.off = off,
        ("extension", Some(RoomObject::Extension(e))) => e.off = off,
        _ => {}
    }
}

/// 合计本房间非 off 的 spawn/extension 能量，供出生阶段的免费能量判断使用。
pub fn energy_available(w: &Working) -> i64 {
    w.room
        .objects
        .values()
        .filter(|o| match o {
            RoomObject::Spawn(s) => !s.off,
            RoomObject::Extension(e) => !e.off,
            _ => false,
        })
        .map(|o| match o {
            RoomObject::Spawn(s) => s.store.energy as i64,
            RoomObject::Extension(e) => e.store.energy as i64,
            _ => 0,
        })
        .sum()
}

/// 依次执行 create、renew、recycle、setSpawnDirections、cancelSpawning，
/// 后续动作会看到前面动作已经修改的状态。
pub fn process_spawn_intents(
    w: &mut Working,
    spawn_id: ObjId,
    actions: &std::collections::BTreeMap<crate::intent::Action, IntentData>,
) {
    use crate::intent::Action;
    if let Some(data) = actions.get(&Action::CreateCreep) {
        intent_create_creep(w, spawn_id, data);
    }
    if let Some(data) = actions.get(&Action::RenewCreep) {
        intent_renew_creep(w, spawn_id, data);
    }
    if let Some(data) = actions.get(&Action::RecycleCreep) {
        intent_recycle_creep(w, spawn_id, data);
    }
    if let Some(data) = actions.get(&Action::SetSpawnDirections) {
        intent_set_spawn_directions(w, spawn_id, data);
    }
    if actions.contains_key(&Action::CancelSpawning) {
        intent_cancel_spawning(w, spawn_id);
    }
}

/// 取身体前 50 件计算费用，先验证资金足额再扣款，创建同格孵化 creep 和任务。
/// 此处不检查名称唯一性；operateSpawn 在任务创建时缩短耗时，向上取整。
pub fn intent_create_creep(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    if super::nukes::blocks_spawning(&w.room, w.time) {
        return;
    }
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get(&spawn_id)
        && s.spawning.is_some()
    {
        return;
    }
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    if !structure_active_against_controller(w, spawn_id, &spawn) {
        return;
    }
    if data.directions.iter().any(|d| !(1..=8).contains(d)) {
        return;
    }
    let directions: Vec<u8> = data.directions.iter().map(|d| *d as u8).collect();
    let body: Vec<kk_schema::room::BodyPart> = data
        .body
        .iter()
        .take(c::MAX_CREEP_SIZE as usize)
        .map(|k| kk_schema::room::BodyPart {
            kind: k.code(),
            hits: 100,
            boost: None,
        })
        .collect();
    if body.is_empty() {
        return;
    }
    let cost = creep_cost(&data.body[..body.len()]);
    if !charge_energy(w, &spawn, cost, data.energy_structures.as_deref()) {
        return;
    }
    let mut need_time = c::CREEP_SPAWN_TIME * body.len() as i64;
    if let Some(effect) =
        kk_world::effects::active_power(w.room.effects.get(&spawn_id), c::PWR_OPERATE_SPAWN, w.time)
    {
        let multiplier =
            super::powers::power_effect_value(effect.power, effect.level).unwrap_or(1.0);
        need_time = (need_time as f64 * multiplier).ceil() as i64;
    }
    let name = data.name.clone().unwrap_or_default();
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&spawn_id) {
        s.spawning = Some(SpawningJob {
            name: name.clone(),
            need_time: need_time as u32,
            spawn_time: w.time + need_time as u64,
            directions,
        });
    }
    // 动作到此结束；分配错误由共享候选在发布前报告。
    let _ = w.insert_new(RoomObject::Creep(CreepObj {
        id: 0,
        x: spawn.x,
        y: spawn.y,
        fatigue: 0,
        age_time: 0,
        hits: (body.len() * 100) as u32,
        spawning: true,
        action_log: 0,
        name,
        owner: spawn.owner.clone(),
        body,
        store: Default::default(),
        store_extra: Vec::new(),
        say_message: None,
        say_public: None,
        notify_when_attacked: Some(true),
    }));
}

/// 为相邻同主的已出生 creep 延长寿命；spawn 必须空闲，身体中不能有 CLAIM。
/// 扣款后清除全部 boost，缩小 CARRY 容量时掉落超量资源。
pub fn intent_renew_creep(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    if spawn.spawning.is_some() {
        return;
    }
    let Some(target_id) = data.id else { return };
    let Some(RoomObject::Creep(target)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if target.owner != spawn.owner || target.spawning {
        return;
    }
    if (target.x as i32 - spawn.x as i32).abs() > 1 || (target.y as i32 - spawn.y as i32).abs() > 1
    {
        return;
    }
    if target
        .body
        .iter()
        .any(|p| BodyKind::from_code(p.kind) == Some(BodyKind::Claim))
    {
        return;
    }
    let effect = ((c::SPAWN_RENEW_RATIO * c::CREEP_LIFE_TIME as f64)
        / c::CREEP_SPAWN_TIME as f64
        / target.body.len() as f64)
        .floor() as i64;
    if target.age_time.saturating_add(effect.max(0) as u64) > w.time + c::CREEP_LIFE_TIME as u64 {
        return;
    }
    let cost = ((c::SPAWN_RENEW_RATIO * creep_cost(&body_parts_kinds(&target.body)) as f64)
        / c::CREEP_SPAWN_TIME as f64
        / target.body.len() as f64)
        .ceil() as i64;
    if !charge_energy(w, &spawn, cost, None) {
        return;
    }
    if let Some(creep) = w.creep_mut(target_id) {
        creep.age_time = creep.age_time.saturating_add(effect.max(0) as u64);
        set_action_log(&mut creep.action_log, ACTION_HEALED);
    }
    // 官方 renew-creep.js:42：目标写 healed，坐标=spawn。
    w.record_action(
        target_id,
        "healed",
        super::action_log::ActionLogPayload::Coords {
            x: spawn.x,
            y: spawn.y,
        },
    );
    if target.body.iter().any(|p| p.boost.is_some()) {
        if let Some(creep) = w.creep_mut(target_id) {
            for p in creep.body.iter_mut() {
                p.boost = None;
            }
        }
        super::damage::drop_energy_over_capacity(w, target_id);
    }
}

/// 回收相邻同主的已出生 creep；允许 spawn 正在孵化，也允许 CLAIM 身体。
pub fn intent_recycle_creep(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    let Some(target_id) = data.id else { return };
    let Some(RoomObject::Creep(target)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if target.owner != spawn.owner || target.spawning {
        return;
    }
    if (target.x as i32 - spawn.x as i32).abs() > 1 || (target.y as i32 - spawn.y as i32).abs() > 1
    {
        return;
    }
    super::creep::die_creep_with_rate(w, target_id, 1.0);
}

/// 只修改现有任务，方向列表必须非空且每项都在 1..=8。
pub fn intent_set_spawn_directions(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    if data.directions.is_empty() {
        return;
    }
    if data.directions.iter().any(|d| !(1..=8).contains(d)) {
        return;
    }
    let directions: Vec<u8> = data.directions.iter().map(|d| *d as u8).collect();
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&spawn_id)
        && let Some(job) = s.spawning.as_mut()
    {
        job.directions = directions;
    }
}

/// 移除同格、同名且仍在孵化的 creep 并清除任务，不创建墓碑或事件。
pub fn intent_cancel_spawning(w: &mut Working, spawn_id: ObjId) {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    let Some(job) = &spawn.spawning else {
        return;
    };
    let job = job.clone();
    let chick = w
        .room
        .objects
        .iter()
        .find(|(_, o)| match o {
            RoomObject::Creep(c) => {
                c.spawning && c.name == job.name && c.x == spawn.x && c.y == spawn.y
            }
            _ => false,
        })
        .map(|(id, _)| *id);
    if let Some(chick_id) = chick {
        w.remove_obj(chick_id);
    }
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&spawn_id) {
        s.spawning = None;
    }
}

/// 先检查所有候选能量总量，再按顺序扣款，资金不足时不扣任何能量。
/// 显式列表去重后按给定顺序使用；默认先使用按距离排序的 spawn，再使用 extension。
/// 两种结构分别排序，不按距离混排；Some 空列表与未指定列表的含义不同。
fn charge_energy(
    w: &mut Working,
    spawn: &kk_schema::room::SpawnObj,
    mut cost: i64,
    explicit: Option<&[ObjId]>,
) -> bool {
    let mut candidates: Vec<ObjId> = Vec::new();
    if let Some(explicit) = explicit {
        for id in explicit {
            let ok = match w.room.objects.get(id) {
                Some(RoomObject::Spawn(s)) => !s.off && s.owner == spawn.owner,
                Some(RoomObject::Extension(e)) => !e.off && e.owner == spawn.owner,
                _ => false,
            };
            if ok && !candidates.contains(id) {
                candidates.push(*id);
            }
        }
    } else {
        let mut spawns: Vec<(ObjId, (u8, u8))> = Vec::new();
        let mut extensions: Vec<(ObjId, (u8, u8))> = Vec::new();
        for (id, obj) in &w.room.objects {
            match obj {
                RoomObject::Spawn(s) if !s.off && s.owner == spawn.owner => {
                    spawns.push((*id, (s.x, s.y)))
                }
                RoomObject::Extension(e) if !e.off && e.owner == spawn.owner => {
                    extensions.push((*id, (e.x, e.y)))
                }
                _ => {}
            }
        }
        spawns.sort_by_key(|a| chebyshev(a.1, (spawn.x, spawn.y)));
        extensions.sort_by(|a, b| {
            chebyshev(a.1, (spawn.x, spawn.y)).cmp(&chebyshev(b.1, (spawn.x, spawn.y)))
        });
        candidates = spawns
            .into_iter()
            .chain(extensions)
            .map(|(id, _)| id)
            .collect();
    }
    let available: i64 = candidates
        .iter()
        .filter_map(|id| match w.room.objects.get(id) {
            Some(RoomObject::Spawn(s)) => Some(s.store.energy as i64),
            Some(RoomObject::Extension(e)) => Some(e.store.energy as i64),
            _ => None,
        })
        .sum();
    if available < cost {
        return false;
    }
    for id in candidates {
        if cost <= 0 {
            break;
        }
        let held = match w.room.objects.get(&id) {
            Some(RoomObject::Spawn(s)) => s.store.energy as i64,
            Some(RoomObject::Extension(e)) => e.store.energy as i64,
            _ => continue,
        };
        let take = held.min(cost);
        if take > 0 {
            match w.room.objects.get_mut(&id) {
                Some(RoomObject::Spawn(s)) => s.store.energy -= take as u32,
                Some(RoomObject::Extension(e)) => e.store.energy -= take as u32,
                _ => {}
            }
            cost -= take;
        }
    }
    true
}

fn structure_active_against_controller(
    w: &Working,
    spawn_id: ObjId,
    spawn: &kk_schema::room::SpawnObj,
) -> bool {
    if spawn.owner.is_empty() {
        return true;
    }
    let Some(ctl) = w.controller() else {
        return false;
    };
    if ctl.level < 1 || ctl.owner.as_deref() != Some(spawn.owner.as_str()) {
        return false;
    }
    let mut allowed = allowed_count(w, "spawn", i64::from(ctl.level));
    if allowed == 0 {
        return false;
    }
    let object_dist = chebyshev((spawn.x, spawn.y), (ctl.x, ctl.y));
    let mut found_self = false;
    for (id, obj) in &w.room.objects {
        let RoomObject::Spawn(s) = obj else { continue };
        if s.owner != spawn.owner {
            continue;
        }
        let dist = chebyshev((s.x, s.y), (ctl.x, ctl.y));
        if dist < object_dist {
            allowed -= 1;
            if allowed == 0 {
                return false;
            }
        } else if !found_self && dist == object_dist {
            if *id == spawn_id {
                found_self = true;
            } else {
                allowed -= 1;
                if allowed == 0 {
                    return false;
                }
            }
        }
    }
    true
}

/// 任务到 spawn_time-1 尝试出生，失败则推迟到下一 tick 重试。
/// 免费能量判断使用全房动作结算后的固定合计值，不随逐个 spawn 增加能量而更新。
pub fn spawn_tick(
    w: &mut Working,
    id: ObjId,
    energy_available: i64,
    initial_creeps: &std::collections::BTreeSet<ObjId>,
) {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&id) else {
        return;
    };
    let disrupted = spawn.spawning.is_some()
        && kk_world::effects::active_power(w.room.effects.get(&id), c::PWR_DISRUPT_SPAWN, w.time)
            .is_some();
    if disrupted {
        if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id)
            && let Some(job) = &mut s.spawning
        {
            job.spawn_time += 1;
        }
    } else if let Some(job) = &spawn.spawning
        && w.time >= job.spawn_time.saturating_sub(1)
    {
        let job = job.clone();
        // 官方 bulk.insert 的新 creep 本轮不在 roomObjects：一拍任务先清
        // spawning，下一轮 creep tick 才走默认方向的孤儿出生路径。已有
        // creep（包括曾被 disrupt 延期的一拍任务）仍使用任务指定方向。
        // 来源：spawns/tick.js 与 creeps/tick.js。
        let newly_inserted = w.room.objects.iter().any(|(id, object)| {
            matches!(object, RoomObject::Creep(c)
                if c.spawning && c.name == job.name && c.x == spawn.x && c.y == spawn.y)
                && !initial_creeps.contains(id)
        });
        let born = newly_inserted || born_creep(w, id, &job.name, &job.directions);
        if born {
            if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id) {
                s.spawning = None;
            }
        } else if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id) {
            s.spawning = Some(SpawningJob {
                spawn_time: 1 + w.time,
                ..job
            });
        }
    }
    let (store_energy, rcl_ok) = match w.room.objects.get(&id) {
        Some(RoomObject::Spawn(s)) => {
            let rcl_ok = w
                .controller()
                .is_some_and(|ctl| ctl.level >= 1 && ctl.owner.as_deref() == Some(&s.owner));
            (s.store.energy, rcl_ok)
        }
        _ => return,
    };
    let not_off = w
        .room
        .objects
        .get(&id)
        .is_some_and(|o| matches!(o, RoomObject::Spawn(s) if !s.off));
    if not_off
        && rcl_ok
        && energy_available < c::SPAWN_ENERGY_CAPACITY
        && store_energy < c::SPAWN_ENERGY_CAPACITY as u32
        && let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id)
    {
        s.store.energy += 1;
    }
}

/// 按指定方向寻找出生格；未指定时按 1..=8。缺失对应孵化 creep 时返回成功，
/// 使调用方清除任务。指定方向没有空位但其他方向有出口时延期，只有完全被围
/// 且指定方向存在敌方 creep 时才踩死第一个敌人。
/// 来源：上游 engine/src/processor/intents/spawns/_born-creep.js。
pub(crate) fn born_creep(
    w: &mut Working,
    spawn_id: ObjId,
    chick_name: &str,
    job_directions: &[u8],
) -> bool {
    let Some((host, owner)) = w.room.objects.get(&spawn_id).and_then(|o| match o {
        RoomObject::Spawn(s) => Some(((s.x, s.y), s.owner.clone())),
        _ => None,
    }) else {
        return false;
    };
    born_creep_at(w, host, &owner, chick_name, job_directions)
}

/// 通用孵化出生：以宿主坐标与主人为中心找首个可落格（invaderCore 的
/// 孵化出生与 spawn 共用官方 _born-creep 的落格/踩踏语义）。
pub(crate) fn born_creep_at(
    w: &mut Working,
    host: (u8, u8),
    owner: &str,
    chick_name: &str,
    job_directions: &[u8],
) -> bool {
    let chick = w
        .room
        .objects
        .iter()
        .find(|(_, o)| match o {
            RoomObject::Creep(c) => {
                c.spawning && c.name == chick_name && c.x == host.0 && c.y == host.1
            }
            _ => false,
        })
        .map(|(id, _)| *id);
    let Some(chick_id) = chick else {
        return true;
    };
    let mut directions: Vec<u8> = if job_directions.is_empty() {
        (1..=8).collect()
    } else {
        job_directions.to_vec()
    };
    let mut occupied_at: Option<(u8, u8)> = None;
    let mut hostile: Option<ObjId> = None;
    for &d in &directions {
        let Some((dx, dy)) = offsets_by_direction(d) else {
            continue;
        };
        let nx = host.0 as i32 + dx;
        let ny = host.1 as i32 + dy;
        if !(0..=49).contains(&nx) || !(0..=49).contains(&ny) {
            continue;
        }
        let (nx, ny) = (nx as u8, ny as u8);
        // 出生检查把任何 creep 和任何主人的障碍工地视为占位，
        // 与普通移动仅检查自家障碍工地的规则不同；路可以覆盖墙地形。
        let static_obstacle =
            w.index
                .at(nx as usize, ny as usize)
                .iter()
                .any(|tid| match w.room.objects.get(tid) {
                    Some(RoomObject::ConstructionSite(s)) => {
                        super::buildings::obstacle_at(w, &s.structure_type)
                    }
                    Some(o) => super::buildings::obstacle_at(w, &movement::type_name_of(o)),
                    None => false,
                })
                || (w.room.terrain().check_mask(
                    nx as usize,
                    ny as usize,
                    kk_core::TERRAIN_MASK_WALL,
                ) && !movement::has_road(&w.room, &w.index, (nx, ny)));
        let busy = w.movement.is_tile_busy(nx, ny);
        let is_occupied = static_obstacle || busy;
        if !is_occupied {
            occupied_at = Some((nx, ny));
            break;
        }
        if hostile.is_none() {
            hostile = w.index.at(nx as usize, ny as usize).iter().find_map(|tid| {
                match w.room.objects.get(tid) {
                    Some(RoomObject::Creep(c)) if c.owner != owner => Some(*tid),
                    _ => None,
                }
            });
        }
    }
    if let Some((nx, ny)) = occupied_at {
        if let Some(c) = w.creep_mut(chick_id) {
            c.spawning = false;
        }
        w.move_obj(chick_id, (nx, ny))
            .expect("出生对象是 creep（born_creep 按 Creep 变体筛出）");
        return true;
    }
    let other: Vec<u8> = (1..=8).filter(|d| !directions.contains(d)).collect();
    directions.extend(other);
    for d in directions {
        let Some((dx, dy)) = offsets_by_direction(d) else {
            continue;
        };
        let nx = host.0 as i32 + dx;
        let ny = host.1 as i32 + dy;
        if !(0..=49).contains(&nx) || !(0..=49).contains(&ny) {
            continue;
        }
        let (nx, ny) = (nx as u8, ny as u8);
        // 检查其他出口时直接拒绝墙地形，不使用前一轮的道路例外。
        let free =
            !w.index
                .at(nx as usize, ny as usize)
                .iter()
                .any(|tid| match w.room.objects.get(tid) {
                    Some(RoomObject::ConstructionSite(s)) => {
                        super::buildings::obstacle_at(w, &s.structure_type)
                    }
                    Some(o) => super::buildings::obstacle_at(w, &movement::type_name_of(o)),
                    None => false,
                })
                && !w.room.terrain().check_mask(
                    nx as usize,
                    ny as usize,
                    kk_core::TERRAIN_MASK_WALL,
                )
                && !w.movement.is_tile_busy(nx, ny);
        if free {
            return false; // 有出口但我们没选它：不 spawnstomp（官方语义）
        }
    }
    if let Some(hostile_id) = hostile {
        let (hx, hy) = w.creep(hostile_id).map(|c| (c.x, c.y)).unwrap_or((0, 0));
        super::creep::die_creep(w, hostile_id);
        if let Some(c) = w.creep_mut(chick_id) {
            c.spawning = false;
        }
        w.move_obj(chick_id, (hx, hy))
            .expect("出生对象是 creep（born_creep 按 Creep 变体筛出）");
        return true;
    }
    false
}
