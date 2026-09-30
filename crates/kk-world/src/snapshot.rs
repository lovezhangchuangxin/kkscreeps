//! 查询面快照投影：embed `getObject`/`findInRoom` 的 screeps-ok 契约形状。
//!
//! 与 world_json（prelude 世界投影，M0 契约形状）是**两个消费面**：本投影产出
//! "玩家 getter 值"形态——契约 `kind` 字段、剩余 tick 数（绝对时间 − time）、
//! 正量 store（0 值资源不出现）、spawning 的 remainingTime 形态。逐组对照
//! vanilla 参照实现 screeps-ok/adapters/vanilla/snapshots.ts（其每组字段注明
//! 官方 game/*.js 出处）；`null` 语义同契约头注（getter 的 undefined 读 null）。
//! 两投影的字段一致性由 embed_tests 对表断言锁定（M0 第七轮"层间一致"教训：
//! 同一语义两层实现必须有显式对表）。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use crate::world::{ObjId, World};

impl World {
    /// 单对象快照（embed getObject 后端；flag 不在契约 ObjectSnapshot 联合内
    /// ——vanilla snapshotObject 无 flag 分支，返回 null，同构）。
    pub fn object_snapshot_by_id(&self, id: ObjId) -> Option<Value> {
        let (room, object) = self.get(id)?;
        if matches!(object, RoomObject::Flag(_)) {
            return None;
        }
        let terrain = &self.rooms.get(room)?.terrain;
        Some(snapshot_json(room, id, object, self.time, terrain))
    }

    /// 按 FIND 常量过滤房间内对象并出快照（embed findInRoom 后端）。
    /// 过滤复用 world_json::matches_find（FIND 语义单源）；flag 不进快照结果
    /// （契约查询面的中性选择器不含 flags——vanilla findTypeMap 同构）。
    /// `viewer=None`：本面是 screeps-ok 视角中立面（find.ts 只支持 9 个中立
    /// 常量）——MY_*/HOSTILE_* 无视角不可判定返回空，不假装有归属结果。
    pub fn find_snapshots_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        let terrain = &state.terrain;
        let rcl = crate::world_json::room_rcl(state);
        state
            .objects
            .iter()
            .filter(|(id, o)| {
                if matches!(*o, RoomObject::Flag(_)) {
                    return false;
                }
                let internal =
                    crate::world_json::object_json(room, **id, o, self.time, terrain, rcl);
                crate::world_json::matches_find(&internal, find_const, None)
            })
            .map(|(id, o)| snapshot_json(room, *id, o, self.time, terrain))
            .collect()
    }
}

/// 官方 getter 的 `t ? t - time : undefined` 形态（vanilla snapshots.ts:54-56；
/// 契约把 undefined 读作 null）。t=0 视同未设置（JS falsy）。
/// 注意：与 vanilla 的差异——过期挂钟官方发负数（契约头注 "unclamped"），
/// 我方 saturating_sub 钳 0；正常路径不可达（钟到点即被结算消费清掉），
/// 仅缺结算/构造性放置可观察，登记灰区表。
fn remaining(t: u64, time: u64) -> Value {
    if t != 0 {
        json!(t.saturating_sub(time))
    } else {
        Value::Null
    }
}

fn remaining_opt(t: Option<u64>, time: u64) -> Value {
    match t {
        Some(t) if t != 0 => json!(t.saturating_sub(time)),
        _ => Value::Null,
    }
}

/// 官方 safeMode/cooldown 的 `t && t > time ? t - time : undefined` 形态
/// （vanilla snapshots.ts:62-64）：过期挂钟也读 null。
fn active_remaining(t: u64, time: u64) -> Value {
    if t > time {
        json!(t - time)
    } else {
        Value::Null
    }
}

/// [`active_remaining`] 的 Option 形态（upgradeBlocked/safeModeCooldown 是
/// opt u64 挂钟）：官方 getter `o.x && o.x > time ? x - time : undefined`
/// （structures.js:147/151）——null/undefined/过期统一读 null（契约语义）。
fn active_remaining_opt(t: Option<u64>, time: u64) -> Value {
    match t {
        Some(t) => active_remaining(t, time),
        None => Value::Null,
    }
}

fn snap_pos(room: &str, x: u8, y: u8) -> Value {
    json!({ "x": x, "y": y, "roomName": room })
}

