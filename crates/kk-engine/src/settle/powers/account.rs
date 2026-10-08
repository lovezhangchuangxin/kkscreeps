//! power 账号域全局阶段：六种全局意图与已生成 powerCreep 的死亡扫描。
//!
//! 移植官方 engine/src/processor/global-intents/power.js。账号注册表在
//! `PlayerState.power_creeps`，房间对象在候选 rooms 中按 ID 同步。
//! spawnCooldownTime/deleteTime 是 Unix 毫秒墙钟（TickInput.now_ms 同源）；
//! 官方 powerExperimentationTime 的实验期直删分支无对应形态（本仓账号
//! 无实验期概念），delete 恒走 24h 排队路径，幂等语义与官方一致。

use std::collections::BTreeMap;

use kk_core::constants as c;
use kk_schema::room::{ResourceStack, RoomObject, Store, TombstoneObj};
use kk_world::ObjId;
use serde_json::{Value, json};

use super::pc_data;
use crate::intent::{IntentBatch, PowerIntent};
use crate::settle::PlayerState;
use crate::settle::{RoomState, SettleError, TickState};

/// GPL 等级：floor((power/1000)^(1/POWER_LEVEL_POW))。
/// 来源：官方 game/power-creeps.js calcFreePowerLevels 与 game.js 的 gpl。
pub fn gpl_level(power: i64) -> i64 {
    ((power.max(0) as f64 / c::POWER_LEVEL_MULTIPLY as f64).powf(1.0 / c::POWER_LEVEL_POW as f64))
        .floor() as i64
}

/// 已占用配额：注册表数量（create 占 1）+ 全部等级和（每级 upgrade 占
/// 1），delete 不退。**三方裁决**：xxscreeps 的 freeLevels（powercreep.ts:
/// 541-544，注释原文 "minus one per creep and each creep's own level"）
/// 与 screeps-ok GPL-004/005 联合契约（GPL2 允许 1 create + 1 upgrade）
/// 一致取此式；官方开源源 createPowerCreep.js 末行的
/// `user._usedPowerLevels += 1` 双计是异类（xxscreeps 未复刻，按其行为
/// GPL-005 的 upgrade 必被拒），不采。
fn used_levels(player: &PlayerState) -> i64 {
    let levels: i64 = player.power_creeps.values().map(|r| r.level).sum();
    player.power_creeps.len() as i64 + levels
}

/// POWER_INFO 常量 JSON 的进程级解析；函数内 static OnceLock 引用是
/// 'static，无锁无克隆。powers::mod 有同款（usePower 面），此处账号域
/// （upgrade 门）独立持有避免跨模块私有泄漏。
fn power_info(power: i64) -> Option<&'static Value> {
    use std::sync::OnceLock;
    static INFO: OnceLock<serde_json::Map<String, Value>> = OnceLock::new();
    let table = INFO.get_or_init(|| {
        serde_json::from_str::<Value>(c::POWER_INFO)
            .unwrap_or_default()
            .as_object()
            .cloned()
            .unwrap_or_default()
    });
    table.get(&power.to_string())
}

