//! 采集族：harvest 对 source/mineral 两态（官方 harvest.js；矿机 extractor
//! active 门与采集功率）。B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::intent::IntentData;
use crate::settle::Working;

use super::store::{intent_drop_amount, intent_drop_amount_res};
use super::{ACTION_HARVEST, set_action_log, slots_add, store_capacity};

/// 官方 harvest.js：目标类型分派（source :25-65 / mineral :67-112——B4 补；
/// deposit :114-139 随对应能力批）。公共预检（:10-23）：spawning 拒、目标
/// 存在、Chebyshev 邻接（先于类型分支）。
pub(super) fn intent_harvest(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(target_id) = data.id else { return };
    if w.creep(id).is_none_or(|c| c.spawning) {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let creep_pos = w.creep(id).map(|c| (c.x, c.y)).unwrap_or((0, 0));
    let (tx, ty) = target.pos();
    if (tx as i32 - creep_pos.0 as i32).abs() > 1 || (ty as i32 - creep_pos.1 as i32).abs() > 1 {
        return;
    }
    match target {
        RoomObject::Source(_) => intent_harvest_source(w, id, target_id),
        RoomObject::Mineral(_) => intent_harvest_mineral(w, id, target_id),
        _ => {}
    }
}

/// 官方 harvest.js source 分支（25-65）：controller 归属/预定门（B2 补
/// reservation 分支）、WORK×HARVEST_POWER、超容 drop energy。
fn intent_harvest_source(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, work_power, held_energy, held_total, store_cap) = match w.creep(id) {
        Some(c) => (
            c.owner.clone(),
            harvest_power(c),
            // energy 槽局部持有（写槽用——官方 :43 槽局部入仓）
            c.store.energy as i64,
            // 全槽和（官方 calcResources 对 store 全键求和——混合持有时
            // energy/G/命名矿物都计入超容判定，B4 评审 3 P1-1）
            kk_world::types::store_total(&c.store, &c.store_extra),
            store_capacity(c),
        ),
        None => return,
    };
    // 官方 harvest.js:31-34：controller 归他人**或被他人预定**拒——
    // `roomController.user && user != object.user ||
    //  roomController.reservation && reservation.user != object.user`
    if let Some(ctl) = w.controller()
        && (ctl.owner.as_deref().is_some_and(|u| u != owner)
            || ctl.reservation.as_ref().is_some_and(|r| r.user != owner))
    {
        return;
    }
    // 官方 harvest.js:37 `if (harvestAmount)`：零功率（无 WORK）整体跳过——
    // 不扣减、无 actionLog、无事件
    if work_power <= 0 {
        return;
    }
    let energy = match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Source(s)) if s.energy > 0 => s.energy.min(work_power as u32),
        _ => return,
    };
    let amount = energy as i64;
    if let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&target_id) {
        s.energy -= amount as u32;
    }
    // 官方顺序（harvest.js:43-60）：energy **槽局部**入仓（:43
    // store.energy = (store.energy||0)+amount），超容判定用 calcResources
    // **全槽和**（:50），掉落量 = min(energy 槽持有, sum−cap)（:54）——只掉
    // 本 tick 产出的 energy，不动其他槽（P1-1 清偿 + 复审 F1 修正：原清偿
    // 误把全槽和写进 energy 槽 = 混持采 source 复制资源）
    let sum = held_total + amount;
    let energy_after = held_energy + amount;
    if let Some(c) = w.creep_mut(id) {
        c.store.energy = energy_after.min(u32::MAX as i64) as u32;
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if sum > store_cap {
        let drop_amt = energy_after.min(sum - store_cap);
        if drop_amt > 0 {
            intent_drop_amount(w, id, drop_amt);
        }
    }
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": amount }),
    );
}

