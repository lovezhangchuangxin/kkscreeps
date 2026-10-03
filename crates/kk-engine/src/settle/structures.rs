//! 结算 Generic 塔、link、rampart 和 terminal 动作。
//!
//! 不保存上游渲染用 actionLog，避免 Generic JSON 透传到玩家视图时泄露内部字段。
//! 尚未模拟 power 对塔和 terminal 的效果，也未发送攻击通知。

use std::collections::BTreeMap;

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::damage::{
    apply_damage, object_hits, rampart_at, set_structure_hits, structure_hits_max,
};
use super::movement::type_name_of;
use super::{RoomCtx, Working};
use crate::intent::{Action, IntentData};

struct GenericView {
    owner: Option<String>,
    structure_type: String,
    data: Value,
}

fn generic_view(w: &Working, id: ObjId) -> Option<GenericView> {
    let RoomObject::Generic(g) = w.room.objects.get(&id)? else {
        return None;
    };
    let data: Value = serde_json::from_str(&g.data).ok()?;
    Some(GenericView {
        owner: data
            .get("owner")
            .and_then(Value::as_str)
            .map(str::to_string),
        structure_type: data
            .get("structureType")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        data,
    })
}

/// 按 Generic.structureType 分派结构动作；归属存在且不匹配时拒绝，无主允许进入。
/// 塔每 tick 只执行一项，优先级为 heal、repair、attack。
pub fn process_structure_intents(
    ctx: &RoomCtx,
    id: ObjId,
    player: &str,
    actions: &BTreeMap<Action, IntentData>,
) -> Result<(), String> {
    let Some(view) = generic_view(&ctx.borrow(), id) else {
        return Ok(());
    };
    if view.owner.as_deref().is_some_and(|owner| owner != player) {
        return Ok(());
    }
    // link 走贡献链：必须先于任何借用执行，链内 wrap 的 next() 会重新
    // 借用同一候选。
    if view.structure_type == "link" {
        if let Some(d) = actions.get(&Action::Transfer) {
            let contrib = ctx.borrow().plane.contrib.clone();
            contrib.run_transfer(ctx, id, d)?;
        }
        return Ok(());
    }
    let mut w = ctx.borrow_mut();
    match view.structure_type.as_str() {
        "tower" => {
            if let Some(d) = actions.get(&Action::Heal) {
                tower_heal(&mut w, id, d);
            } else if let Some(d) = actions.get(&Action::Repair) {
                tower_repair(&mut w, id, d);
            } else if let Some(d) = actions.get(&Action::Attack) {
                tower_attack(&mut w, id, d);
            }
        }
        "rampart" => {
            if let Some(d) = actions.get(&Action::SetPublic) {
                rampart_set_public(&mut w, id, d);
            }
        }
        "terminal" => {
            if let Some(d) = actions.get(&Action::Send) {
                terminal_send(&mut w, id, d);
            }
        }
        _ => {}
    }
    Ok(())
}