/// 全局阶段入口：按玩家名序消费六种意图，再全房扫描已生成 powerCreep 的
/// 到期/归零死亡。幂等门与官方逐条一致（静默 no-op，错误码由预检层产生）。
pub fn settle_power(
    rooms: &mut [(String, RoomState)],
    batch: &IntentBatch,
    time: u64,
    now_ms: u64,
    state: &TickState,
) -> Result<(), SettleError> {
    // players 经共享 TickState 短借用访问：每个意图/死亡一次借用，先取
    // 快照判定再落账，绝不与 allocate_id 的借用重叠。
    for (player, intents) in &batch.users {
        for intent in &intents.power {
            match intent {
                PowerIntent::SpawnPowerCreep { power_spawn, name } => {
                    let snapshot = {
                        let shared = state.inner.borrow();
                        shared.players.get(player).cloned().unwrap_or_default()
                    };
                    spawn_power_creep(
                        rooms,
                        state,
                        player,
                        &snapshot,
                        *power_spawn,
                        name,
                        (time, now_ms),
                    );
                }
                PowerIntent::SuicidePowerCreep { id } => {
                    if let Some((_, room)) = find_room(rooms, *id) {
                        let owner = room
                            .objects
                            .get(id)
                            .filter(|o| super::is_power_creep(o))
                            .and_then(pc_data)
                            .and_then(|v| {
                                v.get("owner").and_then(Value::as_str).map(str::to_string)
                            });
                        if owner.as_deref() == Some(player.as_str()) {
                            die_power_creep(room, state, player, *id, time, now_ms)?;
                        }
                    }
                }
                PowerIntent::DeletePowerCreep { id, cancel } => {
                    let mut shared = state.inner.borrow_mut();
                    delete_power_creep(&mut shared.players, player, *id, *cancel, now_ms);
                }
                PowerIntent::UpgradePowerCreep { id, power } => {
                    let snapshot = {
                        let shared = state.inner.borrow();
                        shared.players.get(player).cloned().unwrap_or_default()
                    };
                    upgrade_power_creep(rooms, state, player, &snapshot, *id, *power);
                }
                PowerIntent::CreatePowerCreep { name, class_name } => {
                    create_power_creep(state, player, name, class_name)?;
                }
                PowerIntent::RenamePowerCreep { id, name } => {
                    let mut shared = state.inner.borrow_mut();
                    rename_power_creep(&mut shared.players, player, *id, name);
                }
            }
        }
    }
    // 到期删除（官方由 backend cronjobs.js:533-553 每 10 分钟执行；kk 在
    // 全局阶段每 tick 检查，时点更及时）：删记录并按 GPL 插值降账号
    // power——把当前段进度比例折算到 (level-1, level) 段。
    let expired: Vec<(String, u64)> = {
        let shared = state.inner.borrow();
        shared
            .players
            .iter()
            .flat_map(|(player, p)| {
                p.power_creeps
                    .iter()
                    .filter_map(|(id, r)| {
                        (r.delete_time.is_some_and(|t| t <= now_ms)).then_some(id)
                    })
                    .map(|id| (player.clone(), *id))
            })
            .collect()
    };
    if !expired.is_empty() {
        let mut shared = state.inner.borrow_mut();
        for (player, id) in expired {
            let Some(p) = shared.players.get_mut(&player) else {
                continue;
            };
            // 官方逐条重读 deleteTime（cron 竞态防护），这里记录仍在才执行。
            if !p
                .power_creeps
                .get(&id)
                .is_some_and(|r| r.delete_time.is_some())
            {
                continue;
            }
            let level = gpl_level(p.power);
            let base = |l: i64| (l * l * c::POWER_LEVEL_MULTIPLY) as f64;
            let (base_prev, base_current, base_next) =
                (base(level - 1), base(level), base(level + 1));
            let power = p.power as f64;
            let change = (power
                - base_prev
                - (power - base_current) * (base_current - base_prev) / (base_next - base_current))
                .round() as i64;
            p.power -= change;
            p.power_creeps.remove(&id);
        }
    }
    // 死亡扫描：TTL 到期（gameTime >= ageTime-1）或 hits 归零。先收集再
    // 应用，遍历期间不改对象表。只扫 powerCreep（官方 power.js:30-36 走
    // roomObjectsByType.powerCreep；无类型过滤会误伤其他 Generic——放置
    // 通道可造 hits=0 的 Generic 结构）。
    let mut dying: Vec<(String, ObjId)> = Vec::new();
    for (name, room) in rooms.iter() {
        for (id, obj) in &room.objects {
            if !super::is_power_creep(obj) {
                continue;
            }
            let Some(v) = pc_data(obj) else {
                continue;
            };
            let age = v.get("ageTime").and_then(Value::as_u64).unwrap_or(u64::MAX);
            let hits = v.get("hits").and_then(Value::as_i64).unwrap_or(1);
            if time >= age.saturating_sub(1) || hits <= 0 {
                dying.push((name.clone(), *id));
            }
        }
    }
    for (room_name, id) in dying {
        // 归属以 data.owner 为准（官方 _diePowerCreep 不查注册表，恒留墓
        // 碑；孤儿对象只免账号冷却更新）。
        let Some((_, room)) = rooms.iter_mut().find(|(n, _)| *n == room_name) else {
            continue;
        };
        let owner = room
            .objects
            .get(&id)
            .and_then(pc_data)
            .and_then(|v| v.get("owner").and_then(Value::as_str).map(str::to_string));
        die_power_creep(
            room,
            state,
            owner.as_deref().unwrap_or_default(),
            id,
            time,
            now_ms,
        )?;
    }
    Ok(())
}

