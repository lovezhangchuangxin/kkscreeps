//! 处理资源再生、结构与遗留物衰减、冷却，以及控制器安全模式和降级。
//!
//! 绝对时钟通常在 deadline-1 触发；link/extractor 的 cooldown 则是剩余 tick 数。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use serde_json::{Value, json};

use super::Working;

/// 不足容量时设置再生时钟，到 next_regeneration_time-1 按旧容量补满。
/// 随后根据房主或预定调整容量；只有转为中性容量时同时压低超量能量。
pub fn source_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Source(s)) = w.room.objects.get(&id) else {
        return;
    };
    let (energy, capacity, regen) = (s.energy, s.energy_capacity, s.next_regeneration_time);
    if energy < capacity {
        let regen = match regen {
            None => {
                let t = w.time + c::ENERGY_REGEN_TIME as u64;
                if let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id) {
                    s.next_regeneration_time = Some(t);
                }
                t
            }
            Some(t) => t,
        };
        if w.time >= regen.saturating_sub(1)
            && let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id)
        {
            s.next_regeneration_time = None;
            s.energy = s.energy_capacity;
        }
    }
    let ctl = w.controller();
    let target_cap = match ctl {
        None => c::SOURCE_ENERGY_KEEPER_CAPACITY as u32,
        Some(ctl) if ctl.owner.is_some() || ctl.reservation.is_some() => {
            c::SOURCE_ENERGY_CAPACITY as u32
        }
        Some(_) => c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u32,
    };
    if capacity != target_cap
        && let Some(RoomObject::Source(s)) = w.room.objects.get_mut(&id)
    {
        s.energy_capacity = target_cap;
        if target_cap == c::SOURCE_ENERGY_NEUTRAL_CAPACITY as u32 {
            s.energy = s.energy.min(target_cap);
        }
    }
}

/// 每 tick 衰减 ceil(amount/ENERGY_DECAY)，适用于所有掉落资源，归零时移除。
pub fn energy_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Energy(e)) = w.room.objects.get(&id) else {
        return;
    };
    let decay = ((e.amount as i64) + c::ENERGY_DECAY - 1) / c::ENERGY_DECAY;
    let new_amount = e
        .amount
        .saturating_sub(decay.clamp(0, u32::MAX as i64) as u32);
    if new_amount == 0 {
        w.remove_obj(id);
    } else if let Some(RoomObject::Energy(e)) = w.room.objects.get_mut(&id) {
        e.amount = new_amount;
    }
}

/// 在 next_decay_time-1 衰减，0 时钟也立即触发；沼泽和墙按各自倍率增加损耗。
/// 耐久耗尽直接移除，不创建废墟或事件。
pub fn road_decay_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Road(o)) = w.room.objects.get(&id) else {
        return;
    };
    let (x, y, next) = (o.x, o.y, o.next_decay_time);
    if next != 0 && w.time < next.saturating_sub(1) {
        return; // 未到点
    }
    let Some(RoomObject::Road(o)) = w.room.objects.get(&id) else {
        return;
    };
    let mut decay = kk_core::constants::ROAD_DECAY_AMOUNT;
    if w.room
        .terrain
        .check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_SWAMP)
    {
        decay *= kk_core::constants::CONSTRUCTION_COST_ROAD_SWAMP_RATIO;
    }
    if w.room
        .terrain
        .check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_WALL)
    {
        decay *= kk_core::constants::CONSTRUCTION_COST_ROAD_WALL_RATIO;
    }
    let hits = o.hits as i64 - decay;
    if hits <= 0 {
        w.remove_obj(id);
    } else if let Some(RoomObject::Road(o)) = w.room.objects.get_mut(&id) {
        o.hits = hits as u32;
        o.next_decay_time = w.time + kk_core::constants::ROAD_DECAY_TIME as u64;
    }
}

