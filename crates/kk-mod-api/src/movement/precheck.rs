//! 移动请求预检策略输入：Hosted 路径含原生静态拒绝的暂存请求。
//!
//! 全部意图登记后、竞争前整房一次提供；登记时拒绝快照与当前静态
//! 初判分列，避免对象后续变异改变原生回退。拉拽标记在配对复核前
//! 提供；动态让位、坐标与对象有效性仍由宿主复核。

use serde::Serialize;

/// 整房视野中的一条移动请求登记（可含原生静态拒绝的请求）。
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
    /// 登记时结构/私有 rampart 的原生拒绝快照。
    pub native_registration_blocked: bool,
    /// 当前静态障碍初判（含墙/己方工地，不含 creep 占格）。
    pub native_static_blocked: bool,
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
