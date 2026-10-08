//! power 域结算：powerCreep 的房间相位意图与对象 tick、powerSpawn 的
//! processPower，以及账号域全局阶段（见 [`account`]）。
//!
//! 移植官方 engine/src/processor/intents/power-creeps/*.js、
//! power-spawns/process-power.js 与 processor/global-intents/power.js。
//! powerCreep 以 Generic 承载（data.type=="powerCreep"），账号注册表在
//! `PlayerState.power_creeps`，两侧同 ID。usePower 已结算 PWR_GENERATE_OPS、
//! PWR_OPERATE_TOWER 与 PWR_DISRUPT_TOWER；塔效果写入房间公共效果表，
//! 由塔的攻击、治疗和修理结算消费。其余 16 个 power 通过官方预检门后
//! 仍不结算，施放与效果消费者待后续补齐；capability powerEffects 已开启。

mod account;
mod resources;

pub use account::{gpl_level, settle_power};

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::SettleError;
use super::Working;
use super::structures::{GenericView, check_structure_against_controller, generic_view};
use crate::intent::{Action, IntentData};
use crate::settle::movement;

/// 解析 powerCreep Generic 的 data JSON；非 Generic 或 JSON 非法返回 None。
pub(crate) fn pc_data(obj: &RoomObject) -> Option<Value> {
    let RoomObject::Generic(g) = obj else {
        return None;
    };
    serde_json::from_str(&g.data).ok()
}

/// data.type 是否为 powerCreep。
pub(crate) fn is_power_creep(obj: &RoomObject) -> bool {
    pc_data(obj)
        .and_then(|v| v.get("type").and_then(Value::as_str).map(str::to_string))
        .as_deref()
        == Some("powerCreep")
}

/// powerCreep 的 owner（data.owner）。
pub(crate) fn pc_owner(v: &Value) -> Option<&str> {
    v.get("owner").and_then(Value::as_str)
}

/// POWER_INFO 常量 JSON 的进程级解析；power 不在其中时返回 None。
/// 函数内 static 的 OnceLock 引用是 'static，无锁无克隆。
fn power_info(power: i64) -> Option<&'static Value> {
    use std::sync::OnceLock;
    static INFO: OnceLock<serde_json::Map<String, Value>> = OnceLock::new();
    let table = INFO.get_or_init(|| {
        serde_json::from_str::<Value>(c::POWER_INFO)
            .unwrap_or_default()
            .as_object()
            .cloned()
            .unwrap_or_default()
    });
    table.get(&power.to_string())
}

/// POWER_INFO[power].effect[level-1]；effect 缺失或越界返回 None。
pub(crate) fn power_effect(power: i64, level: i64) -> Option<i64> {
    power_effect_value(power, level).map(|f| f as i64)
}

/// 效果乘数保留小数，tower 在衰减与两种 power 相乘后统一取整。
pub(crate) fn power_effect_value(power: i64, level: i64) -> Option<f64> {
    power_info(power)?
        .get("effect")?
        .as_array()?
        .get((level - 1).max(0) as usize)?
        .as_f64()
}