/// 处理 container/rampart 衰减；rampart 的 hitsMax 每 tick 同步，不等衰减到期。
/// Container 消失时先写入非正耐久，再掉落仓内资源，避免资源又存回即将删除的容器。
pub fn generic_decay_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let mut v: Value = serde_json::from_str(&g.data).unwrap_or_default();
    let st = v
        .get("structureType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let (x, y) = (g.x, g.y);
    let next = v.get("nextDecayTime").and_then(Value::as_u64).unwrap_or(0);
    if st != "container" && st != "rampart" {
        return;
    }
    if st == "rampart" {
        let target = w.controller().map(|ctl| {
            if v.get("owner").and_then(Value::as_str) == ctl.owner.as_deref() {
                let key = ctl.level.to_string();
                kk_core::constants::RAMPART_HITS_MAX
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| *v)
                    .unwrap_or(0)
            } else {
                0
            }
        });
        if let Some(hits_max) = target
            && hits_max != v.get("hitsMax").and_then(Value::as_i64).unwrap_or(0)
        {
            v["hitsMax"] = json!(hits_max);
            if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
                g.data = v.to_string();
            }
        }
    }
    if next != 0 && w.time < next.saturating_sub(1) {
        return;
    }
    let decay = if st == "container" {
        kk_core::constants::CONTAINER_DECAY
    } else {
        kk_core::constants::RAMPART_DECAY_AMOUNT
    };
    let hits = v.get("hits").and_then(Value::as_i64).unwrap_or(0) - decay;
    if hits <= 0 {
        if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
            v["hits"] = json!(hits);
            g.data = v.to_string();
        }
        if st == "container" {
            let keys: Vec<(String, i64)> = v
                .get("store")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, amount)| {
                            amount.as_i64().filter(|a| *a > 0).map(|a| (k.clone(), a))
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (resource, amount) in keys {
                if resource == "energy" {
                    w.create_energy(x, y, amount);
                } else {
                    w.create_resource(x, y, amount, &resource);
                }
            }
        }
        w.remove_obj(id);
        return;
    }
    v["hits"] = json!(hits);
    let window = if st == "container" {
        match w.controller() {
            Some(ctl) if ctl.level > 0 => kk_core::constants::CONTAINER_DECAY_TIME_OWNED,
            _ => kk_core::constants::CONTAINER_DECAY_TIME,
        }
    } else {
        kk_core::constants::RAMPART_DECAY_TIME
    };
    v["nextDecayTime"] = json!(w.time + window as u64);
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
        g.data = v.to_string();
    }
}

/// Link 的 cooldown 是剩余 tick 数，每轮减 1，与 terminal 的绝对时钟不同。
pub fn link_cooldown_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&g.data) else {
        return;
    };
    let cd = v.get("cooldown").and_then(Value::as_u64).unwrap_or(0);
    if cd == 0 {
        return;
    }
    v["cooldown"] = json!(cd - 1);
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id) {
        g.data = v.to_string();
    }
}

/// 存在控制器时按 RCL 配额和归属设置容量，失效时写为 0；无控制器时保留原值。
pub fn terminal_capacity_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&g.data) else {
        return;
    };
    let Some(ctl) = w.controller() else {
        return;
    };
    let level = ctl.level;
    let owner_opt = v.get("owner").and_then(Value::as_str);
    let allowed = crate::settle::controller_allowed("terminal", i64::from(level));
    let want = if i64::from(level) > 0 && ctl.owner.as_deref() == owner_opt && allowed > 0 {
        c::TERMINAL_CAPACITY
    } else {
        0
    };
    let current = v.get("storeCapacity").and_then(Value::as_i64);
    if current != Some(want)
        && let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
    {
        let Ok(mut v) = serde_json::from_str::<Value>(&g.data) else {
            return;
        };
        v["storeCapacity"] = json!(want);
        g.data = v.to_string();
    }
}

