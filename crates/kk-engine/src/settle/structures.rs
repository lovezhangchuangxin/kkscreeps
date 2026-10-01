//! 结构对象动作族（M2B 扩面 P5）：tower 三动作 + link.transferEnergy +
//! rampart.setPublic——官方 `processor/intents/towers/{intents,attack,heal,
//! repair}.js`、`links/{intents,transfer}.js`、`ramparts/set-public.js` 逐字，
//! 分派序对照 processor.js:251-269（官方按 `object.type` 路由，我方 Generic
//! 透传桶按 `structureType` 键）。
//!
//! 形态决策（P4 镜头延续，compatibility.md 登记）：tower/link 不做 typed
//! 变体——结算写面 = Generic data JSON 字段级写回（link 冷却递减先例在
//! economy.rs）。actionLog（tower 的 attack/heal/repair 键与 link 的
//! transferEnergy 键）不写：官方是驱动内部面（渲染事件管线），玩家 payload
//! 剥离；我方 Generic 全透传投影会把它泄漏到玩家面——不写即玩家面等价。
//! 受击通知（attack.js:49-51 sendAttackingNotification）维持 P1 登记（发送
//! 面未接线）。

use std::collections::BTreeMap;

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::Working;
use super::damage::{
    apply_damage, object_hits, rampart_at, set_structure_hits, structure_hits_max,
};
use super::movement::type_name_of;
use crate::intent::{Action, IntentData};

/// Generic 对象的 data 解析快照（一次解析取 owner/structureType）。
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

/// Generic 结构的动作分派（官方 processor.js:248-269 的结构段）。
/// 所有权门 = 官方 `object.user && object.user != userId` 跳过——有主不符
/// 跳过、**无主不跳**（与 controller 臂同判）。
pub fn process_structure_intents(
    w: &mut Working,
    id: ObjId,
    player: &str,
    actions: &BTreeMap<Action, IntentData>,
) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.owner.as_deref().is_some_and(|owner| owner != player) {
        return;
    }
    match view.structure_type.as_str() {
        // towers/intents.js:3-8：else-if 单动作序——heal > repair > attack，
        // 每 tick 塔只执行一个动作（官方限速语义；无 TOWER_COOLDOWN 常量）
        "tower" => {
            if let Some(d) = actions.get(&Action::Heal) {
                tower_heal(w, id, d);
            } else if let Some(d) = actions.get(&Action::Repair) {
                tower_repair(w, id, d);
            } else if let Some(d) = actions.get(&Action::Attack) {
                tower_attack(w, id, d);
            }
        }
        // links/intents.js:3-6
        "link" => {
            if let Some(d) = actions.get(&Action::Transfer) {
                link_transfer(w, id, d);
            }
        }
        // processor.js:266-269（rampart 段只有 setPublic）
        "rampart" => {
            if let Some(d) = actions.get(&Action::SetPublic) {
                rampart_set_public(w, id, d);
            }
        }
        // processor.js:272-275（terminal 段只有 send——2B 批 P1）
        "terminal" => {
            if let Some(d) = actions.get(&Action::Send) {
                terminal_send(w, id, d);
            }
        }
        _ => {}
    }
}

