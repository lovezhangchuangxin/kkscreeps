//! hits 模型与伤害/治疗施加（官方 `_damage.js` / `creeps/heal.js`、
//! `creeps/rangedHeal.js` / `creeps/tick.js` 伤害-治疗段 /
//! `creeps/_recalc-body.js` / `structures/_destroy.js` 的 B1 无 boost/power 子集）。
//!
//! 官方伤害/治疗链路（file:line 见各函数注释）：
//! - creep 目标的伤害与治疗都**延迟施加**：intent 期分别累加进 `_damageToApply`
//!   （_damage.js:20）与 `_healToApply`（heal.js:29），在该 creep 自己的 tick 里
//!   先伤害后治疗、死亡判定在两者之后（tick.js:118-136）——同 tick 多次攻击与
//!   多次治疗各自叠加、等量伤害与治疗互相抵消；
//! - 结构目标立即扣血（_damage.js:32），击毁走 `_destroy`（:46-52）；
//! - 近战对 creep 目标触发反击（同样延迟），除非攻击者站在 rampart 上（:17-19）；
//! - 部件伤害分布 = 总 hits 从 body 尾部灌满（_recalc-body.js:10-18）——
//!   数组前部的部件先掉 hit，前列部件先报废（治疗反向回补）。

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
            attack_back = kk_world::types::calc_body_effectiveness(
                &t.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            );
        }
        // _damage.js:20-21：creep 伤害延迟（累加器）
        *w.damage_to_apply.entry(target_id).or_insert(0) += damage;
    } else {
        // _damage.js:26-32：墙/rampart 的 PWR_FORTIFY/EFFECT_INVULNERABILITY 免疫
        // 属 power 体系（B1 无）；结构立即扣血
        let new_hits = object_hits(&target) - damage;
        if new_hits <= 0 {
            // _damage.js:46-52：击毁——_destroy 建废墟承接，随后落
            // EVENT_OBJECT_DESTROYED（:51，data.type = 官方对象 type；
            // 该事件先于反击/主攻击事件——:89/:93 的官方序）
            destroy_structure(w, target_id);
            w.push_event(
                c::EVENT_OBJECT_DESTROYED,
                target_id,
                json!({ "type": type_name_of(&target) }),
            );
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

/// creep tick 的延迟伤害/治疗结算（官方 tick.js:118-150，无 boost/power）。
/// 返回 false 表示对象已死亡（调用方停止后续处理）。
///
/// 施加序严格对照官方：先伤害后治疗（:120-128，伤害可使 hits 瞬时为负、
/// 治疗随后回补——**死亡判定在两者之后**，同 tick 等量伤害与治疗互相抵消，
/// 即 COMBAT-SIMULT-002 的引擎侧语义）→ 钳 hitsMax（:130-132，hitsMax =
/// body.length×100，create-creep.js:84）→ ≤0 死（:134-136）→ 有变化才重排
/// 部件（:137-139，治疗也重排——前列部件先回血）→ 净减少才缩容掉落（:141-143）。
pub fn apply_pending_hits(w: &mut Working, id: ObjId) -> bool {
    // 官方两个真值门（`if (object._damageToApply)` / `if (object._healToApply)`）：
    // 0 值跳过（无 ATTACK/HEAL 部件的 intent 不进结算路径）
    let damage = w.damage_to_apply.remove(&id).unwrap_or(0);
    let heal = w.heal_to_apply.remove(&id).unwrap_or(0);
    if damage <= 0 && heal <= 0 {
        return true;
    }
    let (old_hits, hits_max) = match w.creep(id) {
        Some(creep) => (creep.hits as i64, creep.body.len() as i64 * 100),
        None => return true,
    };
    // _applyDamage（无 boost：裸减）+ tick.js:126-128 治疗后加
    let mut new_hits = old_hits - damage + heal;
    if new_hits > hits_max {
        new_hits = hits_max;
    }
    if new_hits <= 0 {
        // tick.js:134-136：hits 归零 → 暴力死亡，与 TTL 死亡同路径
        //（B3 起 die_creep 即官方 _die.js 全量：墓碑/store 转移/事件）
        super::creep::die_creep(w, id);
        return false;
    }
    // tick.js:137：hits 无变化（如满血被治疗后钳回）不重排
    if new_hits == old_hits {
        return true;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.hits = new_hits as u32;
        // tick.js:139 → _recalc-body.js：总 hits 从尾部灌满，前列部件先掉/先回
        distribute_hits(&mut creep.body, new_hits);
    }
    // tick.js:141-143 → _drop-resources-without-space.js：仅净减少（CARRY 缩容）
    if new_hits < old_hits {
        drop_energy_over_capacity(w, id);
    }
    true
}

/// 官方 _recalc-body.js:10-18：从 body 尾部向前灌 `min(剩余, 100)`——
/// 受伤自数组头部向尾部推进（前列部件先报废）。
///
/// 关于官方 `_oldHits`（双路评审追证，结论：不可观测的死机制，不移植）：
/// - _recalc-body.js:11 在重算时冻结 `i._oldHits = i._oldHits || i.hits`
///   （首次受伤时为分发前的初值 100）；utils.js:623-636 calcBodyEffectiveness
///   不带 withoutOldHits 时按 `hits || _oldHits` 计数（attack/dismantle/
///   heal/ranged*/harvest/疲劳/反击全族）——若 _oldHits 能存活到下一 tick，
///   已损毁部件将永久保留功率；
/// - 但 recalcBody 的全部四个调用点都**紧随** bulk.update(obj, {body, ...})
///   持久化（creeps/tick.js:139→145、spawns/renew-creep.js:49→52、
///   labs/boost-creep.js:58→60、labs/unboost-creep.js:29→30），而
///   driver/lib/bulk.js:33-55 的 update 会 `data[key] = originalValue` 把
///   载荷 body **别名到活对象的数组**，removeHidden（:6-17）随即原位 delete
///   所有 `_` 前缀键——活对象与落库载荷同时失去 _oldHits（已用官方 lodash
///   复刻验证）；两行之间只跑 _drop-resources-without-space（只读 store）；
/// - 故下一 tick 的 intent 期功率与疲劳恢复看到的 body 恒无 _oldHits，
///   可观测语义 = `hits > 0` 计数（与 kk count_active 一致；玩家面
///   game/creeps.js:9-18 的尾部扫描 break 同语义互证）。官方若真按
///   "损毁部件永久保留功率"运行，screeps-ok 7.16 族反而会转红。
pub fn distribute_hits(body: &mut [BodyPart], mut hits: i64) {
    for part in body.iter_mut().rev() {
        part.hits = hits.clamp(0, 100) as u32;
        hits = (hits - 100).max(0);
    }
}

/// 官方 _drop-resources-without-space.js（creeps/tick.js:141-143 缩容/拆解
/// 路径）：按 RESOURCES_ALL 序逐资源 `min(store[res], calcResources−cap)`
/// 掉落（B4 评审 3 P2-1 清偿——原 M1 单 energy 形态在命名矿物槽下偏离：
/// 官方掉矿物、我方留在 creep 身上超容）。键序 = RESOURCES_ALL 声明序
///（energy 先、矿物族、G 尾——资源族稳定序）。
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
    // 官方 _drop-resources-without-space 按 RESOURCES_ALL **声明序**逐资源
    // min(存量, overflow) 掉落（复审 F2 修正：原按 store_extra 插入序——
    // 多槽部分掉落时 per-resource 分布偏离官方）
    let mut left = overflow;
    for r in kk_core::constants::RESOURCES_ALL {
        if left <= 0 {
            break;
        }
        let held_res = if *r == "energy" {
            store.energy as i64
        } else if *r == kk_core::constants::RESOURCE_GHODIUM {
            store.ghodium as i64
        } else {
            extra
                .iter()
                .find(|s| s.resource == *r)
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
        // B4：extractor 有 hits（build.js:217）；mineral 无 hits 概念（官方
        // 矿物对象无 hits 键——落入 _ => 0 拒攻）
        RoomObject::Extractor(o) => o.hits as i64,
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
        // B4：官方 build.js:218 的 extractor 落成 hitsMax = EXTRACTOR_HITS
        RoomObject::Extractor(_) => c::EXTRACTOR_HITS,
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
        Some(RoomObject::Extractor(o)) => o.hits = hits,
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

/// 官方 structures/_destroy.js 全量（B3）：孵化中 spawn 的雏形一并移除、
/// 建废墟承接结构存照与全量 store、移除本体。核弹抑制分支（:21
/// `attackType != EVENT_ATTACK_TYPE_NUKE`）M2 无核弹路径——恒建废墟。
/// invaderCore 分支（:17-19）NPC 体系不建模。
/// 事件由调用方落账：_damage 路径在 _damage.js:51（本函数返回后 push）；
/// destroyStructure room intent 路径见 room.rs（OSS 无 push，套件期望有
/// ——登记报告）。
pub(super) fn destroy_structure(w: &mut Working, id: ObjId) {
    // _destroy.js:8-15：孵化中的 spawn 被毁 → 雏形 creep 一并移除（按主人+名查）
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
    // :24-36 衰减钟：RUIN_DECAY_STRUCTURES[type] || RUIN_DECAY（powerBank 10、
    // 其余 500）；结构存照 {id, type, hits: 0, hitsMax, user}
    let decay = c::RUIN_DECAY_STRUCTURES
        .iter()
        .find(|(k, _)| *k == structure_type)
        .map(|(_, v)| *v)
        .unwrap_or(c::RUIN_DECAY) as u64;
    let hits_max = structure_hits_max(w, obj).map(|v| v.clamp(0, u32::MAX as i64) as u32);
    // :37-38：user 仅真值落（官方 `if(object.user)`——空串 falsy）；
    // 归属语义与投影/视野面共 kk_world::world_json::object_owner 单源（P1）
    let owner = kk_world::world_json::object_owner(obj)
        .filter(|o| !o.is_empty())
        .map(|o| o.into_owned());
    // :40：store = object.store || {}（全量承接——双槽 + 额外资源）
    let (store, store_extra) = structure_store(obj);
    // :41-45 effects/collapse（power 体系）N/A
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

/// 结构携带的全量资源（击毁废墟承接用）：typed 结构读双槽；Generic 透传
/// （container/tower 等）解析 data.store 的开放资源对象——energy/G 进双槽、
/// 其余进额外列表；无 store 概念的类型（road/wall）为空。
fn structure_store(
    obj: &RoomObject,
) -> (kk_schema::room::Store, Vec<kk_schema::room::ResourceStack>) {
    let mut store = kk_schema::room::Store::default();
    let mut extra = Vec::new();
    match obj {
        RoomObject::Spawn(o) => store = o.store.clone(),
        RoomObject::Extension(o) => store = o.store.clone(),
        // B4：storage 可持命名矿物——废墟承接全量 store（_destroy.js:37 的
        // `object.store || {}` 开放对象语义）
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
