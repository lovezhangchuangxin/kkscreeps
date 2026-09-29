//! 战斗族：attack/repair/dismantle/heal(ranged)（官方同名 intent 文件；
//! 伤害机制见 damage.rs，safeMode 门）。B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use super::{ACTION_HEAL, ACTION_HEALED, ACTION_REPAIR, set_action_log};
use crate::intent::IntentData;
use crate::settle::Working;
use crate::settle::damage;
use crate::settle::movement;

/// 官方 attack.js（近战）：邻接、有 hits 的目标、他人 safeMode 拒、目标格
/// rampart 重定向、ATTACK 部件数 × ATTACK_POWER（无 boost）。
pub(super) fn intent_attack(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // attack.js:13-15：spawning 拒；:38 功率 = 活跃 ATTACK × ATTACK_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Attack,
                "attack",
                "attack",
                c::ATTACK_POWER,
            ),
        ),
        _ => return,
    };
    // attack.js:18：目标不存在或自指拒
    if target_id == id {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let (tx, ty) = target.pos();
    // attack.js:21-23：近战射程 1（Chebyshev）
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    // attack.js:24-26：spawning 中的 creep 目标拒
    if let RoomObject::Creep(t) = &target
        && t.spawning
    {
        return;
    }
    // attack.js:27-29：无 hits 的对象不可攻（source/controller/工地/掉落/旗）
    if damage::object_hits(&target) <= 0 {
        return;
    }
    // attack.js:30-32：他人（含无主房）safeMode 生效期拒
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // attack.js:33-36：目标格有 rampart → 转打 rampart（任何主人/公开性）
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    // attack.js:40：统一伤害入口（0 功率也走——官方引擎层无部件预检）
    damage::apply_damage(w, id, target_id, power, c::EVENT_ATTACK_TYPE_MELEE);
    // attack.js:42 `object._attack = true`：官方全库只写不读的死标记，不移植
}

/// 官方 attack.js:30-32 / dismantle.js:24-26 的 safeMode 门：controller 归
/// 他人（undefined user ≠ object.user 同样成立）且 safeMode > gameTime。
fn blocked_by_safe_mode(w: &Working, actor: &str) -> bool {
    w.controller()
        .is_some_and(|ctl| ctl.owner.as_deref() != Some(actor) && ctl.safe_mode > w.time)
}

/// 官方 repair.js：射程 3、WORK × REPAIR_POWER、能量按 REPAIR_COST 折算消耗、
/// hitsMax 钳制（无 boost）。
pub(super) fn intent_repair(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // repair.js:8-13：spawning 或空仓拒；:23 功率 = 活跃 WORK × REPAIR_POWER
    // 官方两步（repair.js:23-38，GPT 二轮复审 P1-1）：基础功率定 effect 与
    // cost；强化增量数组定 boostedEffect（加血量）
    let (pos, energy, repair_power, boost_incs) = match w.creep(id) {
        Some(c) if !c.spawning && c.store.energy > 0 => (
            (c.x, c.y),
            c.store.energy as i64,
            BodyKind::count_active(&c.body, BodyKind::Work) as i64 * c::REPAIR_POWER,
            kk_world::types::boost_increments(
                &c.body,
                BodyKind::Work,
                "work",
                "repair",
                c::REPAIR_POWER,
            ),
        ),
        _ => return,
    };
    // repair.js:15-18：目标须为可修结构（CONSTRUCTION_COST 类型 + 有 hitsMax）
    // 且未满血——creep/source 等不在 CONSTRUCTION_COST 表，天然拒
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let Some(hits_max) = damage::structure_hits_max(w, &target) else {
        return;
    };
    let hits = damage::object_hits(&target);
    if hits >= hits_max {
        return;
    }
    let (tx, ty) = target.pos();
    // repair.js:19-21：射程 3（Chebyshev）
    if (tx as i32 - pos.0 as i32).abs() > 3 || (ty as i32 - pos.1 as i32).abs() > 3 {
        return;
    }
    // repair.js:23-27：effect = min(功率, 能量折算, 缺口)；cost = min(存量,
    // ceil(effect×REPAIR_COST))。JS 浮点（energy/0.01、ceil(e*0.01)）在此值域
    // 恒等于整数运算——值域自明：部件数 ≤ MAX_CREEP_SIZE(50) ⇒ effect ≤
    // 50×REPAIR_POWER = 5000；0.01 的 double 表示相对误差 ~2.1e-15，e×0.01 真值
    // 偏离精确整数至多 ~1e-11，而 5000 量级 double 的半 ULP 仅 ~4.6e-13——
    // 乘积必舍入到精确整数 ⇒ ceil == ceil(e/100) 整除（数值验证另覆盖至 6×10⁸
    // 无失配）
    let energy_remaining = energy * 100; // energy / REPAIR_COST
    let missing = hits_max - hits;
    let effect = repair_power.min(energy_remaining).min(missing);
    let cost = energy.min((effect + 99) / 100); // ceil(effect × REPAIR_COST)
    // repair.js:28-38：增量数组按 effect 点数取、floor、钳缺口——零效应拒
    let boosted = kk_world::types::apply_boost(&boost_incs, effect, missing);
    if effect <= 0 {
        return;
    }
    // repair.js:44-51：加血（boosted 亦钳 hitsMax）、扣能按基础 cost
    damage::set_structure_hits(w, target_id, (hits + boosted).min(hits_max));
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy -= cost as u32;
        set_action_log(&mut creep.action_log, ACTION_REPAIR);
    }
    // repair.js:58-60（事件 amount = boostedEffect）
    w.push_event(
        c::EVENT_REPAIR,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": boosted,
            "energySpent": cost,
        }),
    );
}

