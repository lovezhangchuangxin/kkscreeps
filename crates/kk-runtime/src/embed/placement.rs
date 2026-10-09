//! 原子批量管理放置：共享顺序归一化、计划校验与候选提交边界。
use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct ModPlacement {
    type_key: String,
    fields: Value,
}

impl EmbedEngine {
    /// 按适配器的对象类别和初始字段摆放对象，返回对象 ID。
    /// 字段归一化由 [kk_world::place::normalize] 完成，显式字段优先。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        self.place_objects(&[(room, kind, spec)])
            .map(|ids| ids[0].clone())
    }

    /// 一次候选编辑按顺序归一化并放置对象；后项失败不发布前缀。
    /// `mod` 类别使用 `{typeKey, fields}`，与 sim 场景的 spec 同形。
    pub fn place_objects(
        &mut self,
        objects: &[(&str, &str, &Value)],
    ) -> anyhow::Result<Vec<String>> {
        self.no_open_round("placeObjects")?;
        if objects.is_empty() {
            return Ok(Vec::new());
        }
        let links = self.engine.links();
        let stores = self.engine.stores();
        let buildings = self.engine.buildings().clone();
        let prepared = objects
            .iter()
            .map(|(_, kind, spec)| {
                if *kind != "mod" {
                    return Ok(None);
                }
                let parsed: ModPlacement = serde_json::from_value((*spec).clone())?;
                let descriptor = self
                    .engine
                    .mod_object_type(&parsed.type_key)
                    .ok_or_else(|| anyhow::anyhow!("类型 {} 不在已装配计划中", parsed.type_key))?;
                let mut payload = kk_mod_api::objects::prepare_fields(&parsed.fields, descriptor)
                    .map_err(anyhow::Error::msg)?;
                // store 声明类型放置即携带库存信封（G09.a），与结算期
                // 对象创建同形。
                if let Some(capacity) = descriptor.store_capacity {
                    kk_mod_api::objects::inject_store_envelope(&mut payload, capacity);
                }
                // hits 声明类型放置即满血（G09.b），与结算期创建同形。
                if let Some(hits_max) = descriptor.hits_max {
                    kk_mod_api::objects::inject_hits_envelope(&mut payload, hits_max);
                }
                let (x, y) = kk_mod_api::objects::position(&payload, descriptor)
                    .map_err(anyhow::Error::msg)?;
                Ok(Some(kk_schema::room::RoomObject::Mod(
                    kk_schema::room::ModObj {
                        id: 0,
                        x,
                        y,
                        type_key: parsed.type_key,
                        data: payload.to_string(),
                    },
                )))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let placements = objects.to_vec();
        let ids = self
            .engine
            .edit_world(|world| {
                let mut ids = Vec::with_capacity(placements.len());
                for (&(room, kind, spec), prepared) in placements.iter().zip(prepared) {
                    let object = if let Some(object) = prepared {
                        object
                    } else {
                        let ctx = PlaceCtx {
                            time: world.time(),
                            links,
                            stores,
                            buildings: buildings.clone(),
                            controller: world.room(room).and_then(|room| room.controller()),
                            name_seq: world.next_obj_id() as usize,
                        };
                        normalize(kind, spec, &ctx)?
                    };
                    let id = world.insert(room, object)?;
                    if kind == "invaderCore" {
                        place_invader_core_links(
                            world,
                            room,
                            id,
                            spec,
                            links,
                            stores,
                            buildings.clone(),
                        )?;
                    }
                    ids.push((id.as_str().to_string(), kind, room));
                }
                Ok(ids)
            })
            .map_err(anyhow::Error::msg)?;
        // powerCreep 放置即建账号记录（放置即已生成形态，spawnCooldownTime
        // 置 None）；记录字段从成品房间 data 派生，与对象同源不重复推导。
        let pc_records: Vec<(String, String, kk_engine::settle::PowerCreepRecord)> = ids
            .iter()
            .filter(|(_, kind, _)| *kind == "powerCreep")
            .filter_map(|(id, _, room)| {
                let obj_id = kk_world::ObjId::parse(id)?;
                let owner = self
                    .engine
                    .world()
                    .room(room)?
                    .objects
                    .get(&obj_id)
                    .and_then(|o| match o {
                        kk_schema::room::RoomObject::Generic(g) => Some(g.clone()),
                        _ => None,
                    })?;
                let data: serde_json::Value = serde_json::from_str(&owner.data).ok()?;
                let owner_name = data.get("owner")?.as_str()?.to_string();
                let record = kk_engine::settle::PowerCreepRecord {
                    name: data.get("name")?.as_str()?.to_string(),
                    class_name: data
                        .get("className")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("operator")
                        .to_string(),
                    level: data.get("level")?.as_i64()?,
                    hits_max: data.get("hitsMax")?.as_i64()?,
                    store_capacity: data.get("storeCapacity")?.as_i64()?,
                    powers: data
                        .get("powers")
                        .and_then(|p| p.as_object())
                        .map(|map| {
                            map.iter()
                                .filter_map(|(pid, entry)| {
                                    Some((pid.parse::<i64>().ok()?, entry.get("level")?.as_i64()?))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    // 放置即已生成（对象在场）；死亡后由全局阶段改写。
                    spawn_cooldown_time: None,
                    delete_time: None,
                };
                Some((owner_name, id.clone(), record))
            })
            .collect();
        if !pc_records.is_empty() {
            self.engine
                .edit_players(|players| {
                    for (owner, id, record) in pc_records {
                        if let Some(state) = players.get_mut(&owner) {
                            let key = kk_world::ObjId::parse(&id).map(|o| o.0).unwrap_or(0);
                            state.power_creeps.insert(key, record);
                        }
                    }
                    Ok(())
                })
                .map_err(anyhow::Error::msg)?;
        }
        Ok(ids.into_iter().map(|(id, _, _)| id).collect())
    }

    /// 管理命令：按已装配计划里的 Mod 类型创建对象并放入房间，返回对象 ID。
    /// `fields` 须提供除带默认值外的全部声明字段（挂载字段同样要求）；
    /// 缺失的有默认值字段落入默认，整体通过 schema 校验后写入世界存储。
    /// 声明位置字段的类型以 payload 位置为准（信封坐标同步）。
    pub fn place_mod_object(
        &mut self,
        room: &str,
        type_key: &str,
        fields: &Value,
    ) -> anyhow::Result<String> {
        self.place_objects(&[(room, "mod", &json!({"typeKey": type_key, "fields": fields}))])
            .map(|ids| ids[0].clone())
    }
}

/// invaderCore 放置的联动件：ticksToCollapse 写入 EFFECT_COLLAPSE_TIMER
/// 房间效果条目（对象数据不携带 effects——发布校验拒绝双份权威状态）；
/// spawning 孵化按官方 invader-core/create-creep 形态在同格另插
/// spawning 形态的 NPC creep，出生落格由引擎 tick 完成。两项与核心
/// 对象同一 edit_world 候选，后项失败不发布前项。
fn place_invader_core_links(
    world: &mut kk_world::World,
    room: &str,
    id: kk_world::ObjId,
    spec: &Value,
    links: kk_mod_api::builtin::LinkRules,
    stores: kk_mod_api::builtin::StoreRules,
    buildings: kk_mod_api::buildings::BuildingCatalog,
) -> Result<(), String> {
    // 官方放置恒带 effects 数组：空槽也入表（玩家面投影空数组属性，
    // 对象形状契约），ticksToCollapse 时写入 EFFECT_COLLAPSE_TIMER 条目。
    let mut entries = Vec::new();
    if let Some(ttl) = spec.get("ticksToCollapse") {
        // 值域已由域归一先行校验（同一 spec 先经 normalize），此处换算。
        let ttl = ttl
            .as_u64()
            .ok_or_else(|| "ticksToCollapse 必须是非负整数".to_string())?;
        let end_time = kk_world::domain::invader_core::deadline(world.time(), ttl)?;
        entries.push(kk_world::EffectEntry {
            effect: Some(kk_core::constants::EFFECT_COLLAPSE_TIMER),
            power: kk_core::constants::EFFECT_COLLAPSE_TIMER,
            level: 1,
            end_time,
        });
    }
    world.set_effects(room, id, entries)?;
    if let Some(spawning) = spec.get("spawning") {
        // 身体缺省 [MOVE] 与套件 incubation 契约一致；寿命走放置默认，
        // 不绑定核心 decayTime（布防片接线时再随 deployStronghold 对齐）。
        let creep_spec = serde_json::json!({
            "name": spawning.get("name"),
            "owner": kk_world::domain::npc::INVADER_OWNER,
            "pos": spec.get("pos"),
            "body": spawning.get("body").cloned().unwrap_or_else(|| serde_json::json!(["move"])),
            "spawning": true,
        });
        let ctx = PlaceCtx {
            time: world.time(),
            links,
            stores,
            buildings,
            controller: world.room(room).and_then(|r| r.controller()),
            name_seq: world.next_obj_id() as usize,
        };
        let mut creep = normalize("creep", &creep_spec, &ctx)?;
        if let kk_schema::room::RoomObject::Creep(chick) = &mut creep {
            // 孵化期不消耗寿命；出生后的首个 creep tick 按身体初始化截止时刻。
            // Stronghold 部署时仍可将此值改为核心的 decayTime。
            chick.age_time = 0;
        }
        world.insert(room, creep)?;
    }
    Ok(())
}
