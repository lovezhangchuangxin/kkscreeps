//! controller 领域语义：进度总量查表（三消费面共核）。

use kk_core::constants as c;

/// `controller_progress_total`（snapshot.rs/world_json.rs 各自的 folding）
/// 的共核（M2B P4 单源）：1..7 级查
/// CONTROLLER_LEVELS；0/8 级无进度条。消费侧各自折叠——快照 Null 兜底
///（契约面）、世界投影 0 兜底（prelude 数值面）、结算侧 upgrade 量 0 兜底
///（economy/creep controller 的 controller_levels——评审 3 P2-1 勘测盲区
/// 收编：表仅含 "1".."7"，旧结算版无守卫但 miss→0 与 opt 折叠全域等价）。
pub fn controller_progress_total_opt(level: u8) -> Option<i64> {
    if level == 0 || level >= 8 {
        return None;
    }
    let key = level.to_string();
    c::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
}
