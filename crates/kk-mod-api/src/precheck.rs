//! 移动请求预检策略的有界输入；预检是集合收紧，无数值域扩展。
//!
//! 输入在全部意图登记完成后、竞争输入构造之前整房一次提供；列表中的
//! 请求均已通过原生静态预检（结构/私有敌方 rampart），原生挡下的请求
//! 不入矩阵也不进输入。`pulled/pulling` 是登记时点的原始配对标记，
//! 配对一致性复核发生在准入预填时点。地形/占位摘要供策略做区域管制
//! 类决策；坐标与表示合法性仍是宿主约束。

use serde::Serialize;

/// 整房视野中的一条移动请求登记（均已通过原生静态预检）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckRequestInfo {
    pub mover_id: String,
    pub owner: String,
    pub origin_x: u8,
    pub origin_y: u8,
    pub target_x: u8,
    pub target_y: u8,
    /// 含 boost 的 MOVE 功率（防御值 0）。
    pub move_power: i64,
    /// 移动重量（防御值 1）。
    pub weight: i64,
    /// 登记时点的被拉标记（配对复核前的原始状态）。
    pub pulled: bool,
    /// 登记时点的拉动标记。
    pub pulling: bool,
    /// 目的格地形为墙（原生仲裁障碍，无路时 native 挡）。
    pub terrain_wall: bool,
    /// 目的格地形为沼泽。
    pub terrain_swamp: bool,
    /// 目的格有路。
    pub has_road: bool,
    /// 目的格对象占位摘要（结构/工地/creep 等，与障碍槽输入同形）。
    pub objects: Vec<PrecheckObjectInfo>,
}

/// 目的格对象摘要；不携带让位预期（预检时点尚未选胜）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrecheckObjectInfo {
    pub id: String,
    /// `creep` / structureType / `constructionSite` / 类型名；Mod 对象为空串。
    pub kind: String,
    pub owner: String,
    pub is_public: bool,
    /// 工地在建结构类型；其余对象为 null。
    pub structure_type: Option<String>,
}

/// 房间移动预检输入：全部登记请求按对象 ID 升序、不截断。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePrecheckInput {
    pub requests: Vec<PrecheckRequestInfo>,
}
