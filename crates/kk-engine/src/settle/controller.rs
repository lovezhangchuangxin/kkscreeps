//! controller 对象指令（官方 `engine/src/processor.js:306-311` 对 controller
//! 类型分派 unclaim/activateSafeMode——unclaim 先序）。B2 落地
//! activateSafeMode；A4-iii 补 unclaim（B2 时未进 24 动作清单，白名单外
//! 动作在清洗层丢弃——intent.rs 已补 Action::Unclaim）。
//!
//! activateSafeMode 是两步链的**意图半步**：此处只做门槛检查并挂
//! `_safeModeActivated` 瞬态标记，真正的 safeMode 时钟/消耗/冷却在
//! controller tick 消费（controllers/tick.js:25-31，见 economy::controller_tick）。
//! 两步之间存在同 tick 攻击窗口——attackController 写入的 `_upgradeBlocked`
//! 会被 tick.js:20-23 先持久化、:25 的激活门随即读到它而取消激活（官方
//! 顺序语义，非缺陷）。
//!
//! API 层预检（structures.js:210-228 activateSafety 的 NOT_OWNER/
//! NOT_ENOUGH_RESOURCES/TIRED/BUSY 四门与全房单 activateSafeMode intent 的
//! lastActivateSafeMode 去重）属 prelude（轨 A）；BUSY 门（"自己任一房的
//! safeMode 生效中不得再开"）跨房可见，结算层按官方处理器同样只查本房。

use std::collections::BTreeMap;

use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::Working;
use crate::intent::{Action, IntentData};

/// 官方 processor.js:306-311：controller 对象的 intent 分派（unclaim 先于
/// activateSafeMode——同 tick 两者并存时 unclaim 归零后 activateSafeMode 的
/// `!object.user || !object.level` 再拒，官方同序）。
pub fn process_controller_intents(
    w: &mut Working,
    id: ObjId,
    actions: &BTreeMap<Action, IntentData>,
) {
    if actions.contains_key(&Action::Unclaim) {
        intent_unclaim(w, id);
    }
    if actions.contains_key(&Action::ActivateSafeMode) {
        intent_activate_safe_mode(w, id);
    }
}

/// 官方 unclaim.js（processor/intents/controllers/unclaim.js）：主动弃房。
/// `!object.user || !object.level` 早退后写库：user=null、level=0、progress=0、
/// downgradeTime=null、safeMode=null、safeModeAvailable=0、冷却
/// gameTime+SAFE_MODE_COOLDOWN（roomInfo.novice 免冷却路径——我方无新手区，
/// 恒挂钟）、isPowerEnabled=false。
///
/// 与降级归零（controllers/tick.js:52-63）的两处刻意差异（官方原样）：
/// - **不清 reservation/sign/upgradeBlocked**——unclaim.js 的 bulk update
///   键表无此三项（降级路径清 upgradeBlocked）；已占房的 reservation 本就
///   为空（claim 落地即清），sign 跨 unclaim 存续是玩家可观察语义；
/// - `user = null` 落库哨兵——我方 owner=None + owner_cleared=true（A3b
///   三态：玩家侧 my === false，区别于从未拥有）。
///
/// removeRoomFromUser（:15）由 Engine tick 末 rooms 重建承载（settle/mod.rs
/// PlayerState 文档）。
fn intent_unclaim(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
    // :9-12 `!object.user || !object.level`——无主/0 级不可弃
    if t.owner.is_none() || t.level == 0 {
        return;
    }
    if let Some(RoomObject::Controller(t)) = w.room.objects.get_mut(&id) {
        t.owner = None;
        t.owner_cleared = true;
        t.level = 0;
        t.progress = 0;
        t.downgrade_time = None;
        t.safe_mode = 0;
        t.safe_mode_available = 0;
        t.safe_mode_cooldown = Some(w.time + kk_core::constants::SAFE_MODE_COOLDOWN as u64);
        t.is_power_enabled = false;
    }
}

/// 官方 activateSafeMode.js:6-25：五道门槛全过后挂激活标记。
///
/// JS null/undefined 强制转换的边界（数值比较中 null → 0）逐条对齐：
/// - `safeModeCooldown >= gameTime`（:14）：官方写库只有两形——数值或 null
///   （新手区免冷却路径，无新手区的私服恒为数值）。null 时 `0 >= gameTime`
///   仅 gameTime == 0 的首 tick 为真（拒绝）。我方 None 统一按 null 语义
///   （unwrap_or(0)）；世界生成初态若为 undefined（字段缺席）官方放行——
///   值此分歧仅在 gameTime 0 且 available ≥ 1 的放置态可达，登记报告；
/// - `upgradeBlocked > gameTime`（:17）：null/undefined → 0 > t 恒假，放行；
/// - `downgradeTime < gameTime + DOWNGRADE[level]/2 − THRESHOLD`（:20）：
///   未挂钟（新 claim 的房，钟在同 tick 的 controller tick 才初始化）时
///   null → 0，除 gameTime 为 0 的角落外恒小于右式——拒绝（官方语义：新占
///   房当 tick 不可开 safeMode）。
fn intent_activate_safe_mode(w: &mut Working, id: ObjId) {
    let time = w.time;
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
    let (owner, level, available, cooldown, blocked, downgrade_time) = (
        t.owner.clone(),
        t.level,
        t.safe_mode_available,
        t.safe_mode_cooldown,
        t.upgrade_blocked,
        t.downgrade_time,
    );
    // :8 `!object.user || !object.level`——无主/0 级不可激活
    if owner.is_none() || level == 0 {
        return;
    }
    // :11 `!(object.safeModeAvailable > 0)`
    if available == 0 {
        return;
    }
    // :14 `object.safeModeCooldown >= gameTime`（null → 0，见函数注释）
    if cooldown.unwrap_or(0) >= time {
        return;
    }
    // :17 `object.upgradeBlocked > gameTime`（null → 0）
    if blocked.unwrap_or(0) > time {
        return;
    }
    // :20 `downgradeTime < gameTime + DOWNGRADE[level]/2 −
    // CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD`（null → 0；level ≥ 1 时右式
    // 恒 ≥ 0，i64 减法无下溢——DOWNGRADE/2 最小 5000）
    let threshold = super::economy::controller_downgrade(level) as i64 / 2
        - kk_core::constants::CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD;
    if (downgrade_time.unwrap_or(0) as i64) < time as i64 + threshold {
        return;
    }
    // :24 `object._safeModeActivated = 1`——瞬态，controller tick 消费
    w.safe_mode_activated.insert(id);
}
