//! 分派 creep 动作，提供动作日志位、身体和资源槽的共享操作。

mod build;
mod combat;
mod controller;
mod harvest;
mod lifecycle;
mod movement;
mod store;

pub(in crate::settle) use build::intent_build;
pub use build::{BuildAction, build_touch_targets, dismantle_drop_face, slot_build_request};
pub use combat::{
    CombatAction, RetaliationPolicy, combat_touch_targets, slot_combat_apply_damage,
    slot_combat_apply_damage_batch, slot_combat_apply_damage_batch_with_retaliation,
    slot_combat_apply_damage_with_retaliation, slot_combat_apply_heal,
    slot_combat_execute_with_retaliation, slot_combat_get, slot_combat_list, slot_combat_request,
};
pub(in crate::settle) use combat::{intent_dismantle, intent_repair};
pub use harvest::harvest_power;
pub use lifecycle::{die_creep, die_creep_with_rate};
pub use movement::{creep_tick, creep_tick_prepare};
pub use movement::{slot_cancel_direction, slot_pending_direction, slot_request_direction};
pub(in crate::settle) use store::intent_drop_amount_res;
pub(in crate::settle) use store::{intent_transfer, intent_withdraw};

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_schema::room::BodyPart;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::settle::RoomCtx;

use crate::intent::{Action, EXECUTION_ORDER, IntentData};
use crate::settle::energy_resource_name;
use crate::settle::spawn;

pub(in crate::settle) use combat::{
    heal_melee_native, heal_ranged_native, intent_attack, intent_ranged_attack, ranged_mass_native,
};
use controller::{
    intent_attack_controller, intent_claim_controller, intent_generate_safe_mode,
    intent_reserve_controller, intent_sign_controller, intent_upgrade_controller,
};
use harvest::intent_harvest;
use lifecycle::{intent_pull, intent_say, intent_suicide};
pub(in crate::settle) use movement::intent_move;
use store::{intent_drop, intent_pickup};

/// 按 EXECUTION_ORDER 执行动作，Action::executes 根据互斥动作判断是否执行。
/// 判断读取本轮完整动作集合，不随某个动作成功或失败而变化。
pub fn process_creep_intents(
    ctx: &RoomCtx,
    id: ObjId,
    actions: &BTreeMap<Action, IntentData>,
) -> Result<(), String> {
    let present: BTreeSet<Action> = actions.keys().copied().collect();
    for action in EXECUTION_ORDER {
        if !Action::executes(&present, action) {
            continue;
        }
        let data = &actions[&action];
        // 贡献槽（资源/移动/战斗/建造族）进入 JS 前释放全部 Working 借用；
        // 其余动作维持短借用直调。
        match action {
            Action::Transfer => {
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_creep_transfer(ctx, id, data)?;
                continue;
            }
            Action::Withdraw => {
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_creep_withdraw(ctx, id, data)?;
                continue;
            }
            Action::Move => {
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_creep_move(ctx, id, data)?;
                continue;
            }
            Action::Attack
            | Action::RangedAttack
            | Action::RangedMassAttack
            | Action::Heal
            | Action::RangedHeal => {
                let slot_key = action.combat_slot_key().expect("战斗族动作必有槽键");
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_combat(slot_key, ctx, id, data)?;
                continue;
            }
            Action::Build | Action::Repair | Action::Dismantle => {
                let slot_key = action.build_slot_key().expect("建造族动作必有槽键");
                let contrib = ctx.borrow().plane.contrib.clone();
                contrib.run_build(slot_key, ctx, id, data)?;
                continue;
            }
            _ => {}
        }
        let mut w = ctx.borrow_mut();
        let w = &mut *w;
        match action {
            Action::Harvest => intent_harvest(w, id, data),
            Action::UpgradeController => intent_upgrade_controller(w, id, data),
            Action::Drop => intent_drop(w, id, data),
            Action::Pickup => intent_pickup(w, id, data),
            Action::ClaimController => intent_claim_controller(w, id, data),
            Action::ReserveController => intent_reserve_controller(w, id, data),
            Action::AttackController => intent_attack_controller(w, id, data),
            Action::SignController => intent_sign_controller(w, id, data),
            Action::GenerateSafeMode => intent_generate_safe_mode(w, id, data),
            Action::Say => intent_say(w, id, data),
            Action::Suicide => intent_suicide(w, id),
            Action::Pull => intent_pull(w, id, data),
            _ => {}
        }
    }
    Ok(())
}

