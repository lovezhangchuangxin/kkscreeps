//! 登记 creep 移动意图，并在对象 tick 执行位移、寿命、疲劳和待伤害处理。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::intent::IntentData;
use crate::settle::damage;
use crate::settle::movement::{self, offsets_by_direction};
use crate::settle::{SettleError, Working};

use super::lifecycle::die_creep;

/// 按位移符号换算八方向；同格返回无效方向 0。
fn direction_from_delta(dx: i32, dy: i32) -> u8 {
    match (dx.signum(), dy.signum()) {
        (0, -1) => 1,  // TOP
        (1, -1) => 2,  // TOP_RIGHT
        (1, 0) => 3,   // RIGHT
        (1, 1) => 4,   // BOTTOM_RIGHT
        (0, 1) => 5,   // BOTTOM
        (-1, 1) => 6,  // BOTTOM_LEFT
        (-1, 0) => 7,  // LEFT
        (-1, -1) => 8, // TOP_LEFT
        _ => 0,
    }
}

/// 记录旧疲劳并登记移动请求。direction 优先，否则用相邻 creep 的 ID 求方向。
/// 预检静态障碍和私有敌方 rampart，creep 冲突交给 Movement 仲裁。
pub(super) fn intent_move(w: &mut Working, id: ObjId, data: &IntentData) {
    let (spawning, fatigue, x, y, owner) = match w.creep(id) {
        Some(c) => (
            c.spawning,
            c.fatigue,
            c.x as i32,
            c.y as i32,
            c.owner.clone(),
        ),
        None => return,
    };
    if spawning {
        return;
    }
    w.old_fatigue.insert(id, fatigue);

    let direction = match data.direction {
        Some(d) => d,
        None => {
            let Some(target_id) = data.id else {
                return;
            };
            let Some((tx, ty)) = w.creep(target_id).map(|c| (c.x as i32, c.y as i32)) else {
                return;
            };
            let (ddx, ddy) = (tx - x, ty - y);
            if ddx.abs() > 1 || ddy.abs() > 1 {
                return; // 官方 API 层已挡 isNearTo；结算复核
            }
            direction_from_delta(ddx, ddy)
        }
    };
    let Some((dx, dy)) = offsets_by_direction(direction) else {
        return;
    };
    if x + dx < 0 || x + dx > 49 || y + dy < 0 || y + dy > 49 {
        return;
    }
    let target = ((x + dx) as u8, (y + dy) as u8);
    let mover_owner = owner;
    let has_static_obstacle = w
        .index
        .at(target.0 as usize, target.1 as usize)
        .iter()
        .any(|tid| {
            match w.room.objects.get(tid) {
                Some(RoomObject::Creep(_)) => false, // creep 不预检
                Some(RoomObject::Generic(o)) => {
                    let data: serde_json::Value = serde_json::from_str(&o.data).unwrap_or_default();
                    let st = data
                        .get("structureType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if st == "rampart" {
                        let public = data
                            .get("isPublic")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let owner = data.get("owner").and_then(|v| v.as_str()).unwrap_or("");
                        !public && owner != mover_owner
                    } else {
                        kk_world::types::is_obstacle_type(st)
                    }
                }
                Some(other) => kk_world::types::is_obstacle_type(&movement::type_name_of(other)),
                None => false,
            }
        });
    if has_static_obstacle {
        return;
    }
    w.movement.add(id, (x as u8, y as u8), dx, dy);
}

