//! 为运行时 getObject、findInRoom 生成 screeps-ok 的对象查询快照。
//!
//! 输出使用 kind 分类、剩余 tick 数和只包含正量资源的 store；缺失的 getter
//! 值通常表示为 null。沙箱的 world_json 使用不同字段和缺省值，不能直接替换。
//! 契约参考：screeps-ok adapters/vanilla/snapshots.ts。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use crate::world::{ObjId, World};

impl World {
    /// 按 ID 查询对象快照，找不到对象或对象是旗时返回 None。
    /// 旗不属于 screeps-ok 的 ObjectSnapshot 联合（vanilla 的 snapshotObject）。
    pub fn object_snapshot_by_id(&self, id: ObjId) -> Option<Value> {
        let (room, object) = self.get(id)?;
        if matches!(object, RoomObject::Flag(_)) {
            return None;
        }
        let terrain = &self.rooms.get(room)?.terrain;
        Some(snapshot_json(room, id, object, self.time, terrain))
    }

    /// 返回房内匹配 FIND 常量的快照，按对象 ID 排序；房间不存在时返回空列表。
    /// 无玩家视角，MY_*、HOSTILE_* 不匹配；旗和地形派生的出口不在此快照结果中。
    /// Mod 对象不产出原始记录（object_json 对其返回 None），本面不含 Mod；
    /// Mod 的管理查询走 object_snapshot_by_id 的单对象读面。
    /// 使用 world_json 的 matches_find 过滤，但返回的是查询快照格式。
    pub fn find_snapshots_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        let terrain = &state.terrain;
        let rcl = crate::domain::room::room_rcl(state);
        state
            .objects
            .iter()
            .filter_map(|(id, o)| {
                if matches!(*o, RoomObject::Flag(_)) {
                    return None;
                }
                let internal =
                    crate::projection::object_json(room, *id, o, self.time, terrain, rcl)?;
                crate::projection::matches_find(&internal, find_const, None)
                    .then(|| snapshot_json(room, *id, o, self.time, terrain))
            })
            .collect()
    }
}

/// 将非零绝对 tick 转为剩余时间；0 表示未设置，返回 null。
/// 过期值限制为 0，与 vanilla snapshots.ts 的 remaining 可返回负数不同。
/// 本函数不判断计时字段是否应由结算清除。
fn remaining(t: u64, time: u64) -> Value {
    if t != 0 {
        json!(t.saturating_sub(time))
    } else {
        Value::Null
    }
}

// 无钟语义与官方 getter 真值判一致（nextRegenerationTime 为 0 视同无钟）；
// 投影与快照共用，避免两处折叠口径漂移。过期钟截为 0 不产负数（引擎在
// regen-1 清钟，正常流不可达；见 sources/tick.js 同款清钟时机）。
pub(crate) fn remaining_opt(t: Option<u64>, time: u64) -> Value {
    match t {
        Some(t) if t != 0 => json!(t.saturating_sub(time)),
        _ => Value::Null,
    }
}

/// 只返回仍有效的剩余 tick 数，到期或未设置时返回 null。
/// 对应 vanilla snapshots.ts 的 activeRemaining。
fn active_remaining(t: u64, time: u64) -> Value {
    if t > time {
        json!(t - time)
    } else {
        Value::Null
    }
}

/// 未设置或已到期的可选时间都返回 null。
fn active_remaining_opt(t: Option<u64>, time: u64) -> Value {
    match t {
        Some(t) => active_remaining(t, time),
        None => Value::Null,
    }
}

fn snap_pos(room: &str, x: u8, y: u8) -> Value {
    json!({ "x": x, "y": y, "roomName": room })
}

/// 查询快照只包含正量资源，energy=0 时也省略该键。
/// 来源：screeps-ok adapters/vanilla/snapshots.ts 的 snapStore。
fn snap_store(store: &kk_schema::room::Store) -> Value {
    Value::Object(crate::domain::store_positive_entries(store))
}

/// 查询快照合并额外资源，也只保留正量条目。
fn snap_store_extra(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    crate::domain::apply_store_extra(&mut v, extra);
    Value::Object(v)
}

/// 活跃 CARRY 的总容量，包含 BOOSTS.carry.capacity 的强化乘数。
/// hits=0 的部件不贡献容量；沙箱投影和查询快照共用此数值计算。
/// 参考：engine/src/processor/intents/creeps/_recalc-body.js。
pub(crate) fn creep_store_capacity_raw(body: &[kk_schema::room::BodyPart]) -> i64 {
    crate::types::calc_body_effectiveness(
        body,
        crate::types::BodyKind::Carry,
        "carry",
        "capacity",
        kk_core::constants::CARRY_CAPACITY,
    )
}

