//! 移动族：move intent 与 creep tick 的位移执行（官方 move.js / tick.js
//! 疲劳与矩阵裁决）。B1 结构治理批自 creep.rs 纯搬移。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;

use crate::intent::IntentData;
use crate::settle::damage;
use crate::settle::movement::{self, offsets_by_direction};
use crate::settle::{SettleError, Working};

use super::lifecycle::die_creep;

/// 官方 move.js：静态障碍预检后入矩阵（creep 不预检——挤格语义）。
/// 位移符号 → 方向常量（官方 move.js:24-29 的 `d = [dx, dy]` 判定；两轴同零
/// 不可达——同格 creep 被 API 层 isNearTo 排除，防御性返回 0 丢弃）。
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
    // 官方 object._oldFatigue = object.fatigue（canMove 判定用）
    w.old_fatigue.insert(id, fatigue);

    // 官方双形态（move.js:15-29）：朝相邻 creep 移动（intent 带 id）按位移
    // 符号换算方向；direction 形态直取。id 形态目标不可达/不邻接 → 整条丢弃
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
    // 目标格静态障碍预检（官方 move.js:33-37）：OBSTACLE 非 creep 对象 +
    // 非公开 rampart + powerCreep 的跨 shard portal（M1 无）
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

/// 官方 creeps/tick.js（M1 子集）：移动落格、TTL 死亡、疲劳恢复。
/// 官方 creeps/tick.js（M1 子集）：孵化中的雏形自愈/出生（spawn 侧的对偶
/// 路径）、移动落格、ageTime 首触赋值、TTL 死亡、疲劳恢复。
pub fn creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    // 移动落格（movement.js execute）
    movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(()); // 结算中被移除（不会发生于 M1 路径，防御）
    }
    let (spawning, age_time, x, y, name) = match w.creep(id) {
        Some(c) => (c.spawning, c.age_time, c.x, c.y, c.name.clone()),
        None => return Ok(()),
    };
    if spawning {
        // 官方 tick.js:10-27：孵化中的雏形——同格无 spawn（M2 无 invaderCore
        // 孵化）→ 移除；spawn 任务已清/换名 → 雏形侧出生（cancelSpawning 移除
        // 雏形、spawn 侧 spawn_tick 出生并清任务，正常路径不可达——防御对偶）。
        // 出生后**不**走 else 的 ageTime/TTL 分支（官方 if/else 互斥——出生
        // 雏形的寿命在下一 tick 赋值），但共用尾部的疲劳/伤害照常
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
        // ageTime 首触赋值（官方 tick.js:88-91：孵化出生的雏形不带 ageTime——
        // CLAIM 部件 600、其余 1500；放置路径 place.rs 已显式赋值不走此分支）
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
        // TTL 死亡（creeps/tick.js:93-95 gameTime >= ageTime-1）
        if let Some(age_time) = w.creep(id).map(|c| c.age_time)
            && w.time >= age_time.saturating_sub(1)
        {
            die_creep(w, id);
            return Ok(());
        }
    }
    // 疲劳恢复（creeps/tick.js:105-108，两分支共用尾部）：-2 × 存活 MOVE 部件
    if let Some(c) = w.creep(id) {
        let moves =
            kk_world::types::calc_body_effectiveness(&c.body, BodyKind::Move, "move", "fatigue", 1);
        if moves > 0 {
            w.add_fatigue(id, -2 * moves);
        }
    }
    // 延迟伤害/治疗结算（creeps/tick.js:118-150 在疲劳恢复之后）：伤害先于
    // 治疗施加、死亡判定在两者之后——同 tick 等量互相抵消；归零 → 暴力死亡
    // （与 TTL 死亡同路径），否则重排部件分布并处理缩容掉落。官方对孵化中
    // 的雏形同样施加（攻击 intent 期照常入 _damageToApply）
    damage::apply_pending_hits(w, id);
    Ok(())
}

/// 官方 movement.js execute（189-252）。
fn movement_execute(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(target) = w.movement.take_move(id) else {
        return Ok(());
    };
    // 对象可能在 check 与 execute 之间消亡（如 spawnstomp）——官方 objects 映射
    // 查不到即跳过；panic 会绕过"失败即回退"的 D4-3 语义，不设 expect
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
    // 官方疲劳率判定顺序（movement.js:204-222）：默认 2 → 沼泽 10 →
    // 路最后覆盖为 1（路上的沼泽按路走）；路磨损；进格踩掉敌方工地
    //（:224-233 的 safeMode 门：`!roomController || controller.user ===
    // object.user || !(safeMode > gameTime)` 才允许踩——他人 safeMode 生效期
    // 敌方工地受保护；M1 只做了工地归属半边，B2 补齐 safeMode 门）
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
                // 官方 :224-233：safeMode 门放行时踩掉敌方工地（退一半工程为能量）
                if stomp_allowed && site.owner.as_deref() != Some(owner.as_str()) => {
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
    if let Some(rid) = road_id {
        fatigue_rate = 1;
        if let Some(RoomObject::Road(r)) = w.room.objects.get_mut(&rid) {
            r.next_decay_time = r
                .next_decay_time
                .saturating_sub((c::ROAD_WEAROUT * body_len as i64).max(0) as u64);
        }
    }
    // 疲劳（movement.js:235-251）：体重 × 系数；边缘特例（从非边缘进边缘格 = 0，
    // 为跨房间预备——M1 单房间无落地，仍按官方给 0）
    let fatigue = weight as i64 * fatigue_rate;
    let to_edge = target.0 == 0 || target.0 == 49 || target.1 == 0 || target.1 == 49;
    let from_edge = from.0 == 0 || from.0 == 49 || from.1 == 0 || from.1 == 49;
    w.move_obj(id, target);
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
