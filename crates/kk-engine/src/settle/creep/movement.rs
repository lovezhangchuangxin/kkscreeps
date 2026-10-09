//! 登记 creep 移动意图，并在对象 tick 执行位移、寿命、疲劳和待伤害处理。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::json;

use crate::intent::IntentData;
use crate::settle::damage;
use crate::settle::movement::{self, offsets_by_direction};
use crate::settle::{SettleError, Working};

use super::lifecycle::die_creep;

/// 记录旧疲劳并登记移动请求。direction 优先，否则用相邻 creep 的 ID 求方向。
/// 无预检贡献时直接拒绝静态障碍；Hosted 保存判定等预检决策，creep 冲突由仲裁处理。
pub(in crate::settle) fn intent_move(w: &mut Working, id: ObjId, data: &IntentData) {
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
            movement::direction_from_delta(ddx, ddy)
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
    let blocked = movement::static_obstacles::registration_blocked(
        &w.room,
        &w.index,
        &w.plane.buildings,
        w.plane.static_passability,
        target,
        &mover_owner,
    );
    if blocked && !w.plane.contrib.defers_static_precheck() {
        return;
    }
    w.movement.add(id, (x as u8, y as u8), dx, dy);
    w.movement.mark_registration_blocked(id, blocked);
}

/// 移动槽能力：读取执行者当前登记的请求方向；未登记返回 `None`。
/// 执行者不在本房候选（或不是 creep）按故障处理，与写能力同一口径。
pub fn slot_pending_direction(w: &Working, id: ObjId) -> Result<Option<u8>, String> {
    let creep = w.creep(id).ok_or("移动执行者不在本房候选中")?;
    let Some(cell) = w.movement.request_cell_of(id) else {
        return Ok(None);
    };
    let direction = movement::direction_from_delta(
        cell.0 as i32 - creep.x as i32,
        cell.1 as i32 - creep.y as i32,
    );
    Ok((direction != 0).then_some(direction))
}

/// 移动槽能力：以受控方向登记或替换执行者的移动请求。
/// 与 native 意图同一预检：越界、孵化中返回 `Ok(false)`；静态障碍
/// 无贡献时直接拒绝，有预检贡献时暂存请求等该槽决策。同步维护
/// 意图时疲劳，使疲劳门槛与仲裁评分沿用既有语义。重复调用时
/// 后一次登记覆盖前一次（单执行者单请求）。
pub fn slot_request_direction(w: &mut Working, id: ObjId, direction: u8) -> Result<bool, String> {
    let Some((dx, dy)) = offsets_by_direction(direction) else {
        return Err("移动方向须为 1..=8".into());
    };
    let Some(creep) = w.creep(id) else {
        return Err("移动执行者不在本房候选中".into());
    };
    if creep.spawning {
        return Ok(false);
    }
    let (x, y) = (creep.x as i32, creep.y as i32);
    if x + dx < 0 || x + dx > 49 || y + dy < 0 || y + dy > 49 {
        return Ok(false);
    }
    let target = ((x + dx) as u8, (y + dy) as u8);
    let blocked = movement::static_obstacles::registration_blocked(
        &w.room,
        &w.index,
        &w.plane.buildings,
        w.plane.static_passability,
        target,
        &creep.owner,
    );
    if blocked && !w.plane.contrib.defers_static_precheck() {
        return Ok(false);
    }
    w.old_fatigue.insert(id, creep.fatigue);
    w.movement.add(id, (x as u8, y as u8), dx, dy);
    w.movement.mark_registration_blocked(id, blocked);
    Ok(true)
}

/// 移动槽能力：取消执行者的请求登记。仲裁尚未发生，实际位移与
/// 疲劳执行本来也只在有获准请求时进行；此处只移除本执行者的登记。
pub fn slot_cancel_direction(w: &mut Working, id: ObjId) -> Result<(), String> {
    if w.creep(id).is_none() {
        return Err("移动执行者不在本房候选中".into());
    }
    w.movement.remove_requests(id);
    Ok(())
}

/// 依次执行移动、孵化或 TTL 处理、疲劳恢复、待伤害和治疗。
/// TTL 到期会提前返回；孵化分支若刚出生，本轮不再进入 TTL 初始化分支。
/// 来源：上游 engine/src/processor/intents/creeps/tick.js。
pub fn creep_tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    creep_tick_prepare(w, id)?;
    damage::apply_pending_hits(w, id);
    Ok(())
}