/// 查询快照中，容量为 0 时返回 null；有容量时返回数值。
/// 对应 vanilla snapshots.ts 的 storeCapacity，不等同于沙箱中的原始 0 值。
fn creep_store_capacity(body: &[kk_schema::room::BodyPart]) -> Value {
    let capacity = creep_store_capacity_raw(body);
    if capacity > 0 {
        json!(capacity)
    } else {
        Value::Null
    }
}

/// 将控制器的升级总进度转为查询值，无对应等级时返回 null。
fn controller_progress_total(level: u8) -> Value {
    match crate::domain::controller_progress_total_opt(level) {
        Some(v) => json!(v),
        None => Value::Null,
    }
}

/// 将结构化对象转换为查询快照；不修改世界状态。
fn snapshot_json(
    room: &str,
    id: ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
) -> Value {
    let id_str = id.as_str();
    match object {
        // Mod 对象的诊断/管理读面：字段整体展开，不做可见性裁切——
        // 本面只服务管理查询（CLI/embed），玩家可见面走带裁切的投影
        // 通道（world_json/kkpv 的 mod_record_json）。
        RoomObject::Mod(o) => {
            let fields: Value = serde_json::from_str(&o.data).unwrap_or(json!({}));
            json!({
                "kind": "mod", "id": id_str, "typeKey": o.type_key,
                "pos": snap_pos(room, o.x, o.y),
                "fields": fields,
            })
        }
        RoomObject::Creep(o) => json!({
            "kind": "creep", "id": id_str, "name": o.name,
            "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": o.body.len() * 100,
            "fatigue": o.fatigue,
            "body": o.body.iter().map(crate::projection::part_json).collect::<Vec<_>>(),
            "owner": o.owner,
            "ticksToLive": remaining(o.age_time, time),
            "spawning": o.spawning,

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
                // 预订的 endTime 转为剩余 tick，签名保留世界 time，不包含墙钟 datetime。
                // 查询字段形状参考 vanilla snapshots.ts 的 snapshotController。
                "safeModeCooldown": active_remaining_opt(o.safe_mode_cooldown, time),
                "reservation": match &o.reservation {
                    Some(r) => json!({
                        "owner": r.user,
                        "ticksToEnd": r.end_time.saturating_sub(time),
                    }),
                    None => Value::Null,
                },
                "sign": match &o.sign {

                    Some(s) => json!({ "owner": s.user, "text": s.text, "time": s.time }),
                    None => Value::Null,
                },
                // 此查询额外提供 upgradeBlocked；vanilla 的 ControllerSnapshot 不包含该键。
                // 沙箱原始数据保存绝对 tick，此处则只返回有效的剩余时间。
                "upgradeBlocked": active_remaining_opt(o.upgrade_blocked, time),
                "isPowerEnabled": o.is_power_enabled,
            });

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
            // spawn 按资源限制容量；无资源参数的 getCapacity() 对应 null。
            // 参考：vanilla snapshots.ts 的 storeCapacity。
            "storeCapacity": Value::Null,
            // 孵化查询值使用剩余时间，沙箱原始数据则保留绝对 spawnTime。
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
            // extension 同样按资源限制容量，查询的无参总容量为 null。
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
            "hits": o.hits, "hitsMax": crate::domain::room::road_hits_max(terrain, o.x, o.y),
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
            "resourceType": crate::projection::energy_resource_name(o),
            "amount": o.amount,
        }),
        // 衰减时间限制为非负值；vanilla snapshotTombstone、snapshotRuin 使用裸减法。
        // store 仍只保留正量资源。
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

        RoomObject::Mineral(o) => json!({
            "kind": "mineral", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "mineralType": o.mineral_type,
            "mineralAmount": o.mineral_amount,
            "density": o.density,
            "ticksToRegeneration": remaining_opt(o.next_regeneration_time, time),
        }),
        // extractor 的 cooldown 是剩余 tick 计数，不是绝对截止时间。
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
        // Generic 保留 data 中的字段，按 type 或 structureType 设置 kind，
        // 再由对象表覆盖 ID 和位置。这里只转换下方列出的计时字段，
        // 不会为其他类型补齐专用 getter 或缺省值。
        RoomObject::Generic(o) => {
            let mut v = crate::domain::generic::decode_fields(&o.data).unwrap_or_default();
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
            // nextDecayTime 转为 ticksToDecay：未设置读 null，过期限制为 0。
            // cooldownTime 转为 cooldown：到期或 0 都读 0。
            // 转换后删除绝对时间键，避免同时暴露两种计时表示。
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
        // 两个公开查询入口已经排除旗。
        RoomObject::Flag(_) => Value::Null,
    }
}

#[cfg(test)]
mod tests;