/// 定位包含指定对象的候选房。
fn find_room(rooms: &mut [(String, RoomState)], id: ObjId) -> Option<&mut (String, RoomState)> {
    rooms
        .iter_mut()
        .find(|(_, room)| room.objects.contains_key(&id))
}

/// createPowerCreep：配额门 + 类名白名单 + 名字截 50 与唯一性；新建记录
/// spawnCooldownTime=0（未生成、可立即 spawn）。名字长度：官方玩家预检
/// （power-creeps.js:396）与 xxscreeps（powercreep.ts:568）均为 >100 拒
/// ERR_INVALID_ARGS、≤100 放行，但结算落库统一 substring(0,50)（官方
/// createPowerCreep.js / xxscreeps model.ts:83）——API 返回 OK 与存储截断
/// 并存是官方怪癖，xxscreeps 完整复刻。screeps-ok CREATE-003 断言
/// "length===100 完整保留"与两个参考实现均矛盾（套件缺陷），经
/// ratchet-allow 豁免。
fn create_power_creep(
    state: &TickState,
    player: &str,
    name: &str,
    class_name: &str,
) -> Result<(), SettleError> {
    let Some(mut p) = state.inner.borrow().players.get(player).cloned() else {
        return Ok(());
    };
    if used_levels(&p) >= gpl_level(p.power) {
        return Ok(());
    }
    if !c::POWER_CLASS.iter().any(|&(_, v)| v == class_name) {
        return Ok(());
    }
    let name: String = name.chars().take(50).collect();
    if p.power_creeps.values().any(|r| r.name == name) {
        return Ok(());
    }
    let id = state.allocate_id()?;
    p.power_creeps.insert(
        id,
        crate::settle::PowerCreepRecord {
            name,
            class_name: class_name.to_string(),
            level: 0,
            hits_max: 1000,
            store_capacity: 100,
            powers: BTreeMap::new(),
            spawn_cooldown_time: Some(0),
            delete_time: None,
        },
    );
    state
        .inner
        .borrow_mut()
        .players
        .insert(player.to_string(), p);
    Ok(())
}

/// renamePowerCreep：仅未生成（spawnCooldownTime 非 null）；名字截 50、
/// 唯一性（长度门同 create：预检 >100 拒、落库截 50，官方与 xxscreeps
/// 双证；RENAME-001 套件缺陷同 CREATE-003 豁免）。来源：renamePowerCreep.js。
fn rename_power_creep(
    players: &mut BTreeMap<String, PlayerState>,
    player: &str,
    id: ObjId,
    name: &str,
) {
    let Some(p) = players.get_mut(player) else {
        return;
    };
    let name: String = name.chars().take(50).collect();
    let owned = p
        .power_creeps
        .get(&id.0)
        .map(|r| r.spawn_cooldown_time.is_some());
    if owned != Some(true) {
        return; // 不存在或已生成态均不可改名
    }
    if p.power_creeps.values().any(|r| r.name == name) {
        return;
    }
    if let Some(record) = p.power_creeps.get_mut(&id.0) {
        record.name = name;
    }
}

