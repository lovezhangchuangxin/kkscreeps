//! nuke 结算：launchNuke 的房间验证与全局跨房插入、飞行核弹的落地结算。
//!
//! 移植官方 engine/src/processor/intents/nukers/launch-nuke.js 与
//! nukes/tick.js。发射分两层（terminal send 同款标记模式）：房间意图阶段
//! [`launch_nuke`] 验证官方静默门后把请求写入 nuker 的 `launchNuke` 标记；
//! 全局阶段 [`settle_launches`] 清标记、扣库设冷却并向目标房候选插入
//! nuke 对象（目标房可以是另一间已结算房间）。落地 [`nuke_tick`] 在
//! landTime-1 那一 tick 结算，landTime 及以后移除核弹。房间状态门同官方
//!（launch-nuke.js:22-24 与 nukes/tick.js:10-14）：novice/respawn 源房禁发
//!（预检与处理器两层，closed 不拦发射），novice/respawn 房内的飞行核弹
//! 逐 tick 移除不落地。

use kk_core::RoomName;
use kk_core::constants as c;
use kk_schema::room::RoomObject;
use kk_world::{ObjId, RoomState};
use serde_json::{Value, json};

use super::SettleError;
use super::TickState;
use super::Working;
use super::damage::{apply_damage, object_hits, rampart_at};
use super::structures::{check_structure_against_controller, generic_view};
use crate::intent::IntentData;
use crate::settle::market::is_valid_room_name;

