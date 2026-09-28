//! hits 模型与伤害施加（官方 `_damage.js` / `creeps/tick.js` 伤害段 /
//! `creeps/_recalc-body.js` / `structures/_destroy.js` 的 B1 无 boost/power 子集）。
//!
//! 官方伤害链路（file:line 见各函数注释）：
//! - creep 目标的伤害**延迟施加**：intent 期累加进 `_damageToApply`（_damage.js:20），
//!   在该 creep 自己的 tick 里结算（tick.js:118-123）——同 tick 多次攻击叠加；
//! - 结构目标立即扣血（_damage.js:32），击毁走 `_destroy`（:46-52）；
//! - 近战对 creep 目标触发反击（同样延迟），除非攻击者站在 rampart 上（:17-19）；
//! - 部件伤害分布 = 总 hits 从 body 尾部灌满（_recalc-body.js:10-18）——
//!   数组前部的部件先掉 hit，前列部件先报废。

use kk_core::constants as c;
use kk_schema::room::{BodyPart, RoomObject};
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::json;

use super::Working;
use super::creep::{ACTION_ATTACK, ACTION_ATTACKED, set_action_log};
use super::movement::type_name_of;

/// 官方 `_damage.js`（B1 无 boost/power 子集）：伤害施加统一入口。
///
/// `attacker` 恒为 creep（B1 调用方只有 creep 近战/拆解；官方 tower/nuke 走同
/// 函数但属后续批次）。action_type 取 `EVENT_ATTACK_TYPE_*`。
pub fn apply_damage(
    w: &mut Working,
    attacker: ObjId,
    target_id: ObjId,
    damage: i64,
    attack_type: i64,
) {
    // _damage.js:10-12：目标不存在或无 hits → 无事发生（attack.js:27 已挡，
    // 这里是统一入口的二次防御，官方同形）
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if object_hits(&target) <= 0 {
        return;
    }
    let (attacker_pos, attacker_owner) = match w.creep(attacker) {
        Some(creep) => ((creep.x, creep.y), creep.owner.clone()),
        None => return,
    };

    // 伤害施加（_damage.js:16-33）
    let mut attack_back: i64 = 0;
    if let RoomObject::Creep(t) = &target {
        // _damage.js:17-19：近战、目标为 creep、攻击者不站 rampart → 目标以自身
        // 近战攻击力反击（反击伤害同样延迟到攻击者的 tick）
        if attack_type == c::EVENT_ATTACK_TYPE_MELEE && rampart_at(w, attacker_pos).is_none() {
            attack_back =
                BodyKind::count_active(&t.body, BodyKind::Attack) as i64 * c::ATTACK_POWER;
        }
        // _damage.js:20-21：creep 伤害延迟（累加器）
        *w.damage_to_apply.entry(target_id).or_insert(0) += damage;
    } else {
        // _damage.js:26-32：墙/rampart 的 PWR_FORTIFY/EFFECT_INVULNERABILITY 免疫
        // 属 power 体系（B1 无）；结构立即扣血
        let new_hits = object_hits(&target) - damage;
        if new_hits <= 0 {
            // _damage.js:46-52：击毁（_destroy + EVENT_OBJECT_DESTROYED——事件与
            // ruin 承接留 B3，见 destroy_structure 注释）
            destroy_structure(w, target_id);
        } else {
            // _damage.js:54-57：存活则持久化新血量
            set_structure_hits(w, target_id, new_hits);
        }
    }

    // _damage.js:39-41：safeMode 房主自己的 creep 不受反击
    if attack_back > 0
        && let Some(ctl) = w.controller()
        && ctl.owner.as_deref() == Some(attacker_owner.as_str())
        && ctl.safe_mode > w.time
    {
        attack_back = 0;
    }

    // actionLog（_damage.js:59-75）：近战与 dismantle 共用官方 `attack` 键（:60-65）
    if (attack_type == c::EVENT_ATTACK_TYPE_MELEE || attack_type == c::EVENT_ATTACK_TYPE_DISMANTLE)
        && let Some(a) = w.creep_mut(attacker)
    {
        set_action_log(&mut a.action_log, ACTION_ATTACK);
    }
    // _damage.js:73-75：目标侧 `attacked`（官方有 actionLog 的对象还有 tower/lab/
    // link——M1 透传桶无该面，仅 creep）
    if matches!(target, RoomObject::Creep(_))
        && let Some(t) = w.creep_mut(target_id)
    {
        set_action_log(&mut t.action_log, ACTION_ATTACKED);
    }

    // 反击（_damage.js:86-91）：伤害延迟 + 攻击者记 attacked + 事件（先于主事件）
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
    // 主事件（_damage.js:93）：0 伤害也发（引擎层不做功率预检——API 层的事）
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

/// creep tick 的延迟伤害结算（官方 tick.js:118-150 的伤害段，B1 无 heal）。
/// 返回 false 表示对象已死亡（调用方停止后续处理）。
pub fn apply_pending_damage(w: &mut Working, id: ObjId) -> bool {
    // 官方 `if (object._damageToApply)` 真值判定：0 伤害（无 ATTACK 部件的攻击
    // intent）不进伤害路径——只有事件与 actionLog 已在 apply_damage 产出
    let Some(damage) = w.damage_to_apply.remove(&id) else {
        return true;
    };
    if damage <= 0 {
        return true;
    }
    let old_hits = match w.creep(id) {
        Some(creep) => creep.hits as i64,
        None => return true,
    };
    let new_hits = old_hits - damage;
    if new_hits <= 0 {
        // tick.js:134-136：hits 归零 → 暴力死亡，与 TTL 死亡同路径（掉能量、移除；
        // 墓碑与 EVENT_OBJECT_DESTROYED 留 B3）
        super::creep::die_creep(w, id);
        return false;
    }
    // tick.js:130-132（heal 钳 hitsMax 不在 B1）；:137 仅 hits 变化才重算
    if new_hits == old_hits {
        return true;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.hits = new_hits as u32;
        // tick.js:139 → _recalc-body.js：总 hits 从尾部灌满，前列部件先掉
        distribute_hits(&mut creep.body, new_hits);
    }
    // tick.js:141-143 → _drop-resources-without-space.js：CARRY 部件损毁可能缩容
    drop_energy_over_capacity(w, id);
    true
}

/// 官方 _recalc-body.js:10-18：从 body 尾部向前灌 `min(剩余, 100)`——
/// 受伤自数组头部向尾部推进（前列部件先报废）。
pub fn distribute_hits(body: &mut [BodyPart], mut hits: i64) {
    for part in body.iter_mut().rev() {
        part.hits = hits.clamp(0, 100) as u32;
        hits = (hits - 100).max(0);
    }
}

/// 官方 _drop-resources-without-space.js（M1 energy-only 单资源形态）：
/// 持有超容 → 掉出差额（走 drop 的落地路径）。
pub fn drop_energy_over_capacity(w: &mut Working, id: ObjId) {
    let Some(creep) = w.creep(id) else { return };
    let cap = super::creep::store_capacity(creep);
    let held = creep.store.energy as i64;
    if held > cap {
        super::creep::intent_drop_amount(w, id, held - cap);
    }
}

/// 对象当前 hits；无 hits 概念的对象（source/controller/工地/掉落/旗）为 0——
/// 官方 `!target.hits` 拒（attack.js:27 / _damage.js:10）。Generic 透传桶从
/// data JSON 的 `hits` 读。
pub fn object_hits(obj: &RoomObject) -> i64 {
    match obj {
        RoomObject::Creep(o) => o.hits as i64,
        RoomObject::Spawn(o) => o.hits as i64,
        RoomObject::Extension(o) => o.hits as i64,
        RoomObject::ConstructedWall(o) => o.hits as i64,
        RoomObject::Road(o) => o.hits as i64,
        RoomObject::Storage(o) => o.hits as i64,
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data.get("hits").and_then(|v| v.as_i64()).unwrap_or(0)
        }
        _ => 0,
    }
}

