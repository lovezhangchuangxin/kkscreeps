//! 单对象 → KKPV 借用行的转换与单房直写编码：Generic/Mod 走解析
//! 保活 + 借用参考记录，类型化对象直接构造（跳过 serde_json 与 owned
//! `KkpvObject` 两层中间树）。字段集与嵌套键序和 `object_json` +
//! `record_to_kkpv` 时代的产物逐字段等价，由
//! `shared_bundle_assembly_matches_per_viewer_encoding` 与双通道等价
//! 测试锁定。

use std::borrow::Cow;

use super::*;

/// 单房私有块的编码身份：块头 tick/generation 与宿主摘要。`private`
/// 恒真——公开地形块走 [`World::kkpv_public_terrain_blocks`] 另一条路。
#[derive(Clone, Copy)]
pub(super) struct RoomBlockMeta {
    pub(super) tick: u64,
    pub(super) generation: u64,
    pub(super) plan_hash: [u8; 32],
    pub(super) audience_hash: [u8; 32],
}

/// 类型化对象的 structureType 名（object_json 记录同源）；非结构与
/// Generic/Mod 返回 None——后者由调用方从参考记录读取。
pub(super) fn typed_structure_name(object: &RoomObject) -> Option<&str> {
    match object {
        RoomObject::Spawn(_) => Some("spawn"),
        RoomObject::Source(_) => Some("source"),
        RoomObject::Controller(_) => Some("controller"),
        RoomObject::Extension(_) => Some("extension"),
        RoomObject::ConstructedWall(_) => Some("constructedWall"),
        RoomObject::Road(_) => Some("road"),
        RoomObject::Storage(_) => Some("storage"),
        RoomObject::Mineral(_) => Some("mineral"),
        RoomObject::Extractor(_) => Some("extractor"),
        RoomObject::ConstructionSite(o) => Some(o.structure_type.as_str()),
        _ => None,
    }
}

/// 结构类型桶键 `builtin:{structureType}`：结构类型集由官方常量冻结，
/// 静态映射避免逐对象格式化分配；未知类型（Mod 自定义目标结构）回退
/// 运行时拼装，产物字符串等价。
fn builtin_structure_key(structure_type: &str) -> Cow<'static, str> {
    let key = match structure_type {
        "spawn" => "builtin:spawn",
        "extension" => "builtin:extension",
        "road" => "builtin:road",
        "constructedWall" => "builtin:constructedWall",
        "rampart" => "builtin:rampart",
        "keeperLair" => "builtin:keeperLair",
        "portal" => "builtin:portal",
        "controller" => "builtin:controller",
        "link" => "builtin:link",
        "storage" => "builtin:storage",
        "tower" => "builtin:tower",
        "observer" => "builtin:observer",
        "powerBank" => "builtin:powerBank",
        "powerSpawn" => "builtin:powerSpawn",
        "extractor" => "builtin:extractor",
        "lab" => "builtin:lab",
        "terminal" => "builtin:terminal",
        "container" => "builtin:container",
        "nuker" => "builtin:nuker",
        "factory" => "builtin:factory",
        "invaderCore" => "builtin:invaderCore",
        "source" => "builtin:source",
        "mineral" => "builtin:mineral",
        _ => return Cow::Owned(format!("builtin:{structure_type}")),
    };
    Cow::Borrowed(key)
}

/// 参考记录的 KKPV 桶键（与旧 `type_key_of` 等价）：结构/出生点用
/// `builtin:{structureType}`，Mod 记录用类型键本身，其余 `builtin:{type}`。
pub(super) fn type_key_cow(record: &Value) -> Cow<'_, str> {
    let kind = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("generic");
    match kind {
        // Mod 记录的桶键是类型键本身（namespace:name 形，非 builtin 桶）。
        "mod" => Cow::Borrowed(
            record
                .get("modType")
                .and_then(Value::as_str)
                .unwrap_or("builtin:mod"),
        ),
        "structure" | "spawn" => match record.get("structureType").and_then(Value::as_str) {
            Some(st) => builtin_structure_key(st),
            None => Cow::Owned(format!("builtin:{kind}")),
        },
        "creep" => Cow::Borrowed("builtin:creep"),
        "constructionSite" => Cow::Borrowed("builtin:constructionSite"),
        "resource" => Cow::Borrowed("builtin:resource"),
        "flag" => Cow::Borrowed("builtin:flag"),
        "tombstone" => Cow::Borrowed("builtin:tombstone"),
        "ruin" => Cow::Borrowed("builtin:ruin"),
        other => Cow::Owned(format!("builtin:{other}")),
    }
}