/// 官方 store getter 只暴露正量资源（vanilla snapStore：amount > 0 才入表）。
/// 双槽口径（spawn/extension 专用——无 store_extra）；creep/storage 的全资源
/// 面走 [`snap_store_extra`]（B4：命名矿物槽并入，层间一致）
fn snap_store(store: &kk_schema::room::Store) -> Value {
    Value::Object(crate::domain::store_positive_entries(store))
}

/// [`snap_store`] 的双槽 + 额外资源列表形态（B3：tombstone/ruin 的全资源
/// 正量口径——boost 矿物等经 store_extra 承载的资源同入表）。
fn snap_store_extra(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    crate::domain::apply_store_extra(&mut v, extra);
    Value::Object(v)
}

/// creep 载荷容量：活跃（hits>0）CARRY 数 × CARRY_CAPACITY，boosted 部件乘
/// BOOSTS.carry 的 capacity 系数（KH×2/KH2O×3/XKH2O×4——constants.js BOOSTS 表；
/// hits>0 门对照官方 _recalc-body → calcBodyEffectiveness，utils.js:623-636——
/// 受损部件不计容量）。pub(crate)：world_json 的 creep 投影同源直存
/// `storeCapacity`（官方 creep 落库字段，creeps.js carryCapacity/Store 同读），
/// 两消费面（快照契约 null 形态 vs 世界投影裸数值）由各自包装层定型。
pub(crate) fn creep_store_capacity_raw(body: &[kk_schema::room::BodyPart]) -> i64 {
    // 官方 _recalc-body.js:21：calcBodyEffectiveness(CARRY,'capacity',...)——
    // 单源实现（GPT 复审 P1-2 清偿：与移动重量/疲劳恢复/仲裁共用）
    crate::types::calc_body_effectiveness(
        body,
        crate::types::BodyKind::Carry,
        "carry",
        "capacity",
        kk_core::constants::CARRY_CAPACITY,
    )
}

/// 快照契约形态：vanilla `storeCapacity || null`（store.js:31 同构）——0（无活跃
/// CARRY）读 null；带 CARRY 的 creep 恒 number（inspection 用例锁 number）。
fn creep_store_capacity(body: &[kk_schema::room::BodyPart]) -> Value {
    let capacity = creep_store_capacity_raw(body);
    if capacity > 0 {
        json!(capacity)
    } else {
        Value::Null
    }
}

/// 官方 progressTotal：1..7 级查 CONTROLLER_LEVELS；0 级与 8 级为 null
/// （vanilla：`level > 0 && level < 8 ? LEVELS[level] : null`——level 8 无进度条）。
/// 与 world_json 的 prelude 投影（0 兜底）不同形，属两消费面各自契约
///（共核 `crate::domain::controller_progress_total_opt`——M2B 扩面 P0 起
/// 领域模块单源）。
fn controller_progress_total(level: u8) -> Value {
    match crate::domain::controller_progress_total_opt(level) {
        Some(v) => json!(v),
        None => Value::Null,
    }
}

