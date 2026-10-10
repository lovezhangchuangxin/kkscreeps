//! invaderCore 的坍塌/孵化生命周期与 NPC 预定意图；布防/raid 调度与
//! 战术 AI（refillTowers/focusClosest/maintainPopulation）另片接线。
use super::{SettleError, Working};
use kk_core::constants as c;
use kk_schema::room::{Reservation, RoomObject};
use kk_world::{ObjId, domain::npc::INVADER_OWNER};
use serde_json::{Value, json};

use crate::intent::{Action, IntentData, UserIntents};

/// 仅 Invader 归属的 invaderCore 参与预意图与结构意图处理；归属由
/// 放置强制写为保留用户名，发布校验拒绝其他归属。
fn is_invader_core(data: &str) -> bool {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| {
            if value.get("structureType").and_then(Value::as_str) != Some("invaderCore") {
                return None;
            }
            Some(value.get("owner").and_then(Value::as_str)? == INVADER_OWNER)
        })
        .unwrap_or(false)
}

/// 坍塌到期先清空控制器（官方 invader-core/tick.js 首段重置清单：
/// owner/level/progress/downgradeTime/safeMode/safeModeAvailable/
/// safeModeCooldown/isPowerEnabled/effects；upgradeBlocked、sign 与
/// reservation 不在清单，保持原值），随后整段返回不再孵化。本体移除
/// 由对象循环的通用坍塌门执行（无 ruin、无事件）。
pub(super) fn tick(w: &mut Working, id: ObjId) -> Result<(), SettleError> {
    let Some(RoomObject::Generic(core)) = w.room.objects.get(&id) else {
        return Ok(());
    };
    let Ok(mut data) = serde_json::from_str::<Value>(&core.data) else {
        return Ok(());
    };
    let host = (core.x, core.y);
    let collapse_expired = w.room.effects.get(&id).is_some_and(|entries| {
        entries
            .iter()
            .any(|e| e.effect == Some(c::EFFECT_COLLAPSE_TIMER) && e.end_time <= w.time)
    });
    if collapse_expired {
        let ctl_id = w
            .room
            .objects
            .iter()
            .find_map(|(tid, obj)| matches!(obj, RoomObject::Controller(_)).then_some(*tid));
        if let Some(ctl_id) = ctl_id
            && let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&ctl_id)
        {
            t.owner = None;
            t.owner_cleared = true;
            t.level = 0;
            t.progress = 0;
            t.downgrade_time = None;
            t.safe_mode = 0;
            t.safe_mode_available = 0;
            t.safe_mode_cooldown = None;
            t.is_power_enabled = false;
        }
        // 官方重置含 effects:null；控制器自身效果条目随清空移除。
        if let Some(ctl_id) = ctl_id {
            w.room.effects.remove(&ctl_id);
        }
        return Ok(());
    }
    // 布防部署（官方 pretick 的 'deploy' 行为）：模板核心到期铺场。官方
    // deploy 在房间 pretick、先于玩家意图执行；kk 在对象 tick 内接线，
    // 晚于意图与移动仲裁——部署当拍从布防格移出的 creep 得以逃逸（窗口
    // 限部署拍与模板脚印，登记差异）；坍塌检查先行——到期时官方两侧都
    // 会跑但部署随即被移除覆盖，此处保持先坍塌后部署，正常世界两者不
    // 同时到期。
    let deploy_due = data
        .get("deployTime")
        .and_then(Value::as_u64)
        .is_some_and(|d| d <= 1 + w.time)
        && data.get("templateName").and_then(Value::as_str).is_some();
    if deploy_due {
        super::stronghold::deploy(w, id, &mut data)?;
    }
    // 孵化到期（官方 gameTime >= spawnTime-1）：经通用出生落格并清
    // spawning；被围时按官方 spawnTime = 1+gameTime 顺延重试。
    if let Some(spawning) = data.get("spawning")
        && let Some(spawn_time) = spawning.get("spawnTime").and_then(Value::as_u64)
        && w.time >= spawn_time.saturating_sub(1)
    {
        let name = spawning
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let born = super::spawn::born_creep_at(w, host, INVADER_OWNER, &name, &[]);
        if born {
            data["spawning"] = Value::Null;
        } else {
            data["spawning"]["spawnTime"] = json!(1 + w.time);
        }
    }
    if let Some(RoomObject::Generic(core)) = w.room.objects.get_mut(&id) {
        core.data = data.to_string().into();
    }
    Ok(())
}