/// 官方 dismantle.js：邻接、CONSTRUCTION_COST 结构、他人 safeMode 拒、目标格
/// rampart 重定向、WORK × DISMANTLE_POWER、按 DISMANTLE_COST 回流能量。
pub(super) fn intent_dismantle(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    // dismantle.js:10-12：spawning 拒；:33 功率 = 活跃 WORK × DISMANTLE_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            kk_world::types::calc_body_effectiveness(
                &c.body,
                BodyKind::Work,
                "work",
                "dismantle",
                c::DISMANTLE_POWER,
            ),
        ),
        _ => return,
    };
    // dismantle.js:17-19：目标须为可建类型结构（creep/source 等拒）
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if kk_world::types::construction_cost(&movement::type_name_of(&target)).is_none() {
        return;
    }
    let (tx, ty) = target.pos();
    // dismantle.js:21-23：邻接
    if (tx as i32 - pos.0 as i32).abs() > 1 || (ty as i32 - pos.1 as i32).abs() > 1 {
        return;
    }
    // dismantle.js:24-26：safeMode 门
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // dismantle.js:27-30：rampart 重定向——amount 须按重定向后的目标 hits 取
    let target_id = damage::rampart_at(w, (tx, ty)).unwrap_or(target_id);
    let hits = w
        .room
        .objects
        .get(&target_id)
        .map(damage::object_hits)
        .unwrap_or(0);
    // dismantle.js:33-35：amount = min(功率, 目标 hits)；回流 = floor(amount ×
    // DISMANTLE_COST)——值域自明：amount ≤ 50×DISMANTLE_POWER = 2500，
    // 0.005 的 double 相对误差 ~2.1e-15 使乘积偏离整数至多 ~5e-12，远小于该
    // 量级半 ULP（~2.3e-13）⇒ JS floor == amount/200 整除（数值验证另覆盖至
    // 6×10⁸ 无失配）
    let amount = power.min(hits);
    let energy_gain = amount / 200;
    // dismantle.js:42-52：amount 为零不动；先回流能量、超容掉落、再施伤（可击毁）
    if amount <= 0 {
        return;
    }
    if let Some(creep) = w.creep_mut(id) {
        creep.store.energy = creep
            .store
            .energy
            .saturating_add(u32::try_from(energy_gain).unwrap_or(u32::MAX));
    }
    // dismantle.js:47-50：回流后超容立即掉落
    damage::drop_energy_over_capacity(w, id);
    damage::apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_DISMANTLE);
}

/// 官方 heal.js（ranged=false）/ rangedHeal.js（ranged=true）：射程 1/3、
/// creep 目标（自愈合法——无自指拒，对照 attack.js:18）、他人 safeMode 拒、
/// HEAL × HEAL_POWER(12)/RANGED_HEAL_POWER(4) 延迟施加（无能量消耗、无 boost）。
pub(super) fn intent_heal(w: &mut Working, id: ObjId, data: &IntentData, ranged: bool) {
    let Some(target_id) = data.id else { return };
    // heal.js:10-15 / rangedHeal.js:10-15：spawning 拒；功率 = 活跃 HEAL ×
    // HEAL_POWER / RANGED_HEAL_POWER
    let (pos, owner, power) = match w.creep(id) {
        Some(c) if !c.spawning => (
            (c.x, c.y),
            c.owner.clone(),
            // heal.js:26 / rangedHeal.js:26——效应键随分支（BOOSTS.heal 表的
            // heal/rangedHeal 两键乘数同值，官方两调用各自查键）
            if ranged {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "rangedHeal",
                    c::RANGED_HEAL_POWER,
                )
            } else {
                kk_world::types::calc_body_effectiveness(
                    &c.body,
                    BodyKind::Heal,
                    "heal",
                    "heal",
                    c::HEAL_POWER,
                )
            },
        ),
        _ => return,
    };
    // heal.js:17-19：目标必须是已出生的 creep（powerCreep 属 M4）；spawning 拒
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let RoomObject::Creep(t) = &target else {
        return;
    };
    if t.spawning {
        return;
    }
    let (tx, ty) = (t.x, t.y);
    // heal.js:21-23 射程 1 / rangedHeal.js:21-23 射程 3（Chebyshev）
    let range = if ranged { 3 } else { 1 };
    if (tx as i32 - pos.0 as i32).abs() > range || (ty as i32 - pos.1 as i32).abs() > range {
        return;
    }
    // heal.js:25-27：他人 safeMode 拒（与 attack 同形）
    if blocked_by_safe_mode(w, &owner) {
        return;
    }
    // heal.js:29：延迟施加（_healToApply——伤害通道的对称面，tick.js:126-128
    // 在死亡判定前回补，同 tick 与伤害互相抵消）
    *w.heal_to_apply.entry(target_id).or_insert(0) += power;
    // heal.js:31-32：actionLog heal/rangedHeal（官方两键分立，u8 位图并入
    // 同一位——见 ACTION_HEAL 注释）+ healed（自愈同体）
    if let Some(c) = w.creep_mut(id) {
        set_action_log(&mut c.action_log, ACTION_HEAL);
    }
    if let Some(t) = w.creep_mut(target_id) {
        set_action_log(&mut t.action_log, ACTION_HEALED);
    }
    // heal.js:34：事件无条件（0 治疗量也发——引擎层不做部件预检，与 attack 同形）
    w.push_event(
        c::EVENT_HEAL,
        id,
        serde_json::json!({
            "targetId": target_id.as_str(),
            "amount": power,
            "healType": if ranged {
                c::EVENT_HEAL_TYPE_RANGED
            } else {
                c::EVENT_HEAL_TYPE_MELEE
            },
        }),
    );
}
