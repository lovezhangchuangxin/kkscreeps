//! powerCreep usePower 意图的施放门序与各 power 分支结算。
//!
//! 移植官方 engine/src/processor/intents/power-creeps/usePower.js 的门序
//! （房主 powerEnabled → power 等级/冷却 → ops → 射程/覆盖 → 各 power
//! 有效目标门 → 共用尾部：效果表写入、ops 扣减、冷却、EVENT_POWER 与
//! 动作日志）。OPERATE_EXTENSION 为即时灌能（`pc_operate_extension`），
//! 不落效果表；SHIELD 原地插入临时 rampart（放置门见分支）。
use super::*;

/// usePower：官方门序（房主 powerEnabled → power 等级/冷却 → ops → 射程
/// /效果覆盖）后结算 generateOps、tower、spawn、lab/powerSpawn/factory、
/// source/mineral、storage 容量与 extension 灌能。
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
            if !pc_operate_extension(w, target_id, level) {
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
                g.data = data.to_string();
            }
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
        c::PWR_SHIELD => {
            // 官方 usePower.js:228-257：先删除自身位置的 constructionSite，
            // 通过对象/地形两门 rampart 放置检查后原地插入临时 rampart：
            // hits=effect[level-1]、hitsMax=0、nextDecayTime=endTime，携带
            // PWR_SHIELD 效果；衰减由 generic_decay_tick 的删除门接管。
            // POWER_INFO.energy 字段官方无消费者，不扣（ops 亦为 0）。
            let Some(duration) = info.get("duration").and_then(Value::as_u64) else {
                return;
            };
            let Some(hits) = power_effect(c::PWR_SHIELD, level) else {
                return;
            };
            let (x, y) = (g.x, g.y);
            let mut rampart_blocked = false;
            let mut site_ids = Vec::new();
            for (tid, obj) in w.room.objects.iter() {
                let (ox, oy) = obj.pos();
                if ox != x || oy != y {
                    continue;
                }
                match obj {
                    RoomObject::ConstructionSite(_) => site_ids.push(*tid),
                    other if pc_data(other).is_some_and(|t| t["structureType"] == "rampart") => {
                        rampart_blocked = true;
                    }
                    _ => {}
                }
            }
            for tid in site_ids {
                w.remove_obj(tid);
            }
            // 官方 checkConstructionSite 对象门末尾的坐标界检查在工地
            // 删除之后（utils.js:186）：越界 pc 的工地同样被删再拒绝。
            if x == 0 || y == 0 || x >= 49 || y >= 49 {
                return;
            }
            if rampart_blocked {
                return;
            }
            // 官方 checkConstructionSite 地形门：本格非墙；贴边（1/48）时
            // 三块边界格须全为墙（rampart 不适用其余结构避让规则）。
            if w.room
                .terrain()
                .check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_WALL)
            {
                return;
            }
            // 官方 utils.js:134-137 为顺序 if：四角位置 y 分支覆盖 x 分支，
            // 最终检查 y 侧三块边界格（非四角两实现等价）。
            let mut border: Option<[(usize, usize); 3]> = None;
            if x == 1 {
                border = Some([(0, y as usize - 1), (0, y as usize), (0, y as usize + 1)]);
            }
            if x == 48 {
                border = Some([(49, y as usize - 1), (49, y as usize), (49, y as usize + 1)]);
            }
            if y == 1 {
                border = Some([(x as usize - 1, 0), (x as usize, 0), (x as usize + 1, 0)]);
            }
            if y == 48 {
                border = Some([(x as usize - 1, 49), (x as usize, 49), (x as usize + 1, 49)]);
            }
            if let Some(tiles) = border
                && tiles.iter().any(|(bx, by)| {
                    !w.room
                        .terrain()
                        .check_mask(*bx, *by, kk_core::TERRAIN_MASK_WALL)
                })
            {
                return;
            }
            let end_time = w.time + duration;
            // 官方 bulk.insert 不带 notifyWhenAttacked（受击不发通知）。
            let data = json!({
                "type": "structure", "structureType": "rampart",
                "owner": owner, "hits": hits, "hitsMax": 0,
                "nextDecayTime": end_time,
            });
            let Ok(new_id) = w.insert_new(RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: data.to_string(),
            })) else {
                return;
            };
            w.room
                .effects
                .entry(new_id)
                .or_default()
                .push(kk_world::EffectEntry {
                    effect: power,
                    power,
                    level,
                    end_time,
                });
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

