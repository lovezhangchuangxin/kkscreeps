//! OPERATE_EXTENSION 的目标准入、灌能排序与库存候选更新。
use super::super::power_effect_value;
use crate::settle::Working;
use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

/// OPERATE_EXTENSION：从带 store 的 storage/terminal/factory/container
/// 目标向房内与房主同主的非 off extension 按到目标的 Chebyshev 距离升序
/// 灌能；预算为 min(目标 energy, effect[level-1] × Σextension 容量)。
/// 目标有主须与房主一致、目标带活跃 DISRUPT_TERMINAL 效果拒绝；未移动
/// 任何能量则整次静默拒绝（不扣 ops、不进冷却、不发事件——由调用方
/// 共尾处理成功路径）。effect 为小数倍率，官方 JS 浮点乘法在 50 的倍数
/// 容量下恒为整数，Rust 以 f64 同式计算后截断。返回是否移动了能量。
/// 来源：processor/intents/power-creeps/usePower.js 的 OPERATE_EXTENSION 分支。
pub(super) fn cast(w: &mut Working, target_id: ObjId, level: i64) -> bool {
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
    // 当前 RCL 查表，同 tick 控制器升级时可能提前采用新容量，
    // 对象 tick 后与官方容量收敛；此限制不适用于 storage。
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
                g.data = data.to_string().into();
            }
        }
        _ => {}
    }
    true
}