/// 在 decay_time-1 移除墓碑，将固定槽和命名槽资源转为掉落资源；0 时钟立即触发。
pub fn tombstone_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Tombstone(o)) = w.room.objects.get(&id) else {
        return;
    };
    let (x, y, decay_time) = (o.x, o.y, o.decay_time);
    if decay_time != 0 && w.time < decay_time.saturating_sub(1) {
        return; // 未到点
    }
    let Some(RoomObject::Tombstone(o)) = w.room.objects.get(&id).cloned() else {
        return;
    };
    if o.store.energy > 0 {
        w.create_resource(x, y, o.store.energy as i64, "energy");
    }
    if o.store.ghodium > 0 {
        w.create_resource(
            x,
            y,
            o.store.ghodium as i64,
            kk_core::constants::RESOURCE_GHODIUM,
        );
    }
    for stack in &o.store_extra {
        w.create_resource(x, y, stack.amount as i64, &stack.resource);
    }
    w.remove_obj(id);
}

/// 在 decay_time-1 移除废墟并转移其中资源；同格容器可接收掉落资源。
pub fn ruin_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Ruin(o)) = w.room.objects.get(&id) else {
        return;
    };
    let (x, y, decay_time) = (o.x, o.y, o.decay_time);
    if decay_time != 0 && w.time < decay_time.saturating_sub(1) {
        return;
    }
    let Some(RoomObject::Ruin(o)) = w.room.objects.get(&id).cloned() else {
        return;
    };
    if o.store.energy > 0 {
        w.create_resource(x, y, o.store.energy as i64, "energy");
    }
    if o.store.ghodium > 0 {
        w.create_resource(
            x,
            y,
            o.store.ghodium as i64,
            kk_core::constants::RESOURCE_GHODIUM,
        );
    }
    for stack in &o.store_extra {
        w.create_resource(x, y, stack.amount as i64, &stack.resource);
    }
    w.remove_obj(id);
}

/// 先清理到期预定和提交升级封锁，再处理安全模式激活与降级时钟。
/// 允许过期多级时连续降级；降为 0 清除归属和相关能力字段。
/// 来源：上游 engine/src/processor/intents/controllers/tick.js。
pub fn controller_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
    let (level, progress, downgrade_time, owner) =
        (t.level, t.progress, t.downgrade_time, t.owner.clone());
    if let Some(r) = &t.reservation
        && (w.time >= r.end_time.saturating_sub(1) || owner.is_some())
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.reservation = None;
    }
    let Some(owner) = owner else { return };
    if owner == "2" {
        return; // NPC invader（owner=2）跳过控制器寿命处理。
    }
    if let Some(blocked_until) = w.upgrade_block_pending.get(&id).copied()
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.upgrade_blocked = Some(blocked_until);
    }
    let blocked_now = w
        .room
        .objects
        .get(&id)
        .and_then(|o| match o {
            RoomObject::Controller(t) => t.upgrade_blocked,
            _ => None,
        })
        .unwrap_or(0);
    if w.safe_mode_activated.contains(&id)
        && blocked_now <= w.time
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id)
    {
        t.safe_mode_available = t.safe_mode_available.saturating_sub(1);
        t.safe_mode = w.time + c::SAFE_MODE_DURATION as u64;
        t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
    }
    let Some(mut dt) = downgrade_time else {
        let t0 = w.time + controller_downgrade(level) + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(t0);
        }
        return;
    };
    if w.upgraded.get(&id).is_some_and(|u| *u > 0) && blocked_now <= w.time {
        let refreshed = (dt + c::CONTROLLER_DOWNGRADE_RESTORE as u64 + 1)
            .min(w.time + controller_downgrade(level) + 1);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.downgrade_time = Some(refreshed);
        }
        return;
    }
    while w.time >= dt.saturating_sub(1) && level > 0 {
        let (mut cur_level, mut cur_progress) = (level, progress);
        if let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) {
            cur_level = t.level;
            cur_progress = t.progress;
        }
        let new_level = cur_level - 1;
        if new_level == 0 {
            if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
                t.level = 0;
                t.progress = 0;
                t.owner = None;
                t.owner_cleared = true;
                t.downgrade_time = None;
                t.upgrade_blocked = None;
                t.safe_mode = 0;
                t.safe_mode_available = 0;
                t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
                t.is_power_enabled = false;
            }
            return;
        }
        let add_progress = (controller_levels(new_level) as f64 * 0.9).round() as u64;
        dt += controller_downgrade(new_level) / 2 + 1;
        if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
            t.level = new_level;
            t.progress = cur_progress + add_progress;
            t.downgrade_time = Some(dt);
            t.safe_mode_available = 0;
            t.safe_mode_cooldown = Some(w.time + c::SAFE_MODE_COOLDOWN as u64);
        }
    }
}

