//! 移动准入策略的有界输入；准入是布尔候选，无数值域扩展。
//!
//! 输入按仲裁选胜后的获准移动者整房一次提供；疲劳/MOVE 门槛属于可
//! 替换玩法，坐标与表示合法性仍是宿主约束。`pulled`/`pulledBy` 采用
//! 与默认判据同源的有效配对标记（配对失效按未拉处理）；`pulling` 是
//! 拉者侧登记态（目标落选时本轮不生效）。
//! `requests` 是整房原始请求视野：竞争落选者也可见，供策略按全房
//! 拓扑（让位链、交换、竞争）做获准集合决策。

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
    /// 有效拉拽配对的拉者对象 ID；配对失效按未拉处理（与 `pulled`
    /// 同点复核），未被有效拉动为 null。
    pub pulled_by: Option<String>,
    /// 登记的拉动目标对象 ID；目标竞争落选时配对残标不从拉者侧
    /// 复核（本轮拉动不生效），未在拉动为 null。
    pub pulling: Option<String>,
}

/// 整房视野中的一条原始移动请求登记（含竞争落选者）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveRequestInfo {
    pub mover_id: String,
    /// 请求的目的格。
    pub x: u8,
    pub y: u8,
    /// 格内登记序（同格竞争时的先后依据）。
    pub index: u64,
    pub origin_x: u8,
    pub origin_y: u8,
    /// 该登记者是否为其目的格的仲裁胜者。
    pub won: bool,
}

/// 房间移动准入输入：全部获准移动者按对象 ID 序排列、不截断；
/// `requests` 是仲裁前的完整原始请求矩阵（按目的格与格内登记序），
/// 落选者带 `won=false`。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveAdmissionInput {
    pub movers: Vec<MoverInfo>,
    pub requests: Vec<MoveRequestInfo>,
}
