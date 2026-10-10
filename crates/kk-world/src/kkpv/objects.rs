//! 单对象 → KKPV 块对象的转换：Generic 走 JSON 参考路径，类型化对象
//! 直接构造（跳过 serde_json 中间树）。字段集与嵌套键序和
//! `object_json` + `record_to_kkpv` 的产物逐字段等价，由
//! `shared_bundle_assembly_matches_per_viewer_encoding` 等测试锁定。

use super::*;

/// 类型化对象的 structureType 名（object_json 记录同源）；非结构与
/// Generic 返回 None——后者由调用方从参考记录读取。
pub(super) fn typed_structure_name(object: &RoomObject) -> Option<String> {
    match object {
        RoomObject::Spawn(_) => Some("spawn".into()),
        RoomObject::Source(_) => Some("source".into()),
        RoomObject::Controller(_) => Some("controller".into()),
        RoomObject::Extension(_) => Some("extension".into()),
        RoomObject::ConstructedWall(_) => Some("constructedWall".into()),
        RoomObject::Road(_) => Some("road".into()),
        RoomObject::Storage(_) => Some("storage".into()),
        RoomObject::Mineral(_) => Some("mineral".into()),
        RoomObject::Extractor(_) => Some("extractor".into()),
        RoomObject::ConstructionSite(o) => Some(o.structure_type.clone()),
        _ => None,
    }
}

