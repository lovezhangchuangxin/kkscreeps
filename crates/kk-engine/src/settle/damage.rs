//! 处理即时结构伤害、延迟 creep 伤害与治疗，以及死亡后的资源转移。
//!
//! Creep 在自身 tick 中先受伤、后治疗，再判断是否死亡；结构击毁立即创建
//! 废墟。攻击功率可以包含 boost，但本模块尚未模拟 TOUGH 减伤或 power 免疫。

use kk_core::constants as c;
use kk_schema::room::{BodyPart, RoomObject};
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::json;

use super::Working;
use super::creep::{ACTION_ATTACK, ACTION_ATTACKED, ACTION_RANGED_ATTACK, set_action_log};
use super::movement::type_name_of;

/// 统一处理 creep 和 Generic 结构攻击者的伤害。
/// Creep 目标累加待伤害，结构立即扣耐久并按需建废墟。近战反击仅在
/// 攻击者未站在 rampart 时发生，房主安全模式会抑制该反击。
/// 事件顺序为结构摧毁、反击、主攻击；零伤害仍记主事件。
/// 来源：上游 engine/src/processor/intents/_damage.js。
pub fn apply_damage(
    w: &mut Working,
    attacker: ObjId,
    target_id: ObjId,
    damage: i64,
    attack_type: i64,
) {
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if object_hits(&target) <= 0 {
        return;
    }
    let (attacker_pos, attacker_owner) = match w.room.objects.get(&attacker) {
        Some(RoomObject::Creep(o)) => ((o.x, o.y), o.owner.clone()),
        Some(RoomObject::Generic(g)) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap_or_default();
            (
                (g.x, g.y),
                data.get("owner")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            )
        }
        _ => return,
    };

    let mut attack_back: i64 = 0;
    if let RoomObject::Creep(t) = &target {
        if attack_type == c::EVENT_ATTACK_TYPE_MELEE && rampart_at(w, attacker_pos).is_none() {
            attack_back = kk_world::types::calc_body_effectiveness(
                &t.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            );
        }
        *w.damage_to_apply.entry(target_id).or_insert(0) += damage;
    } else {
        let new_hits = object_hits(&target) - damage;
        if new_hits <= 0 {
            destroy_structure(w, target_id);
            w.push_event(
                c::EVENT_OBJECT_DESTROYED,
                target_id,
                json!({ "type": type_name_of(&target) }),
            );
        } else {
            set_structure_hits(w, target_id, new_hits);
        }
    }

    if attack_back > 0
        && let Some(ctl) = w.controller()
        && ctl.owner.as_deref() == Some(attacker_owner.as_str())
        && ctl.safe_mode > w.time
    {
        attack_back = 0;
    }

    if (attack_type == c::EVENT_ATTACK_TYPE_MELEE || attack_type == c::EVENT_ATTACK_TYPE_DISMANTLE)
        && let Some(a) = w.creep_mut(attacker)
    {
        set_action_log(&mut a.action_log, ACTION_ATTACK);
    }
    if attack_type == c::EVENT_ATTACK_TYPE_RANGED
        && let Some(a) = w.creep_mut(attacker)
    {
        set_action_log(&mut a.action_log, ACTION_RANGED_ATTACK);
    }
    if matches!(target, RoomObject::Creep(_))
        && let Some(t) = w.creep_mut(target_id)
    {
        set_action_log(&mut t.action_log, ACTION_ATTACKED);
    }

    if attack_back > 0 {
        *w.damage_to_apply.entry(attacker).or_insert(0) += attack_back;
        if let Some(a) = w.creep_mut(attacker) {
            set_action_log(&mut a.action_log, ACTION_ATTACKED);
        }
        w.push_event(
            c::EVENT_ATTACK,
            target_id,
            json!({
                "targetId": attacker.as_str(),
                "damage": attack_back,
                "attackType": c::EVENT_ATTACK_TYPE_HIT_BACK,
            }),
        );
    }
    w.push_event(
        c::EVENT_ATTACK,
        attacker,
        json!({
            "targetId": target_id.as_str(),
            "damage": damage,
            "attackType": attack_type,
        }),
    );
}

/// 先扣待伤害再加待治疗，钳到最大生命值，最后判断死亡；死亡返回 false。
/// 净变化才重分配部件耐久，净减少才处理 CARRY 缩容掉落。
/// 攻击与治疗功率可含 boost，这里的受伤处理尚不模拟 TOUGH 减伤。
pub fn apply_pending_hits(w: &mut Working, id: ObjId) -> bool {
    let damage = w.damage_to_apply.remove(&id).unwrap_or(0);
    let heal = w.heal_to_apply.remove(&id).unwrap_or(0);
    if damage <= 0 && heal <= 0 {
        return true;
    }
    let (old_hits, hits_max) = match w.creep(id) {
        Some(creep) => (creep.hits as i64, creep.body.len() as i64 * 100),
        None => return true,
    };
    let mut new_hits = old_hits - damage + heal;
    if new_hits > hits_max {
        new_hits = hits_max;
    }
    if new_hits <= 0 {
        super::creep::die_creep(w, id);
        return false;
    }
    if new_hits == old_hits {
        return true;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.hits = new_hits as u32;
        distribute_hits(&mut creep.body, new_hits);
    }
    if new_hits < old_hits {
        drop_energy_over_capacity(w, id);
    }
    true
}

