//! 移动准入策略的有界输入；准入是布尔候选，无数值域扩展。
//!
//! 输入按仲裁选胜后的获准移动者整房一次提供；疲劳/MOVE 门槛属于可
//! 替换玩法，坐标与表示合法性仍是宿主约束。`pulled` 采用与默认判据
//! 同源的有效配对标记（配对失效按未拉处理）。

use serde::Serialize;

/// 单个待位移执行者的准入要素摘要。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoverInfo {
    pub id: String,
    pub owner: String,
    pub origin_x: u8,
    pub origin_y: u8,
    /// 仲裁选胜的目的格。
    pub target_x: u8,
    pub target_y: u8,
    /// 登记意图时的疲劳（默认门槛判据）。
    pub old_fatigue: u32,
    /// 活跃 MOVE 部件数（默认门槛要求 >0）。
    pub active_moves: i64,
    /// 含 boost 的 MOVE 功率（疲劳恢复速率）。
    pub move_power: i64,
    /// 移动重量（至少 1）。
    pub weight: i64,
    /// 有效被拉标记（默认免除疲劳与 MOVE 检查）。
    pub pulled: bool,
}

/// 房间移动准入输入：全部获准移动者，按对象 ID 序排列、不截断。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveAdmissionInput {
    pub movers: Vec<MoverInfo>,
}
