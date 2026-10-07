//! 处理控制器放弃所有权和安全模式激活指令。
//!
//! 激活分为指令检查与 controller tick 两步；标记通过不代表本轮最终激活。

use std::collections::BTreeMap;

use kk_schema::room::RoomObject;
use kk_world::ObjId;

use super::Working;
use crate::intent::{Action, IntentData};

/// 先放弃房间，再处理安全模式激活。两者同轮存在时，放弃会使激活条件不成立。
/// 来源：上游 engine/src/processor.js 的 controller 指令分派。
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

/// 清除控制器所有权、等级和安全模式，保留签名、预定和升级封锁。
/// owner_cleared 区分主动清除与从未拥有；玩家房间集合由 Engine 提交时重建。
/// 不模拟新手区免冷却，始终设置 SAFE_MODE_COOLDOWN。
/// 来源：上游 engine/src/processor/intents/controllers/unclaim.js。
fn intent_unclaim(w: &mut Working, id: ObjId) {
    let Some(RoomObject::Controller(t)) = w.room.objects.get(&id) else {
        return;
    };
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

/// 通过本房间检查后仅登记激活请求，controller_tick 才扣次数并设置时钟。
/// 期间 attackController 设置的封锁仍可阻止激活。跨房间已有安全模式的
/// 检查在 Engine::prepare_tick 完成。
/// 来源：上游 engine/src/processor/intents/controllers/activateSafeMode.js
/// 及 engine/src/processor/intents/controllers/tick.js。
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
    if owner.is_none() || level == 0 {
        return;
    }
    if available == 0 {
        return;
    }
    // 冷却/封锁缺失即不阻止：上游 JS 里 undefined 参与比较恒为 false
    //（`undefined >= gameTime` 不成立），None 不能按 0 数值化——否则
    // 世界时间 0 时缺失冷却会拒绝首次激活。
    if let Some(c) = cooldown
        && c >= time
    {
        return;
    }
    if let Some(b) = blocked
        && b > time
    {
        return;
    }
    // 降级时钟缺失同样不阻止（undefined 比较为 false）；有值时低于
    // 阈值则拒绝。已知角落差异：官方 claim 落 null（数值化 0 会拒），
    // 本实现 claim 落 None——原始 intent 序可达时同批激活语义不同，
    // 官方玩家面 claim 非己方挡在 API 层，不视作阻塞。
    let threshold = super::economy::controller_downgrade(level) as i64 / 2
        - kk_core::constants::CONTROLLER_DOWNGRADE_SAFEMODE_THRESHOLD;
    if let Some(dt) = downgrade_time
        && (dt as i64) < time as i64 + threshold
    {
        return;
    }
    w.safe_mode_activated.insert(id);
}