/// 房间阶段验证目标、资源和能量费用后保存 send 请求，不在此处转移资源。
/// 全局 market 阶段会再次检查容量、费用和冷却，并消费该字段。
fn terminal_send(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "terminal" {
        return;
    }
    let Some(target_room) = d.target_room_name.as_deref() else {
        return;
    };
    if !crate::settle::market::is_valid_room_name(target_room) {
        return;
    }
    let Some(resource_type) = d.resource_type.as_deref() else {
        return;
    };
    if !c::RESOURCES_ALL.contains(&resource_type) {
        return;
    }
    let Some(amount) = d.amount else {
        return;
    };
    let store = view.data.get("store");
    let have = store
        .and_then(|s| s.get(resource_type))
        .and_then(Value::as_i64);
    if store.is_none() || !(have.unwrap_or(0) >= amount) || amount == 0 {
        return;
    }
    let range =
        crate::settle::market::calc_rooms_distance(&w.room.name, target_room).unwrap_or(i64::MAX);
    let cost = crate::settle::market::calc_terminal_energy_cost(amount, range);
    let energy = store
        .and_then(|s| s.get("energy"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let energy_ok = if resource_type == c::RESOURCE_ENERGY {
        energy >= amount + cost
    } else {
        energy >= cost
    };
    if !energy_ok {
        return;
    }
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        v["send"] = json!({
            "targetRoomName": target_room,
            "resourceType": resource_type,
            "amount": amount,
            "description": d.description,
        });
        g.data = v.to_string();
    }
}

struct TowerState {
    x: u8,
    y: u8,
    // Tower 必须有 store；缺失 energy 键按 0 处理。
    energy: i64,
}

fn tower_state(w: &Working, id: ObjId) -> Option<TowerState> {
    let RoomObject::Generic(g) = w.room.objects.get(&id)? else {
        return None;
    };
    let data: Value = serde_json::from_str(&g.data).ok()?;
    if data.get("structureType").and_then(Value::as_str) != Some("tower") {
        return None;
    }
    let store = data.get("store")?;
    Some(TowerState {
        x: g.x,
        y: g.y,
        energy: store.get("energy").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 以 Chebyshev 距离线性衰减：5 格内全功率，20 格及以上保留 25%，最后向下取整。
fn tower_falloff_amount(power: i64, range: i64) -> i64 {
    let mut amount = power as f64;
    if range > c::TOWER_OPTIMAL_RANGE {
        let r = range.min(c::TOWER_FALLOFF_RANGE);
        amount -= amount * c::TOWER_FALLOFF * (r - c::TOWER_OPTIMAL_RANGE) as f64
            / (c::TOWER_FALLOFF_RANGE - c::TOWER_OPTIMAL_RANGE) as f64;
    }
    amount.floor() as i64
}

fn consume_tower_energy(w: &mut Working, id: ObjId) {
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        let energy = v
            .pointer("/store/energy")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        v["store"]["energy"] = json!((energy - c::TOWER_ENERGY_COST).max(0));
        g.data = v.to_string();
    }
}

/// 目标格有 rampart 时重定向攻击，再计算距离功率，施加伤害后扣能量。
fn tower_attack(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    if target_id == id {
        return;
    }
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    if let RoomObject::Creep(t) = &target
        && t.spawning
    {
        return;
    }
    if object_hits(&target) <= 0 {
        return;
    }
    if tower.energy < c::TOWER_ENERGY_COST {
        return;
    }
    let target_id = rampart_at(w, target.pos()).unwrap_or(target_id);
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let range = chebyshev((tower.x, tower.y), target.pos());
    let amount = tower_falloff_amount(c::TOWER_POWER_ATTACK, range);
    if amount == 0 {
        return;
    }
    apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_RANGED);
    consume_tower_energy(w, id);
}

/// 治疗已出生 creep，累加待治疗量，在目标自身 tick 处理；事件类型为远程治疗。
fn tower_heal(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    let Some(RoomObject::Creep(t)) = w.room.objects.get(&target_id) else {
        return;
    };
    if t.spawning {
        return;
    }
    if tower.energy < c::TOWER_ENERGY_COST {
        return;
    }
    let range = chebyshev((tower.x, tower.y), (t.x, t.y));
    let amount = tower_falloff_amount(c::TOWER_POWER_HEAL, range);
    if amount == 0 {
        return;
    }
    *w.heal_to_apply.entry(target_id).or_insert(0) += amount;
    consume_tower_energy(w, id);
    w.push_event(
        c::EVENT_HEAL,
        id,
        json!({
            "targetId": target_id.as_str(),
            "amount": amount,
            "healType": c::EVENT_HEAL_TYPE_RANGED,
        }),
    );
}

/// 修复可建造结构，实际耐久钳到 hitsMax；事件 amount 保存距离计算的功率，
/// 即使距离满血只差少量耐久，事件也不改成实际恢复量。
fn tower_repair(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let type_name = type_name_of(&target);
    if !c::CONSTRUCTION_COST
        .iter()
        .any(|(k, _)| *k == type_name.as_ref())
    {
        return;
    }
    let Some(hits_max) = structure_hits_max(w, &target) else {
        return;
    };
    if object_hits(&target) >= hits_max {
        return;
    }
    if tower.energy < c::TOWER_ENERGY_COST {
        return;
    }
    let range = chebyshev((tower.x, tower.y), target.pos());
    let amount = tower_falloff_amount(c::TOWER_POWER_REPAIR, range);
    if amount == 0 {
        return;
    }
    let new_hits = (object_hits(&target) + amount).min(hits_max);
    set_structure_hits(w, target_id, new_hits);
    consume_tower_energy(w, id);
    w.push_event(
        c::EVENT_REPAIR,
        id,
        json!({
            "targetId": target_id.as_str(),
            "amount": amount,
            "energySpent": c::TOWER_ENERGY_COST,
        }),
    );
}

/// 按目标余量限制能量，发送方扣全部数量，接收方另扣 ceil(amount*loss_ratio)。
/// 冷却增加 cooldown 规则乘两者距离；事件 amount 是损耗前数量。
/// 数值取 Working.links 的有效规则；不要在此改读 kk-core 常量，
/// 否则规则覆盖无法到达结算。
pub(crate) fn link_transfer(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(RoomObject::Generic(src)) = w.room.objects.get(&id) else {
        return;
    };
    let (src_x, src_y) = (src.x, src.y);
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "link" {
        return;
    }
    let Some(amount) = d.amount else {
        return;
    };
    let store = view.data.get("store");
    let energy = store
        .and_then(|s| s.get("energy"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if store.is_none() || energy < amount || amount < 0 {
        return;
    }
    let Some(target_id) = d.id else {
        return;
    };
    let Some(target_raw) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let RoomObject::Generic(target) = &target_raw else {
        return;
    };
    let target_data: Value = serde_json::from_str(&target.data).unwrap_or_default();
    if target_data.get("structureType").and_then(Value::as_str) != Some("link") {
        return;
    }
    let cooldown = view
        .data
        .get("cooldown")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if cooldown > 0 {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    let target_cap = target_data
        .pointer("/storeCapacityResource/energy")
        .and_then(Value::as_i64);
    let target_total = target_data
        .pointer("/store/energy")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let Some(target_cap) = target_cap else {
        return;
    };
    if target_total == target_cap {
        return;
    }
    let amount = if target_total + amount > target_cap {
        target_cap - target_total
    } else {
        amount
    };
    // 同一 link 自转移时只损失手续费。必须在一个 JSON 对象上连续更新，
    // 不能分别写回源和目标的旧快照，否则会覆盖接收量。
    let loss = (amount as f64 * w.plane.links.loss_ratio).ceil() as i64;
    let range = chebyshev((src_x, src_y), (target.x, target.y));
    if target_id == id {
        if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
            && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
        {
            let e = v
                .pointer("/store/energy")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            v["store"]["energy"] = json!((e - loss).max(0));
            let cd = v.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
            v["cooldown"] = json!(cd + w.plane.links.cooldown * range);
            g.data = v.to_string();
        }
    } else {
        if let Some(RoomObject::Generic(t)) = w.room.objects.get_mut(&target_id)
            && let Ok(mut tv) = serde_json::from_str::<Value>(&t.data)
        {
            tv["store"]["energy"] = json!((target_total + amount - loss).max(0));
            t.data = tv.to_string();
        }
        if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
            && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
        {
            v["store"]["energy"] = json!((energy - amount).max(0));
            let cd = v.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
            v["cooldown"] = json!(cd + w.plane.links.cooldown * range);
            g.data = v.to_string();
        }
    }
    w.push_event(
        c::EVENT_TRANSFER,
        id,
        json!({
            "targetId": target_id.as_str(),
            "resourceType": "energy",
            "amount": amount,
        }),
    );
}

fn rampart_set_public(w: &mut Working, id: ObjId, d: &IntentData) {
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        v["isPublic"] = json!(d.is_public.unwrap_or(false));
        g.data = v.to_string();
    }
}

fn chebyshev(a: (u8, u8), b: (u8, u8)) -> i64 {
    (a.0 as i64 - b.0 as i64)
        .abs()
        .max((a.1 as i64 - b.1 as i64).abs())
}

/// 读取结构在某 RCL 的配额。link 走 Working.links 的有效规则，
/// 其余类型查 CONTROLLER_STRUCTURES 官方表。
pub(super) fn allowed_for(w: &Working, structure_type: &str, level: i64) -> i64 {
    if structure_type == "link" {
        // 等级越界（负数或 >8）判 0 配额，与 CONTROLLER_STRUCTURES
        // 查表缺键时的 unwrap_or(0) 同语义；不要钳到 rcl8。
        match usize::try_from(level) {
            Ok(idx) if idx < 9 => w.plane.links.rcl[idx],
            _ => 0,
        }
    } else {
        super::controller_allowed(structure_type, level)
    }
}

/// 无主结构视为有效；有主须与 RCL>=1 的房主一致且有配额。
/// 单件类型通过等级检查即可，多件类型按到控制器的距离占配额，
/// 等距时 ObjId 较小者优先。来源：上游 engine/src/utils.js::checkStructureAgainstController。
fn check_structure_against_controller(w: &Working, id: ObjId, view: &GenericView) -> bool {
    let Some(owner) = view.owner.as_deref() else {
        return true;
    };
    let Some(ctl) = super::structure_rcl_prologue(w, owner) else {
        return false;
    };
    let mut allowed_remaining = allowed_for(w, &view.structure_type, ctl.level as i64);
    if allowed_remaining == 0 {
        return false;
    }
    if allowed_for(w, &view.structure_type, 8) == 1 {
        return true;
    }
    let obj = match w.room.objects.get(&id) {
        Some(RoomObject::Generic(g)) => (g.x, g.y),
        _ => return true,
    };
    let object_dist = chebyshev(obj, (ctl.x, ctl.y));
    let mut found_self = false;
    for o in w.room.objects.values() {
        let RoomObject::Generic(g) = o else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&g.data) else {
            continue;
        };
        if v.get("structureType").and_then(Value::as_str) != Some(view.structure_type.as_str())
            || v.get("owner").and_then(Value::as_str) != Some(owner)
        {
            continue;
        }
        let compare_dist = chebyshev((g.x, g.y), (ctl.x, ctl.y));
        if compare_dist < object_dist {
            allowed_remaining -= 1;
            if allowed_remaining == 0 {
                return false;
            }
        } else if !found_self && compare_dist == object_dist {
            if g.id == id.0 {
                found_self = true;
            } else {
                allowed_remaining -= 1;
                if allowed_remaining == 0 {
                    return false;
                }
            }
        }
    }
    true
}
