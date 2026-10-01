//! 结算工地建造，完成后创建带初始容量、耐久和时钟的结构。
//!
//! 能耗按未强化效果计算，进度按 boost 后效果计算；两者不能合并为一个数值。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_BUILD, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::movement;

/// 在三格内建造工地，计算基础能耗与 boost 后的进度；完成时替换为新结构。
/// 来源：上游 engine/src/processor/intents/creeps/build.js。
pub(super) fn intent_build(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    let (pos, owner, energy) = match w.creep(id) {
        Some(c) if !c.spawning => ((c.x, c.y), c.owner.clone(), c.store.energy as i64),
        _ => return,
    };
    if energy <= 0 {
        return;
    }
    let Some(RoomObject::ConstructionSite(site)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&site.structure_type).is_none() {
        return;
    }
    if (site.x as i32 - pos.0 as i32).abs() > 3 || (site.y as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    let cell = (site.x, site.y);
    let site_is_obstacle = kk_world::types::is_obstacle_type(&site.structure_type);
    // 建造障碍结构时，自己安全模式下只被自家 creep 阻挡，否则任何 creep 都阻挡。
    let my_safe_mode = w.controller().is_some_and(|ctl| {
        ctl.owner.as_deref().is_some_and(|u| u == owner.as_str()) && ctl.safe_mode > w.time
    });
    for tid in w.index.at(cell.0 as usize, cell.1 as usize) {
        match w.room.objects.get(tid) {
            Some(RoomObject::ConstructionSite(_)) if *tid != target_id => return,
            Some(RoomObject::Creep(c)) => {
                if site_is_obstacle && (!my_safe_mode || c.owner == owner) {
                    return;
                }
            }
            Some(other) => {
                let name = movement::type_name_of(other);
                if name == "constructionSite" {
                    continue;
                }
                if name == site.structure_type
                    || (site_is_obstacle && kk_world::types::is_obstacle_type(name.as_ref()))
                {
                    return;
                }
            }
            None => {}
        }
    }
    if site.structure_type != "extractor"
        && site.structure_type != "road"
        && w.room
            .terrain
            .check_mask(cell.0 as usize, cell.1 as usize, kk_core::TERRAIN_MASK_WALL)
    {
        return;
    }
    // progress_total 使用创建时的存值。宿主可放置进度超限的工地，此时 remaining
    // 为负，build 会减少进度并返还能量。保留有符号运算，不能改成饱和减法。
    let total = site.progress_total as i64;
    let remaining = total - site.progress as i64;
    // 基础效果决定能耗，boost 后的效果决定进度与事件量。效果为 0 时仍记动作与事件。
    let Some((effect, boosted)) = w.creep(id).map(|c| {
        let base = BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::BUILD_POWER;
        let effect = base.min(remaining).min(energy);
        let boosted = kk_world::types::boosted_effect(
            &c.body,
            BodyKind::Work,
            "work",
            "build",
            c::BUILD_POWER,
            effect,
            remaining,
        );
        (effect, boosted)
    }) else {
        return;
    };
    let new_progress = site.progress as i64 + boosted;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy = u32::try_from((c.store.energy as i64 - effect).max(0)).unwrap_or(u32::MAX);
        set_action_log(&mut c.action_log, ACTION_BUILD);
    }
    let incomplete = new_progress < total;
    w.push_event(
        c::EVENT_BUILD,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": boosted,
            "structureType": site.structure_type,
            "x": site.x,
            "y": site.y,
            "incomplete": incomplete,
        }),
    );
    if incomplete {
        if let Some(RoomObject::ConstructionSite(s)) = w.room.objects.get_mut(&target_id) {
            s.progress = new_progress as u32;
        }
    } else {
        complete_construction(w, target_id, site, owner);
    }
}