/// `object_json` 记录值的借用 KKPV 转换：数值 as_i64 优先（与
/// `value_to_kkpv` 同序），字符串与对象键零拷贝借用。
fn json_val(value: &Value) -> Result<KkpvVal<'_>, String> {
    Ok(match value {
        Value::Null => KkpvVal::Null,
        Value::Bool(b) => KkpvVal::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                KkpvVal::Int(i)
            } else if let Some(u) = n.as_u64() {
                KkpvVal::Uint(u)
            } else if let Some(f) = n.as_f64() {
                // JSON 数值不会是非有限浮点；此处仅为穷尽性防御。
                KkpvVal::Float(f)
            } else {
                return Err(format!("无法编码的数值 {n}"));
            }
        }
        Value::String(s) => KkpvVal::Str(s),
        Value::Array(items) => items
            .iter()
            .map(json_val)
            .collect::<Result<Vec<_>, _>>()
            .map(KkpvVal::Array)?,
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| json_val(v).map(|v| (k.as_str(), v)))
            .collect::<Result<Vec<_>, _>>()
            .map(KkpvVal::Object)?,
    })
}

/// 解析参考记录 → 借用行：`id` 由目录承担跳过；目录 ext 存在时原生
/// `ext` 键按覆盖语义丢弃，由调用方追加目录版本。
fn json_row(id: ObjId, record: &Value, ext_replaced: bool) -> Result<KkpvRow<'_>, String> {
    let fields_src = record.as_object().ok_or("对象记录必须是 JSON 对象")?;
    let mut fields = Vec::with_capacity(fields_src.len());
    for (key, value) in fields_src {
        if key == "id" || (ext_replaced && key == "ext") {
            continue;
        }
        fields.push((key.as_str(), json_val(value)?));
    }
    Ok(KkpvRow {
        id: id.0,
        type_key: type_key_cow(record),
        fields,
    })
}