/// 从身体尾部灌满耐久，每件最多 100，前列部件最先受损。
/// 不保存上游瞬态 _oldHits：driver/lib/bulk.js 的 update/removeHidden 会
/// 删除隐藏字段，后续 tick 仍按现有部件耐久计算。
/// 来源：上游 engine/src/processor/intents/creeps/_recalc-body.js。
pub fn distribute_hits(body: &mut [BodyPart], mut hits: i64) {
    for part in body.iter_mut().rev() {
        part.hits = hits.clamp(0, 100) as u32;
        hits = (hits - 100).max(0);
    }
}

/// 按资源目录顺序掉落超量资源：内建 RESOURCES_ALL 声明序在前，计划
/// 声明的命名空间资源按键字典序随后（含矿物，并非只处理 energy）。
/// 用于伤后 CARRY 缩容和拆解回收；顺序会影响各资源保留量。
pub fn drop_energy_over_capacity(w: &mut Working, id: ObjId) {
    let (cap, held, store, extra) = match w.creep(id) {
        Some(c) => (
            super::creep::store_capacity(c),
            kk_world::types::store_total(&c.store, &c.store_extra),
            c.store.clone(),
            c.store_extra.clone(),
        ),
        None => return,
    };
    if held <= cap {
        return;
    }
    let overflow = held - cap;
    let mut left = overflow;
    let catalog = w.plane.resources.clone();
    for r in catalog.iter() {
        if left <= 0 {
            break;
        }
        let held_res = if r == "energy" {
            store.energy as i64
        } else if r == kk_core::constants::RESOURCE_GHODIUM {
            store.ghodium as i64
        } else {
            extra
                .iter()
                .find(|s| s.resource == r)
                .map(|s| s.amount as i64)
                .unwrap_or(0)
        };
        let d = held_res.min(left);
        if d > 0 {
            super::creep::intent_drop_amount_res(w, id, r, d);
            left -= d;
        }
    }
}

/// 读取对象耐久，无耐久类型返回 0；Generic JSON 非整数或缺失 hits 同样返回 0。
pub fn object_hits(obj: &RoomObject) -> i64 {
    match obj {
        RoomObject::Creep(o) => o.hits as i64,
        RoomObject::Spawn(o) => o.hits as i64,
        RoomObject::Extension(o) => o.hits as i64,
        RoomObject::ConstructedWall(o) => o.hits as i64,
        RoomObject::Road(o) => o.hits as i64,
        RoomObject::Storage(o) => o.hits as i64,
        RoomObject::Extractor(o) => o.hits as i64,
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data.get("hits").and_then(|v| v.as_i64()).unwrap_or(0)
        }
        _ => 0,
    }
}

/// 读取可修结构的耐久上限，其他类型返回 None。道路按地形重算；
/// rampart 须与控制器同主并按 RCL 取值，归属不符时返回 0。
pub fn structure_hits_max(w: &Working, obj: &RoomObject) -> Option<i64> {
    Some(match obj {
        RoomObject::Spawn(_) => c::SPAWN_HITS,
        RoomObject::Extension(_) => c::EXTENSION_HITS,
        RoomObject::ConstructedWall(_) => c::WALL_HITS_MAX,
        RoomObject::Road(o) => {
            let mut hits = c::ROAD_HITS;
            if w.room
                .terrain
                .check_mask(o.x as usize, o.y as usize, kk_core::TERRAIN_MASK_SWAMP)
            {
                hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO;
            }
            if w.room
                .terrain
                .check_mask(o.x as usize, o.y as usize, kk_core::TERRAIN_MASK_WALL)
            {
                hits *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO;
            }
            hits
        }
        RoomObject::Storage(_) => c::STORAGE_HITS,
        RoomObject::Extractor(_) => c::EXTRACTOR_HITS,
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            let structure_type = data.get("structureType").and_then(|v| v.as_str())?;
            if structure_type == "rampart" {
                let owner = data.get("owner").and_then(|v| v.as_str()).unwrap_or("");
                let ctl = w.controller()?;
                if ctl.owner.as_deref() != Some(owner) {
                    return Some(0);
                }
                let key = ctl.level.to_string();
                c::RAMPART_HITS_MAX
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| *v)
                    .unwrap_or(0)
            } else if let Some(building) = w.plane.buildings.get(structure_type) {
                building.hits_max.min(u32::MAX as u64) as i64
            } else {
                static_hits_max(structure_type, &w.plane.links)?
            }
        }
        _ => return None,
    })
}