/// 官方 invader-core/reserveController.js：执行者是 invaderCore 结构，
/// 无邻接/部件门（结构臂的所有权门已限定 NPC 执行者）；新预定从
/// gameTime+1 起步，常量加成 INVADER_CORE_CONTROLLER_POWER×
/// CONTROLLER_RESERVE，上限 CONTROLLER_RESERVE_MAX。
pub(super) fn reserve_controller(w: &mut Working, id: ObjId, data: &IntentData) {
    let owner = match w.room.objects.get(&id) {
        Some(RoomObject::Generic(g)) => {
            let Ok(value) = serde_json::from_str::<Value>(&g.data) else {
                return;
            };
            if value.get("structureType").and_then(Value::as_str) != Some("invaderCore") {
                return;
            }
            value
                .get("owner")
                .and_then(Value::as_str)
                .unwrap_or(INVADER_OWNER)
                .to_string()
        }
        _ => return,
    };
    let Some(target_id) = data.id else {
        return;
    };
    let (ctl_owner, reservation) = match w.room.objects.get(&target_id) {
        Some(RoomObject::Controller(t)) => (t.owner.clone(), t.reservation.clone()),
        _ => return,
    };
    if ctl_owner.is_some() || reservation.as_ref().is_some_and(|r| r.user != owner) {
        return;
    }
    let effect = c::INVADER_CORE_CONTROLLER_POWER * c::CONTROLLER_RESERVE;
    let base_end = reservation.as_ref().map_or(w.time + 1, |r| r.end_time);
    if base_end + effect as u64 > w.time + c::CONTROLLER_RESERVE_MAX as u64 {
        return;
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&target_id) {
        t.reservation = Some(Reservation {
            user: owner,
            end_time: base_end + effect as u64,
        });
    }
    // 官方 invaderCore 模板（processor.js:74-80）含 reserveController 键；
    // 坐标=控制器。
    if let Some((tx, ty)) = w.room.objects.get(&target_id).map(|o| o.pos()) {
        w.record_action(
            id,
            "reserveController",
            super::action_log::ActionLogPayload::Coords { x: tx, y: ty },
        );
    }
    w.push_event(c::EVENT_RESERVE_CONTROLLER, id, json!({ "amount": effect }));
}

/// 官方 stronghold.js handleController 的 default/deploy 共用首段。本片
/// 只发射 neutral/己方预定的 reserveController；核心自有控制器的
/// upgradeController 与他人预定的 attackController 分支留待布防/战术片
/// 接线处理器后再发射，避免静默丢弃造成可观察行为分歧。
/// 多核心同房时对每个核心发射（官方 processor.js 以单变量取最后一个
/// 核心、只跑一个 pretick；正常世界每房至多一个核心，差异仅在夹具）。
pub(super) fn intents(w: &Working) -> UserIntents {
    let mut out = UserIntents::default();
    let Some(ctl) = w.controller() else {
        return out;
    };
    if ctl.owner.is_some()
        || ctl
            .reservation
            .as_ref()
            .is_some_and(|r| r.user != INVADER_OWNER)
    {
        return out;
    }
    let Some(ctl_id) = w
        .room
        .objects
        .iter()
        .find_map(|(tid, obj)| matches!(obj, RoomObject::Controller(_)).then_some(*tid))
    else {
        return out;
    };
    for (id, obj) in w.room.objects.iter() {
        if let RoomObject::Generic(g) = obj
            && is_invader_core(&g.data)
        {
            out.objects
                .entry(*id)
                .or_default()
                .insert(Action::ReserveController, IntentData::target(ctl_id));
        }
    }
    out
}
