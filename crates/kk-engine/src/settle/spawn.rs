//! 孵化链路：_calc_spawns、能量扣费（_charge-energy）、createCreep、
//! spawn tick 与出生（spawns/tick.js + _born-creep.js）。

use kk_core::constants as c;
use kk_schema::room::{CreepObj, RoomObject, SpawningJob};
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::Working;
use super::creep::{ACTION_HEALED, body_parts_kinds, set_action_log};
use super::movement::{self, offsets_by_direction};
use crate::intent::{IntentData, creep_cost};

/// 官方 _calc_spawns.js：spawn/extension 超出 RCL 配额时，距 controller 最远者
/// 标 off（Chebyshev 距离，稳定排序保序）。
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

fn allowed_count(structure_type: &str, rcl: i64) -> usize {
    super::controller_allowed(structure_type, rcl).max(0) as usize
}

fn mark_off(
    w: &mut Working,
    items: &mut [(ObjId, (u8, u8))],
    structure_type: &str,
    rcl: i64,
    ctl_pos: Option<(u8, u8)>,
) {
    let allowed = allowed_count(structure_type, rcl);
    if items.len() <= allowed {
        for (id, _) in items.iter() {
            set_off(w, *id, structure_type, false);
        }
        return;
    }
    // 官方：按距 controller 升序保留前 allowed（comparatorDistance = Chebyshev）
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

/// 官方 processor.js:326：非 off 的 spawn/extension 存量合计。
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

/// 官方 spawns/intents.js:3-24 的分派序：createCreep → renewCreep →
/// recycleCreep → setSpawnDirections → cancelSpawning（同对象可并存多个
/// 动作——前者改状态后后者按新状态裁决，如 createCreep 置 spawning 后
/// renewCreep 的 object.spawning 门拒）。
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

/// 官方 spawns/intents.js → create-creep.js：校验、扣费、建孵化任务与雏形 creep。
pub fn intent_create_creep(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    // 孵化中拒（create-creep.js:14-16）
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get(&spawn_id)
        && s.spawning.is_some()
    {
        return;
    }
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    // controller 活跃校验（create-creep.js:17 → utils.js:456-506）：owner-less
    // 恒活；需 controller level≥1 且归本人，超配额时按距 controller 远近判停用
    if !structure_active_against_controller(w, spawn_id, &spawn) {
        return;
    }
    // directions 校验（官方 create-creep.js:27-33：parseInt→uniq 后任一越界或
    // 非整数 ⇒ 整单拒绝，而非过滤保留）；sanitize 已做 parseInt 截断与保序去重，
    // 无法解析的元素以 i64::MIN 哨兵传递——天然落入越界分支
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
    // 扣费（_charge-energy：显式 energyStructures 按序；缺省全房间 spawn→extension）
    if !charge_energy(w, &spawn, cost, data.energy_structures.as_deref()) {
        return;
    }
    let need_time = c::CREEP_SPAWN_TIME * body.len() as i64;
    let name = data.name.clone().unwrap_or_default();
    if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&spawn_id) {
        s.spawning = Some(SpawningJob {
            name: name.clone(),
            need_time: need_time as u32,
            spawn_time: w.time + need_time as u64,
            directions,
        });
    }
    // 雏形 creep（create-creep.js:80-98）：占位于 spawn 格、spawning=true。
    // 官方**不带 ageTime**——孵化中 ticksToLive 读 undefined（creeps.js:78 的
    // `o.ageTime ? … : undefined`），寿命在出生后首个 creep tick 赋值
    // （creeps/tick.js:88-91；CREEP-SPAWNING-002 锁值）；0 即官方 falsy 未设位
    w.insert_new(RoomObject::Creep(CreepObj {
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
        // 雏形出生恒空仓（孵化中不可 transfer/harvest——无进矿路径）
        store_extra: Vec::new(),
    }));
}

