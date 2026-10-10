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

pub(super) struct GenericView {
    pub(super) owner: Option<String>,
    pub(super) structure_type: String,
    pub(super) data: Value,
}

pub(super) fn generic_view(w: &Working, id: ObjId) -> Option<GenericView> {
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
        // 官方 labs/intents.js：先强化/解除，再在 run 与 reverse 间择一。
        // 解除会设置冷却，必须先于反应处理，不能靠不同顺序的冷却门代替。
        "lab" => {
            if let Some(d) = actions.get(&Action::BoostCreep) {
                super::labs::boost_creep(&mut w, id, d);
            }
            if let Some(d) = actions.get(&Action::UnboostCreep) {
                super::labs::unboost_creep(&mut w, id, d);
            }
            if let Some(d) = actions.get(&Action::RunReaction) {
                super::labs::run_reaction(&mut w, id, d);
            } else if let Some(d) = actions.get(&Action::ReverseReaction) {
                super::labs::reverse_reaction(&mut w, id, d);
            }
        }
        // factory 单动作直派发（produce）；冷却门在处理器内。
        "factory" => {
            if let Some(d) = actions.get(&Action::Produce) {
                super::factories::produce(&mut w, id, d);
            }
        }
        // invaderCore 单动作直派发（reserveController）；布防/战术片的
        // create-creep/transfer/attack/upgrade 意图处理器接线后再开放。
        "invaderCore" => {
            if let Some(d) = actions.get(&Action::ReserveController) {
                super::invader_core::reserve_controller(&mut w, id, d);
            }
        }
        // nuker 单动作直派发（launchNuke 写标记，跨房插入在全局阶段）。
        "nuker" => {
            if let Some(d) = actions.get(&Action::LaunchNuke) {
                super::nukes::launch_nuke(&mut w, id, d);
            }
        }
        // observer 单动作直派发（observeRoom 写标记，下一拍投影授视野）。
        "observer" => {
            if let Some(d) = actions.get(&Action::ObserveRoom) {
                observer_observe_room(&mut w, id, d);
            }
        }
        // powerSpawn 单动作直派发（processPower 双扣并累计账号 power）。
        "powerSpawn" => {
            if let Some(_payload) = actions.get(&Action::ProcessPower) {
                super::powers::process_power(&mut w, id);
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
    // 跨房发送明确只认内建资源：Mod 声明的命名空间资源（键含冒号）
    // 在此显式拒绝，与未知资源同路径丢弃请求；正式跨房资源通路随
    // 后续批次开放（见架构文档 mod-resource-catalog 的当前边界）。
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
        crate::settle::market::calc_rooms_distance(&w.room.name, target_room, w.plane.world_span)
            .unwrap_or(i64::MAX);
    let mut cost = crate::settle::market::calc_terminal_energy_cost(amount, range);
    // 官方 terminal/send.js:22-25：意图相费用乘 OPERATE_TERMINAL 倍率并
    // ceil，边界为 endTime >= time（与全局 market 相的严格 > 不同，
    // 截止拍两相分叉按官方保留）。
    if let Some(effect) = w
        .room
        .effects
        .get(&id)
        .and_then(|entries| entries.iter().find(|e| e.power == c::PWR_OPERATE_TERMINAL))
        && effect.end_time >= w.time
        && let Some(multiplier) =
            crate::settle::powers::power_effect_value(c::PWR_OPERATE_TERMINAL, effect.level)
    {
        cost = ((cost as f64) * multiplier).ceil() as i64;
    }
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
        g.data = v.to_string().into();
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
fn tower_falloff_amount(w: &Working, id: ObjId, power: i64, range: i64) -> i64 {
    let mut amount = power as f64;
    if range > c::TOWER_OPTIMAL_RANGE {
        let r = range.min(c::TOWER_FALLOFF_RANGE);
        amount -= amount * c::TOWER_FALLOFF * (r - c::TOWER_OPTIMAL_RANGE) as f64
            / (c::TOWER_FALLOFF_RANGE - c::TOWER_OPTIMAL_RANGE) as f64;
    }
    for power in [c::PWR_OPERATE_TOWER, c::PWR_DISRUPT_TOWER] {
        if let Some(effect) =
            kk_world::effects::active_power(w.room.effects.get(&id), power, w.time)
            && let Some(multiplier) = super::powers::power_effect_value(power, effect.level)
        {
            amount *= multiplier;
        }
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
        g.data = v.to_string().into();
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
    let amount = tower_falloff_amount(w, id, c::TOWER_POWER_ATTACK, range);
    if amount == 0 {
        return;
    }
    apply_damage(w, id, target_id, amount, c::EVENT_ATTACK_TYPE_RANGED);
    consume_tower_energy(w, id);
}

/// 治疗已出生 creep 或 powerCreep（官方塔目标门 towers/heal.js:13 双类型），
/// 累加待治疗量，在目标自身 tick 处理；事件类型为远程治疗。
fn tower_heal(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(tower) = tower_state(w, id) else {
        return;
    };
    let Some(target_id) = d.id else {
        return;
    };
    let Some(target) = w.room.objects.get(&target_id).cloned() else {
        return;
    };
    let pos = match &target {
        RoomObject::Creep(t) if !t.spawning => (t.x, t.y),
        RoomObject::Generic(_) if super::powers::is_power_creep(&target) => target.pos(),
        _ => return,
    };
    if tower.energy < c::TOWER_ENERGY_COST {
        return;
    }
    let range = chebyshev((tower.x, tower.y), pos);
    let amount = tower_falloff_amount(w, id, c::TOWER_POWER_HEAL, range);
    if amount == 0 {
        return;
    }
    *w.heal_to_apply.entry(target_id).or_insert(0) += amount;
    consume_tower_energy(w, id);
    // 官方 towers/heal.js:48-49 双写：塔 heal 记目标坐标、目标 healed
    // 记塔坐标（towerAttack 的 attack/attacked 由 apply_damage 的
    // 非 creep 攻击者分支统一记录）。
    w.record_action(
        id,
        "heal",
        super::action_log::ActionLogPayload::Coords { x: pos.0, y: pos.1 },
    );
    w.record_action(
        target_id,
        "healed",
        super::action_log::ActionLogPayload::Coords {
            x: tower.x,
            y: tower.y,
        },
    );
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
    if !super::buildings::cost_bearing(w, &type_name) {
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
    let amount = tower_falloff_amount(w, id, c::TOWER_POWER_REPAIR, range);
    if amount == 0 {
        return;
    }
    let new_hits = (object_hits(&target) + amount).min(hits_max);
    set_structure_hits(w, target_id, new_hits);
    consume_tower_energy(w, id);
    {
        let (tx, ty) = target.pos();
        w.record_action(
            id,
            "repair",
            super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
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
            g.data = v.to_string().into();
        }
    } else {
        if let Some(RoomObject::Generic(t)) = w.room.objects.get_mut(&target_id)
            && let Ok(mut tv) = serde_json::from_str::<Value>(&t.data)
        {
            tv["store"]["energy"] = json!((target_total + amount - loss).max(0));
            t.data = tv.to_string().into();
        }
        if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
            && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
        {
            v["store"]["energy"] = json!((energy - amount).max(0));
            let cd = v.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
            v["cooldown"] = json!(cd + w.plane.links.cooldown * range);
            g.data = v.to_string().into();
        }
    }
    // 官方 links/transfer.js:48：transferEnergy 记目标 link 坐标。
    w.record_action(
        id,
        "transferEnergy",
        super::action_log::ActionLogPayload::Coords {
            x: target.x,
            y: target.y,
        },
    );
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
        g.data = v.to_string().into();
    }
}

pub(super) fn chebyshev(a: (u8, u8), b: (u8, u8)) -> i64 {
    (a.0 as i64 - b.0 as i64)
        .abs()
        .max((a.1 as i64 - b.1 as i64).abs())
}

/// observer 的 observeRoom：复验官方玩家面门（房名合法、RCL 激活、射程
/// 或活跃 PWR_OPERATE_OBSERVER 效果豁免）后写 data.observeRoom 标记；
/// 标记的清零在房间结算预扫描（官方 processor.js 预意图阶段同形）。
/// 官方结算侧本身无门（信任游戏层预检），kk 从严防直注意图注入。
/// 来源：engine/src/game/structures.js 的 observeRoom 与 processor.js:284-287。
fn observer_observe_room(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "observer" {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    let Some(target) = d.target_room_name.as_deref() else {
        return;
    };
    if !crate::settle::market::is_valid_room_name(target) {
        return;
    }
    // 射程门与玩家预检同形：活跃效果（严格 > 边界）完全豁免——官方无
    // 10+N 公式，效果激活即无限射程；否则逐轴绝对差 ≤ OBSERVER_RANGE。
    let effect_active =
        kk_world::effects::active_power(w.room.effects.get(&id), c::PWR_OPERATE_OBSERVER, w.time)
            .is_some();
    if !effect_active {
        let from = w.room.name.clone();
        let out = match (
            kk_core::RoomName::parse(&from),
            kk_core::RoomName::parse(target),
        ) {
            (Ok(a), Ok(b)) => {
                i64::from((a.xx - b.xx).abs()) > c::OBSERVER_RANGE
                    || i64::from((a.yy - b.yy).abs()) > c::OBSERVER_RANGE
            }
            _ => true,
        };
        if out {
            return;
        }
    }
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut data) = serde_json::from_str::<Value>(&g.data)
        && let Some(map) = data.as_object_mut()
    {
        // 与清零分支同款对象守卫：损坏快照注入的非对象 data 不 panic。
        map.insert("observeRoom".into(), json!(target));
        g.data = data.to_string().into();
    }
}

/// 读取结构在某 RCL 的配额：计划配额快照优先（含 link——原 link 专属
/// rcl 规则已并入配额族），快照无该型/级时回退 CONTROLLER_STRUCTURES
/// 官方表。等级越界（负数或 >8）快照返回 None 后由官方表同语义判 0。
pub(crate) fn allowed_for(w: &Working, structure_type: &str, level: i64) -> i64 {
    w.plane
        .quotas
        .get(structure_type, level)
        .unwrap_or_else(|| super::controller_allowed(structure_type, level))
}

/// 无主结构视为有效；有主须与 RCL>=1 的房主一致且有配额。
/// 按到控制器的距离占配额（覆盖后的 RCL8 上限不保证实际单件），
/// 等距时 ObjId 较小者优先。来源：上游 engine/src/utils.js::checkStructureAgainstController。
pub(super) fn check_structure_against_controller(
    w: &Working,
    id: ObjId,
    view: &GenericView,
) -> bool {
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
        let Some(identity) = g.data.identity() else {
            continue;
        };
        if identity.structure_type.as_deref() != Some(view.structure_type.as_str())
            || identity.owner.as_deref() != Some(owner)
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