/// deletePowerCreep：cancel 清排队；非 cancel 设 24h 墙钟排队（幂等——
/// 已排队时 no-op）。官方实验期直删分支未移植（见模块注释）。
/// 来源：deletePowerCreep.js。
fn delete_power_creep(
    players: &mut BTreeMap<String, PlayerState>,
    player: &str,
    id: ObjId,
    cancel: bool,
    now_ms: u64,
) {
    let Some(p) = players.get_mut(player) else {
        return;
    };
    let Some(record) = p.power_creeps.get_mut(&id.0) else {
        return;
    };
    if record.spawn_cooldown_time.is_none() {
        return; // 已生成态不可排队删除
    }
    if cancel {
        record.delete_time = None;
    } else if record.delete_time.is_none() {
        record.delete_time = Some(now_ms + c::POWER_CREEP_DELETE_COOLDOWN as u64);
    }
}

/// upgradePowerCreep：配额 + 等级上限 + 类匹配 + power 等级门槛
/// （level >= POWER_INFO[power].level[当前 power 等级]）；升级同步已生成
/// 侧对象（level/hitsMax/storeCapacity/powers 等级，保留冷却时间）。
/// 来源：upgradePowerCreep.js。
fn upgrade_power_creep(
    rooms: &mut [(String, RoomState)],
    state: &TickState,
    player: &str,
    snapshot: &PlayerState,
    id: ObjId,
    power: i64,
) {
    let new_state = {
        let p = snapshot;
        if used_levels(p) >= gpl_level(p.power) {
            return;
        }
        let Some(record) = p.power_creeps.get(&id.0) else {
            return;
        };
        if record.level >= c::POWER_CREEP_MAX_LEVEL {
            return;
        }
        let Some(info) = power_info(power) else {
            return;
        };
        if info.get("className").and_then(Value::as_str) != Some(record.class_name.as_str()) {
            return;
        }
        let current = record.powers.get(&power).copied().unwrap_or(0);
        if current == 5 {
            return;
        }
        let required = info
            .get("level")
            .and_then(Value::as_array)
            .and_then(|a| a.get(current as usize))
            .and_then(Value::as_i64);
        if record.level < required.unwrap_or(i64::MAX) {
            return;
        }
        (
            record.level + 1,
            record.hits_max + 1000,
            record.store_capacity + 100,
            current + 1,
        )
    };
    let (level, hits_max, store_capacity, power_level) = new_state;
    let mut shared = state.inner.borrow_mut();
    if let Some(p) = shared.players.get_mut(player)
        && let Some(record) = p.power_creeps.get_mut(&id.0)
    {
        record.level = level;
        record.hits_max = hits_max;
        record.store_capacity = store_capacity;
        record.powers.insert(power, power_level);
    }
    drop(shared);
    // 已生成侧同步：对象 data 的 level/hitsMax/storeCapacity/powers 等级，
    // 冷却时间原样保留。
    if let Some((_, room)) = rooms.iter_mut().find(|(_, r)| r.objects.contains_key(&id))
        && let Some(RoomObject::Generic(o)) = room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
    {
        v["level"] = json!(level);
        v["hitsMax"] = json!(hits_max);
        v["storeCapacity"] = json!(store_capacity);
        let key = power.to_string();
        let cooldown = v
            .pointer(&format!("/powers/{key}/cooldownTime"))
            .cloned()
            .unwrap_or(Value::Null);
        v["powers"][&key] = json!({ "level": power_level, "cooldownTime": cooldown });
        o.data = v.to_string();
    }
}