/// 官方 spawns/renew-creep.js：结算层全额复核（不信任 API 层预检）——
/// 孵化中拒、同主非孵化 creep、Chebyshev ≤1 邻接、CLAIM 部件拒、满寿拒
/// （`ageTime + effect > gameTime + CREEP_LIFE_TIME`，effect = floor(
/// SPAWN_RENEW_RATIO·CREEP_LIFE_TIME/CREEP_SPAWN_TIME/body.length)）、
/// 全房扣费（_charge-energy 的 energyStructures=undefined → oldEnergyHandling）、
/// ageTime += effect + actionLog.healed；boost 部件清除后容量收缩、溢出掉落。
/// `stats.inc('energyCreeps')` 无 M1 统计面（跳过，同登记口径）。
pub fn intent_renew_creep(w: &mut Working, spawn_id: ObjId, data: &IntentData) {
    // 官方 :12-14：孵化中拒（先于 target 校验）
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return;
    };
    if spawn.spawning.is_some() {
        return;
    }
    // 官方 :16-21：目标存在、creep、同主、非孵化、邻接、无 CLAIM
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
    // 官方 :25-28：满寿拒（effect = floor(1.2·1500/3/len) = floor(600/len)）
    let effect = ((c::SPAWN_RENEW_RATIO * c::CREEP_LIFE_TIME as f64)
        / c::CREEP_SPAWN_TIME as f64
        / target.body.len() as f64)
        .floor() as i64;
    if target.age_time.saturating_add(effect.max(0) as u64) > w.time + c::CREEP_LIFE_TIME as u64 {
        return;
    }
    // 官方 :30-35：cost = ceil(SPAWN_RENEW_RATIO·calcCreepCost/CREEP_SPAWN_TIME/
    // body.length)，全房扣费（同主非 off 的 spawn→extension，距本 spawn 升序）
    let cost = ((c::SPAWN_RENEW_RATIO * creep_cost(&body_parts_kinds(&target.body)) as f64)
        / c::CREEP_SPAWN_TIME as f64
        / target.body.len() as f64)
        .ceil() as i64;
    if !charge_energy(w, &spawn, cost, None) {
        return;
    }
    // 官方 :42-43：actionLog.healed + bulk.inc(target, 'ageTime', effect)
    if let Some(creep) = w.creep_mut(target_id) {
        creep.age_time = creep.age_time.saturating_add(effect.max(0) as u64);
        set_action_log(&mut creep.action_log, ACTION_HEALED);
    }
    // 官方 :45-53：boost 部件清除（_recalc-body 容量收缩）+
    // _drop-resources-without-space（全资源序贯——复审 F4 清偿：原内联
    // energy-only 形态在 G/命名矿物槽下偏离，统一走 damage 的全资源版）
    if target.body.iter().any(|p| p.boost.is_some()) {
        if let Some(creep) = w.creep_mut(target_id) {
            for p in creep.body.iter_mut() {
                p.boost = None;
            }
        }
        super::damage::drop_energy_over_capacity(w, target_id);
    }
}

/// 官方 spawns/recycle-creep.js：目标复核（同主非孵化 creep + 邻接——
/// **无** CLAIM 条件、**无** object.spawning 门，与 renew 的差异面）后
/// `_die(target, 1.0, false)`——dropRate=1.0 的全额造价回收（B3 起墓碑承接
/// 链区分 TTL/暴亡/suicide 的 0.2 缺省）。
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
    // recycle-creep.js:23 的 dropRate=1.0
    super::creep::die_creep_with_rate(w, target_id, 1.0);
}

/// 官方 spawns/set-spawn-directions.js：孵化任务存在时整组替换 directions。
/// sanitize 已做 parseInt 截断与保序去重（number[] transform）；此处按官方
/// `_.any(越界/非整)` 复核后替换——任一非法元素整单忽略（非过滤）。
/// 空列表官方静默忽略（`directions.length > 0` 门）。
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

/// 官方 spawns/cancel-spawning.js：移除孵化中的雏形 creep（同名、spawn 格）+
/// 清孵化任务。官方 `_.find` 找不到时 `spawningCreep._id` 直接 TypeError
/// 崩 tick——雏形与任务由 create-creep 同笔创建、不变式内不可达，我方按
/// 防御形态跳过移除仅清任务（不崩 tick——D4 结算失败即整房回退的代价面，
/// 灰区登记）。注意官方是裸 bulk.remove（非 _die：无掉落无事件）。
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