/// Action log 位标记；近程和远程治疗共用 HEAL 位，由事件的 healType 区分。
pub const ACTION_HARVEST: u8 = 1 << 0;

pub const ACTION_BUILD: u8 = 1 << 1;

pub const ACTION_UPGRADE: u8 = 1 << 2;

pub const ACTION_ATTACK: u8 = 1 << 3;

pub const ACTION_ATTACKED: u8 = 1 << 4;

pub const ACTION_REPAIR: u8 = 1 << 5;

pub const ACTION_HEAL: u8 = 1 << 6;

pub const ACTION_HEALED: u8 = 1 << 7;

pub const ACTION_RESERVE: u16 = 1 << 8;

pub const ACTION_SAY: u16 = 1 << 9;

pub const ACTION_RANGED_ATTACK: u16 = 1 << 10;

pub const ACTION_RANGED_MASS: u16 = 1 << 11;

pub fn set_action_log(bits: &mut u16, bit: impl Into<u16>) {
    *bits |= bit.into();
}

/// 读取固定 energy/G 槽，其他资源返回 0；不适用于有命名资源的共享仓。
fn store_slot(store: &kk_schema::room::Store, resource: &str) -> i64 {
    match resource {
        "energy" => store.energy as i64,
        "G" => store.ghodium as i64,
        _ => 0,
    }
}

fn store_slot_sub(store: &mut kk_schema::room::Store, resource: &str, amount: i64) {
    match resource {
        "energy" => store.energy = store.energy.saturating_sub(amount as u32),
        "G" => store.ghodium = store.ghodium.saturating_sub(amount as u32),
        _ => {}
    }
}

fn store_slot_add(store: &mut kk_schema::room::Store, resource: &str, amount: i64) {
    match resource {
        "energy" => store.energy = store.energy.saturating_add(amount as u32),
        "G" => store.ghodium = store.ghodium.saturating_add(amount as u32),
        _ => {}
    }
}

fn creep_slot(c: &kk_schema::room::CreepObj, resource: &str) -> i64 {
    kk_world::types::store_extra_slot(&c.store, &c.store_extra, resource)
}

/// 增加固定槽或命名资源槽，缺失的命名槽按需创建；非正增量不处理。
pub(in crate::settle) fn slots_add(
    store: &mut kk_schema::room::Store,
    extra: &mut Vec<kk_schema::room::ResourceStack>,
    resource: &str,
    amount: i64,
) {
    if amount <= 0 {
        return;
    }
    match resource {
        "energy" => store.energy = store.energy.saturating_add(amount as u32),
        kk_core::constants::RESOURCE_GHODIUM => {
            store.ghodium = store.ghodium.saturating_add(amount as u32)
        }
        other => {
            if let Some(slot) = extra.iter_mut().find(|s| s.resource == other) {
                slot.amount = slot
                    .amount
                    .saturating_add(u32::try_from(amount).unwrap_or(u32::MAX));
            } else {
                extra.push(kk_schema::room::ResourceStack {
                    resource: other.to_string(),
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
                });
            }
        }
    }
}

/// 减少固定槽或已有命名槽，不创建缺失槽；命名槽减为 0 后仍保留记录。
pub(in crate::settle) fn slots_sub(
    store: &mut kk_schema::room::Store,
    extra: &mut [kk_schema::room::ResourceStack],
    resource: &str,
    amount: i64,
) {
    if amount <= 0 {
        return;
    }
    match resource {
        "energy" => store.energy = store.energy.saturating_sub(amount as u32),
        kk_core::constants::RESOURCE_GHODIUM => {
            store.ghodium = store.ghodium.saturating_sub(amount as u32)
        }
        other => {
            if let Some(slot) = extra.iter_mut().find(|s| s.resource == other) {
                slot.amount = slot
                    .amount
                    .saturating_sub(u32::try_from(amount).unwrap_or(u32::MAX));
            }
        }
    }
}

/// 按存活 CARRY 和 capacity boost 计算共享容量。
pub fn store_capacity(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Carry,
        "carry",
        "capacity",
        c::CARRY_CAPACITY,
    )
}

/// 将有效身体代码转为种类，包含损毁件；费用与 CLAIM 寿命判断仍需要这些部件。
pub fn body_parts_kinds(body: &[BodyPart]) -> Vec<BodyKind> {
    body.iter()
        .filter_map(|p| BodyKind::from_code(p.kind))
        .collect()
}