fn static_hits_max(structure_type: &str, links: &kk_mod_api::builtin::LinkRules) -> Option<i64> {
    Some(match structure_type {
        "tower" => c::TOWER_HITS,
        "observer" => c::OBSERVER_HITS,
        "extractor" => c::EXTRACTOR_HITS,
        "lab" => c::LAB_HITS,
        "terminal" => c::TERMINAL_HITS,
        "container" => c::CONTAINER_HITS,
        "powerSpawn" => c::POWER_SPAWN_HITS,
        "nuker" => c::NUKER_HITS,
        "factory" => c::FACTORY_HITS,
        "link" => links.hits_max,
        _ => return None,
    })
}

/// 写回结构耐久，钳在 u32 范围；Generic 写入 JSON，其他类型不变。
pub fn set_structure_hits(w: &mut Working, id: ObjId, hits: i64) {
    let hits = hits.clamp(0, u32::MAX as i64) as u32;
    match w.room.objects.get_mut(&id) {
        Some(RoomObject::Spawn(o)) => o.hits = hits,
        Some(RoomObject::Extension(o)) => o.hits = hits,
        Some(RoomObject::ConstructedWall(o)) => o.hits = hits,
        Some(RoomObject::Road(o)) => o.hits = hits,
        Some(RoomObject::Storage(o)) => o.hits = hits,
        Some(RoomObject::Extractor(o)) => o.hits = hits,
        Some(RoomObject::Generic(o)) => {
            let mut data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data["hits"] = json!(hits);
            o.data = data.to_string();
        }
        _ => {}
    }
}

/// 返回同格第一个 rampart，不区分主人或公开性，供攻击重定向。
pub fn rampart_at(w: &Working, at: (u8, u8)) -> Option<ObjId> {
    w.index
        .at(at.0 as usize, at.1 as usize)
        .iter()
        .copied()
        .find(|id| {
            w.room
                .objects
                .get(id)
                .is_some_and(|o| type_name_of(o) == "rampart")
        })
}

/// 移除结构并创建废墟，转存资源；spawn 的同主同名 creep 一并移除。
/// 不在这里发事件，由调用方决定。未模拟核弹抑制、NPC 与 power 特殊分支。
/// 来源：上游 engine/src/processor/intents/structures/_destroy.js。
pub(super) fn destroy_structure(w: &mut Working, id: ObjId) {
    let chick_query = match w.room.objects.get(&id) {
        Some(RoomObject::Spawn(s)) => s
            .spawning
            .as_ref()
            .map(|job| (s.owner.clone(), job.name.clone())),
        _ => None,
    };
    if let Some((owner, name)) = chick_query {
        let chicks: Vec<ObjId> = w
            .room
            .objects
            .iter()
            .filter(
                |(_, o)| matches!(o, RoomObject::Creep(c) if c.owner == owner && c.name == name),
            )
            .map(|(cid, _)| *cid)
            .collect();
        for cid in chicks {
            w.remove_obj(cid);
        }
    }
    let Some(obj) = w.room.objects.get(&id) else {
        return;
    };
    let (pos, structure_type) = (obj.pos(), type_name_of(obj).to_string());
    let decay = c::RUIN_DECAY_STRUCTURES
        .iter()
        .find(|(k, _)| *k == structure_type)
        .map(|(_, v)| *v)
        .unwrap_or(c::RUIN_DECAY) as u64;
    let hits_max = structure_hits_max(w, obj).map(|v| v.clamp(0, u32::MAX as i64) as u32);
    let owner = kk_world::domain::object_owner(obj)
        .filter(|o| !o.is_empty())
        .map(|o| o.into_owned());
    let (store, store_extra) = structure_store(obj);
    w.remove_obj(id);
    w.insert_new(RoomObject::Ruin(kk_schema::room::RuinObj {
        id: 0,
        x: pos.0,
        y: pos.1,
        structure_id: id.0,
        structure_type,
        structure_hits_max: hits_max,
        destroy_time: w.time,
        decay_time: w.time + decay,
        owner,
        store,
        store_extra,
    }));
}

/// 复制结构资源；Generic 只保留正整数并钳到 u32，
/// energy/G 为固定槽，其他资源为命名槽，没有 store 的结构为空。
fn structure_store(
    obj: &RoomObject,
) -> (kk_schema::room::Store, Vec<kk_schema::room::ResourceStack>) {
    let mut store = kk_schema::room::Store::default();
    let mut extra = Vec::new();
    match obj {
        RoomObject::Spawn(o) => store = o.store.clone(),
        RoomObject::Extension(o) => store = o.store.clone(),
        RoomObject::Storage(o) => {
            store = o.store.clone();
            extra = o.store_extra.clone();
        }
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            if let Some(map) = data
                .pointer("/store")
                .and_then(serde_json::Value::as_object)
            {
                for (resource, value) in map {
                    let amount = value.as_i64().unwrap_or(0);
                    if amount <= 0 {
                        continue;
                    }
                    let amount = amount.min(u32::MAX as i64) as u32;
                    match resource.as_str() {
                        "energy" => store.energy = amount,
                        kk_core::constants::RESOURCE_GHODIUM => store.ghodium = amount,
                        other => extra.push(kk_schema::room::ResourceStack {
                            resource: other.to_string(),
                            amount,
                        }),
                    }
                }
            }
        }
        _ => {}
    }
    (store, extra)
}
