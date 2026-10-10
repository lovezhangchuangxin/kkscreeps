//! 沙箱对象记录的字段编码，保留原始值与缺省语义。
use super::*;

/// 原始沙箱 store 保留 energy 键（包括 0），G 只在正量时出现。
/// prelude 的 Store 构造器再过滤 0 值资源，因此原始 JSON 不等于玩家枚举结果。
pub(crate) fn store_json(store: &kk_schema::room::Store) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    Value::Object(v)
}

/// 显式资源名优先；否则码值 1、2 对应 power、G，其余值对应 energy。
pub(crate) fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> std::borrow::Cow<'_, str> {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => std::borrow::Cow::Borrowed(name.as_str()),
        (None, 1) => std::borrow::Cow::Borrowed("power"),
        (None, 2) => std::borrow::Cow::Borrowed(kk_core::constants::RESOURCE_GHODIUM),
        (None, _) => std::borrow::Cow::Borrowed("energy"),
    }
}

/// 将对象转换为 prelude 消费的原始记录，并附加 pos。
/// 控制器（等级与归属）决定 extension 容量与 storage 权威容量，效果表
/// 决定 storage 的 power 加成；不会读取或改变对象所属世界的状态。
/// 可选通知开关只在 Some 时输出，None 保持缺键。Mod 对象返回 None：
/// 玩家投影两通道走带可见性裁切的 mod_record_json，本函数从返回类型上
/// 杜绝"空对象 + 补造 pos 坐标"的残影记录进入任何查询路径。
#[allow(clippy::too_many_arguments)]
pub(crate) fn object_json(
    room: &str,
    id: crate::world::ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
    controller: Option<&kk_schema::room::ControllerObj>,
    effects: Option<&Vec<crate::EffectEntry>>,
) -> Option<Value> {
    let id_str = id.as_str();
    let rcl = controller.map(|ctl| ctl.level).unwrap_or(0);
    let mut v = match object {
        RoomObject::Creep(o) => {
            // age_time=0 表示尚未设置寿命，不输出 ticksToLive。
            // 其余值转为非负剩余 tick；额外资源也须传给玩家 Store。
            let store = store_with_extra_json(&o.store, &o.store_extra);
            let mut v = json!({
                "type": "creep", "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "body": o.body.iter().map(part_json).collect::<Vec<_>>(),
                "fatigue": o.fatigue,
                "ageTime": o.age_time,
                "hits": o.hits,
                "hitsMax": o.body.len() * 100,
                "store": store,
                // 活跃 CARRY（包含强化）决定容量，供 carryCapacity 和 Store 共用。
                "storeCapacity": crate::snapshot::creep_store_capacity_raw(&o.body),
                "spawning": o.spawning,
            });
            if o.age_time != 0 {
                v["ticksToLive"] = json!(o.age_time.saturating_sub(time));
            }
            // prelude 根据 owner 和 sayPublic 决定能否读到说话内容。
            // notifyWhenAttacked 原值用于避免重复产生相同开关指令。
            if let Some(m) = &o.say_message {
                v["say"] = json!(m);
                v["sayPublic"] = json!(o.say_public.unwrap_or(false));
            }
            if let Some(f) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(f);
            }
            v
        }
        RoomObject::Source(o) => json!({
            "type": "structure", "structureType": "source",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "energy": o.energy, "energyCapacity": o.energy_capacity,
            // 官方 getter（sources.js:38）：无再生钟为 undefined——投影以
            // null 保留，由 prelude getter 还原为 undefined，不折成 0。
            "ticksToRegeneration": crate::snapshot::remaining_opt(
                o.next_regeneration_time,
                time,
            ),
        }),
        RoomObject::Controller(o) => {
            let mut v = json!({
                "type": "structure", "structureType": "controller",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "level": o.level, "progress": o.progress,
                "progressTotal": controller_progress_total(o.level),
                "safeMode": o.safe_mode,
                "safeModeAvailable": o.safe_mode_available,
                "downgradeTime": o.downgrade_time,
                "isPowerEnabled": o.is_power_enabled,
                // 保留绝对 tick 和 reservation.endTime，prelude getter 再计算剩余时间。
                // sign.time 是世界 tick，sign.datetime 是签名时的 Unix 墙钟毫秒。
                "upgradeBlocked": o.upgrade_blocked,
                "safeModeCooldown": o.safe_mode_cooldown,
                "reservation": match &o.reservation {
                    Some(r) => json!({ "user": r.user, "endTime": r.end_time }),
                    None => Value::Null,
                },
                "sign": match &o.sign {
                    Some(s) => {
                        let mut sv = serde_json::Map::new();
                        sv.insert("user".into(), json!(s.user));
                        sv.insert("text".into(), json!(s.text));
                        sv.insert("time".into(), json!(s.time));
                        // 未设置 datetime 时省略该键；这与查询快照始终省略 datetime 不同。
                        if let Some(dt) = s.datetime {
                            sv.insert("datetime".into(), json!(dt));
                        }
                        Value::Object(sv)
                    }
                    None => Value::Null,
                },
            });

            if let Some(val) = crate::domain::controller_owner_value(o) {
                v["owner"] = val;
            }
            v
        }
        RoomObject::Spawn(o) => {
            // spawn 只有按资源的容量字段，没有通用 storeCapacity。
            // 这使 Store.getCapacity() 返回 null、getCapacity('energy') 返回数值。
            let mut v = json!({
                "type": "spawn", "structureType": "spawn",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "store": store_json(&o.store),
                "energy": o.store.energy,
                "energyCapacity": c::SPAWN_ENERGY_CAPACITY,
                "storeCapacityResource": { "energy": c::SPAWN_ENERGY_CAPACITY },
                // off 表示当前 RCL 配额下不可用，spawn API 会回查此字段。
                "off": o.off,
                "hits": o.hits, "hitsMax": c::SPAWN_HITS,
                "spawning": spawn_spawning_json(o),
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Extension(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "extension",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": store_json(&o.store),
            "energy": o.store.energy,
            // extension 按资源限制容量，能量容量随房间 RCL 改变。
            "storeCapacityResource": { "energy": crate::types::extension_energy_capacity(rcl) },
            "off": o.off,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::ConstructedWall(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "constructedWall",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": c::WALL_HITS_MAX,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Road(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "road",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": road_hits_max(terrain, o.x, o.y),
            "nextDecayTime": o.next_decay_time,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Storage(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "storage",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": store_with_extra_json(&o.store, &o.store_extra),
            // storage 使用通用容量，无 storeCapacityResource；容量含
            // 上轮对象 tick 保存的控制器基础量与 power 加成。
            "storeCapacity": o.store_capacity,
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::ConstructionSite(o) => {
            let mut v = json!({
                "type": "constructionSite",
                "structureType": o.structure_type,
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "progress": o.progress,
                "progressTotal": o.progress_total,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            if let Some(name) = &o.name {
                v["name"] = json!(name);
            }
            v
        }
        RoomObject::Energy(o) => {
            let mut v = json!({
                "type": "resource",
                "resourceType": energy_resource_name(o).to_string(),
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "amount": o.amount,
            });
            // energy 资源额外提供 energy 别名，其他资源没有该键。
            if v["resourceType"] == json!("energy") {
                v["energy"] = json!(o.amount);
            }
            v
        }

        RoomObject::Flag(o) => {
            let mut v = json!({
                "type": "flag",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "color": o.color, "secondaryColor": o.secondary_color,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        // 墓碑保留绝对 decayTime，并提供非负 ticksToDecay。
        // creepBody 用部件名，creepId 用玩家可见 ID；主人字段为 user，供 prelude 读取。
        RoomObject::Tombstone(o) => {
            let mut v = json!({
                "type": "tombstone",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "deathTime": o.death_time,
                "decayTime": o.decay_time,
                "ticksToDecay": o.decay_time.saturating_sub(time),
                "creepId": match o.creep_id {
                    Some(cid) => json!(crate::world::ObjId(cid).as_str()),
                    None => Value::Null,
                },
                "creepName": o.creep_name,
                "creepTicksToLive": o.creep_ticks_to_live,
                "creepBody": o.creep_body.iter().filter_map(|code| {
                    crate::types::BodyKind::from_code(*code).map(crate::types::BodyKind::name)
                }).collect::<Vec<_>>(),
                "store": store_with_extra_json(&o.store, &o.store_extra),
            });
            if let Some(saying) = &o.creep_saying {
                v["creepSaying"] = json!(saying);
            }
            if let Some(owner) = &o.owner {
                v["user"] = json!(owner);
            }
            v
        }
        // 废墟保留被毁结构的 ID、类型及最大 hits，当前 hits 固定为 0。
        // 嵌套类型字段必须叫 type，prelude 的 Ruin.structure 从这里读取。
        // 主人同时写入顶层和 structure.user。
        // 来源：engine/src/processor/intents/structures/_destroy.js。
        RoomObject::Ruin(o) => {
            let mut structure = json!({
                "id": crate::world::ObjId(o.structure_id).as_str(),
                "type": o.structure_type,
                "hits": 0,
            });
            if let Some(hits_max) = o.structure_hits_max {
                structure["hitsMax"] = json!(hits_max);
            }
            if let Some(owner) = &o.owner {
                structure["user"] = json!(owner);
            }
            let mut v = json!({
                "type": "ruin",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "structure": structure,
                "destroyTime": o.destroy_time,
                "decayTime": o.decay_time,
                "ticksToDecay": o.decay_time.saturating_sub(time),
                "store": store_with_extra_json(&o.store, &o.store_extra),
            });
            if let Some(owner) = &o.owner {
                v["user"] = json!(owner);
            }
            v
        }
        // 无再生时间时提供 0；有值时提供非负剩余 tick。
        // 虽使用原始 type='structure'，矿物仍须在 FIND_STRUCTURES 中排除。
        RoomObject::Mineral(o) => json!({
            "type": "structure", "structureType": "mineral",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "mineralType": o.mineral_type,
            "mineralAmount": o.mineral_amount,
            "density": o.density,
            // 同 source：官方 minerals.js:39 无钟为 undefined，投影保留 null。
            "ticksToRegeneration": crate::snapshot::remaining_opt(
                o.next_regeneration_time,
                time,
            ),
        }),
        // extractor 的 cooldown 是逐 tick 递减的计数，不能当成截止时间换算。
        RoomObject::Extractor(o) => {
            let mut v = json!({
                "type": "structure", "structureType": "extractor",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "hits": o.hits, "hitsMax": c::EXTRACTOR_HITS,
                "cooldown": o.cooldown,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Generic(o) => {
            // 恢复 Generic 的 JSON，ID 和位置以对象表中的字段为准。
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or(json!({}));
            if let Some(map) = v.as_object_mut() {
                // 当前世界只运行 shard0，房内 powerCreep 的生成态决定 shard；
                // 派生字段不入持久数据，旧检查点与放置路径采用同一口径。
                if map.get("type").and_then(Value::as_str) == Some("powerCreep") {
                    map.insert("shard".into(), json!("shard0"));
                }
                map.insert("id".into(), json!(id_str));
                map.insert("room".into(), json!(room));
                map.insert("x".into(), json!(o.x));
                map.insert("y".into(), json!(o.y));
            }
            v
        }
        // Mod 对象不产出原始记录（见函数注释）：直接短路返回，防止
        // 未分流的调用方拿到残影记录。
        RoomObject::Mod(_) => return None,
    };
    // pos 供使用原始 JSON 的调用方读取；快照模块另行构造查询格式。
    if let Some(obj) = v.as_object_mut() {
        let x = obj.get("x").cloned().unwrap_or(json!(0));
        let y = obj.get("y").cloned().unwrap_or(json!(0));
        let room_name = obj.get("room").cloned().unwrap_or(json!(""));
        obj.insert(
            "pos".into(),
            json!({ "x": x, "y": y, "roomName": room_name }),
        );
    }
    crate::effects::project_raw(&mut v, effects, time);
    Some(v)
}

pub(crate) fn part_json(p: &kk_schema::room::BodyPart) -> Value {
    let kind = crate::types::BodyKind::from_code(p.kind).map(crate::types::BodyKind::name);
    match (&p.boost, kind) {
        (Some(b), Some(k)) => json!({ "type": k, "hits": p.hits, "boost": b }),
        (None, Some(k)) => json!({ "type": k, "hits": p.hits }),
        (_, None) => json!({ "hits": p.hits }),
    }
}

/// 孵化任务保留绝对 spawnTime，供 prelude 计算 remainingTime。
/// 空 directions 表示未指定，省略键以使玩家读到 undefined。
/// 来源：engine/src/processor/intents/spawns/create-creep.js。
fn spawn_spawning_json(o: &kk_schema::room::SpawnObj) -> Value {
    match &o.spawning {
        None => Value::Null,
        Some(job) => {
            let mut v = json!({
                "name": job.name, "needTime": job.need_time,
                "spawnTime": job.spawn_time,
            });
            if !job.directions.is_empty() {
                v["directions"] = json!(job.directions);
            }
            v
        }
    }
}

/// 原始沙箱值使用 0 表示无升级进度条，查询快照则使用 null。
fn controller_progress_total(level: u8) -> i64 {
    crate::domain::controller_progress_total_opt(level).unwrap_or(0)
}