/// 官方 _charge-energy.js：返回是否足额扣毕。
fn charge_energy(
    w: &mut Working,
    spawn: &kk_schema::room::SpawnObj,
    mut cost: i64,
    explicit: Option<&[ObjId]>,
) -> bool {
    // 候选结构（官方 _charge-energy + create-creep.js:19-24）：Some(列表) 走
    // 显式模式（按序、过滤合法——**空列表也是显式模式**，可用 0 → 失败）；
    // None 才是缺省全房间 spawn（距本 spawn 升序）→ extension
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

/// 官方 utils.checkStructureAgainstController（utils.js:456-506）：结构对其
/// controller 是否"活跃"。owner-less 恒活；否则需 controller level≥1 且归同主，
/// 超配额时同型同主对象按距 controller 近者保留（等距者按扫描序——我们的
/// id 升序序替换官方插入序，D4-1 灰区）。
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
    let mut allowed = allowed_count("spawn", ctl.level as i64);
    if allowed == 0 {
        return false;
    }
    // 该类型满额恒为 1 时无计数路径（官方 [8]===1 短路）；spawn 满额为 3、走距离计数
    if allowed_count("spawn", 8) == 1 {
        return true;
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

/// 官方 spawns/tick.js：孵化到点出生 + 免费能量涓流。
pub fn spawn_tick(w: &mut Working, id: ObjId, energy_available: i64) {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&id) else {
        return;
    };
    // 到点出生判定
    if let Some(job) = &spawn.spawning
        && w.time >= job.spawn_time.saturating_sub(1)
    {
        let job = job.clone();
        let born = born_creep(w, id, &job.name, &job.directions);
        if born {
            if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id) {
                s.spawning = None;
            }
        } else if let Some(RoomObject::Spawn(s)) = w.room.objects.get_mut(&id) {
            // 官方：出生失败推迟一 tick 重试（spawns/tick.js:23-26）
            s.spawning = Some(SpawningJob {
                spawn_time: 1 + w.time,
                ..job
            });
        }
    }
    // 免费能量涓流（spawns/tick.js:44-46）：房内可用不足时 spawn 每 tick +1
    let (store_energy, rcl_ok) = match w.room.objects.get(&id) {
        Some(RoomObject::Spawn(s)) => {
            let rcl_ok = w
                .controller()
                .is_some_and(|ctl| ctl.level >= 1 && ctl.owner.as_deref() == Some(&s.owner));
            (s.store.energy, rcl_ok)
        }
        _ => return,
    };
    // 官方涓流前置（spawns/tick.js:31-46）：spawn 不超额（!off）才涓流
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

/// 官方 _born-creep.js：八方向找空位出生；被围时踩死第一个敌 creep（spawnstomp）。
/// 入参取（雏形名, 出生方向序）而非整个任务——creep 侧出生路径
/// （creeps/tick.js:20-24：spawn 任务已清/换名时的对偶出生）任务可不存在。
pub(crate) fn born_creep(
    w: &mut Working,
    spawn_id: ObjId,
    chick_name: &str,
    job_directions: &[u8],
) -> bool {
    let Some(RoomObject::Spawn(spawn)) = w.room.objects.get(&spawn_id).cloned() else {
        return false;
    };
    // 找到对应雏形 creep（同名、同格、spawning）
    let chick = w
        .room
        .objects
        .iter()
        .find(|(_, o)| match o {
            RoomObject::Creep(c) => {
                c.spawning && c.name == chick_name && c.x == spawn.x && c.y == spawn.y
            }
            _ => false,
        })
        .map(|(id, _)| *id);
    let Some(chick_id) = chick else {
        // 雏形不存在：清任务（官方 spawn 被移除时 creep 自清；此处反向防御）
        return true;
    };
    // 方向序列：任务指定或 1..8
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
        let nx = spawn.x as i32 + dx;
        let ny = spawn.y as i32 + dy;
        if !(0..=49).contains(&nx) || !(0..=49).contains(&ny) {
            continue;
        }
        let (nx, ny) = (nx as u8, ny as u8);
        // 官方 checkObstacleFn（_born-creep.js:12-15）：OBSTACLE_OBJECT_TYPES
        // 成员类型（creep 在表内——友敌皆占位）或 obstacle 结构类型的工地（无
        // user 过滤，与 movement.js:25-26 的自家限定不同）
        let static_obstacle =
            w.index
                .at(nx as usize, ny as usize)
                .iter()
                .any(|tid| match w.room.objects.get(tid) {
                    Some(RoomObject::ConstructionSite(s)) => {
                        kk_world::types::is_obstacle_type(&s.structure_type)
                    }
                    Some(o) => kk_world::types::is_obstacle_type(&movement::type_name_of(o)),
                    None => false,
                })
                || (w.room.terrain.check_mask(
                    nx as usize,
                    ny as usize,
                    kk_core::TERRAIN_MASK_WALL,
                ) && !movement::has_road(w.room, &w.index, (nx, ny)));
        let busy = w.movement.is_tile_busy(nx, ny);
        let is_occupied = static_obstacle || busy;
        if !is_occupied {
            occupied_at = Some((nx, ny));
            break;
        }
        if hostile.is_none() {
            hostile = w.index.at(nx as usize, ny as usize).iter().find_map(|tid| {
                match w.room.objects.get(tid) {
                    Some(RoomObject::Creep(c)) if c.owner != spawn.owner => Some(*tid),
                    _ => None,
                }
            });
        }
    }
    if let Some((nx, ny)) = occupied_at {
        if let Some(c) = w.creep_mut(chick_id) {
            c.spawning = false;
        }
        w.move_obj(chick_id, (nx, ny));
        return true;
    }
    // 被围：spawnstomp——除非其他方向有出口（官方 66-79 的"选择性"判定）
    let other: Vec<u8> = (1..=8).filter(|d| !directions.contains(d)).collect();
    directions.extend(other);
    for d in directions {
        let Some((dx, dy)) = offsets_by_direction(d) else {
            continue;
        };
        let nx = spawn.x as i32 + dx;
        let ny = spawn.y as i32 + dy;
        if !(0..=49).contains(&nx) || !(0..=49).contains(&ny) {
            continue;
        }
        let (nx, ny) = (nx as u8, ny as u8);
        // 与第一轮同一 checkObstacleFn（官方 bail 检查同判据；墙检查无路例外）
        let free =
            !w.index
                .at(nx as usize, ny as usize)
                .iter()
                .any(|tid| match w.room.objects.get(tid) {
                    Some(RoomObject::ConstructionSite(s)) => {
                        kk_world::types::is_obstacle_type(&s.structure_type)
                    }
                    Some(o) => kk_world::types::is_obstacle_type(&movement::type_name_of(o)),
                    None => false,
                })
                && !w
                    .room
                    .terrain
                    .check_mask(nx as usize, ny as usize, kk_core::TERRAIN_MASK_WALL)
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
        w.move_obj(chick_id, (hx, hy));
        return true;
    }
    false
}