/// 依次执行移动、孵化或 TTL 处理、疲劳恢复、待伤害和治疗。
/// TTL 到期会提前返回；孵化分支若刚出生，本轮不再进入 TTL 初始化分支。
/// 来源：上游 engine/src/processor/intents/creeps/tick.js。
pub fn creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(()); // 对象可能在前面的结算动作中被移除。
    }
    let (spawning, age_time, x, y, name) = match w.creep(id) {
        Some(c) => (c.spawning, c.age_time, c.x, c.y, c.name.clone()),
        None => return Ok(()),
    };
    if spawning {
        let spawn_id = w
            .index
            .at(x as usize, y as usize)
            .iter()
            .find(|tid| matches!(w.room.objects.get(tid), Some(RoomObject::Spawn(_))))
            .copied();
        match spawn_id {
            None => {
                w.remove_obj(id);
                return Ok(());
            }
            Some(sid) => {
                let mismatch = match w.room.objects.get(&sid) {
                    Some(RoomObject::Spawn(s)) => {
                        s.spawning.as_ref().map(|j| j.name.as_str()) != Some(name.as_str())
                    }
                    _ => true,
                };
                if mismatch {
                    let job_directions = match w.room.objects.get(&sid) {
                        Some(RoomObject::Spawn(s)) => s
                            .spawning
                            .as_ref()
                            .map(|j| j.directions.clone())
                            .unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    super::spawn::born_creep(w, sid, &name, &job_directions);
                }
            }
        }
    } else {
        // 孵化生成的 creep 在出生后首次进入此分支时初始化寿命；
        // 含任意 CLAIM（包括损毁件）使用 600 tick，否则 1500 tick。
        if age_time == 0 {
            let has_claim = w.creep(id).is_some_and(|c| {
                c.body
                    .iter()
                    .any(|p| BodyKind::from_code(p.kind) == Some(BodyKind::Claim))
            });
            let life = if has_claim {
                c::CREEP_CLAIM_LIFE_TIME
            } else {
                c::CREEP_LIFE_TIME
            } as u64;
            let born_at = w.time + life;
            if let Some(creep) = w.creep_mut(id) {
                creep.age_time = born_at;
            }
        }
        if let Some(age_time) = w.creep(id).map(|c| c.age_time)
            && w.time >= age_time.saturating_sub(1)
        {
            die_creep(w, id);
            return Ok(());
        }
    }
    if let Some(c) = w.creep(id) {
        let moves =
            kk_world::types::calc_body_effectiveness(&c.body, BodyKind::Move, "move", "fatigue", 1);
        if moves > 0 {
            w.add_fatigue(id, -2 * moves);
        }
    }
    // 疲劳恢复先读取本轮受伤前的身体，之后才施加伤害、治疗并判断战斗死亡。
    damage::apply_pending_hits(w, id);
    Ok(())
}

/// 执行仲裁获准的移动，更新索引、疲劳和道路磨损，并踩掉未受安全模式保护的敌方工地。
fn movement_execute(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(target) = w.movement.take_move(id) else {
        return Ok(());
    };
    // 对象可能在仲裁后被孵化踩死，查不到时跳过，不能用 expect。
    let Some(creep) = w.creep(id) else {
        return Ok(());
    };
    let old_fatigue = w.old_fatigue.get(&id).copied().unwrap_or(0);
    if !movement::can_move(creep, old_fatigue, w.movement.is_pulled(id)) {
        return Ok(());
    }
    let (from, owner, body_len, weight) = (
        (creep.x, creep.y),
        creep.owner.clone(),
        creep.body.len(),
        movement::move_weight(creep),
    );
    let stomp_allowed = w
        .controller()
        .is_none_or(|ctl| ctl.owner.as_deref() == Some(owner.as_str()) || ctl.safe_mode <= w.time);
    let mut fatigue_rate = 2i64;
    let mut road_id = None;
    let cell_ids: Vec<ObjId> = w.index.at(target.0 as usize, target.1 as usize).to_vec();
    for tid in cell_ids {
        match w.room.objects.get(&tid).cloned() {
            Some(RoomObject::Road(_)) => {
                road_id = Some(tid);
            }
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
    if w.room.terrain.check_mask(
        target.0 as usize,
        target.1 as usize,
        kk_core::TERRAIN_MASK_SWAMP,
    ) {
        fatigue_rate = 10;
    }
    // 道路最后覆盖地形疲劳率，沼泽上的路仍为 1；磨损按全部身体部件数计算。
    if let Some(rid) = road_id {
        fatigue_rate = 1;
        if let Some(RoomObject::Road(r)) = w.room.objects.get_mut(&rid) {
            r.next_decay_time = r
                .next_decay_time
                .saturating_sub((c::ROAD_WEAROUT * body_len as i64).max(0) as u64);
        }
    }
    let fatigue = weight as i64 * fatigue_rate;
    let to_edge = target.0 == 0 || target.0 == 49 || target.1 == 0 || target.1 == 49;
    let from_edge = from.0 == 0 || from.0 == 49 || from.1 == 0 || from.1 == 49;
    w.move_obj(id, target).map_err(SettleError)?;
    // 从非边缘进入边缘时将疲劳归零，即使当前实现没有跨房落地也保留此规则。
    if to_edge && !from_edge {
        if let Some(c) = w.creep_mut(id) {
            c.fatigue = 0;
        }
        w.tick_fatigue.insert(id, 0);
    } else {
        w.add_fatigue(id, fatigue);
    }
    Ok(())
}