/// 类型化对象的直接 KKPV 构造：跳过 `object_json` 的 serde_json Value
/// 中间树，字段集、键序（嵌套内）、数值形态（整数一律 Int，与
/// value_to_kkpv 的 as_i64 优先序一致）和 pos/effects 尾部与
/// `object_json` + `record_to_kkpv` 的产物逐字段等价——由
/// `shared_bundle_assembly_matches_per_viewer_encoding` 等测试锁定。
/// Generic 不走本路径：data 本身就是 JSON，解析不可省。
pub(super) fn object_kkpv(
    room: &str,
    id: ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
    controller: Option<&kk_schema::room::ControllerObj>,
    effects: Option<&Vec<crate::EffectEntry>>,
) -> KkpvObject {
    use kk_core::constants as c;
    use kk_schema::room::{
        BodyPart, ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj, ExtensionObj, FlagObj,
        MineralObj, RoadObj, RoomObject as RO, RuinObj, SourceObj, SpawnObj, StorageObj,
        TombstoneObj,
    };
    let mut fields: Vec<(String, KkpvValue)> = Vec::new();
    let mut kv = |key: &str, value: KkpvValue| fields.push((key.to_string(), value));
    let int = |v: i64| KkpvValue::Int(v);
    let strv = |v: &str| KkpvValue::Str(v.to_string());
    // store 键序与 store_json/store_with_extra_json 的插入序一致：
    // 正量 energy、正量 G、energy（含 0，覆盖或追加）、正量额外资源。
    let store_kkpv = |store: &kk_schema::room::Store, extra: &[kk_schema::room::ResourceStack]| {
        let mut entries: Vec<(String, KkpvValue)> = Vec::new();
        if store.energy > 0 {
            entries.push(("energy".into(), int(store.energy as i64)));
        }
        if store.ghodium > 0 {
            entries.push((
                kk_core::constants::RESOURCE_GHODIUM.into(),
                int(store.ghodium as i64),
            ));
        }
        if let Some(entry) = entries.iter_mut().find(|(key, _)| key == "energy") {
            entry.1 = int(store.energy as i64);
        } else {
            entries.push(("energy".into(), int(store.energy as i64)));
        }
        for stack in extra {
            if stack.amount == 0 {
                continue;
            }
            if let Some(entry) = entries
                .iter_mut()
                .find(|(key, _)| key.as_str() == stack.resource.as_str())
            {
                entry.1 = int(stack.amount as i64);
            } else {
                entries.push((stack.resource.clone(), int(stack.amount as i64)));
            }
        }
        KkpvValue::Object(entries)
    };
    let part_kkpv = |p: &BodyPart| {
        let kind = crate::types::BodyKind::from_code(p.kind).map(crate::types::BodyKind::name);
        let mut entries = Vec::new();
        if let Some(kind) = kind {
            entries.push(("type".to_string(), strv(kind)));
        }
        entries.push(("hits".to_string(), int(p.hits as i64)));
        if let Some(boost) = &p.boost {
            entries.push(("boost".to_string(), strv(boost)));
        }
        KkpvValue::Object(entries)
    };
    let type_key = match object {
        RO::Creep(_) => "builtin:creep",
        RO::Spawn(_) => "builtin:spawn",
        RO::ConstructionSite(_) => "builtin:constructionSite",
        RO::Energy(_) => "builtin:resource",
        RO::Flag(_) => "builtin:flag",
        RO::Tombstone(_) => "builtin:tombstone",
        RO::Ruin(_) => "builtin:ruin",
        RO::Source(_) => "builtin:source",
        RO::Mineral(_) => "builtin:mineral",
        // 其余 typed 变体都是 type=structure；桶键由 structureType 决定。
        _ => "",
    }
    .to_string();

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
            kv(
                "body",
                KkpvValue::Array(body.iter().map(part_kkpv).collect()),
            );
            kv("fatigue", int(o.fatigue as i64));
            kv("ageTime", int(*age_time as i64));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(body.len() as i64 * 100));
            kv("store", store_kkpv(store, store_extra));
            kv(
                "storeCapacity",
                int(crate::snapshot::creep_store_capacity_raw(body)),
            );
            kv("spawning", KkpvValue::Bool(*spawning));
            if *age_time != 0 {
                kv("ticksToLive", int(age_time.saturating_sub(time) as i64));
            }
            if let Some(message) = say_message {
                kv("say", strv(message));
                kv("sayPublic", KkpvValue::Bool(say_public.unwrap_or(false)));
            }
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
                    _ => KkpvValue::Null,
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
                    .unwrap_or(KkpvValue::Null),
            );
            kv("isPowerEnabled", KkpvValue::Bool(*is_power_enabled));
            kv(
                "upgradeBlocked",
                upgrade_blocked
                    .map(|t| int(t as i64))
                    .unwrap_or(KkpvValue::Null),
            );
            kv(
                "safeModeCooldown",
                safe_mode_cooldown
                    .map(|t| int(t as i64))
                    .unwrap_or(KkpvValue::Null),
            );
            kv(
                "reservation",
                match reservation {
                    Some(r) => KkpvValue::Object(vec![
                        ("user".to_string(), strv(&r.user)),
                        ("endTime".to_string(), int(r.end_time as i64)),
                    ]),
                    None => KkpvValue::Null,
                },
            );
            kv(
                "sign",
                match sign {
                    Some(sv) => {
                        let mut entries = vec![
                            ("user".to_string(), strv(&sv.user)),
                            ("text".to_string(), strv(&sv.text)),
                            ("time".to_string(), int(sv.time as i64)),
                        ];
                        if let Some(datetime) = sv.datetime {
                            entries.push(("datetime".to_string(), int(datetime as i64)));
                        }
                        KkpvValue::Object(entries)
                    }
                    None => KkpvValue::Null,
                },
            );
            match (owner, owner_cleared) {
                (Some(owner), _) => kv("owner", strv(owner)),
                (None, true) => kv("owner", KkpvValue::Null),
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
            kv("store", store_kkpv(store, &[]));
            kv("energy", int(store.energy as i64));
            kv("energyCapacity", int(c::SPAWN_ENERGY_CAPACITY));
            kv(
                "storeCapacityResource",
                KkpvValue::Object(vec![("energy".to_string(), int(c::SPAWN_ENERGY_CAPACITY))]),
            );
            kv("off", KkpvValue::Bool(*off));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::SPAWN_HITS));
            kv(
                "spawning",
                match spawning {
                    None => KkpvValue::Null,
                    Some(job) => {
                        let mut entries = vec![
                            ("name".to_string(), strv(&job.name)),
                            ("needTime".to_string(), int(job.need_time as i64)),
                            ("spawnTime".to_string(), int(job.spawn_time as i64)),
                        ];
                        if !job.directions.is_empty() {
                            entries.push((
                                "directions".to_string(),
                                KkpvValue::Array(
                                    job.directions.iter().map(|d| int(*d as i64)).collect(),
                                ),
                            ));
                        }
                        KkpvValue::Object(entries)
                    }
                },
            );
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
            kv("store", store_kkpv(store, &[]));
            kv("energy", int(store.energy as i64));
            kv(
                "storeCapacityResource",
                KkpvValue::Object(vec![(
                    "energy".to_string(),
                    int(crate::types::extension_energy_capacity(rcl)),
                )]),
            );
            kv("off", KkpvValue::Bool(*off));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::EXTENSION_HITS));
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
            kv("store", store_kkpv(store, store_extra));
            kv("storeCapacity", int(*store_capacity));
            kv("hits", int(*hits as i64));
            kv("hitsMax", int(c::STORAGE_HITS));
            if let Some(flag) = notify_when_attacked {
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
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
            let resource_name = crate::projection::energy_resource_name(o);
            kv("type", strv("resource"));
            kv("resourceType", KkpvValue::Str(resource_name.to_string()));
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
                creep_id.map(KkpvValue::Ref).unwrap_or(KkpvValue::Null),
            );
            kv(
                "creepName",
                creep_name.as_deref().map(strv).unwrap_or(KkpvValue::Null),
            );
            kv("creepTicksToLive", int(*creep_ticks_to_live as i64));
            kv(
                "creepBody",
                KkpvValue::Array(
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
            kv("store", store_kkpv(store, store_extra));
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
                ("id".to_string(), KkpvValue::Ref(*structure_id)),
                ("type".to_string(), strv(structure_type)),
                ("hits".to_string(), int(0)),
            ];
            if let Some(hits_max) = structure_hits_max {
                structure.push(("hitsMax".to_string(), int(*hits_max as i64)));
            }
            if let Some(owner) = owner {
                structure.push(("user".to_string(), strv(owner)));
            }
            kv("type", strv("ruin"));
            kv("room", strv(room));
            kv("x", int(*x as i64));
            kv("y", int(*y as i64));
            kv("structure", KkpvValue::Object(structure));
            kv("destroyTime", int(*destroy_time as i64));
            kv("decayTime", int(*decay_time as i64));
            kv("ticksToDecay", int(decay_time.saturating_sub(time) as i64));
            kv("store", store_kkpv(store, store_extra));
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
                    _ => KkpvValue::Null,
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
                kv("notifyWhenAttacked", KkpvValue::Bool(*flag));
            }
        }
        // Generic/Mod 不进入本函数：调用方在 KKPV 投影里分流。
        RO::Generic(_) | RO::Mod(_) => unreachable!("object_kkpv 只处理类型化对象"),
    };
    // 与 object_json 的公共尾部同形：pos 供使用原始 JSON 的调用方读取。
    kv(
        "pos",
        KkpvValue::Object(vec![
            ("x".to_string(), KkpvValue::Int(object.pos().0 as i64)),
            ("y".to_string(), KkpvValue::Int(object.pos().1 as i64)),
            ("roomName".to_string(), strv(room)),
        ]),
    );
    if let Some(entries) = effects {
        kv(
            "effects",
            KkpvValue::Array(
                entries
                    .iter()
                    .map(|e| {
                        let mut entry = Vec::new();
                        if let Some(effect) = e.effect {
                            entry.push(("effect".to_string(), int(effect)));
                        }
                        entry.push(("power".to_string(), int(e.power)));
                        entry.push(("level".to_string(), int(e.level)));
                        entry.push(("endTime".to_string(), int(e.end_time as i64)));
                        entry.push((
                            "ticksRemaining".to_string(),
                            int(e.end_time.saturating_sub(time) as i64),
                        ));
                        KkpvValue::Object(entry)
                    })
                    .collect(),
            ),
        );
    }
    let type_key = match type_key.as_str() {
        "" => format!(
            "builtin:{}",
            // 其余 typed 变体都是 structure 记录，桶键由 structureType 决定。
            match fields
                .iter()
                .find(|(key, _)| key == "structureType")
                .and_then(|(_, value)| match value {
                    KkpvValue::Str(s) => Some(s.as_str()),
                    _ => None,
                }) {
                Some(st) => st.to_string(),
                None => "structure".to_string(),
            }
        ),
        key => key.to_string(),
    };
    KkpvObject {
        id: id.0,
        type_key,
        fields,
    }
}