fn controller_levels(level: u8) -> i64 {
    kk_world::domain::controller_progress_total_opt(level).unwrap_or(0)
}

/// 矿物耗尽后设置再生时钟，到期按旧 density 补充数量，再决定下一次 density。
/// 随机序列由房间名、对象 ID 和 tick 派生，不使用 World.seed。
pub fn mineral_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Mineral(m)) = w.room.objects.get(&id) else {
        return;
    };
    let (density, amount, next_regen) = (m.density, m.mineral_amount, m.next_regeneration_time);
    if amount != 0 {
        return;
    }
    let next_regen = match next_regen {
        None => {
            let t = w.time + c::MINERAL_REGEN_TIME as u64;
            if let Some(RoomObject::Mineral(m)) = w.room.objects.get_mut(&id) {
                m.next_regeneration_time = Some(t);
            }
            t
        }
        Some(t) => t,
    };
    if w.time < next_regen.saturating_sub(1) {
        return; // 未到点
    }
    let mut rng = MineralRng::new(&w.room.name, id, w.time);
    let new_amount = kk_world::types::mineral_density(density);
    let rotate = density == c::DENSITY_LOW as u8
        || density == c::DENSITY_ULTRA as u8
        || rng.next_f64() < c::MINERAL_DENSITY_CHANGE;
    let new_density = if rotate {
        loop {
            let r = rng.next_f64();
            let picked = if r <= 0.1 {
                1
            } else if r <= 0.5 {
                2
            } else if r <= 0.9 {
                3
            } else {
                4
            };
            if picked != density {
                break picked;
            }
        }
    } else {
        density
    };
    if let Some(RoomObject::Mineral(m)) = w.room.objects.get_mut(&id) {
        m.next_regeneration_time = None;
        m.mineral_amount = new_amount.max(0) as u32;
        m.density = new_density;
    }
}

/// 先递减旧 cooldown，再覆盖本轮采集登记的新值，首次采集后的冷却不被提前减 1。
pub fn extractor_tick(w: &mut Working, id: ObjId) {
    let cooldown = match w.room.objects.get(&id) {
        Some(RoomObject::Extractor(e)) => e.cooldown,
        _ => return,
    };
    if cooldown > 0 {
        let next = cooldown - 1;
        if let Some(RoomObject::Extractor(e)) = w.room.objects.get_mut(&id) {
            e.cooldown = next;
        }
    }
    if let Some(pending) = w.extractor_cooldown_pending.remove(&id)
        && let Some(RoomObject::Extractor(e)) = w.room.objects.get_mut(&id)
    {
        e.cooldown = pending;
    }
}

/// 用于矿物密度选择的确定性随机序列；相同房名、对象 ID 和 tick 可重放。
struct MineralRng(u64);

impl MineralRng {
    fn new(room: &str, id: ObjId, time: u64) -> Self {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in room.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        Self(
            h ^ id.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ time.wrapping_mul(0xbf58_476d_1ce4_e5b9),
        )
    }

    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// 返回该 RCL 的降级间隔，未知等级返回 0。
pub fn controller_downgrade(level: u8) -> u64 {
    let key = level.to_string();
    kk_core::constants::CONTROLLER_DOWNGRADE
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0) as u64
}
