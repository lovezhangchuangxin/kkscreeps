//! creep 指令执行与 creep tick（官方 creeps/intents.js 执行序 + tick.js）。
//! B1 增补近战 attack / repair / dismantle 执行器（伤害机制见 damage.rs）；
//! B2 增补 controller 结算族（claim/reserve/attack/sign/generateSafeMode）。
//! B1 结构治理批：按 intent 家族拆 creep/ 子模块（纯搬移零行为），本模块持
//! 调度器（process_creep_intents）与跨家族共享的底仓工具（actionLog 位图与
//! 资源槽位原语）；对外 pub 面经 `pub use` 保持 crate::settle::creep::X 路径。

mod build;
mod combat;
mod controller;
mod harvest;
mod lifecycle;
mod movement;
mod store;

pub use harvest::harvest_power;
pub use lifecycle::{die_creep, die_creep_with_rate};
pub use movement::creep_tick;
pub(in crate::settle) use store::intent_drop_amount_res;

use std::collections::{BTreeMap, BTreeSet};

use kk_core::constants as c;
use kk_schema::room::BodyPart;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::settle::Working;
// 子模块函数体内的 `super::X` 限定路径原样保留（原 creep.rs 内 super=settle），
// 经此私有导入使 super::controller_allowed / super::spawn:: / super::
// energy_resource_name 在子模块解析回 settle 树
use crate::intent::{Action, EXECUTION_ORDER, IntentData};
use crate::settle::spawn;
use crate::settle::{controller_allowed, energy_resource_name};

use build::intent_build;
use combat::{intent_attack, intent_dismantle, intent_heal, intent_repair};
use controller::{
    intent_attack_controller, intent_claim_controller, intent_generate_safe_mode,
    intent_reserve_controller, intent_sign_controller, intent_upgrade_controller,
};
use harvest::intent_harvest;
use lifecycle::{intent_pull, intent_say, intent_suicide};
use movement::intent_move;
use store::{intent_drop, intent_pickup, intent_transfer, intent_withdraw};

/// 官方 creeps/intents.js：按 creepActions 序执行（压制过滤内建）。
pub fn process_creep_intents(w: &mut Working, id: ObjId, actions: &BTreeMap<Action, IntentData>) {
    let present: BTreeSet<Action> = actions.keys().copied().collect();
    for action in EXECUTION_ORDER {
        if !Action::executes(&present, action) {
            continue;
        }
        let data = &actions[&action];
        match action {
            Action::Move => intent_move(w, id, data),
            Action::Harvest => intent_harvest(w, id, data),
            Action::Build => intent_build(w, id, data),
            Action::UpgradeController => intent_upgrade_controller(w, id, data),
            Action::Transfer => intent_transfer(w, id, data),
            Action::Withdraw => intent_withdraw(w, id, data),
            Action::Drop => intent_drop(w, id, data),
            Action::Pickup => intent_pickup(w, id, data),
            Action::Attack => intent_attack(w, id, data),
            Action::Repair => intent_repair(w, id, data),
            Action::Dismantle => intent_dismantle(w, id, data),
            Action::Heal => intent_heal(w, id, data, false),
            Action::RangedHeal => intent_heal(w, id, data, true),
            // B2 controller 结算族（claimController/reserveController/
            // attackController/signController/generateSafeMode）
            Action::ClaimController => intent_claim_controller(w, id, data),
            Action::ReserveController => intent_reserve_controller(w, id, data),
            Action::AttackController => intent_attack_controller(w, id, data),
            Action::SignController => intent_sign_controller(w, id, data),
            Action::GenerateSafeMode => intent_generate_safe_mode(w, id, data),
            // A4-iv 移动杂项族（say/suicide/pull——官方 processor/intents/creeps/
            // 同名文件）
            Action::Say => intent_say(w, id, data),
            Action::Suicide => intent_suicide(w, id),
            Action::Pull => intent_pull(w, id, data),
            // M1 无执行器的动作（战斗/占位/杂项）：压制图已生效，此处静默跳过
            _ => {}
        }
    }
}

// ---- 公共小工具 ----

pub const ACTION_HARVEST: u8 = 1 << 0;

pub const ACTION_BUILD: u8 = 1 << 1;

pub const ACTION_UPGRADE: u8 = 1 << 2;

/// 官方 actionLog.attack：近战与 dismantle 共用此键（_damage.js:60-65）。
pub const ACTION_ATTACK: u8 = 1 << 3;

/// 官方 actionLog.attacked（_damage.js:73-75）。
pub const ACTION_ATTACKED: u8 = 1 << 4;

/// 官方 actionLog.repair（repair.js:53）。
pub const ACTION_REPAIR: u8 = 1 << 5;

/// 官方 actionLog.heal（heal.js:31）。rangedHeal.js:31 的官方独立键
/// `rangedHeal` 在 u8 位图形态下并入本位（位图已满 8 位；远近程的玩家可见
/// 区分由 EVENT_HEAL 的 healType 字段承载——schema 换正式 actionLog 形态时
/// 恢复官方双键）。
pub const ACTION_HEAL: u8 = 1 << 6;

/// 官方 actionLog.healed（heal.js:32；renew-creep.js:42 也置此键）。
pub const ACTION_HEALED: u8 = 1 << 7;

/// 官方 actionLog.reserveController（reserveController.js:47）。u8 位图在 B1
/// 已满 8 位，B2 起 action_log 拓宽为 u16，本键落在 bit8。
pub const ACTION_RESERVE: u16 = 1 << 8;

/// 官方 actionLog.say（say.js:18-21）。官方键是带 message(截 10)/isPublic 的
/// 对象——位图只落事件位，文本载荷不可承载（intent_say 注释的灰区口径）。
pub const ACTION_SAY: u16 = 1 << 9;

pub fn set_action_log(bits: &mut u16, bit: impl Into<u16>) {
    *bits |= bit.into();
}

/// 官方 storeCapacity（M1 无 boost）：CARRY 部件数 × CARRY_CAPACITY。
/// Store 两槽（energy/G）的资源键访问——官方 store 对象按资源名为键
/// （GPT 复审 P1-1 清偿：transfer/withdraw/drop 参数化，schema 现两资源槽）。
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

// ---- B4 多资源槽（creep/storage 的命名资源面：双槽 + store_extra 列表）----
// 官方 store 是按资源名键的开放对象（harvest.js:95/transfer.js:45 的
// `store[resourceType]` 任意键读写）；energy/G 走双槽字段，其余并入列表。

/// 读槽（无键 = 0——官方 store[res] 的 undefined 参与数值运算同值）。
fn creep_slot(c: &kk_schema::room::CreepObj, resource: &str) -> i64 {
    kk_world::types::store_extra_slot(&c.store, &c.store_extra, resource)
}

/// 写槽（加）：无键现场建（官方 `(store[res] || 0) + amount` 同式；饱和加
/// 是放置面可造超域堆的既有口径）。
fn slots_add(
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

/// 写槽（减）：饱和减（调用方已保证持有足额——官方直接减）。
fn slots_sub(
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

pub fn store_capacity(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Carry,
        "carry",
        "capacity",
        c::CARRY_CAPACITY,
    )
}

pub fn body_parts_kinds(body: &[BodyPart]) -> Vec<BodyKind> {
    body.iter()
        .filter_map(|p| BodyKind::from_code(p.kind))
        .collect()
}