/// 移除工地，创建对应结构并使用新 ID；所有权来自工地，默认开启受袭通知。
fn complete_construction(
    w: &mut Working,
    site_id: ObjId,
    site: kk_schema::room::ConstructionSiteObj,
    builder_owner: String,
) {
    let time = w.time;
    w.remove_obj(site_id);
    let obj = match site.structure_type.as_str() {
        "spawn" => RoomObject::Spawn(kk_schema::room::SpawnObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::SPAWN_HITS as u32,
            off: false,
            name: site.name.unwrap_or_default(),
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            spawning: None,
            notify_when_attacked: Some(true),
        }),
        "extension" => RoomObject::Extension(kk_schema::room::ExtensionObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::EXTENSION_HITS as u32,
            off: false,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            notify_when_attacked: Some(true),
        }),
        "constructedWall" => RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::WALL_HITS as u32,
            notify_when_attacked: Some(true),
        }),
        "road" => {
            let mut hits = c::ROAD_HITS as u64;
            if w.room.terrain.check_mask(
                site.x as usize,
                site.y as usize,
                kk_core::TERRAIN_MASK_SWAMP,
            ) {
                hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO as u64;
            }
            if w.room.terrain.check_mask(
                site.x as usize,
                site.y as usize,
                kk_core::TERRAIN_MASK_WALL,
            ) {
                hits *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO as u64;
            }
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: site.x,
                y: site.y,
                hits: hits.min(u32::MAX as u64) as u32,
                next_decay_time: time + c::ROAD_DECAY_TIME as u64,
                notify_when_attacked: Some(true),
            })
        }
        "storage" => RoomObject::Storage(kk_schema::room::StorageObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::STORAGE_HITS as u32,
            owner: site.owner.unwrap_or_default(),
            store: Default::default(),
            notify_when_attacked: Some(true),
            store_extra: Vec::new(),
        }),
        "extractor" => RoomObject::Extractor(kk_schema::room::ExtractorObj {
            id: 0,
            x: site.x,
            y: site.y,
            hits: c::EXTRACTOR_HITS as u32,
            owner: site.owner,
            cooldown: 0,
            notify_when_attacked: Some(true),
        }),
        other => {
            let (hits, hits_max) = kk_world::place::generic_structure_hits(other).unwrap_or((1, 1));
            // Rampart 的 hitsMax 取决于建造者是否为房主，而不是工地主人。
            let hits_max = if other == "rampart" {
                match w.controller() {
                    Some(ctl) if ctl.owner.as_deref() == Some(builder_owner.as_str()) => {
                        kk_core::constants::RAMPART_HITS_MAX
                            .iter()
                            .find(|(k, _)| *k == ctl.level.to_string())
                            .map(|(_, v)| *v)
                            .unwrap_or(0)
                    }
                    _ => 0,
                }
            } else {
                hits_max
            };
            let mut data = serde_json::json!({
                "type": "structure",
                "structureType": other,
                "owner": site.owner,
                "hits": hits,
                "hitsMax": hits_max,
                "notifyWhenAttacked": true,
            });
            // 与放置入口共用容量及初始时钟。新结构会参加本轮对象 tick，
            // 衰减时钟若遗漏，刚建好的 rampart 可能立即衰减消失。
            // 总容量与按资源容量须配套，玩家 Store 的 null/数值语义依赖两者。
            if let Some(cap) = kk_world::place::built_store_capacity(other) {
                data["storeCapacity"] = serde_json::json!(cap);
            }
            if let Some(res) = kk_world::place::built_store_capacity_resource(other) {
                data["storeCapacityResource"] = res;
            }
            if let Some(clock) = kk_world::place::built_decay_clock(other, time) {
                data["nextDecayTime"] = serde_json::json!(clock);
            }
            // Nuker 从建成时开始冷却；其他结构使用同一初始化表。
            if let Some(cd) = kk_world::place::built_cooldown_time(other, time) {
                data["cooldownTime"] = serde_json::json!(cd);
            }
            // Generic 初始不保存空 store，玩家 Store 将缺失当作空仓；原始 JSON 字段不同。
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: site.x,
                y: site.y,
                data: data.to_string(),
            })
        }
    };
    w.insert_new(obj);
}