/// 结构 hits 上限（repair 满血判定与钳制；None = 不是可修结构）。
/// 初值表对照官方 build.js 落成分支（:124-217）；rampart 按 RCL 归一
/// （ramparts/tick.js:17）；road 的地形系数重建（build.js:172-187——hitsMax
/// 即落成值，schema 未存故按同式重算）。
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
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            let structure_type = data.get("structureType").and_then(|v| v.as_str())?;
            if structure_type == "rampart" {
                // ramparts/tick.js:17：房主的 rampart 上限按 RCL 表，否则 0
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
            } else {
                static_hits_max(structure_type)?
            }
        }
        // creep/source/controller/工地/掉落/旗：官方无 hitsMax（repair.js:16 拒）
        _ => return None,
    })
}

/// 静态 hits 上限表（官方 build.js 落成分支 :202-217 各类型的 hitsMax）。
fn static_hits_max(structure_type: &str) -> Option<i64> {
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
        "link" => c::LINK_HITS_MAX,
        _ => return None,
    })
}

/// 修改结构 hits（含 Generic 透传桶的 JSON 字段）。
pub fn set_structure_hits(w: &mut Working, id: ObjId, hits: i64) {
    let hits = hits.clamp(0, u32::MAX as i64) as u32;
    match w.room.objects.get_mut(&id) {
        Some(RoomObject::Spawn(o)) => o.hits = hits,
        Some(RoomObject::Extension(o)) => o.hits = hits,
        Some(RoomObject::ConstructedWall(o)) => o.hits = hits,
        Some(RoomObject::Road(o)) => o.hits = hits,
        Some(RoomObject::Storage(o)) => o.hits = hits,
        Some(RoomObject::Generic(o)) => {
            let mut data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data["hits"] = json!(hits);
            o.data = data.to_string();
        }
        _ => {}
    }
}

/// 同格 rampart 查找（官方 `_.find(roomObjects, {type:'rampart', x, y})`，
/// attack.js:33 / dismantle.js:27——命中则伤害转打 rampart）。
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

/// 官方 structures/_destroy.js 的 B1 子集：孵化中 spawn 的雏形一并移除、
/// 结构移除。偏离登记：官方建 ruin 承接 store 并发 EVENT_OBJECT_DESTROYED
/// （_damage.js:51）——ruin 变体与死亡事件族整批留 B3，B1 让 store 落地为
/// 掉落能量（延续 M1 creep 死亡掉地语义，能量不凭空消失）。
fn destroy_structure(w: &mut Working, id: ObjId) {
    // _destroy.js:9-15：孵化中的 spawn 被毁 → 雏形 creep 一并移除（按主人+名查）
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
    let (pos, store) = (obj.pos(), structure_store_energy(obj));
    w.remove_obj(id);
    if store > 0 {
        w.create_energy(pos.0, pos.1, store);
    }
}

/// 结构携带的能量（击毁落地用；M1 仅 energy 槽）。
fn structure_store_energy(obj: &RoomObject) -> i64 {
    match obj {
        RoomObject::Spawn(o) => o.store.energy as i64,
        RoomObject::Extension(o) => o.store.energy as i64,
        RoomObject::Storage(o) => o.store.energy as i64,
        RoomObject::Generic(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data.pointer("/store/energy")
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        }
        _ => 0,
    }
}
