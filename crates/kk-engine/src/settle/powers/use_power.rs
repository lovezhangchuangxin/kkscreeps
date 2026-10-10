//! powerCreep usePower 意图的施放门序与各 power 分支结算。
//!
//! 移植官方 engine/src/processor/intents/power-creeps/usePower.js 的门序
//! （房主 powerEnabled → power 等级/冷却 → ops → 射程/覆盖 → 各 power
//! 有效目标门 → 共用尾部：效果表写入、ops 扣减、冷却、EVENT_POWER 与
//! 动作日志）。OPERATE_EXTENSION 为即时灌能（见 `extension`），
//! 不落效果表；SHIELD 原地插入临时 rampart（放置门见分支）。
mod extension;
mod shield;

use super::*;

/// usePower：官方门序（房主 powerEnabled → power 等级/冷却 → ops → 射程
/// /效果覆盖）后派发目标效果或即时动作；成功路径统一提交冷却、事件与日志。
/// 来源：processor/intents/power-creeps/usePower.js。
pub(super) fn pc_use_power(w: &mut Working, id: ObjId, d: &IntentData) {
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
        c::PWR_OPERATE_EXTENSION => {
            let Some(target_id) = d.id else { return };
            if !extension::cast(w, target_id, level) {
                return;
            }
            v["store"]["ops"] = json!(held_ops - ops);
            0
        }
        c::PWR_OPERATE_TOWER
        | c::PWR_DISRUPT_TOWER
        | c::PWR_OPERATE_SPAWN
        | c::PWR_DISRUPT_SPAWN
        | c::PWR_OPERATE_LAB
        | c::PWR_OPERATE_POWER
        | c::PWR_OPERATE_FACTORY
        | c::PWR_REGEN_SOURCE
        | c::PWR_DISRUPT_SOURCE
        | c::PWR_REGEN_MINERAL
        | c::PWR_OPERATE_STORAGE
        | c::PWR_OPERATE_TERMINAL
        | c::PWR_DISRUPT_TERMINAL
        | c::PWR_OPERATE_CONTROLLER
        | c::PWR_OPERATE_OBSERVER
        | c::PWR_FORTIFY => {
            let Some(target_id) = d.id else { return };
            let valid_target = match w.room.objects.get(&target_id) {
                Some(RoomObject::Spawn(_)) => {
                    matches!(power, c::PWR_OPERATE_SPAWN | c::PWR_DISRUPT_SPAWN)
                }
                Some(RoomObject::Controller(_)) => power == c::PWR_OPERATE_CONTROLLER,
                // 官方 usePower.js:220-226：FORTIFY 目标限 rampart/constructedWall。
                Some(RoomObject::ConstructedWall(_)) => power == c::PWR_FORTIFY,
                Some(RoomObject::Source(_)) => {
                    matches!(power, c::PWR_REGEN_SOURCE | c::PWR_DISRUPT_SOURCE)
                }
                Some(RoomObject::Storage(_)) => power == c::PWR_OPERATE_STORAGE,
                // 官方 usePower.js 的 REGEN_MINERAL 门：非耗尽且无待再生
                // 时钟（nextRegenerationTime 真值判，0 视为未设置）。
                Some(RoomObject::Mineral(m)) => {
                    power == c::PWR_REGEN_MINERAL
                        && m.mineral_amount != 0
                        && !m.next_regeneration_time.is_some_and(|t| t != 0)
                }
                Some(target) => {
                    let expected = match power {
                        c::PWR_OPERATE_TOWER | c::PWR_DISRUPT_TOWER => "tower",
                        c::PWR_OPERATE_LAB => "lab",
                        c::PWR_OPERATE_POWER => "powerSpawn",
                        c::PWR_OPERATE_FACTORY => "factory",
                        c::PWR_OPERATE_TERMINAL | c::PWR_DISRUPT_TERMINAL => "terminal",
                        c::PWR_OPERATE_OBSERVER => "observer",
                        c::PWR_FORTIFY => "rampart",
                        _ => return,
                    };
                    pc_data(target).is_some_and(|target| target["structureType"] == expected)
                }
                _ => false,
            };
            if !valid_target {
                return;
            }
            if power == c::PWR_OPERATE_FACTORY {
                let factory_level = w
                    .room
                    .objects
                    .get(&target_id)
                    .and_then(pc_data)
                    .and_then(|data| data["level"].as_i64())
                    .unwrap_or(0);
                if factory_level != 0 && factory_level != level {
                    return;
                }
            }
            let duration = match info.get("duration") {
                Some(Value::Array(a)) => a.get((level - 1) as usize).and_then(Value::as_u64),
                Some(n) => n.as_u64(),
                None => None,
            };
            let Some(duration) = duration else { return };
            if power == c::PWR_OPERATE_FACTORY
                && let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&target_id)
                && let Ok(mut data) = serde_json::from_str::<Value>(&g.data)
            {
                // 首次施放永久锁定等级；到期不降级，同级可重新刷新效果。
                // 来源：power-creeps/usePower.js 的 OPERATE_FACTORY 分支。
                data["level"] = json!(level);
                g.data = data.to_string().into();
            }
            let effects = w.room.effects.entry(target_id).or_default();
            effects.retain(|effect| effect.power != power);
            effects.push(kk_world::EffectEntry {
                effect: Some(power),
                power,
                level,
                end_time: w.time + duration,
            });
            v["store"]["ops"] = json!(held_ops - ops);
            0
        }
        c::PWR_SHIELD => {
            if !shield::cast(w, (g.x, g.y), &owner, level) {
                return;
            }
            0
        }
        _ => return,
    };
    let cooldown = info.get("cooldown").and_then(Value::as_i64).unwrap_or(0);
    let power_key = power.to_string();
    v["powers"][&power_key] = json!({ "level": level, "cooldownTime": w.time + cooldown as u64 });
    if let Some(RoomObject::Generic(o)) = w.room.objects.get_mut(&id) {
        o.data = v.to_string().into();
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
    w.record_action(
        id,
        "power",
        super::super::action_log::ActionLogPayload::Power {
            id: power,
            x: ax,
            y: ay,
        },
    );
}