/// typed 对象 → 契约快照（逐组对照 vanilla snapshots.ts）。
fn snapshot_json(
    room: &str,
    id: ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
) -> Value {
    let id_str = id.as_str();
    match object {
        RoomObject::Creep(o) => json!({
            "kind": "creep", "id": id_str, "name": o.name,
            "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": o.body.len() * 100,
            "fatigue": o.fatigue,
            "body": o.body.iter().map(crate::world_json::part_json).collect::<Vec<_>>(),
            "owner": o.owner,
            "ticksToLive": remaining(o.age_time, time),
            "spawning": o.spawning,
            // B4：命名矿物槽并入（vanilla snapStore 全资源正量同构）
            "store": snap_store_extra(&o.store, &o.store_extra),
            "storeCapacity": creep_store_capacity(&o.body),
        }),
        RoomObject::Source(o) => json!({
            "kind": "source", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "energy": o.energy, "energyCapacity": o.energy_capacity,
            "ticksToRegeneration": remaining_opt(o.next_regeneration_time, time),
        }),
        RoomObject::Controller(o) => {
            let mut v = json!({
                "kind": "structure", "structureType": "controller",
                "id": id_str, "pos": snap_pos(room, o.x, o.y),
                "level": o.level,
                "progress": if o.level > 0 { json!(o.progress) } else { Value::Null },
                "progressTotal": controller_progress_total(o.level),
                "ticksToDowngrade": remaining_opt(o.downgrade_time, time),
                "safeMode": active_remaining(o.safe_mode, time),
                "safeModeAvailable": o.safe_mode_available,
                // B2 schema 字段的查询面清偿（A4-iii）：vanilla snapshots.ts
                // :151-161——safeModeCooldown 过期/无值读 null、reservation
                // {owner, ticksToEnd}、sign {owner, text, time}（契约无
                // datetime 键——墙钟不落盘，灰区表既有登记）。ticksToEnd 的
                // 饱和钳 0 与 remaining 家族同一既有口径
                "safeModeCooldown": active_remaining_opt(o.safe_mode_cooldown, time),
                "reservation": match &o.reservation {
                    Some(r) => json!({
                        "owner": r.user,
                        "ticksToEnd": r.end_time.saturating_sub(time),
                    }),
                    None => Value::Null,
                },
                "sign": match &o.sign {
                    // 契约面只有 owner/text/time（vanilla snapshots.ts:157-161，
                    // GPT 二轮 P2-4：datetime 不进查询快照——世界投影 Some 带键
                    // 供 prelude getter 构造）
                    Some(s) => json!({ "owner": s.user, "text": s.text, "time": s.time }),
                    None => Value::Null,
                },
                // 官方 getter 有（structures.js:147）而 vanilla ControllerSnapshot
                // 契约无此键——超额投影（数值消费方为本引擎调试面；契约消费方
                // 结构校验不受额外键影响），登记灰区表
                "upgradeBlocked": active_remaining_opt(o.upgrade_blocked, time),
                "isPowerEnabled": o.is_power_enabled,
            });
            // 归属三态（与 world_json 同形——controller_owner_value 单源）
            if let Some(val) = crate::domain::controller_owner_value(o) {
                v["owner"] = val;
            }
            v
        }
        RoomObject::Spawn(o) => json!({
            "kind": "structure", "structureType": "spawn",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::SPAWN_HITS,
            "name": o.name,
            "store": snap_store(&o.store),
            // 官方 spawn 落库即带 storeCapacityResource（build.js:117-124），
            // 无资源参数的 getCapacity() 读 null（store.js:28-33；
            // adapter-spec.md:359-360 明文 "null for a store with per-resource
            // limits such as a spawn's"；vanilla 快照同构）——数值容量经
            // energyCapacityAvailable/结算扣费面消费
            "storeCapacity": Value::Null,
            // 官方 Spawning 暴露 remainingTime = spawnTime − time
            // （structures.js:1302-1310；vanilla snapSpawning 同构）
            "spawning": match &o.spawning {
                None => Value::Null,
                Some(job) => json!({
                    "name": job.name, "needTime": job.need_time,
                    "remainingTime": job.spawn_time.saturating_sub(time),
                }),
            },
        }),
        RoomObject::Extension(o) => json!({
            "kind": "structure", "structureType": "extension",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
            "store": snap_store(&o.store),
            // 同 spawn：官方 extension 带 storeCapacityResource（build.js:131-135、
            // extensions/tick.js:11-17 按 RCL 归一），getCapacity() 读 null
            "storeCapacity": Value::Null,
        }),
        RoomObject::ConstructedWall(o) => json!({
            "kind": "structure", "structureType": "constructedWall",
            "id": id_str, "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": c::WALL_HITS_MAX,
        }),
        RoomObject::Road(o) => json!({
            "kind": "structure", "structureType": "road",
            "id": id_str, "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": crate::world_json::road_hits_max(terrain, o.x, o.y),
            "ticksToDecay": remaining(o.next_decay_time, time),
        }),
        RoomObject::Storage(o) => json!({
            "kind": "structure", "structureType": "storage",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
            "store": snap_store_extra(&o.store, &o.store_extra),
            "storeCapacity": c::STORAGE_CAPACITY,
        }),
        RoomObject::ConstructionSite(o) => {
            let mut v = json!({
                "kind": "site", "id": id_str,
                "pos": snap_pos(room, o.x, o.y),
                "structureType": o.structure_type,
                "progress": o.progress, "progressTotal": o.progress_total,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        RoomObject::Energy(o) => json!({
            "kind": "resource", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "resourceType": crate::world_json::energy_resource_name(o),
            "amount": o.amount,
        }),
        // B3 typed 变体的契约形态（vanilla snapshots.ts:405-414 / :392-402）：
        // ticksToDecay 为裸减法（不走 remaining 家族——vanilla 原文
        // `obj.decayTime - gameTime`，我方 saturating 钳 0 属既有登记口径）、
        // store 全资源正量
        RoomObject::Tombstone(o) => json!({
            "kind": "tombstone", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "creepName": o.creep_name,
            "deathTime": o.death_time,
            "store": snap_store_extra(&o.store, &o.store_extra),
            "ticksToDecay": o.decay_time.saturating_sub(time),
        }),
        RoomObject::Ruin(o) => json!({
            "kind": "ruin", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "structureType": o.structure_type,
            "destroyTime": o.destroy_time,
            "store": snap_store_extra(&o.store, &o.store_extra),
            "ticksToDecay": o.decay_time.saturating_sub(time),
        }),
        // B4 typed 变体的契约形态（vanilla snapshots.ts:365-373 的
        // snapshotMineral——七键面：kind/id/pos/mineralType/mineralAmount/
        // density/ticksToRegeneration）
        RoomObject::Mineral(o) => json!({
            "kind": "mineral", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "mineralType": o.mineral_type,
            "mineralAmount": o.mineral_amount,
            "density": o.density,
            "ticksToRegeneration": remaining_opt(o.next_regeneration_time, time),
        }),
        // B4：vanilla snapshotStructure 的 extractor 分支（...base/...hits +
        // cooldown 计数器——`obj.cooldown || 0`）；owner 无主读 null
        //（snapOwner 同构）
        RoomObject::Extractor(o) => {
            let mut v = json!({
                "kind": "structure", "structureType": "extractor",
                "id": id_str, "pos": snap_pos(room, o.x, o.y),
                "hits": o.hits, "hitsMax": c::EXTRACTOR_HITS,
                "cooldown": o.cooldown,
            });
            match &o.owner {
                Some(owner) => v["owner"] = json!(owner),
                None => {
                    v["owner"] = Value::Null;
                }
            }
            v
        }
        // 透传桶：kind 按透传 type 或 structureType 映射（tombstone/ruin/
        // deposit 有独立契约 kind——B4 起 mineral/extractor 放置面已 typed，
        // 映射保留给旧持久化字节与手写 Generic 透传的遗留路径）。其余按
        // structure 兜底（vanilla snapshotStructure 的 default 分支同构——只
        // 保底 base 字段）。计时字段不折算 getter 剩余形态——遗留路径无该面
        RoomObject::Generic(o) => {
            let mut v: serde_json::Map<String, Value> = serde_json::from_str::<Value>(&o.data)
                .ok()
                .and_then(|d| d.as_object().cloned())
                .unwrap_or_default();
            let kind = match (
                v.get("type").and_then(Value::as_str),
                v.get("structureType").and_then(Value::as_str),
            ) {
                (Some("tombstone"), _) | (_, Some("tombstone")) => "tombstone",
                (Some("ruin"), _) | (_, Some("ruin")) => "ruin",
                (Some("mineral"), _) | (_, Some("mineral")) => "mineral",
                (Some("deposit"), _) | (_, Some("deposit")) => "deposit",
                _ => "structure",
            };
            v.insert("kind".into(), json!(kind));
            v.insert("id".into(), json!(id_str));
            v.insert("pos".into(), snap_pos(room, o.x, o.y));
            // M2B 扩面 P4：绝对挂钟折叠为官方 getter 等价差值（容器/壁垒
            // 衰减钟、link/lab/factory 冷却钟——放置面写入的绝对形态在快照
            // 契约面折算；官方 vanilla snapshotObject 同折）。nextDecayTime
            // 用 remaining 家族（`t ? t-time : undefined`——0 falsy 跳过），
            // cooldownTime 官方 getter 是 `t && t>time ? t-time : 0`（到点
            // 后读 0 非 undefined）
            if let Some(t) = v.get("nextDecayTime").and_then(Value::as_u64) {
                v.insert("ticksToDecay".into(), remaining(t, time));
                v.remove("nextDecayTime");
            }
            if let Some(t) = v.get("cooldownTime").and_then(Value::as_u64) {
                v.insert(
                    "cooldown".into(),
                    if t > time { json!(t - time) } else { json!(0) },
                );
                v.remove("cooldownTime");
            }
            Value::Object(v)
        }
        // flag 不在查询面契约内（vanilla snapshotObject 无 flag 分支）——
        // 调用方（object_snapshot_by_id/find_snapshots_in_room）已前置排除
        RoomObject::Flag(_) => Value::Null,
    }
}

#[cfg(test)]
mod tests;
