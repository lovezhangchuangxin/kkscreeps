//! 处理即时结构伤害、延迟 creep 伤害与治疗，以及死亡后的资源转移。
//!
//! Creep 在自身 tick 中先受伤、后治疗，再判断是否死亡；结构击毁立即创建
//! 废墟。Creep 的累计伤害按现有身体顺序计算 TOUGH 强化减伤，再合并治疗；
//! power 免疫尚未模拟。

mod tough;

/// 意图阶段后逐项运行结构受击槽并落地，目标已消失则跳过。
/// 来源消失不撤销主伤害；槽失败由调用方丢弃整个房间候选。
pub(super) fn drain_mod_damage(ctx: &super::RoomCtx) -> Result<(), super::SettleError> {
    let pending = std::mem::take(&mut ctx.borrow_mut().mod_damage_pending);
    if !pending.is_empty() {
        let contrib = ctx.borrow().plane.contrib.clone();
        for item in pending {
            let gone = {
                let w = ctx.borrow();
                !matches!(w.room.objects.get(&item.target), Some(RoomObject::Mod(_)))
            };
            if gone {
                continue;
            }
            let damage = contrib
                .run_mod_incoming(
                    ctx,
                    item.target,
                    item.damage,
                    item.attacker,
                    item.attack_type,
                )
                .map_err(super::SettleError)?;
            apply_mod_damage(&mut ctx.borrow_mut(), item, damage);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;

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
    apply_damage_inner(w, attacker, target_id, damage, attack_type, true, None);
}

/// JS 策略已决定目标和伤害，不再隐含追加原生近战反击。生命值、
/// 延迟队列、死亡/废墟与事件仍走共用应用器，避免出现第二套死亡规则。
pub(in crate::settle) fn apply_damage_without_retaliation(
    w: &mut Working,
    attacker: ObjId,
    target_id: ObjId,
    damage: i64,
    attack_type: i64,
) {
    apply_damage_inner(w, attacker, target_id, damage, attack_type, false, None);
}

/// 本动作显式反击量；直接登记一次 HIT_BACK，不递归进入伤害策略。
/// 调用方先验证反击量及执行者队列的累计表示域。
pub(in crate::settle) fn apply_damage_with_retaliation(
    w: &mut Working,
    attacker: ObjId,
    target: ObjId,
    amount: i64,
    attack_type: i64,
    retaliation: i64,
) {
    apply_damage_inner(
        w,
        attacker,
        target,
        amount,
        attack_type,
        false,
        Some(retaliation),
    );
}

fn apply_damage_inner(
    w: &mut Working,
    attacker: ObjId,
    target_id: ObjId,
    damage: i64,
    attack_type: i64,
    native_retaliation: bool,
    retaliation: Option<i64>,
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

    let mut attack_back: i64 = retaliation.unwrap_or(0);
    // 反击抑制取攻击发生时的房主状态，托管排空不能被后续 unclaim/激活改变。
    let retaliation_suppressed = w.controller().is_some_and(|ctl| {
        ctl.owner.as_deref() == Some(attacker_owner.as_str()) && ctl.safe_mode > w.time
    });
    if let RoomObject::Creep(t) = &target {
        if native_retaliation
            && attack_type == c::EVENT_ATTACK_TYPE_MELEE
            && rampart_at(w, attacker_pos).is_none()
        {
            attack_back = kk_world::types::calc_body_effectiveness(
                &t.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            );
        }
        *w.damage_to_apply.entry(target_id).or_insert(0) += damage;
    } else if let RoomObject::Generic(t) = &target
        && kk_world::domain::generic::type_name(&t.data).as_deref() == Some("powerCreep")
    {
        // powerCreep 受击与 creep 同走延迟队列（对象 tick 应用并钳上限），
        // 无近战反击（官方 _damage.js 的 powerCreep 分支）；死亡由全局
        // 阶段的 hits<=0 扫描结算，不走结构摧毁/废墟路径。
        *w.damage_to_apply.entry(target_id).or_insert(0) += damage;
        // 官方 _damage.js:74 对受击的 powerCreep 同写 attacked（坐标=
        // 攻击者），与 creep 分支同形。
        super::powers::set_pc_action_log(
            w,
            target_id,
            "attacked",
            json!({ "x": attacker_pos.0, "y": attacker_pos.1 }),
        );
        w.record_action(
            target_id,
            "attacked",
            super::action_log::ActionLogPayload::Coords {
                x: attacker_pos.0,
                y: attacker_pos.1,
            },
        );
    } else if let RoomObject::Mod(m) = &target {
        // G09.c：托管结构受击槽（Mod 包覆 builtin:mod.receiveDamage）时
        // 扣减与声明反击挂起，意图阶段后由房间循环经共享句柄跑槽再落地；
        // 主攻击事件与攻击者动作标记仍在意图时刻经共用尾部发出。Native
        // 派发（默认引擎）零开销内联。
        if w.plane.contrib.is_mod_incoming_hosted() {
            w.mod_damage_pending.push(PendingModDamage {
                target: target_id,
                attacker,
                damage,
                attack_type,
                retaliation,
                retaliation_suppressed,
            });
        } else {
            mod_damage_effects(
                w,
                &m.type_key,
                target_id,
                attacker,
                retaliation_suppressed,
                damage,
                damage,
                retaliation,
            );
        }
    } else {
        // 官方 _damage.js:26-31：非核弹对 constructedWall/rampart 的伤害被
        // FORTIFY 或 EFFECT_INVULNERABILITY 完全免疫（严格 >），整个跳过
        // 扣血、摧毁与后续事件；SHIELD 不在此门（护盾 rampart 可被打）。
        if attack_type != c::EVENT_ATTACK_TYPE_NUKE
            && matches!(
                type_name_of(&target).as_ref(),
                "constructedWall" | "rampart"
            )
            && w.room.effects.get(&target_id).is_some_and(|entries| {
                entries.iter().any(|e| {
                    (e.power == c::PWR_FORTIFY || e.effect == Some(c::EFFECT_INVULNERABILITY))
                        && e.end_time > w.time
                })
            })
        {
            return;
        }
        let new_hits = object_hits(&target) - damage;
        if new_hits <= 0 {
            destroy_structure_with(w, target_id, attack_type);
            w.push_event(
                c::EVENT_OBJECT_DESTROYED,
                target_id,
                json!({ "type": type_name_of(&target) }),
            );
        } else {
            set_structure_hits(w, target_id, new_hits);
        }
        // 官方 _damage.js:35-37：对 powerBank 的任意伤害按
        // POWER_BANK_HIT_BACK 反射给攻击者（近战/远程/群体同形），经
        // 攻击者待伤害队列在应用时受 TOUGH 减伤。毫精度整除向下取整、
        // 不足 1 不触发——官方 JS 把 0.5 小数存入 _damageToApply 并以
        // 小数 hits 消费，套件整数反射不可观测，登记为有意分歧；
        // 显式反击策略存在即整体替换（与 G09.d 声明反击同关系）。
        // 核弹砸银行的攻击者无耐久面（官方 :88 对无 actionLog 的核弹
        // 写 attacked 是崩溃角案例），反射入待伤害队列后无人消费，
        // 等效丢弃；击杀不产废墟由 destroy 的 NUKE 门承担。
        if retaliation.is_none() && type_name_of(&target) == "powerBank" {
            let hit_back_milli = (c::POWER_BANK_HIT_BACK * 1000.0) as i64;
            attack_back = attack_back.max(damage.saturating_mul(hit_back_milli) / 1000);
        }
    }

    if retaliation.is_none()
        && attack_back > 0
        && let Some(ctl) = w.controller()
        && ctl.owner.as_deref() == Some(attacker_owner.as_str())
        && ctl.safe_mode > w.time
    {
        attack_back = 0;
    }

    // 渲染面动作载荷与位图同点写入（官方 _damage.js:60-91：attack/
    // rangedAttack 记目标坐标，attacked 记攻击者坐标；反伤的 attacked
    // 记受击目标坐标）。非 creep 攻击者（塔）远程也写 attack 键
    //（官方 towers/attack.js:58）；RANGED_MASS 不在此记——群体攻击由
    // 调用方写 rangedMassAttack {}（rangedMassAttack.js:58）。
    let target_pos = target.pos();
    if attack_type == c::EVENT_ATTACK_TYPE_MELEE || attack_type == c::EVENT_ATTACK_TYPE_DISMANTLE {
        if let Some(a) = w.creep_mut(attacker) {
            set_action_log(&mut a.action_log, ACTION_ATTACK);
        }
        w.record_action(
            attacker,
            "attack",
            super::action_log::ActionLogPayload::Coords {
                x: target_pos.0,
                y: target_pos.1,
            },
        );
    } else if attack_type == c::EVENT_ATTACK_TYPE_RANGED {
        if let Some(a) = w.creep_mut(attacker) {
            set_action_log(&mut a.action_log, ACTION_RANGED_ATTACK);
        }
        let is_creep = matches!(w.room.objects.get(&attacker), Some(RoomObject::Creep(_)));
        w.record_action(
            attacker,
            if is_creep { "rangedAttack" } else { "attack" },
            super::action_log::ActionLogPayload::Coords {
                x: target_pos.0,
                y: target_pos.1,
            },
        );
    }
    if matches!(target, RoomObject::Creep(_)) {
        if let Some(t) = w.creep_mut(target_id) {
            set_action_log(&mut t.action_log, ACTION_ATTACKED);
        }
        w.record_action(
            target_id,
            "attacked",
            super::action_log::ActionLogPayload::Coords {
                x: attacker_pos.0,
                y: attacker_pos.1,
            },
        );
    } else if let RoomObject::Generic(g) = &target
        && matches!(
            kk_world::domain::generic::structure_type(&g.data).as_deref(),
            Some("tower" | "link" | "lab" | "invaderCore" | "factory")
        )
    {
        // 官方 _damage.js:73 对持有动作模板的结构目标（processor.js:72-105
        // 的 tower/link/lab/invaderCore/factory）同写 attacked（坐标=
        // 攻击者）；无模板的墙/路/容器不写。powerCreep 目标已在上文
        // 受击分支记录。
        w.record_action(
            target_id,
            "attacked",
            super::action_log::ActionLogPayload::Coords {
                x: attacker_pos.0,
                y: attacker_pos.1,
            },
        );
    }

    if attack_back > 0 {
        *w.damage_to_apply.entry(attacker).or_insert(0) += attack_back;
        if let Some(a) = w.creep_mut(attacker) {
            set_action_log(&mut a.action_log, ACTION_ATTACKED);
        }
        w.record_action(
            attacker,
            "attacked",
            super::action_log::ActionLogPayload::Coords {
                x: target_pos.0,
                y: target_pos.1,
            },
        );
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
/// 待伤害是减伤前数值；TOUGH 按受伤前部件耐久统一减伤，治疗不增加本轮承伤额度。
pub fn apply_pending_hits(w: &mut Working, id: ObjId) -> bool {
    apply_pending_hits_selected(w, id, None)
}

pub(crate) fn default_pending_damage(w: &Working, id: ObjId) -> Option<i64> {
    let raw = w.damage_to_apply.get(&id).copied().unwrap_or(0);
    (raw > 0)
        .then(|| {
            w.creep(id)
                .map(|creep| tough::effective_damage(&creep.body, raw))
        })
        .flatten()
}

/// selected 是受击链选出的最终扣血量，不能再次应用 TOUGH。
pub(crate) fn apply_pending_hits_selected(
    w: &mut Working,
    id: ObjId,
    selected: Option<i64>,
) -> bool {
    let damage = w.damage_to_apply.remove(&id).unwrap_or(0);
    let heal = w.heal_to_apply.remove(&id).unwrap_or(0);
    if damage <= 0 && heal <= 0 {
        return true;
    }
    let (old_hits, hits_max, damage) = match w.creep(id) {
        Some(creep) => (
            creep.hits as i64,
            creep.body.len() as i64 * 100,
            selected.unwrap_or_else(|| tough::effective_damage(&creep.body, damage)),
        ),
        None => return true,
    };
    let mut new_hits = i128::from(old_hits) - i128::from(damage) + i128::from(heal);
    if new_hits > i128::from(hits_max) {
        new_hits = i128::from(hits_max);
    }
    if new_hits <= 0 {
        super::creep::die_creep(w, id);
        return false;
    }
    if new_hits == i128::from(old_hits) {
        return true;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.hits = new_hits as u32;
        distribute_hits(&mut creep.body, new_hits as i64);
    }
    if new_hits < i128::from(old_hits) {
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
        // 耐久信封键（G09.b）：hits 声明类型的耐久由伤害动作维护，
        // 未携带或非法值按无耐久处理（发布前校验封直改形态）。
        RoomObject::Mod(o) => {
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
                .terrain()
                .check_mask(o.x as usize, o.y as usize, kk_core::TERRAIN_MASK_SWAMP)
            {
                hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO;
            }
            if w.room
                .terrain()
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
        // 耐久声明（G09.b）的上限即冻结声明值，经信封键读出——
        // 玩家战斗读面与策略伤害目标校验同此接纳。
        RoomObject::Mod(o) => {
            let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
            data.get("hitsMax").and_then(|v| v.as_i64())?
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
        // powerBank 上限恒 POWER_BANK_HITS（放置覆盖 hits 不改上限，
        // 官方 hitsMax 不随伤害变化；废墟 structure.hitsMax 同源）。
        "powerBank" => c::POWER_BANK_HITS,
        "link" => links.hits_max,
        _ => return None,
    })
}

/// G09.c：托管受击槽挂起的一次 Mod 伤害（含反击原语），排空时经槽
/// 修改伤害量后落地。攻击者 ID 与抑制决定保留攻击时刻语义，不依赖来源继续存活。
#[derive(Clone, Copy)]
pub struct PendingModDamage {
    pub target: ObjId,
    pub attacker: ObjId,
    pub damage: i64,
    pub attack_type: i64,
    pub retaliation: Option<i64>,
    pub retaliation_suppressed: bool,
}

/// Mod 受击的扣减与声明式反击效果（G09.d）；Native 派发在伤害入口
/// 内联调用，托管派发在意图阶段后的排空点调用（伤害量已经过结构
/// 受击槽；主攻击事件已在意图时刻发出，此处不再重复）。
// 扣减与反击参数各司其职（damage 与 retaliation_base 语义不同，见下），
// 收敛结构体会掩盖这一区分；与 market/projection 的既有豁免同款。
#[allow(clippy::too_many_arguments)]
pub(in crate::settle) fn mod_damage_effects(
    w: &mut Working,
    type_key: &str,
    target_id: ObjId,
    attacker: ObjId,
    retaliation_suppressed: bool,
    damage: i64,
    // 声明反击的基数是原始伤害（combat 契约「按减伤前量」）；槽修改
    // 只改变扣减量。Native 路径两者同值，托管排空路径分别传入。
    retaliation_base: i64,
    retaliation: Option<i64>,
) {
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return; // 内联路径目标已在场；排空路径有前置门，此为防御分支
    };
    // 耐久声明对象（G09.b）走结构语义：立即扣减、不经受击槽/TOUGH；
    // 死亡即移除并发事件，type 为类型键——不产废墟（废墟绑定建筑
    // 成本模型，Mod 对象不经工地），store 信封内容随对象消失。
    let new_hits = object_hits(&target) - damage;
    if new_hits <= 0 {
        w.remove_obj(target_id);
        w.push_event(
            c::EVENT_OBJECT_DESTROYED,
            target_id,
            json!({ "type": type_key }),
        );
    } else {
        set_structure_hits(w, target_id, new_hits);
    }
    // 声明式反击（G09.d，官方 powerBank damage×0.5 同构）：任意攻击
    // 类型、不耗自身耐久，毫精度整除向下取整（不足 1 不触发）。与
    // 显式反击策略的关系同原生反击：策略存在即整体替换（空表可禁
    // 用反击，combat.md 契约）。房主安全模式抑制与上游 _damage.js
    // 的 powerBank 分支同款（controller 主人==攻击者主人且 safemode
    // 激活时清零）。反击落点按攻击者类型分流：creep 走延迟队列
    // （意图阶段先于其 tick，不丢）；塔等结构攻击者的队列无人消费，
    // 立即扣减。立即扣减不经伤害入口重入（禁递归）。事件序与既有
    // 反击一致：摧毁→反击→主攻击。
    if retaliation.is_none()
        && let Some(milli) = mod_retaliation_milli(w, type_key)
    {
        let back = if retaliation_suppressed {
            0
        } else {
            retaliation_base.saturating_mul(milli) / 1000
        };
        if back > 0 {
            // 来源消失不撤销主伤害；反击无存活落点时不登记幽灵队列或事件。
            let Some(attacker_obj) = w.room.objects.get(&attacker).cloned() else {
                return;
            };
            if matches!(attacker_obj, RoomObject::Creep(_)) {
                *w.damage_to_apply.entry(attacker).or_insert(0) += back;
                if let Some(a) = w.creep_mut(attacker) {
                    set_action_log(&mut a.action_log, ACTION_ATTACKED);
                }
                // 与原生反击同形：攻击者 attacked 记 Mod 目标坐标
                //（_damage.js:86-88 同构）。
                if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
                    w.record_action(
                        attacker,
                        "attacked",
                        super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
                    );
                }
            } else {
                // 无耐久面攻击者（如核弹本体）无处落反击：官方把
                // powerBank 式 hitback 写进攻击者 _damageToApply，而该
                // 队列只有 creeps/tick 消费（creeps/tick.js:120-122）——
                // 对核弹等效丢弃。此处同款丢弃，防止移除核弹本体截断
                // 余下爆炸格。
                if object_hits(&attacker_obj) <= 0 {
                    return;
                }
                let attacker_hits = object_hits(&attacker_obj) - back;
                if attacker_hits <= 0 {
                    destroy_structure(w, attacker);
                    w.push_event(
                        c::EVENT_OBJECT_DESTROYED,
                        attacker,
                        json!({ "type": type_name_of(&attacker_obj) }),
                    );
                } else {
                    set_structure_hits(w, attacker, attacker_hits);
                }
            }
            w.push_event(
                c::EVENT_ATTACK,
                target_id,
                json!({
                    "targetId": attacker.as_str(),
                    "damage": back,
                    "attackType": c::EVENT_ATTACK_TYPE_HIT_BACK,
                }),
            );
        }
    }
}

/// 排空点入口：跑过结构受击槽的伤害量落回 Mod 效果（含反击原语）。
pub(in crate::settle) fn apply_mod_damage(w: &mut Working, pending: PendingModDamage, damage: i64) {
    let target = w.room.objects.get(&pending.target).cloned();
    let Some(RoomObject::Mod(m)) = target else {
        return; // 目标在排空前已消失（同轮更早的挂起项击毁）
    };
    mod_damage_effects(
        w,
        &m.type_key,
        pending.target,
        pending.attacker,
        pending.retaliation_suppressed,
        damage,
        pending.damage,
        pending.retaliation,
    );
}

/// 读 Mod 类型的声明式反击比率（G09.d，毫精度 1..=1000）；类型不在
/// 计划中或未声明返回 None。Engine::mod_object_type 的查找在 Working
/// 语境不可达，此处按计划投影同形解析。
fn mod_retaliation_milli(w: &Working, type_key: &str) -> Option<i64> {
    let key = kk_mod_api::Key::new(type_key.to_string()).ok()?;
    let handle = w.plane.plan.projection().resolve(&key)?;
    let ty = w.plane.plan.projection().get(handle)?;
    ty.descriptor.retaliation_milli
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
            o.data = data.to_string().into();
        }
        Some(RoomObject::Mod(o)) => {
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
/// 常规摧毁：建废墟（EVENT_OBJECT_DESTROYED 由调用方发出）。
pub(super) fn destroy_structure(w: &mut Working, id: ObjId) {
    destroy_structure_inner(w, id, true);
}

/// 按攻击类型摧毁：核弹击杀不建废墟（官方 _destroy.js:21 对
/// EVENT_ATTACK_TYPE_NUKE 跳过废墟创建）。
pub(super) fn destroy_structure_with(w: &mut Working, id: ObjId, attack_type: i64) {
    destroy_structure_inner(w, id, attack_type != c::EVENT_ATTACK_TYPE_NUKE);
}

fn destroy_structure_inner(w: &mut Working, id: ObjId, make_ruin: bool) {
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
    // 官方 _destroy.js：对象带 EFFECT_COLLAPSE_TIMER 时废墟衰减延至
    // 坍塌截止（布防结构战毁后残骸留到据点坍塌）。正常世界截止恒在
    // 未来；已过期条目仅经管理/恢复通道出现，饱和减避免下溢回绕。
    let decay = w
        .room
        .effects
        .get(&id)
        .and_then(|entries| {
            entries
                .iter()
                .find(|e| e.effect == Some(c::EFFECT_COLLAPSE_TIMER))
                .map(|e| e.end_time.saturating_sub(w.time))
        })
        .map_or(decay, |remaining| decay.max(remaining));
    let hits_max = structure_hits_max(w, obj).map(|v| v.clamp(0, u32::MAX as i64) as u32);
    let owner = kk_world::domain::object_owner(obj)
        .filter(|o| !o.is_empty())
        .map(|o| o.into_owned());
    let (store, store_extra) = structure_store(obj);
    w.remove_obj(id);
    // 动作到此结束；分配错误由共享候选在发布前报告。
    if !make_ruin {
        return;
    }
    let _ = w.insert_new(RoomObject::Ruin(kk_schema::room::RuinObj {
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