/// OPERATE_EXTENSION：从带 store 的 storage/terminal/factory/container
/// 目标向房内与房主同主的非 off extension 按到目标的 Chebyshev 距离升序
/// 灌能；预算为 min(目标 energy, effect[level-1] × Σextension 容量)。
/// 目标有主须与房主一致、目标带活跃 DISRUPT_TERMINAL 效果拒绝；未移动
/// 任何能量则整次静默拒绝（不扣 ops、不进冷却、不发事件——由调用方
/// 共尾处理成功路径）。effect 为小数倍率，官方 JS 浮点乘法在 50 的倍数
/// 容量下恒为整数，Rust 以 f64 同式计算后截断。返回是否移动了能量。
/// 来源：processor/intents/power-creeps/usePower.js 的 OPERATE_EXTENSION 分支。
pub(super) fn pc_operate_extension(w: &mut Working, target_id: ObjId, level: i64) -> bool {
    let Some(fraction) = power_effect_value(c::PWR_OPERATE_EXTENSION, level) else {
        return false;
    };
    let ctl_owner = w.controller().and_then(|ctl| ctl.owner.clone());
    let (target_energy, t_pos) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Storage(t)) => {
            // 官方 target.user && target.user != roomController.user：仅当
            // 目标有主且与房主不同才拒绝，无主目标放行（与 Generic 臂
            // 同一 truthiness 语义；kk 无主 storage 的 owner 为空串）。
            if let Some(owner) = (!t.owner.is_empty()).then_some(t.owner.as_str())
                && Some(owner) != ctl_owner.as_deref()
            {
                return false;
            }
            (i64::from(t.store.energy), (t.x, t.y))
        }
        Some(RoomObject::Generic(g)) => {
            let Ok(data) = serde_json::from_str::<Value>(&g.data) else {
                return false;
            };
            if !matches!(
                data.get("structureType").and_then(Value::as_str),
                Some("terminal") | Some("factory") | Some("container")
            ) {
                return false;
            }
            let Some(store) = data.get("store") else {
                return false; // 官方 !target.store 拒绝
            };
            let owner = data.get("owner").and_then(Value::as_str);
            if let Some(owner) = owner
                && Some(owner) != ctl_owner.as_deref()
            {
                return false;
            }
            (
                store.get("energy").and_then(Value::as_i64).unwrap_or(0),
                (g.x, g.y),
            )
        }
        _ => return false,
    };
    if kk_world::effects::active_power(
        w.room.effects.get(&target_id),
        c::PWR_DISRUPT_TERMINAL,
        w.time,
    )
    .is_some()
    {
        return false;
    }
    // 官方读上一拍由 extensions/tick 归一的 storeCapacityResource；kk 读
    // 当前 RCL 查表，同 tick 控制器升级的窗口内更宽松（与 storage_capacity
    // 的时序偏差同族，tick 末收敛一致）。
    let rcl = w.controller().map(|ctl| ctl.level).unwrap_or(0);
    let mut extensions: Vec<(ObjId, i64, i64, i32)> = Vec::new();
    for (oid, object) in w.room.objects.iter() {
        let RoomObject::Extension(e) = object else {
            continue;
        };
        if e.off {
            continue;
        }
        let owner = if e.owner.is_empty() {
            None
        } else {
            Some(e.owner.as_str())
        };
        if owner != ctl_owner.as_deref() {
            continue;
        }
        let cap = kk_world::types::extension_energy_capacity(rcl);
        let cur = i64::from(e.store.energy);
        let dist = (e.x as i32 - t_pos.0 as i32)
            .abs()
            .max((e.y as i32 - t_pos.1 as i32).abs());
        extensions.push((*oid, cap, cur, dist));
    }
    // 稳定排序保持对象 ID 序的距离平局顺序（官方 stable sort + 房内插入序）。
    extensions.sort_by_key(|&(_, _, _, dist)| dist);
    let total_cap: i64 = extensions.iter().map(|&(_, cap, _, _)| cap).sum();
    let energy_limit = (target_energy as f64).min(fraction * total_cap as f64);
    let mut sent = 0f64;
    let mut fills: Vec<(ObjId, i64)> = Vec::new();
    for &(oid, cap, cur, _) in &extensions {
        let room_left = energy_limit - sent;
        if room_left <= 0.0 {
            break;
        }
        // 官方 Math.min 不钳负数：RCL 降级后 cap<存量的超填 extension 会被
        // 扣回并使目标反向获能（官方涌现行为）；kk 有意钳 0 跳过该 extension。
        let energy = room_left.min((cap - cur) as f64).max(0.0);
        if energy > 0.0 {
            fills.push((oid, cur + energy as i64));
            sent += energy;
        }
    }
    if fills.is_empty() {
        return false; // energySent === 0：静默拒绝
    }
    for (oid, new_energy) in fills {
        if let Some(RoomObject::Extension(e)) = w.room.objects.get_mut(&oid) {
            e.store.energy = new_energy as u32;
        }
    }
    let sent = sent as i64;
    match w.room.objects.get_mut(&target_id) {
        Some(RoomObject::Storage(t)) => t.store.energy = (i64::from(t.store.energy) - sent) as u32,
        Some(RoomObject::Generic(g)) => {
            if let Ok(mut data) = serde_json::from_str::<Value>(&g.data) {
                let energy = data
                    .pointer("/store/energy")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                data["store"]["energy"] = json!(energy - sent);
                g.data = data.to_string();
            }
        }
        _ => {}
    }
    true
}
