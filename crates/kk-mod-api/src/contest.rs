//! 房间移动竞争策略的有界输入；优先级须可由 JS Number 精确表示。
//!
//! 输入按目的格整房一次性提供，不截断房间请求；竞争者按登记序排列。
//! 优先级只改变竞争者排序（同值回落默认四键与登记序），不开放获准
//! 集合、准入或执行效果。

use serde::Serialize;

/// JS 安全整数边界（含负数）：|priority| ≤ 2^53-1。
pub const MAX_PRIORITY: i64 = 9_007_199_254_740_991;

/// 单个竞争格中按登记序排列的一个竞争者摘要。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContenderInfo {
    pub id: String,
    pub owner: String,
    pub origin_x: u8,
    pub origin_y: u8,
    /// 原格被请求总数（默认评分第一键的原始值；对穿时第一键取 100）。
    pub origin_request_count: i64,
    /// 对穿：原格请求中存在站在本竞争格上的对象。
    pub reciprocal: bool,
    /// 被拉（默认评分第二键）。
    pub pulled: bool,
    /// 拉者（默认评分第三键）。
    pub pulling: bool,
    /// 含 boost 的 MOVE 功率。
    pub move_power: i64,
    /// 移动重量（至少 1）。
    pub weight: i64,
    /// move_power / weight（默认评分第四键）。
    pub power_ratio: f64,
}

/// 一个有多个竞争者的目的格。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContestCell {
    pub x: u8,
    pub y: u8,
    pub contenders: Vec<ContenderInfo>,
}

/// 房间移动竞争输入：全部有竞争者的目的格，不截断。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveContestInput {
    pub contests: Vec<ContestCell>,
}
