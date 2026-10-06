//! 移动代价策略的有界输入；代价是逐移动者数值替换，不是集合操作。
//!
//! 输入在取消链结算后、移动执行循环前整房一次提供；列表为终局幸存者
//! （被链取消的请求者不出现）。`nativeFatigue` 是原生代价
//! （重量×地形费率），策略可直接在其上做相对调整。道路磨损、工地
//! 踩踏与跨房疲劳归零是宿主固定执行效果，不在替换面内。

use serde::Serialize;

/// 终局幸存者的一条代价要素。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCostMoverInfo {
    pub id: String,
    pub owner: String,
    pub origin_x: u8,
    pub origin_y: u8,
    pub target_x: u8,
    pub target_y: u8,
    /// 非 MOVE/CARRY 件数加实际负载占用；与实收同源的裸值，无下限
    /// （纯 MOVE 件自重 0）。预检/准入槽的展示值另有 max(1) 下限。
    pub weight: i64,
    /// 目的格为沼泽（原生费率 10）。
    pub terrain_swamp: bool,
    /// 目的格有路（原生费率 1，覆盖沼泽）。
    pub has_road: bool,
    /// 全身部件数；道路磨损按此值计算，仅供策略参考。
    pub body_length: i64,
    /// 链首拉者 ID；被拉者的疲劳原生转记给它。无配对为 null。
    pub pulled_by: Option<String>,
    /// 原生疲劳代价（weight×费率）；未覆盖时的执行值。
    pub native_fatigue: i64,
}

/// 房间移动代价输入：终局幸存者按对象 ID 升序、不截断。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCostInput {
    pub movers: Vec<MoveCostMoverInfo>,
}