/// 房间编排在此函数之后释放候选借用，再进入可选 JS 受击链。
pub fn creep_tick_prepare(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    movement_execute(w, id)?;
    if !w.room.objects.contains_key(&id) {
        return Ok(()); // 对象可能在前面的结算动作中被移除。
    }
    let (spawning, age_time, x, y, name) = match w.creep(id) {
        Some(c) => (c.spawning, c.age_time, c.x, c.y, c.name.clone()),
        None => return Ok(()),
    };
    if spawning {
        // 官方宿主同格判定：spawn 或 invaderCore（creeps/tick.js:38）；
        // 无宿主的孵化 creep 按孤儿移除，宿主孵化名不匹配时立即出生。
        let host = w.index.at(x as usize, y as usize).iter().find_map(|tid| {
            match w.room.objects.get(tid) {
                Some(RoomObject::Spawn(_)) => Some((*tid, true)),
                Some(RoomObject::Generic(g)) => {
                    (kk_world::domain::generic::structure_type(&g.data).as_deref()
                        == Some("invaderCore"))
                    .then_some((*tid, false))
                }
                _ => None,
            }
        });
        match host {
            None => {
                w.remove_obj(id);
                return Ok(());
            }
            Some((sid, is_spawn)) => {
                let (mismatch, directions) = if is_spawn {
                    match w.room.objects.get(&sid) {
                        Some(RoomObject::Spawn(s)) => (
                            s.spawning.as_ref().map(|j| j.name.as_str()) != Some(name.as_str()),
                            s.spawning
                                .as_ref()
                                .map(|j| j.directions.clone())
                                .unwrap_or_default(),
                        ),
                        _ => (true, Vec::new()),
                    }
                } else {
                    // 核心的孵化状态存于 Generic data；无孵化或名不匹配即出生。
                    let core_name = w
                        .room
                        .objects
                        .get(&sid)
                        .and_then(|o| match o {
                            RoomObject::Generic(g) => {
                                serde_json::from_str::<serde_json::Value>(&g.data).ok()
                            }
                            _ => None,
                        })
                        .and_then(|v| {
                            v.get("spawning")
                                .and_then(|s| s.get("name"))
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_string)
                        });
                    (core_name.as_deref() != Some(name.as_str()), Vec::new())
                };
                if mismatch {
                    if is_spawn {
                        super::spawn::born_creep(w, sid, &name, &directions);
                    } else {
                        super::spawn::born_creep_at(
                            w,
                            (x, y),
                            kk_world::domain::npc::INVADER_OWNER,
                            &name,
                            &[],
                        );
                    }
                }
            }
        }
    } else {
        // 跨房过渡标记：位于边缘格的 creep（无论本 tick 是否移动过）过渡
        // 到邻房镜像格，方向按西/北/东/南的优先级取值。官方在移动执行后
        // 立即标记、位于寿命与战斗处理之前，并同拍向源房事件日志推
        // EVENT_EXIT（含目的房不可达的情形——事件先于迁移应用发出）。
        // 对象随后死亡时不迁移：本仓实现在应用阶段按缺席跳过。
        // 来源：上游 engine/src/processor/intents/creeps/tick.js。
        if w.creep(id)
            .is_some_and(|c| !kk_world::domain::npc::is_npc_owner(&c.owner))
            && let Some((to_room, nx, ny)) =
                crate::settle::inter_room::edge_target(&w.room.name, x, y)
        {
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
            // 官方在寿命补设之前求 portal 门（creeps/tick.js:80 先于
            // :88-91），首个活跃拍豁免传送——登记进本拍 fresh 集合。
            w.fresh_age.insert(id);
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
    // 准入以仲裁终值为准（与链取消同源），不再按当前身体二次复核；
    // 缺项视为未获准。
    if !w.movement.admitted(id) {
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
    if w.room.terrain().check_mask(
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
    // 代价槽覆盖优先（取消链后整房设置）；缺项为原生代价。
    let fatigue = w
        .movement
        .cost_of(id)
        .map(|cost| cost as i64)
        .unwrap_or(weight as i64 * fatigue_rate);
    let to_edge = target.0 == 0 || target.0 == 49 || target.1 == 0 || target.1 == 49;
    let from_edge = from.0 == 0 || from.0 == 49 || from.1 == 0 || from.1 == 49;
    w.move_obj(id, target).map_err(SettleError)?;
    // 从非边缘进入边缘时将疲劳归零；归零值随跨房过渡带到邻房。
    // 来源：上游 engine/src/processor/intents/movement.js 的 isAtEdge 判定。
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