/// spawnPowerCreep：按名找账号记录（非生成态且冷却到期），powerSpawn 归
/// 属本人且落位格无 powerCreep。账号置已生成态并清 deleteTime，房间插
/// 入满血对象（ageTime=time+5000、notifyWhenAttacked=true、空 store）。
/// 同一 powerSpawn 每 tick 至多一胎由对象动作覆盖语义天然保证（同名
/// processPower 覆盖，此处无需官方 _justSpawned 的跨意图去重）。
/// 来源：spawnPowerCreep.js。
fn spawn_power_creep(
    rooms: &mut [(String, RoomState)],
    state: &TickState,
    player: &str,
    snapshot: &PlayerState,
    power_spawn: ObjId,
    name: &str,
    clock: (u64, u64),
) {
    let (time, now_ms) = clock;
    let Some((_, spawn_room)) = rooms
        .iter_mut()
        .find(|(_, r)| r.objects.contains_key(&power_spawn))
    else {
        return;
    };
    let Some(RoomObject::Generic(spawn_obj)) = spawn_room.objects.get(&power_spawn).cloned() else {
        return;
    };
    let Ok(sv) = serde_json::from_str::<Value>(&spawn_obj.data) else {
        return;
    };
    if sv.get("structureType").and_then(Value::as_str) != Some("powerSpawn") {
        return;
    }
    if sv.get("owner").and_then(Value::as_str) != Some(player) {
        return;
    }
    let occupied = spawn_room.objects.values().any(|o| {
        pc_data(o).is_some_and(|v| {
            v.get("type").and_then(Value::as_str) == Some("powerCreep")
                && o.pos() == (spawn_obj.x, spawn_obj.y)
        })
    });
    if occupied {
        return;
    }
    // 按名取账号记录快照：非生成态（Some）且冷却到期/从未生成才放行。
    let found = snapshot
        .power_creeps
        .iter()
        .find(|(_, r)| r.name == name)
        .map(|(id, r)| (*id, r.clone()));
    let Some((id, record)) = found else {
        return;
    };
    if !record.spawn_cooldown_time.is_some_and(|t| t <= now_ms) {
        return;
    }
    if let Some(p) = state.inner.borrow_mut().players.get_mut(player)
        && let Some(r) = p.power_creeps.get_mut(&id)
    {
        r.spawn_cooldown_time = None;
        r.delete_time = None;
    }
    let powers = Value::Object(
        record
            .powers
            .iter()
            .map(|(pid, level)| {
                (
                    pid.to_string(),
                    json!({ "level": level, "cooldownTime": 0 }),
                )
            })
            .collect(),
    );
    let data = json!({
        "type": "powerCreep",
        "owner": player,
        "name": record.name,
        "className": record.class_name,
        "level": record.level,
        "hits": record.hits_max,
        "hitsMax": record.hits_max,
        "store": {},
        "storeCapacity": record.store_capacity,
        "ageTime": time + c::POWER_CREEP_LIFE_TIME as u64,
        "notifyWhenAttacked": true,
        "powers": powers,
        // 官方 spawnPowerCreep.js:37 落位即写 actionLog.spawned（无玩家
        // 读面，客户端渲染用）；下一 tick 开头随房统一清空。
        "actionLog": { "spawned": true },
    });
    if let Some((_, room)) = rooms
        .iter_mut()
        .find(|(_, r)| r.objects.contains_key(&power_spawn))
    {
        room.objects.insert(
            ObjId(id),
            RoomObject::Generic(kk_schema::room::GenericObj {
                id,
                x: spawn_obj.x,
                y: spawn_obj.y,
                data: data.to_string(),
            }),
        );
    }
}