/// 官方 harvest.js mineral 分支（67-112，B4）：矿量门（:69-71）、矿物同格
/// extractor 存在性（:73-77）、extractor 敌主拒/无主放行（:78-80）、
/// checkStructureAgainstController（:81-83——RCL 门，extractor 需 6 级）、
/// cooldown 拒（:84-86）、WORK×HARVEST_MINERAL_POWER（:88）、产出入矿种
/// 命名槽（:95）、超容按矿种 drop（:97-104）、extractor._cooldown=5（:108）。
/// 注意官方 mineral 分支**无** source 分支的 controller 归属/预定门（:31-34）。
fn intent_harvest_mineral(w: &mut Working, id: ObjId, target_id: ObjId) {
    let (owner, body, store_cap) = match w.creep(id) {
        Some(c) => (c.owner.clone(), c.body.clone(), store_capacity(c)),
        None => return,
    };
    let Some(RoomObject::Mineral(m)) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    // :69-71：矿量为 0 整条丢弃
    if m.mineral_amount == 0 {
        return;
    }
    // :73-77：矿物同格的 extractor（_.find 按坐标——index.at 同语义）
    let extractor_id = w
        .index
        .at(m.x as usize, m.y as usize)
        .iter()
        .copied()
        .find(|eid| matches!(w.room.objects.get(eid), Some(RoomObject::Extractor(_))));
    let Some(extractor_id) = extractor_id else {
        return;
    };
    let (ex_owner, ex_cooldown) = match w.room.objects.get(&extractor_id) {
        Some(RoomObject::Extractor(e)) => (e.owner.clone(), e.cooldown),
        _ => return,
    };
    // :78-80：敌主 extractor 拒（无主/自家放行）
    if ex_owner.as_deref().is_some_and(|u| u != owner) {
        return;
    }
    // :81-83：RCL 门（官方 utils.js:456-495 的 extractor 面——无主恒 active；
    // 有主须 controller level>=1 同主且配额非零；extractor 的 quota[8]===1
    // 走"唯一配额即本尊"分支，距离计数分支对该类型不可达 ⇒ RCL>=6 才 active）
    if !extractor_active(w, &ex_owner) {
        return;
    }
    // :84-86：冷却中拒
    if ex_cooldown != 0 {
        return;
    }
    // :88：HARVEST_MINERAL_POWER（1/WORK——boost 的 harvest 效果同一 BOOSTS 表）
    let work_power = kk_world::types::calc_body_effectiveness(
        &body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_MINERAL_POWER,
    );
    // :90 `if (harvestAmount)`：零功率整体跳过（不扣减、无 actionLog/事件）
    if work_power <= 0 {
        return;
    }
    let amount = (m.mineral_amount as i64).min(work_power);
    // :94：扣矿物本体
    if let Some(RoomObject::Mineral(t)) = w.room.objects.get_mut(&target_id) {
        t.mineral_amount -= amount as u32;
    }
    // :95：产出入矿种命名槽（bulk.update 的 _.merge 先改内存对象——:97 的
    // calcResources 读到含新产出的全槽和）；:97-104 超容按矿种 drop
    let held_total = match w.creep(id) {
        Some(c) => kk_world::types::store_total(&c.store, &c.store_extra),
        None => return,
    };
    // sum 全槽 + 新产出（官方 calcResources 含 _.merge 后的全部键——本矿种
    // 旧持有已在 store_total 内；混合持有超容按矿种 drop，P1-1 清偿）
    let sum = held_total + amount;
    if let Some(c) = w.creep_mut(id) {
        slots_add(&mut c.store, &mut c.store_extra, &m.mineral_type, amount);
        set_action_log(&mut c.action_log, ACTION_HARVEST);
    }
    if sum > store_cap {
        intent_drop_amount_res(w, id, &m.mineral_type, sum - store_cap);
    }
    // :108：extractor._cooldown 瞬态（同 tick 的 extractor tick 持久化）
    w.extractor_cooldown_pending
        .insert(extractor_id, c::EXTRACTOR_COOLDOWN as u64);
    // :110：官方事件 amount 是**未钳制**的 harvestAmount（source 分支 :63 是
    // 钳后 amount——两分支的官方 quirk，原样保留）
    w.push_event(
        c::EVENT_HARVEST,
        id,
        serde_json::json!({ "targetId": target_id.as_str(), "amount": work_power }),
    );
}

/// 官方 checkStructureAgainstController（utils.js:456-495）的 extractor 面
/// （harvest.js:81 的门）：无主恒 active（:457-460）；有主须 controller
/// level>=1 且同主（:463-465）且 CONTROLLER_STRUCTURES.extractor[level]
/// 配额非零（:467-471——RCL<6 为 0）。:474-476 的 quota[8]===1 提前返回
/// 分支使距离计数段（:478-495）对 extractor 不可达。
fn extractor_active(w: &Working, ex_owner: &Option<String>) -> bool {
    // :458-465 前奏与 towers/links 全量版共用单源（M2B 扩面 P5 抽取）
    let Some(owner) = ex_owner.as_deref() else {
        return true;
    };
    let Some(ctl) = crate::settle::structure_rcl_prologue(w, owner) else {
        return false;
    };
    super::controller_allowed("extractor", ctl.level as i64) > 0
}

/// 官方 harvest 功率：WORK 部件数 × HARVEST_POWER（无 boost）。
pub fn harvest_power(creep: &kk_schema::room::CreepObj) -> i64 {
    kk_world::types::calc_body_effectiveness(
        &creep.body,
        BodyKind::Work,
        "work",
        "harvest",
        c::HARVEST_POWER,
    )
}