/// 单对象的 KKPV 块对象投影：Mod 裁切、导出门、记录构造与 ext 裁切的
/// 完整路径；可见房全对象枚举与不可见房候选枚举共用，行为与逐房全量
/// 枚举版一致。
///
/// # Errors
/// 对象记录无法映射为 KKPV 值形态时返回错误（与全量枚举版同源）。
#[allow(clippy::too_many_arguments)]
pub(super) fn kkpv_object_of(
    room: &str,
    state: &crate::world::RoomState,
    controller: Option<&kk_schema::room::ControllerObj>,
    id: ObjId,
    object: &RoomObject,
    user: &str,
    room_visible: bool,
    catalog: &kk_mod_api::extension::ExtensionCatalog,
    surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
    time: u64,
) -> Result<Option<KkpvObject>, String> {
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
            return Ok(Some(record_to_kkpv(id, object, &record)?));
        }
        return Ok(None);
    }
    if !object_exported_for(Some(user), room_visible, object) {
        return Ok(None);
    }
    let (mut kkpv_object, record) = match object {
        // Generic 保持 Value 参考路径：data 本身就是 JSON，解析不可省；
        // 类型化对象直接构造，跳过 serde_json 中间树。
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
            .expect("Mod 对象已在 kkpv_object_of 上文分流");
            (record_to_kkpv(id, object, &record)?, Some(record))
        }
        _ => (
            object_kkpv(
                room,
                id,
                object,
                time,
                &state.terrain,
                controller,
                state.effects.get(&id),
            ),
            None,
        ),
    };
    // 扩展目录只按 structureType 登记；非结构类型无 ext 键，
    // 与 JSON 投影的裁切入口一致。
    // ext 目录按 structureType 登记：Generic 从参考记录读取，类型化对象
    // 的结构名从权威枚举派生（与 object_json 输出同源）。
    let structure_type = record
        .as_ref()
        .and_then(|record| {
            record
                .get("structureType")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| typed_structure_name(object));
    if let Some(type_key) = structure_type
        .as_deref()
        .and_then(|st| catalog.type_for_structure(st))
        && let Some(ext) = ext_entries_json(
            catalog,
            type_key,
            &state.extensions,
            id,
            crate::domain::object_owner(object).as_deref(),
            user,
        )
    {
        // ext 以嵌套记录进入块字段，键与值和 JSON 投影的 ext 键一致；
        // Generic 原生数据自带 ext 键时按参考的覆盖语义替换。
        let mut entries = Vec::with_capacity(ext.len());
        for (key, value) in ext {
            let converted = value_to_kkpv(&value).map_err(|e| format!("ext {key}: {e}"))?;
            entries.push((key, converted));
        }
        kkpv_object.fields.retain(|(key, _)| key != "ext");
        kkpv_object
            .fields
            .push(("ext".to_string(), KkpvValue::Object(entries)));
    }
    Ok(Some(kkpv_object))
}