/// 死亡结算：墓碑（容器先收余量）、对象移除、账号置 8h 重生冷却。
/// 墓碑 ID 经全局游标分配（与 nuke 插入同形）。来源：
/// global-intents/power/_diePowerCreep.js。
fn die_power_creep(
    room: &mut RoomState,
    state: &TickState,
    player: &str,
    id: ObjId,
    time: u64,
    now_ms: u64,
) -> Result<(), SettleError> {
    let Some(RoomObject::Generic(o)) = room.objects.get(&id).cloned() else {
        return Ok(());
    };
    let Ok(v) = serde_json::from_str::<Value>(&o.data) else {
        return Ok(());
    };
    let owner = v
        .get("owner")
        .and_then(Value::as_str)
        .unwrap_or(player)
        .to_string();
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let age_time = v.get("ageTime").and_then(Value::as_u64).unwrap_or(time);
    // 库存转移：同格存活容器先收（按通用 storeCapacity 余量逐资源转入），
    // 余量入墓碑 store。
    let mut entries: Vec<(String, i64)> = v
        .get("store")
        .and_then(Value::as_object)
        .map(|s| {
            s.iter()
                .filter_map(|(k, n)| n.as_i64().map(|n| (k.clone(), n)))
                .collect()
        })
        .unwrap_or_default();
    let container = room.objects.iter().find(|(_, obj)| {
        obj.pos() == (o.x, o.y)
            && pc_data(obj)
                .and_then(|cv| {
                    cv.get("structureType")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .as_deref()
                == Some("container")
    });
    if let Some((cid, _)) = container {
        let cid = *cid;
        let cap = room
            .objects
            .get(&cid)
            .and_then(pc_data)
            .and_then(|cv| cv.get("storeCapacity").and_then(Value::as_i64))
            .unwrap_or(0);
        if let Some(RoomObject::Generic(g)) = room.objects.get_mut(&cid)
            && let Ok(mut cv) = serde_json::from_str::<Value>(&g.data)
        {
            let held: i64 = cv
                .get("store")
                .and_then(Value::as_object)
                .map(|s| s.values().filter_map(Value::as_i64).sum())
                .unwrap_or(0);
            let mut free = (cap - held).max(0);
            for (resource, amount) in entries.iter_mut() {
                if *amount <= 0 || free <= 0 {
                    continue;
                }
                let moved = (*amount).min(free);
                let current = cv
                    .pointer(&format!("/store/{resource}"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                cv["store"][resource.as_str()] = json!(current + moved);
                *amount -= moved;
                free -= moved;
            }
            g.data = cv.to_string();
        }
    }
    let (store, store_extra) = split_store(&entries);
    // 公开遗言（官方 _diePowerCreep.js:23 的 powerCreepSaying 分支；私有
    // 或无话为 None）。className/level/powers 三字段无 TombstoneObj 承载
    // 位，M2H.b 随效果族一并定形状。
    let saying = v
        .pointer("/actionLog/say")
        .filter(|s| s.get("isPublic").and_then(Value::as_bool).unwrap_or(false))
        .and_then(|s| s.get("message").and_then(Value::as_str).map(str::to_string));
    // 墓碑 ID 先于任何 players 借用分配（allocate_id 自持独立借用）。
    let tombstone_id = state.allocate_id()?;
    room.objects.remove(&id);
    room.objects.insert(
        ObjId(tombstone_id),
        RoomObject::Tombstone(TombstoneObj {
            id: tombstone_id,
            x: o.x,
            y: o.y,
            death_time: time,
            decay_time: time + c::TOMBSTONE_DECAY_POWER_CREEP as u64,
            creep_id: Some(id.0),
            creep_name: Some(name),
            creep_ticks_to_live: age_time.saturating_sub(time) as u32,
            creep_body: Vec::new(),
            creep_saying: saying,
            owner: Some(owner),
            store,
            store_extra,
        }),
    );
    if let Some(p) = state.inner.borrow_mut().players.get_mut(player)
        && let Some(record) = p.power_creeps.get_mut(&id.0)
    {
        record.spawn_cooldown_time = Some(now_ms + c::POWER_CREEP_SPAWN_COOLDOWN as u64);
    }
    Ok(())
}

/// 官方墓碑 store 形态：energy 固定槽 + 其余资源栈。
fn split_store(entries: &[(String, i64)]) -> (Store, Vec<ResourceStack>) {
    let mut store = Store::default();
    let mut extra = Vec::new();
    for (resource, amount) in entries {
        if *amount <= 0 {
            continue;
        }
        match resource.as_str() {
            "energy" => store.energy = store.energy.saturating_add(*amount as u32),
            kk_core::constants::RESOURCE_GHODIUM => {
                store.ghodium = store.ghodium.saturating_add(*amount as u32)
            }
            other => extra.push(ResourceStack {
                resource: other.to_string(),
                amount: u32::try_from(*amount).unwrap_or(u32::MAX),
            }),
        }
    }
    (store, extra)
}