fn store_amount(data: &Value, resource: &str) -> i64 {
    data.pointer(&format!("/store/{resource}"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn capacity(data: &Value, resource: &str) -> Option<i64> {
    data.pointer(&format!("/storeCapacityResource/{resource}"))
        .and_then(Value::as_i64)
}

/// 官方射程判定：|tx-x| > NUKE_RANGE || |ty-y| > NUKE_RANGE。
/// 任一房名不可解析时返回 true（静默拒绝）。
fn out_of_range(from: &str, to: &str) -> bool {
    match (RoomName::parse(from), RoomName::parse(to)) {
        (Ok(a), Ok(b)) => {
            i64::from((a.xx - b.xx).abs()) > c::NUKE_RANGE
                || i64::from((a.yy - b.yy).abs()) > c::NUKE_RANGE
        }
        _ => true,
    }
}

/// nuker 的 launchNuke：验证官方静默门后写 data.launchNuke 标记。
/// 一切失败静默 no-op（错误码由玩家预检层产生）。来源：
/// processor/intents/nukers/launch-nuke.js。
pub fn launch_nuke(w: &mut Working, id: ObjId, d: &IntentData) {
    let Some(view) = generic_view(w, id) else {
        return;
    };
    if view.structure_type != "nuker" {
        return;
    }
    if !check_structure_against_controller(w, id, &view) {
        return;
    }
    // 官方逐资源比较 store 与 storeCapacityResource；容量键缺失时官方
    // NaN 比较为假即放行，此处以 None 跳过对应门等价实现。store 键
    // 缺失方向不等价：官方 undefined<cap 为假放行、此处按 0<cap 拒——
    // 玩家预检层先行拒绝（ghodium 0<容量），仅直注意图注入可达，从严。
    for resource in ["energy", "G"] {
        if let Some(cap) = capacity(&view.data, resource)
            && store_amount(&view.data, resource) < cap
        {
            return;
        }
    }
    if view
        .data
        .get("cooldownTime")
        .and_then(Value::as_u64)
        .is_some_and(|t| t > w.time)
    {
        return;
    }
    let (Some(x), Some(y)) = (d.x, d.y) else {
        return;
    };
    if !(0..=49).contains(&x) || !(0..=49).contains(&y) {
        return;
    }
    // 官方 launch-nuke.js:22-24：源房 novice/respawn 到期前服务端静默丢弃
    //（目标房门只在玩家面 roomStatusData 检查；飞行核弹落入保护区由
    // nuke_tick 的移除分支兜底）。
    if source_room_protected(&w.room, w.now_ms) {
        return;
    }
    let Some(target) = d.target_room_name.as_deref() else {
        return;
    };
    if !is_valid_room_name(target) {
        return;
    }
    let room = w.room.name.clone();
    if out_of_range(&room, target) {
        return;
    }
    if let Some(RoomObject::Generic(g)) = w.room.objects.get_mut(&id)
        && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
    {
        v["launchNuke"] = json!({ "roomName": target, "x": x, "y": y });
        g.data = v.to_string();
    }
}

/// 官方 truthy 判定（terminal send 同款）：null/false/0/空串为假。
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// 源房是否处于 novice/respawn 保护（官方 `room.novice/respawnArea >
/// Date.now()` 的种子域等价判定；closed 不拦发射）。
fn source_room_protected(room: &RoomState, now_ms: u64) -> bool {
    room.room_status.as_ref().is_some_and(|status| {
        matches!(
            status.active_kind(now_ms),
            Some(
                kk_world::domain::room_status::RoomStatusKind::Novice
                    | kk_world::domain::room_status::RoomStatusKind::Respawn
            )
        )
    })
}

/// 全局阶段消费 launchNuke 标记：清库、设冷却、向目标房候选插入 nuke
/// 对象。目标房不在候选（未注册房间）时静默不插——官方世界全房存在，
/// 本仓世界仅含注册房。来源：launch-nuke.js 的 bulk.update/insert 尾段。
pub fn settle_launches(
    rooms: &mut [(String, RoomState)],
    state: &TickState,
    time: u64,
    now_ms: u64,
) -> Result<(), SettleError> {
    struct Pending {
        launch_room: String,
        target: String,
        x: i64,
        y: i64,
    }
    let mut launches: Vec<Pending> = Vec::new();
    for (name, room) in rooms.iter_mut() {
        let source_protected = source_room_protected(room, now_ms);
        for obj in room.objects.values_mut() {
            let RoomObject::Generic(g) = obj else {
                continue;
            };
            // 前置字面量门（含 \u 转义藏匿的可靠兜底，见
            // kk_world::domain::literal_gate）：structureType 值为 nuker
            // 的载荷必命中；误命中只多一次解析，结果不变。
            if !kk_world::domain::literal_gate(&g.data, "\"nuker\"") {
                continue;
            }
            let Ok(mut v) = serde_json::from_str::<Value>(&g.data) else {
                continue;
            };
            if v.get("structureType").and_then(Value::as_str) != Some("nuker") {
                continue;
            }
            let Some(send) = v.get("launchNuke").cloned().filter(truthy) else {
                continue;
            };
            v["launchNuke"] = Value::Null;
            // 源房 novice/respawn：官方处理器在扣库前丢弃（launch-nuke.js:22-24），
            // 只清标记不消耗库存与冷却。
            if source_protected {
                g.data = v.to_string();
                continue;
            }
            // 官方置 0 保留键；不预置 store 键（畸形数据无库时同样清冷却
            // ——官方会物化 store{energy:0,G:0} 两键，仅手造数据可观测）。
            if v.get("store").is_some() {
                v["store"]["energy"] = json!(0);
                v["store"]["G"] = json!(0);
            }
            v["cooldownTime"] = json!(time + c::NUKER_COOLDOWN as u64);
            g.data = v.to_string();
            launches.push(Pending {
                launch_room: name.clone(),
                target: send
                    .get("roomName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                x: send.get("x").and_then(Value::as_i64).unwrap_or(-1),
                y: send.get("y").and_then(Value::as_i64).unwrap_or(-1),
            });
        }
    }
    for p in launches {
        let Some((_, target_room)) = rooms.iter_mut().find(|(n, _)| *n == p.target) else {
            continue;
        };
        if !(0..=49).contains(&p.x) || !(0..=49).contains(&p.y) {
            continue;
        }
        let id = state.allocate_id()?;
        let data = json!({
            "type": "nuke",
            "landTime": time + c::NUKE_LAND_TIME as u64,
            "launchRoomName": p.launch_room,
        });
        target_room.objects.insert(
            ObjId(id),
            RoomObject::Generic(kk_schema::room::GenericObj {
                id,
                x: p.x as u8,
                y: p.y as u8,
                data: data.to_string(),
            }),
        );
    }
    Ok(())
}

/// 官方 nukes/pretick.js 在意图阶段取消落地房间的 createCreep，
/// 只禁本拍新孵化；已有孵化仍由冲击清扫，其他 spawn 动作照常处理。
pub(super) fn blocks_spawning(room: &RoomState, time: u64) -> bool {
    room.objects.values().any(|obj| {
        let RoomObject::Generic(g) = obj else {
            return false;
        };
        let Ok(data) = serde_json::from_str::<Value>(&g.data) else {
            return false;
        };
        data.get("type").and_then(Value::as_str) == Some("nuke")
            && data
                .get("landTime")
                .and_then(Value::as_u64)
                .is_some_and(|land| Some(land) == time.checked_add(1))
    })
}

/// 飞行核弹的逐 tick 结算。来源：processor/intents/nukes/tick.js。
/// 落入 novice/respawn 房的核弹直接移除（tick.js:10-14，房间状态域）。
pub fn nuke_tick(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Generic(g)) = w.room.objects.get(&id) else {
        return;
    };
    let Ok(data) = serde_json::from_str::<Value>(&g.data) else {
        return;
    };
    let Some(land_time) = data.get("landTime").and_then(Value::as_u64) else {
        return;
    };
    if source_room_protected(&w.room, w.now_ms) {
        w.remove_obj(id);
        return;
    }
    let (nx, ny) = (g.x, g.y);
    if w.time == land_time.saturating_sub(1) {
        impact(w, id, nx, ny);
    }
    if w.time >= land_time {
        w.remove_obj(id);
    }
}

/// 落地冲击：全房清扫 + 5×5 爆炸循环 + 控制器结算。顺序与官方
/// tick.js 一致：清扫先于爆炸，事件按此顺序入日志。
fn impact(w: &mut Working, nuke_id: ObjId, nx: u8, ny: u8) {
    // 全房 forEach：creep 全灭（核弹击杀无墓碑，恒发 OBJECT_DESTROYED）、
    // 四类临时对象清除。先收集后应用，遍历期间不改对象表。
    let sweep: Vec<ObjId> = w
        .room
        .objects
        .iter()
        .filter(|(_, o)| {
            matches!(
                o,
                RoomObject::Creep(_)
                    | RoomObject::ConstructionSite(_)
                    | RoomObject::Energy(_)
                    | RoomObject::Tombstone(_)
                    | RoomObject::Ruin(_)
            )
        })
        .map(|(id, _)| *id)
        .collect();
    for oid in sweep {
        if matches!(w.room.objects.get(&oid), Some(RoomObject::Creep(_))) {
            super::creep::die_creep_nuke(w, oid);
        } else {
            w.remove_obj(oid);
        }
    }
    for obj in w.room.objects.values_mut() {
        match obj {
            // 官方对孵化中 spawn 只清 spawning 字段，不退款；被孵化的
            // creep 已在全房击杀循环移除。
            RoomObject::Spawn(s) => s.spawning = None,
            // powerCreep 官方置 hits=0（power 域形态，Generic 承载）。
            RoomObject::Generic(g) => {
                if kk_world::domain::generic::type_name(&g.data).as_deref() == Some("powerCreep")
                    && let Ok(mut v) = serde_json::from_str::<Value>(&g.data)
                {
                    v["hits"] = json!(0);
                    g.data = v.to_string();
                }
            }
            _ => {}
        }
    }

    for dx in -2i32..=2 {
        for dy in -2i32..=2 {
            let tx = nx as i32 + dx;
            let ty = ny as i32 + dy;
            if !(0..=49).contains(&tx) || !(0..=49).contains(&ty) {
                continue;
            }
            let at = (tx as u8, ty as u8);
            let range = dx.abs().max(dy.abs());
            let mut damage = if range == 0 {
                nuke_damage("0")
            } else {
                nuke_damage("2")
            };
            // 同格 rampart 先吃全量，余量按受击前 hits 抵扣；未击穿时
            // 同格其余对象不受伤害。无 hits 面（controller/source/
            // mineral/flag 等）经 object_hits=0 天然豁免，官方以
            // `!target.hits` 守卫同义。
            let mut tile: Vec<ObjId> =
                w.index
                    .at(at.0 as usize, at.1 as usize)
                    .iter()
                    .copied()
                    .filter(|oid| {
                        w.room.objects.get(oid).is_some_and(|o| {
                            object_hits(o) > 0 && !matches!(o, RoomObject::Creep(_))
                        })
                    })
                    .collect();
            if let Some(rampart) = rampart_at(w, at)
                && tile.contains(&rampart)
            {
                let rampart_hits = w.room.objects.get(&rampart).map(object_hits).unwrap_or(0);
                tile.retain(|oid| *oid != rampart);
                apply_damage(w, nuke_id, rampart, damage, c::EVENT_ATTACK_TYPE_NUKE);
                damage -= rampart_hits;
            }
            if damage > 0 {
                for oid in tile {
                    apply_damage(w, nuke_id, oid, damage, c::EVENT_ATTACK_TYPE_NUKE);
                }
            }
        }
    }

    controller_impact(w);
}

fn nuke_damage(slot: &str) -> i64 {
    c::NUKE_DAMAGE
        .iter()
        .find(|(k, _)| *k == slot)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

/// 控制器结算：激活中的 safeMode 立即结束并清冷却；有主且当前无更晚
/// upgradeBlocked 时设置核弹升级封锁。来源：nukes/tick.js 尾段。
/// EFFECT_INVULNERABILITY 仅约束无现有封锁的有主首支，不拦过期刷新或安全模式结束。
fn controller_impact(w: &mut Working) {
    let Some(ctl) = w.controller().cloned() else {
        return;
    };
    let Some(ctl_id) = w
        .room
        .objects
        .iter()
        .find_map(|(id, o)| matches!(o, RoomObject::Controller(_)).then_some(*id))
    else {
        return;
    };
    if ctl.safe_mode > w.time
        && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&ctl_id)
    {
        t.safe_mode = w.time;
        t.safe_mode_cooldown = None;
    }
    // 官方判据 `(user && !effects && !blocked) || blocked < gameTime`
    //（tick.js:70-72，&& 优先于 ||）：无主控制器的遗留过期封锁也会被
    // 刷新（unclaim 不清 upgradeBlocked），仅「无主且无封锁」不设。
    let immune = kk_world::effects::active_effect(
        w.room.effects.get(&ctl_id),
        c::EFFECT_INVULNERABILITY,
        w.time,
    )
    .is_some();
    let set_blocked = (ctl.owner.is_some() && !immune && ctl.upgrade_blocked.unwrap_or(0) == 0)
        || ctl.upgrade_blocked.is_some_and(|t| t < w.time);
    if set_blocked && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&ctl_id) {
        t.upgrade_blocked = Some(w.time + c::CONTROLLER_NUKE_BLOCKED_UPGRADE as u64);
    }
}
