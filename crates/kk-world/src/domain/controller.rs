//! 查询控制器升级所需的总进度。

use kk_core::constants as c;

/// 查询 1..=7 级的升级总进度；0 级及 8 级以上返回 None。
/// 查询快照将 None 表示为 null，沙箱原始数据和结算调用方使用 0。
/// 数据来自 kk-core 的 CONTROLLER_LEVELS 常量表。
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