/// terminal/send.js 全 36 行逐字（房间相——**只校验并落 `object.send` 字段**，
/// 跨房转移在全局阶段执行 global-intents/market.js:71-95；API 相的
/// my/RCL/description 门已在 prelude 过，官方房间相不复验）。energy 缺键
/// 按 0（TowerState 同款 DB 不变量口径——官方 `undefined < cost` 为假放行
/// 的 JS 奇行不复制）。
fn terminal_send(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "terminal" {
        return;
    }
    // :7-9 `/^(W|E)\d+(N|S)\d+$/`（None ≙ undefined 不匹配）
    let Some(target_room) = d.target_room_name.as_deref() else {
        return;
    };
    if !crate::settle::market::is_valid_room_name(target_room) {
        return;
    }
    // :10-12 RESOURCES_ALL 成员
    let Some(resource_type) = d.resource_type.as_deref() else {
        return;
    };
    if !c::RESOURCES_ALL.contains(&resource_type) {
        return;
    }
    // :13-15 `!intent.amount || !object.store || !(store[rt] >= amount)`
    //（缺键 → undefined >= amount 假 → 拒；None ≙ NaN falsy 同拒）
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
    // :17-19 距离/成本（环绕差异登记见 market::calc_rooms_distance）
    let range =
        crate::settle::market::calc_rooms_distance(&w.room.name, target_room).unwrap_or(i64::MAX);
    let cost = crate::settle::market::calc_terminal_energy_cost(amount, range);
    // :21-28 PWR_OPERATE_TERMINAL 折扣归 power 批（结构无 effects 数据，
    // 分支死码——登记）；:23-28 能量成本门
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
    // :30 `bulk.update(object, {send: intent})`——存**已清洗 intent**
    //（targetRoomName/resourceType/amount/description userString 帽后）
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

/// towers 三执行器的共同状态（attack.js:6-8 / heal.js:6-8 / repair.js:6-8 的
/// `!object || type != 'tower' || !object.store` 守卫）。
struct TowerState {
    x: u8,
    y: u8,
    /// store 必须存在（官方 `!object.store` 拒）；energy 缺键按 0（DB 不变量
    /// 下不可达——官方 `undefined < cost` 为假会放行的 JS 奇行不复制）
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

/// 官方 towers/{attack,heal,repair}.js 三处同式的距离衰减
/// （attack.js:32-38）：range = 本格坐标 Chebyshev；range > OPTIMAL(5) 时按
/// `(range−5)/(FALLOFF_RANGE(20)−5) × FALLOFF(0.75)` 扣减、range 钳 20；
/// floor。optimal 内全功率（600/400/800），≥20 格底 150/100/200。
fn tower_falloff_amount(power: i64, range: i64) -> i64 {
    let mut amount = power as f64;
    if range > c::TOWER_OPTIMAL_RANGE {
        let r = range.min(c::TOWER_FALLOFF_RANGE);
        amount -= amount * c::TOWER_FALLOFF * (r - c::TOWER_OPTIMAL_RANGE) as f64
            / (c::TOWER_FALLOFF_RANGE - c::TOWER_OPTIMAL_RANGE) as f64;
    }
    amount.floor() as i64
}

/// 塔扣能量（attack.js:45-46 等三处 `store.energy -= TOWER_ENERGY_COST` +
/// bulk 持久化——我方直接写回 data JSON）。
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

/// 官方 towers/attack.js：守卫链 → 同格 rampart 重定向 → 距离衰减 →
/// `_damage`（creep 目标延迟 / 结构目标立即+击毁废墟）→ 扣能量。
fn tower_attack(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    // attack.js:10-21：目标在、非自身、孵化中 creep 拒、有 hits、能量足
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
    // attack.js:33-36：目标同格 rampart 承伤（官方 _.find {type:'rampart',
    // x, y}——重定向后距离/后续结算全部对着 rampart）
    let target_id = rampart_at(w, target.pos()).unwrap_or(target_id);
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let range = chebyshev((tower.x, tower.y), target.pos());
    let amount = tower_falloff_amount(c::TOWER_POWER_ATTACK, range);
    // attack.js:41-43：floor 后 0 伤害收场（PWR_OPERATE/DISRUPT_TOWER 属
    // power 体系 M4，跳过）；事件/延迟序复用 P1 的 apply_damage
    if amount == 0 {
        return;
    }
    apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_RANGED);
    consume_tower_energy(w, id);
}

/// 官方 towers/heal.js：目标限 creep（powerCreep 属 M4）、spawning 拒 →
/// 距离衰减 → **延迟治疗**累加 heal_to_apply（官方 `_healToApply`，在该
/// creep 自己的 phase 8 tick 里先伤害后治疗——damage.rs::apply_pending_hits）
/// → 扣能量 → EVENT_HEAL（healType=RANGED）。
fn tower_heal(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    // heal.js:10-15：目标必须是已出生 creep
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
    // heal.js:27：`target._healToApply += amount`（同 tick phase 8 落地）
    *w.heal_to_apply.entry(target_id).or_insert(0) += amount;
    consume_tower_energy(w, id);
    // heal.js:34：EVENT_HEAL {targetId, amount, healType}
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

/// 官方 towers/repair.js：目标限可修结构（CONSTRUCTION_COST 门）+ 满血拒 →
/// 距离衰减 → hits 回补钳 hitsMax → 扣能量 → EVENT_REPAIR。
/// stats.inc('energyConstruction') 无统计管线，跳过登记。
fn tower_repair(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    // repair.js:10-11：`!C.CONSTRUCTION_COST[target.type]` 拒（非可修结构）
    // + `hits >= hitsMax` 拒（满血）
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
    // repair.js:30-33：回补钳满（floor 后 amount 恒 >0，到顶差量即实效）
    let new_hits = (object_hits(&target) + amount).min(hits_max);
    set_structure_hits(w, target_id, new_hits);
    consume_tower_energy(w, id);
    // repair.js:38-40：EVENT_REPAIR {targetId, amount, energySpent}
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

/// 官方 links/transfer.js 逐字（守卫序保持）。
fn link_transfer(w: &mut Working, id: ObjId, d: &IntentData) {
    // transfer.js:6-9：link + store 存在 + `energy < amount || amount < 0` 拒
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
    // transfer.js:11-18：目标在且是 link
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
    // transfer.js:22-24：源冷却中拒
    let cooldown = view
        .data
        .get("cooldown")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if cooldown > 0 {
        return;
    }
    // transfer.js:25-27：RCL 配额门（官方 utils.checkStructureAgainstController）
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    // transfer.js:29-31：目标无 per-resource 容量键或已满拒
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
    // transfer.js:33-34：超出部分截断到目标余量
    let amount = if target_total + amount > target_cap {
        target_cap - target_total
    } else {
        amount
    };
    // transfer.js:36-38：双侧记账 + 3% 损耗（ceil）+ 冷却 += LINK_COOLDOWN ×
    // 本格坐标 Chebyshev 距离（跨房 transfer 被 API 面 ERR_NOT_IN_RANGE 拒，
    // 本格坐标即全语义）。
    // **自指转移**（陈旧壳可达——API 门 `target === this` 是引用比较，跨 tick
    // 旧壳引用不等 id 相同全过，官方 transfer.js 无自指守卫照常执行）：官方
    // 三步序对**同一 JS 引用**顺序变更，净收敛 `E − ceil(amount×3%)`、
    // cooldown += 1×0。我方写回必须按同序对单一解析值连续变更——若对
    // target/source 各自独立「重解析+快照值覆写」，source 侧会把 target 侧
    // 的增量覆盖回 `E − amount`（评审 2 P1-1 的分歧形态）
    let loss = (amount as f64 * c::LINK_LOSS_RATIO).ceil() as i64;
    let range = chebyshev((src_x, src_y), (target.x, target.y));
    if target_id == id {
        // 官方三步序同引用：E += amount; E -= amount; E -= ceil(loss)
        if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
            && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
        {
            let e = v
                .pointer("/store/energy")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            v["store"]["energy"] = json!((e - loss).max(0));
            let cd = v.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
            v["cooldown"] = json!(cd + c::LINK_COOLDOWN * range);
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
            v["cooldown"] = json!(cd + c::LINK_COOLDOWN * range);
            g.data = v.to_string();
        }
    }
    // transfer.js:43-44：EVENT_TRANSFER {targetId, resourceType, amount}
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

/// ramparts/set-public.js：`isPublic: !!intent.isPublic` 写回（sanitizer 已
/// 保证 boolean；缺键 false 同官方 `!!undefined`）。
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

/// 官方 utils.checkStructureAgainstController（utils.js:456-505）全量移植
/// （link 的 RCL 门，transfer.js:25-27；harvest.rs::extractor_active 是
/// extractor 特化——quota[8]===1 短路使距离段不可达，link[8]=6 须走全段）：
/// 无主恒 active（:458-460）；有主须 controller level>=1 且同主（:463-465）；
/// 配额 CONTROLLER_STRUCTURES[type][level] 零拒（:467-471）；quota[8]===1 的
/// 单件结构直接放行（:474-476）；否则按「同类型同主、离 controller 更近者
/// 占配额」计数——等距并列时**先扫到的算更近**（:486-497，官方按 _.keys
/// 序即 DB 序，我方按 ObjId 升序——既有 D4-1 遍历序登记项）。
fn check_structure_against_controller(w: &Working, id: ObjId, view: &GenericView) -> bool {
    // :458-465 前奏（无主恒 active / controller 门）——与 harvest.rs 的
    // extractor 特化共用单源（utils.js:456-505 头三段）
    let Some(owner) = view.owner.as_deref() else {
        return true;
    };
    let Some(ctl) = super::structure_rcl_prologue(w, owner) else {
        return false;
    };
    let mut allowed_remaining = super::controller_allowed(&view.structure_type, ctl.level as i64);
    if allowed_remaining == 0 {
        return false;
    }
    // :474-476：quota[8]===1 的单件结构（terminal/storage 等）放行
    if super::controller_allowed(&view.structure_type, 8) == 1 {
        return true;
    }
    // :478-503：距离计数段（controller 坐标为基准）
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
            // 等距且先于本体扫到的同主同类型对象算更近；本体自身只置标记
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