/// 房间相位意图入口：所有权门后按官方动作序分派。
/// 来源：processor/intents/power-creeps/intents.js 的 creepActions。
pub fn process_power_creep_intents(
    w: &mut Working,
    id: ObjId,
    player: &str,
    actions: &BTreeMap<Action, IntentData>,
) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.data.get("type").and_then(Value::as_str) != Some("powerCreep") {
        return;
    }
    if pc_owner(&view.data).is_some_and(|owner| owner != player) {
        return;
    }
    // 官方动作序：move 先行，资源动作在 usePower 之后；同名覆盖下每动作
    // 至多一条。say 不收费也不在压制表，与 creep 同形。
    if let Some(d) = actions.get(&Action::Move) {
        pc_move(w, id, d);
    }
    if let Some(d) = actions.get(&Action::UsePower) {
        pc_use_power(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Withdraw) {
        resources::pc_withdraw(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Transfer) {
        resources::pc_transfer(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Say) {
        pc_say(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Drop) {
        resources::pc_drop(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Pickup) {
        resources::pc_pickup(w, id, d);
    }
    if let Some(d) = actions.get(&Action::EnableRoom) {
        pc_enable_room(w, id, d);
    }
    if let Some(d) = actions.get(&Action::Renew) {
        pc_renew(w, id, d);
    }
}

/// 登记 move 请求；方向缺失时按相邻对象 ID 求方向。powerCreep 无疲劳，
/// 不写 old_fatigue。静态障碍预检与 creep 同门。
/// 来源：processor/intents/power-creeps/move.js（委托 creeps/move.js）。
fn pc_move(w: &mut Working, id: ObjId, data: &IntentData) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let (x, y) = (g.x as i32, g.y as i32);
    let owner = pc_data(&RoomObject::Generic(g.clone()))
        .and_then(|v| pc_owner(&v).map(str::to_string))
        .unwrap_or_default();
    let direction = match data.direction {
        Some(d) => d,
        None => {
            let Some(target_id) = data.id else {
                return;
            };
            let Some(target) = w.room.objects.get(&target_id) else {
                return;
            };
            let (tx, ty) = (target.pos().0 as i32, target.pos().1 as i32);
            if (tx - x).abs() > 1 || (ty - y).abs() > 1 {
                return; // 官方 API 层已挡 isNearTo；结算复核
            }
            movement::direction_from_delta(tx - x, ty - y)
        }
    };
    let Some((dx, dy)) = movement::offsets_by_direction(direction) else {
        return;
    };
    if x + dx < 0 || x + dx > 49 || y + dy < 0 || y + dy > 49 {
        return;
    }
    let target = ((x + dx) as u8, (y + dy) as u8);
    let blocked = movement::static_obstacles::registration_blocked(
        &w.room,
        &w.index,
        &w.plane.buildings,
        target,
        &owner,
    );
    if blocked && !w.plane.contrib.defers_static_precheck() {
        return;
    }
    w.movement.add(id, (x as u8, y as u8), dx, dy);
    w.movement.mark_registration_blocked(id, blocked);
}

/// powerCreep 动作记录写入 data.actionLog（官方 processor.js:151-157 每
/// tick 开头重置六键；房间开始时 room_loop 整体移除键等价重置，此处只管
/// 写入；IndexMut 对缺失的中间键自动建对象）。
pub(in crate::settle) fn set_pc_action_log(w: &mut Working, id: ObjId, key: &str, value: Value) {
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
    {
        v["actionLog"][key] = value;
        o.data = v.to_string();
    }
}

/// usePower：官方门序（房主 powerEnabled → power 等级/冷却 → ops → 射程
/// /效果覆盖）后结算 generateOps 和 tower 效果；其余 power 尚待消费者接线。
/// 来源：processor/intents/power-creeps/usePower.js。
fn pc_use_power(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id).cloned() else {
        return;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&g.data) else {
        return;
    };
    let Some(owner) = pc_owner(&v).map(str::to_string) else {
        return;
    };
    // 房主安全模式门与 powerEnabled 门（无控制器房间不设门）。
    if let Some(ctl) = w.controller()
        && (!ctl.is_power_enabled
            || (ctl.owner.as_deref() != Some(owner.as_str()) && ctl.safe_mode > w.time))
    {
        return;
    }
    let Some(power) = d.power else { return };
    let Some(info) = power_info(power) else {
        return;
    };
    let Some(powers) = v.get("powers").and_then(Value::as_object) else {
        return;
    };
    let Some(entry) = powers.get(&power.to_string()) else {
        return;
    };
    let level = entry.get("level").and_then(Value::as_i64).unwrap_or(0);
    if !(1..=5).contains(&level) {
        return;
    }
    if entry
        .get("cooldownTime")
        .and_then(Value::as_u64)
        .is_some_and(|t| t > w.time)
    {
        return;
    }
    // ops 费用：标量为常量、数组按等级取值（官方 powerInfo.ops 形态）。
    let ops = match info.get("ops") {
        Some(Value::Array(a)) => a
            .get((level - 1) as usize)
            .and_then(Value::as_i64)
            .unwrap_or(0),
        Some(n) => n.as_i64().unwrap_or(0),
        None => 0,
    };
    let held_ops = v.pointer("/store/ops").and_then(Value::as_i64).unwrap_or(0);
    if held_ops < ops {
        return;
    }
    // 射程门：POWER_INFO 带 range 时目标必须存在且在射程内；同 power 的
    // 更高级未过期效果已存在时拒绝（ERR_FULL 侧，结算静默同效）。
    if let Some(range) = info.get("range").and_then(Value::as_i64) {
        let Some(target_id) = d.id else {
            return;
        };
        let Some(target) = w.room.objects.get(&target_id) else {
            return;
        };
        let tp = target.pos();
        if ((tp.0 as i32 - g.x as i32).abs() as i64) > range
            || ((tp.1 as i32 - g.y as i32).abs() as i64) > range
        {
            return;
        }
        if kk_world::effects::active_power(w.room.effects.get(&target_id), power, w.time)
            .is_some_and(|effect| effect.level > level)
        {
            return;
        }
    }
    let overflow = match power {
        c::PWR_GENERATE_OPS => {
            let gained = power_effect(power, level).unwrap_or(0);
            let new_ops = held_ops - ops + gained;
            let capacity = v.get("storeCapacity").and_then(Value::as_i64).unwrap_or(0);
            let other_total: i64 = v
                .get("store")
                .and_then(Value::as_object)
                .map(|s| {
                    s.iter()
                        .filter(|(k, _)| k.as_str() != "ops")
                        .map(|(_, n)| n.as_i64().unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            let overflow = (other_total + new_ops - capacity).max(0).min(new_ops);
            v["store"]["ops"] = json!(new_ops - overflow);
            overflow
        }
        c::PWR_OPERATE_TOWER | c::PWR_DISRUPT_TOWER => {
            let Some(target_id) = d.id else { return };
            if !w
                .room
                .objects
                .get(&target_id)
                .and_then(pc_data)
                .is_some_and(|target| target["structureType"] == "tower")
            {
                return;
            }
            let duration = match info.get("duration") {
                Some(Value::Array(a)) => a.get((level - 1) as usize).and_then(Value::as_u64),
                Some(n) => n.as_u64(),
                None => None,
            };
            let Some(duration) = duration else { return };
            let effects = w.room.effects.entry(target_id).or_default();
            effects.retain(|effect| effect.power != power);
            effects.push(kk_world::EffectEntry {
                effect: power,
                power,
                level,
                end_time: w.time + duration,
            });
            v["store"]["ops"] = json!(held_ops - ops);
            0
        }
        _ => return,
    };
    let cooldown = info.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
    let power_key = power.to_string();
    v["powers"][&power_key] = json!({ "level": level, "cooldownTime": w.time + cooldown as u64 });
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id) {
        o.data = v.to_string();
    }
    w.push_event(
        c::EVENT_POWER,
        id,
        json!({ "power": power, "targetId": d.id.map(|t| t.as_str().to_string()) }),
    );
    if overflow > 0 {
        w.create_resource(g.x, g.y, overflow, c::RESOURCE_OPS);
    }
    // 官方 usePower.js:305-309：结算成功统一记动作（目标存在取目标坐标，
    // 否则自身坐标）。
    let (ax, ay) =
        d.id.and_then(|t| w.room.objects.get(&t))
            .map(|t| t.pos())
            .unwrap_or((g.x, g.y));
    set_pc_action_log(w, id, "power", json!({ "id": power, "x": ax, "y": ay }));
}

/// enableRoom：控制器目标、射程 1、敌方安全模式拒绝后置 isPowerEnabled。
/// 来源：processor/intents/power-creeps/enableRoom.js。
fn pc_enable_room(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(target_id) = d.id else { return };
    let (ctl_id, ctl) = match w.room.objects.get(&target_id) {
        Some(obj @ RoomObject::Controller(_)) => (target_id, obj.clone()),
        _ => return,
    };
    let RoomObject::Controller(ctl_obj) = &ctl else {
        return;
    };
    let _ = ctl_id;
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let owner = pc_data(&RoomObject::Generic(g.clone()))
        .and_then(|v| pc_owner(&v).map(str::to_string))
        .unwrap_or_default();
    if ctl_obj.owner.as_deref() != Some(owner.as_str()) && ctl_obj.safe_mode > w.time {
        return;
    }
    if ((ctl_obj.x as i32 - g.x as i32).abs() > 1) || ((ctl_obj.y as i32 - g.y as i32).abs() > 1) {
        return;
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.is_power_enabled = true;
    }
    // 官方 enableRoom.js:21：成功后记动作（目标=控制器坐标）。
    set_pc_action_log(w, id, "attack", json!({ "x": ctl_obj.x, "y": ctl_obj.y }));
}

/// renew：powerBank/powerSpawn 相邻目标重置寿命为 time+5000。
/// powerSpawn 另过 RCL 门。来源：processor/intents/power-creeps/renew.js。
fn pc_renew(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(target_id) = d.id else { return };
    let Some(RoomObject::Generic(target)) = w.room.objects.get(&target_id) else {
        return;
    };
    let Ok(tv) = serde_json::from_str::<Value>(&target.data) else {
        return;
    };
    let structure_type = tv.get("structureType").and_then(Value::as_str);
    if !matches!(structure_type, Some("powerBank") | Some("powerSpawn")) {
        return;
    }
    if structure_type == Some("powerSpawn") {
        let view = GenericView {
            owner: tv.get("owner").and_then(Value::as_str).map(str::to_string),
            structure_type: "powerSpawn".into(),
            data: tv.clone(),
        };
        if !check_structure_against_controller(w, target_id, &view) {
            return;
        }
    }
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    if ((target.x as i32 - g.x as i32).abs() > 1) || ((target.y as i32 - g.y as i32).abs() > 1) {
        return;
    }
    let (gx, gy) = (g.x, g.y);
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
    {
        v["ageTime"] = json!(w.time + c::POWER_CREEP_LIFE_TIME as u64);
        // 官方 renew.js:22：续命成功记 healed（自身坐标）。
        v["actionLog"]["healed"] = json!({ "x": gx, "y": gy });
        o.data = v.to_string();
    }
}

/// say：写 data.actionLog.say（截 10 字符）；投影透传给 prelude 的
/// saying getter。来源：processor/intents/power-creeps/say.js。
fn pc_say(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(message) = &d.message else {
        return;
    };
    let message: String = message.chars().take(10).collect();
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
    {
        v["actionLog"]["say"] =
            json!({ "message": message, "isPublic": d.is_public.unwrap_or(false) });
        o.data = v.to_string();
    }
}

/// powerSpawn.processPower：RCL 门 + 双扣（power 量 1、energy 量 50）后
/// 经 TickState 累计账号 power。PWR_OPERATE_POWER 增产依赖 effects 面，
/// 未实现（M2H.b）恒为量 1。来源：processor/intents/power-spawns/process-power.js。
pub fn process_power(w: &mut Working, id: ObjId) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "powerSpawn" {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    let amount = 1_i64;
    let power = view
        .data
        .pointer("/store/power")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let energy = view
        .data
        .pointer("/store/energy")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if power < amount || energy < amount * c::POWER_SPAWN_ENERGY_RATIO {
        return;
    }
    let owner = view.owner.clone().unwrap_or_default();
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
    {
        v["store"]["power"] = json!(power - amount);
        v["store"]["energy"] = json!(energy - amount * c::POWER_SPAWN_ENERGY_RATIO);
        o.data = v.to_string();
    }
    w.state.add_power(&owner, amount);
}

/// powerCreep 逐 tick：执行获准移动（无疲劳、公路磨损固定 100、踩敌方
/// 工地）、边缘跨房标记、待伤害应用并钳上限。死亡不在房间相位判定，
/// 全局阶段扫描 ageTime/hits（官方 global-intents/power.js 尾段）。
/// 来源：processor/intents/power-creeps/tick.js。
pub fn power_creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    pc_movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(());
    }
    let (x, y, hits_max) = match w.room.objects.get(&id) {
        Some(RoomObject::Generic(g)) => {
            let Ok(v) = serde_json::from_str::<Value>(&g.data) else {
                return Ok(());
            };
            (
                g.x,
                g.y,
                v.get("hitsMax").and_then(Value::as_i64).unwrap_or(0),
            )
        }
        _ => return Ok(()),
    };
    // 边缘跨房过渡：方向优先级与 creep tick 同源（inter_room::edge_target），
    // 事件先于迁移应用发出；对象随后死亡时不迁移（应用阶段按缺席跳过）。
    if let Some((to_room, nx, ny)) = crate::settle::inter_room::edge_target(&w.room.name, x, y) {
        w.push_event(
            c::EVENT_EXIT,
            id,
            json!({ "room": to_room, "x": nx, "y": ny }),
        );
        w.pending_inter_room
            .push(crate::settle::inter_room::InterRoomMove {
                id,
                to_room,
                x: nx,
                y: ny,
            });
    }
    // 待伤害/治疗应用（官方 tick.js:53-63：先扣伤害再叠治疗，随后统一钳
    // hitsMax）；hits<=0 留给全局死亡扫。
    let damage = w.damage_to_apply.remove(&id).unwrap_or(0);
    let heal = w.heal_to_apply.remove(&id).unwrap_or(0);
    if damage != 0 || heal != 0 {
        let current = w
            .room
            .objects
            .get(&id)
            .and_then(pc_data)
            .and_then(|v| v.get("hits").and_then(Value::as_i64))
            .unwrap_or(0);
        let next = (current - damage + heal).clamp(0, hits_max);
        if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id)
            && let Ok(mut v) = serde_json::from_str::<Value>(&o.data)
        {
            v["hits"] = json!(next);
            o.data = v.to_string();
        }
    }
    Ok(())
}

/// 执行获准移动：公路磨损固定 ROAD_WEAROUT_POWER_CREEP（不乘部件数）、
/// 踩敌方工地与 creep 同门；无疲劳累计与恢复。来源：
/// processor/intents/movement.js execute 的 powerCreep 分支。
fn pc_movement_execute(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(target) = w.movement.take_move(id) else {
        return Ok(());
    };
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id).cloned() else {
        return Ok(());
    };
    if !w.movement.admitted(id) {
        return Ok(());
    }
    let owner = pc_data(&RoomObject::Generic(g.clone()))
        .and_then(|v| pc_owner(&v).map(str::to_string))
        .unwrap_or_default();
    let stomp_allowed = w
        .controller()
        .is_none_or(|ctl| ctl.owner.as_deref() == Some(owner.as_str()) || ctl.safe_mode <= w.time);
    let mut road_id = None;
    let cell_ids: Vec<ObjId> = w.index.at(target.0 as usize, target.1 as usize).to_vec();
    for tid in cell_ids {
        match w.room.objects.get(&tid).cloned() {
            Some(RoomObject::Road(_)) => road_id = Some(tid),
            Some(RoomObject::ConstructionSite(site))
                if stomp_allowed && site.owner.as_deref() != Some(owner.as_str()) =>
            {
                let pos = (site.x, site.y);
                w.remove_obj(tid);
                if site.progress > 1 {
                    w.create_energy(pos.0, pos.1, (site.progress / 2) as i64);
                }
            }
            _ => {}
        }
    }
    if let Some(rid) = road_id
        && let Some(RoomObject::Road(r)) = w.room.objects.get_mut(&rid)
    {
        r.next_decay_time = r
            .next_decay_time
            .saturating_sub((c::ROAD_WEAROUT_POWER_CREEP).max(0) as u64);
    }
    w.move_obj(id, target).map_err(SettleError)?;
    Ok(())
}