/// 类型化对象的借用行构造：跳过 `object_json` 的 serde_json Value 与
/// owned `KkpvObject` 两层中间树，字段集、键序（嵌套内）、数值形态
/// （整数一律 Int，与 as_i64 优先序一致）和 pos/effects 尾部与 JSON
/// 参考通道逐字段等价。Generic/Mod 不走本路径：data 本身就是 JSON，
/// 解析不可省，由调用方保活参考记录后走 [`json_row`]。
#[allow(clippy::too_many_arguments)]
fn object_row<'a>(
    room: &'a str,
    id: ObjId,
    object: &'a RoomObject,
    time: u64,
    terrain: &'a kk_core::Terrain,
    controller: Option<&kk_schema::room::ControllerObj>,
    effects: Option<&'a Vec<crate::EffectEntry>>,
) -> KkpvRow<'a> {
    use kk_core::constants as c;
    use kk_schema::room::{
        BodyPart, ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj, ExtensionObj, FlagObj,
        MineralObj, RoadObj, RoomObject as RO, RuinObj, SourceObj, SpawnObj, StorageObj,
        TombstoneObj,
    };
    let mut fields: Vec<(&'a str, KkpvVal<'a>)> = Vec::new();
    let mut kv = |key: &'static str, value: KkpvVal<'a>| fields.push((key, value));
    let int = |v: i64| KkpvVal::Int(v);
    let strv = |v: &'a str| KkpvVal::Str(v);
    // store 键序与 store_json/store_with_extra_json 的插入序一致：
    // 正量 energy、正量 G、energy（含 0，覆盖或追加）、正量额外资源。
    let store_val = |store: &'a kk_schema::room::Store,
                     extra: &'a [kk_schema::room::ResourceStack]| {
        let mut entries: Vec<(&'a str, KkpvVal<'a>)> = Vec::new();
        if store.energy > 0 {
            entries.push(("energy", int(store.energy as i64)));
        }
        if store.ghodium > 0 {
            entries.push((c::RESOURCE_GHODIUM, int(store.ghodium as i64)));
        }
        if let Some(entry) = entries.iter_mut().find(|(key, _)| *key == "energy") {
            entry.1 = int(store.energy as i64);
        } else {
            entries.push(("energy", int(store.energy as i64)));
        }
        for stack in extra {
            if stack.amount == 0 {
                continue;
            }
            if let Some(entry) = entries
                .iter_mut()
                .find(|(key, _)| *key == stack.resource.as_str())
            {
                entry.1 = int(stack.amount as i64);
            } else {
                entries.push((stack.resource.as_str(), int(stack.amount as i64)));
            }
        }
        KkpvVal::Object(entries)
    };
    let part_val = |p: &'a BodyPart| {
        let kind = crate::types::BodyKind::from_code(p.kind).map(crate::types::BodyKind::name);
        let mut entries = Vec::new();
        if let Some(kind) = kind {
            entries.push(("type", strv(kind)));
        }
        entries.push(("hits", int(p.hits as i64)));
        if let Some(boost) = &p.boost {
            entries.push(("boost", strv(boost)));
        }
        KkpvVal::Object(entries)
    };
    let type_key: Cow<'a, str> = match object {
        RO::Creep(_) => Cow::Borrowed("builtin:creep"),
        RO::Spawn(_) => Cow::Borrowed("builtin:spawn"),
        RO::ConstructionSite(o) => builtin_structure_key(&o.structure_type),
        RO::Energy(_) => Cow::Borrowed("builtin:resource"),
        RO::Flag(_) => Cow::Borrowed("builtin:flag"),
        RO::Tombstone(_) => Cow::Borrowed("builtin:tombstone"),
        RO::Ruin(_) => Cow::Borrowed("builtin:ruin"),
        RO::Source(_) => Cow::Borrowed("builtin:source"),
        RO::Mineral(_) => Cow::Borrowed("builtin:mineral"),
        // 其余 typed 变体都是 structure 记录，桶键由 structureType 决定。
        RO::Controller(_) => Cow::Borrowed("builtin:controller"),
        RO::Extension(_) => Cow::Borrowed("builtin:extension"),
        RO::ConstructedWall(_) => Cow::Borrowed("builtin:constructedWall"),
        RO::Road(_) => Cow::Borrowed("builtin:road"),
        RO::Storage(_) => Cow::Borrowed("builtin:storage"),
        RO::Extractor(_) => Cow::Borrowed("builtin:extractor"),
        RO::Generic(_) | RO::Mod(_) => unreachable!("object_row 只处理类型化对象"),
    };

    match object {
        RO::Creep(o) => {
            let CreepObj {
                x,
                y,
                hits,
                spawning,
                age_time,
                name,
                owner,
                body,
                store,
                store_extra,
                say_message,
                say_public,
                notify_when_attacked,
                ..
            } = o;
            kv("type", strv("creep"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("name", strv(name));
            kv("owner", strv(owner));
            kv("body", KkpvVal::Array(body.iter().map(part_val).collect()));
            kv("fatigue", int(o.fatigue as i64));
            kv("ageTime", int(*age_time as i64));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(body.len() as i64 * 100));
            kv("store", store_val(store, store_extra));
            kv(
                "storeCapacity",
                int(crate::snapshot::creep_store_capacity_raw(body)),
            );
            kv("spawning", KkpvVal::Bool(*spawning));
            if *age_time != 0 {
                kv("ticksToLive", int(age_time.saturating_sub(time) as i64));
            }
            if let Some(message) = say_message {
                kv("say", strv(message));
                kv("sayPublic", KkpvVal::Bool(say_public.unwrap_or(false)));
            }
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::Source(o) => {
            let SourceObj {
                x,
                y,
                energy,
                energy_capacity,
                next_regeneration_time,
                ..
            } = o;
            kv("type", strv("structure"));
            kv("structureType", strv("source"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("energy", int(*energy as i64));
            kv("energyCapacity", int(*energy_capacity as i64));
            kv(
                "ticksToRegeneration",
                match next_regeneration_time {
                    Some(t) if *t != 0 => int(t.saturating_sub(time) as i64),
                    _ => KkpvVal::Null,
                },
            );
        }
        RO::Controller(o) => {
            let ControllerObj {
                x,
                y,
                level,
                safe_mode,
                safe_mode_available,
                progress,
                downgrade_time,
                owner,
                owner_cleared,
                is_power_enabled,
                reservation,
                sign,
                upgrade_blocked,
                safe_mode_cooldown,
                ..
            } = o;
            kv("type", strv("structure"));
            kv("structureType", strv("controller"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("level", int(*level as i64));
            kv("progress", int(*progress as i64));
            kv(
                "progressTotal",
                int(crate::domain::controller_progress_total_opt(*level).unwrap_or(0)),
            );
            kv("safeMode", int(*safe_mode as i64));
            kv("safeModeAvailable", int(*safe_mode_available as i64));
            kv(
                "downgradeTime",
                downgrade_time
                    .map(|t| int(t as i64))
                    .unwrap_or(KkpvVal::Null),
            );
            kv("isPowerEnabled", KkpvVal::Bool(*is_power_enabled));
            kv(
                "upgradeBlocked",
                upgrade_blocked
                    .map(|t| int(t as i64))
                    .unwrap_or(KkpvVal::Null),
            );
            kv(
                "safeModeCooldown",
                safe_mode_cooldown
                    .map(|t| int(t as i64))
                    .unwrap_or(KkpvVal::Null),
            );
            kv(
                "reservation",
                match reservation {
                    Some(r) => KkpvVal::Object(vec![
                        ("user", strv(&r.user)),
                        ("endTime", int(r.end_time as i64)),
                    ]),
                    None => KkpvVal::Null,
                },
            );
            kv(
                "sign",
                match sign {
                    Some(sv) => {
                        let mut entries = vec![
                            ("user", strv(&sv.user)),
                            ("text", strv(&sv.text)),
                            ("time", int(sv.time as i64)),
                        ];
                        if let Some(datetime) = sv.datetime {
                            entries.push(("datetime", int(datetime as i64)));
                        }
                        KkpvVal::Object(entries)
                    }
                    None => KkpvVal::Null,
                },
            );
            match (owner, owner_cleared) {
                (Some(owner), _) => kv("owner", strv(owner)),
                (None, true) => kv("owner", KkpvVal::Null),
                (None, false) => {}
            }
        }
        RO::Spawn(o) => {
            let SpawnObj {
                x,
                y,
                hits,
                off,
                name,
                owner,
                store,
                spawning,
                notify_when_attacked,
                ..
            } = o;
            kv("type", strv("spawn"));
            kv("structureType", strv("spawn"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("name", strv(name));
            kv("owner", strv(owner));
            kv("store", store_val(store, &[]));
            kv("energy", int(store.energy as i64));
            kv("energyCapacity", int(c::SPAWN_ENERGY_CAPACITY));
            kv(
                "storeCapacityResource",
                KkpvVal::Object(vec![("energy", int(c::SPAWN_ENERGY_CAPACITY))]),
            );
            kv("off", KkpvVal::Bool(*off));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::SPAWN_HITS));
            kv(
                "spawning",
                match spawning {
                    None => KkpvVal::Null,
                    Some(job) => {
                        let mut entries = vec![
                            ("name", strv(&job.name)),
                            ("needTime", int(job.need_time as i64)),
                            ("spawnTime", int(job.spawn_time as i64)),
                        ];
                        if !job.directions.is_empty() {
                            entries.push((
                                "directions",
                                KkpvVal::Array(
                                    job.directions.iter().map(|d| int(*d as i64)).collect(),
                                ),
                            ));
                        }
                        KkpvVal::Object(entries)
                    }
                },
            );
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::Extension(o) => {
            let ExtensionObj {
                x,
                y,
                hits,
                off,
                owner,
                store,
                notify_when_attacked,
                ..
            } = o;
            let rcl = controller.map(|ctl| ctl.level).unwrap_or(0);
            kv("type", strv("structure"));
            kv("structureType", strv("extension"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("owner", strv(owner));
            kv("store", store_val(store, &[]));
            kv("energy", int(store.energy as i64));
            kv(
                "storeCapacityResource",
                KkpvVal::Object(vec![(
                    "energy",
                    int(crate::types::extension_energy_capacity(rcl)),
                )]),
            );
            kv("off", KkpvVal::Bool(*off));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::EXTENSION_HITS));
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::ConstructedWall(o) => {
            kv("type", strv("structure"));
            kv("structureType", strv("constructedWall"));
            kv("room", strv(room));
            kv("x", int(o.x as i64));
            kv("y", int(o.y as i64));
            kv("hits", int(o.hits as i64));
            kv("hitsMax", int(c::WALL_HITS_MAX));
            if let Some(flag) = &o.notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::Road(o) => {
            let RoadObj {
                x,
                y,
                hits,
                next_decay_time,
                notify_when_attacked,
                ..
            } = o;
            kv("type", strv("structure"));
            kv("structureType", strv("road"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("hits", int(*hits as i64));
            kv(
                "hitsMax",
                int(crate::projection::road_hits_max(terrain, *x, *y)),
            );
            kv("nextDecayTime", int(*next_decay_time as i64));
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::Storage(o) => {
            let StorageObj {
                x,
                y,
                hits,
                owner,
                store,
                store_extra,
                store_capacity,
                notify_when_attacked,
                ..
            } = o;
            kv("type", strv("structure"));
            kv("structureType", strv("storage"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("owner", strv(owner));
            kv("store", store_val(store, store_extra));
            kv("storeCapacity", int(*store_capacity));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::STORAGE_HITS));
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        RO::ConstructionSite(o) => {
            let ConstructionSiteObj {
                x,
                y,
                structure_type,
                progress,
                progress_total,
                owner,
                name,
                ..
            } = o;
            kv("type", strv("constructionSite"));
            kv("structureType", strv(structure_type));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("progress", int(*progress as i64));
            kv("progressTotal", int(*progress_total as i64));
            if let Some(owner) = owner {
                kv("owner", strv(owner));
            }
            if let Some(name) = name {
                kv("name", strv(name));
            }
        }
        RO::Energy(o) => {
            let EnergyObj { x, y, amount, .. } = o;
            // 与 energy_resource_name 同源的借用形态：o 借用自 'a，
            // 行内静态/字段引用皆满足行存活期。
            let resource_name: &str = match (&o.resource, o.resource_type) {
                (Some(name), _) => name.as_str(),
                (None, 1) => "power",
                (None, 2) => c::RESOURCE_GHODIUM,
                (None, _) => "energy",
            };
            kv("type", strv("resource"));
            kv("resourceType", strv(resource_name));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("amount", int(*amount as i64));
            if resource_name == "energy" {
                kv("energy", int(*amount as i64));
            }
        }
        RO::Flag(o) => {
            let FlagObj {
                x,
                y,
                name,
                color,
                secondary_color,
                owner,
                ..
            } = o;
            kv("type", strv("flag"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("name", strv(name));
            kv("color", int(*color as i64));
            kv("secondaryColor", int(*secondary_color as i64));
            if let Some(owner) = owner {
                kv("owner", strv(owner));
            }
        }
        RO::Tombstone(o) => {
            let TombstoneObj {
                x,
                y,
                death_time,
                decay_time,
                creep_id,
                creep_name,
                creep_ticks_to_live,
                creep_body,
                creep_saying,
                owner,
                store,
                store_extra,
                ..
            } = o;
            kv("type", strv("tombstone"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("deathTime", int(*death_time as i64));
            kv("decayTime", int(*decay_time as i64));
            kv("ticksToDecay", int(decay_time.saturating_sub(time) as i64));
            kv(
                "creepId",
                creep_id.map(KkpvVal::Ref).unwrap_or(KkpvVal::Null),
            );
            kv(
                "creepName",
                creep_name.as_deref().map(strv).unwrap_or(KkpvVal::Null),
            );
            kv("creepTicksToLive", int(*creep_ticks_to_live as i64));
            kv(
                "creepBody",
                KkpvVal::Array(
                    creep_body
                        .iter()
                        .filter_map(|code| {
                            crate::types::BodyKind::from_code(*code)
                                .map(crate::types::BodyKind::name)
                                .map(strv)
                        })
                        .collect(),
                ),
            );
            kv("store", store_val(store, store_extra));
            if let Some(saying) = creep_saying {
                kv("creepSaying", strv(saying));
            }
            if let Some(owner) = owner {
                kv("user", strv(owner));
            }
        }
        RO::Ruin(o) => {
            let RuinObj {
                x,
                y,
                structure_id,
                structure_type,
                structure_hits_max,
                destroy_time,
                decay_time,
                owner,
                store,
                store_extra,
                ..
            } = o;
            let mut structure = vec![
                ("id", KkpvVal::Ref(*structure_id)),
                ("type", strv(structure_type)),
                ("hits", int(0)),
            ];
            if let Some(hits_max) = structure_hits_max {
                structure.push(("hitsMax", int(*hits_max as i64)));
            }
            if let Some(owner) = owner {
                structure.push(("user", strv(owner)));
            }
            kv("type", strv("ruin"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("structure", KkpvVal::Object(structure));
            kv("destroyTime", int(*destroy_time as i64));
            kv("decayTime", int(*decay_time as i64));
            kv("ticksToDecay", int(decay_time.saturating_sub(time) as i64));
            kv("store", store_val(store, store_extra));
            if let Some(owner) = owner {
                kv("user", strv(owner));
            }
        }
        RO::Mineral(o) => {
            let MineralObj {
                x,
                y,
                mineral_type,
                mineral_amount,
                density,
                next_regeneration_time,
                ..
            } = o;
            kv("type", strv("structure"));
            kv("structureType", strv("mineral"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("mineralType", strv(mineral_type));
            kv("mineralAmount", int(*mineral_amount as i64));
            kv("density", int(*density as i64));
            kv(
                "ticksToRegeneration",
                match next_regeneration_time {
                    Some(t) if *t != 0 => int(t.saturating_sub(time) as i64),
                    _ => KkpvVal::Null,
                },
            );
        }
        RO::Extractor(o) => {
            kv("type", strv("structure"));
            kv("structureType", strv("extractor"));
            kv("room", strv(room));
            kv("x", int(o.x as i64));
            kv("y", int(o.y as i64));
            kv("hits", int(o.hits as i64));
            kv("hitsMax", int(c::EXTRACTOR_HITS));
            kv("cooldown", int(o.cooldown as i64));
            if let Some(owner) = &o.owner {
                kv("owner", strv(owner));
            }
            if let Some(flag) = &o.notify_when_attacked {
                kv("notifyWhenAttacked", KkpvVal::Bool(*flag));
            }
        }
        // Generic/Mod 不进入本函数：调用方在编码入口分流。
        RO::Generic(_) | RO::Mod(_) => unreachable!("object_row 只处理类型化对象"),
    };
    // 与 object_json 的公共尾部同形：pos 供使用原始 JSON 的调用方读取。
    kv(
        "pos",
        KkpvVal::Object(vec![
            ("x", KkpvVal::Int(object.pos().0 as i64)),
            ("y", KkpvVal::Int(object.pos().1 as i64)),
            ("roomName", strv(room)),
        ]),
    );
    if let Some(entries) = effects {
        kv(
            "effects",
            KkpvVal::Array(
                entries
                    .iter()
                    .map(|e| {
                        let mut entry = Vec::new();
                        if let Some(effect) = e.effect {
                            entry.push(("effect", int(effect)));
                        }
                        entry.push(("power", int(e.power)));
                        entry.push(("level", int(e.level)));
                        entry.push(("endTime", int(e.end_time as i64)));
                        entry.push((
                            "ticksRemaining",
                            int(e.end_time.saturating_sub(time) as i64),
                        ));
                        KkpvVal::Object(entry)
                    })
                    .collect(),
            ),
        );
    }
    KkpvRow {
        id: id.0,
        type_key,
        fields,
    }
}

/// 单房私有块直写编码：可见性门、导出门、记录构造与 ext 裁切的完整
/// 路径（与旧 `kkpv_object_of` 逐对象同序同义），行借用权威对象与
/// 参考记录后直接走 `encode_ref`，不构建 owned `KkpvObject` 树。
/// `candidates` 为 None 表示可见房全对象枚举；Some 表示不可见房的
/// 候选对象（索引给出，ID 升序），与全量枚举的相对顺序一致。
///
/// # Errors
/// 对象记录无法映射为 KKPV 值形态或编码超限时返回错误。
#[allow(clippy::too_many_arguments)]
pub(super) fn encode_room_block(
    room: &str,
    state: &crate::world::RoomState,
    controller: Option<&kk_schema::room::ControllerObj>,
    time: u64,
    meta: RoomBlockMeta,
    user: &str,
    room_visible: bool,
    candidates: Option<&[ObjId]>,
    catalog: &kk_mod_api::extension::ExtensionCatalog,
    surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
) -> Result<Vec<u8>, String> {
    // 第一遍：门与解析。Generic/Mod 的参考记录先解析保活（行借用其
    // 内容，owned 数据在本函数存活期内不释放）；ext 对同序预计算。
    enum Slot {
        Typed,
        Parsed(usize),
    }
    let mut ids: Vec<ObjId> = Vec::new();
    let mut slots: Vec<Slot> = Vec::new();
    let mut parsed: Vec<Value> = Vec::new();
    let mut exts: Vec<Option<serde_json::Map<String, Value>>> = Vec::new();
    let mut collect = |id: ObjId, object: &RoomObject| {
        // Mod 分支先于内建导出门（与 world_json_impl 同序）：
        // mod_record_json 自带完整的对象级/字段级门——OwnerCollection
        // 的归属者集合语义不受房间视野门，若先过 object_exported_for
        // 会在不可见房被丢弃，造成与 JSON 通道的可见性分叉。
        if let RoomObject::Mod(mod_object) = object {
            if let Some(surface) = surface
                && let Some(record) = crate::projection::mod_record_json(
                    room,
                    id,
                    mod_object,
                    surface,
                    Some(user),
                    room_visible,
                )
            {
                parsed.push(record);
                ids.push(id);
                slots.push(Slot::Parsed(parsed.len() - 1));
                exts.push(None);
            }
            return;
        }
        if !object_exported_for(Some(user), room_visible, object) {
            return;
        }
        // Generic 保持 Value 参考路径：data 本身就是 JSON，解析不可省。
        let slot = match object {
            RoomObject::Generic(_) => {
                let record = object_json(
                    room,
                    id,
                    object,
                    time,
                    &state.terrain,
                    controller,
                    state.effects.get(&id),
                )
                .expect("Mod 对象已在 encode_room_block 上文分流");
                parsed.push(record);
                Slot::Parsed(parsed.len() - 1)
            }
            _ => Slot::Typed,
        };
        // ext 目录按 structureType 登记：Generic 从本对象的参考记录读取
        // （parsed 尾部即刚推入者），类型化对象的结构名从权威枚举派生
        // （与 object_json 输出同源）。
        let structure_type = match &slot {
            Slot::Parsed(_) => parsed
                .last()
                .and_then(|record| record.get("structureType").and_then(Value::as_str)),
            Slot::Typed => None,
        }
        .or_else(|| typed_structure_name(object));
        let ext = structure_type
            .and_then(|st| catalog.type_for_structure(st))
            .and_then(|type_key| {
                ext_entries_json(
                    catalog,
                    type_key,
                    &state.extensions,
                    id,
                    crate::domain::object_owner(object).as_deref(),
                    user,
                )
            });
        ids.push(id);
        slots.push(slot);
        exts.push(ext);
    };
    match candidates {
        None => {
            for (id, object) in &state.objects {
                collect(*id, object);
            }
        }
        Some(candidates) => {
            for id in candidates {
                collect(*id, &state.objects[id]);
            }
        }
    }
    // 第二遍：建行（借用第一遍的保活数据）并直写编码。
    let mut rows: Vec<KkpvRow<'_>> = Vec::with_capacity(ids.len());
    for (i, (id, slot)) in ids.iter().zip(&slots).enumerate() {
        let object = &state.objects[id];
        let ext = exts[i].as_ref();
        let ext_replaced = ext.is_some();
        let mut row = match slot {
            Slot::Typed => object_row(
                room,
                *id,
                object,
                time,
                &state.terrain,
                controller,
                state.effects.get(id),
            ),
            Slot::Parsed(p) => json_row(*id, &parsed[*p], ext_replaced)?,
        };
        if let Some(ext) = ext {
            // ext 以嵌套记录进入块字段，键与值和 JSON 投影的 ext 键一致。
            let mut entries = Vec::with_capacity(ext.len());
            for (key, value) in ext {
                let converted = json_val(value).map_err(|e| format!("ext {key}: {e}"))?;
                entries.push((key.as_str(), converted));
            }
            row.fields.push(("ext", KkpvVal::Object(entries)));
        }
        rows.push(row);
    }
    let block = KkpvBlockRef {
        private: true,
        tick: meta.tick,
        generation: meta.generation,
        plan_hash: meta.plan_hash,
        audience_hash: meta.audience_hash,
        room_name: room,
        terrain: None,
        rows,
    };
    encode_ref(&block, &KkpvLimits::default()).map_err(|e| e.to_string())
}
