//! 仲裁障碍准入槽的纯数据输入：胜者目的格的对象/地形摘要。
//!
//! 输入在准入槽之后、阻塞链取消之前构造；native 初判反映预填时点
//! （全部胜者未取消）的静态判定，链内让位取消可能使实际判定收紧。

use serde::Serialize;

/// 胜者目的格上的单个对象摘要。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObstacleObjectInfo {
    pub id: String,
    /// creep / structureType / constructionSite 等类型名，Mod 对象为空串。
    pub kind: String,
    /// 归属玩家；无主对象为空串。
    pub owner: String,
    /// rampart 公开性；其余结构为 false。
    pub is_public: bool,
    /// 工地专用：在建结构类型；其余对象为 null。
    pub structure_type: Option<String>,
    /// creep / powerCreep 专用：胜者且准入终值为真（准入被拒的胜者必被链内取消，
    /// 让位预期不成立）。
    pub yielding: bool,
}

/// 单个胜者的障碍判定要素。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObstacleMoverInfo {
    pub id: String,
    pub owner: String,
    pub target_x: u8,
    pub target_y: u8,
    /// 目的格地形为墙（无路时 native 阻挡）。
    pub terrain_wall: bool,
    pub has_road: bool,
    /// 目的格上的对象摘要（不含移动者自身，其站在原格）。
    pub objects: Vec<ObstacleObjectInfo>,
    /// 预填时点的 native 初判；链内让位取消只会收紧不会放宽。
    pub native_blocked: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveObstacleInput {
    /// 当前结算时点有效的安全模式主人；未激活、已到期或无主时为 null。
    /// 与 native 初判、链预演及正式取消链读取同一个主人。
    pub safe_mode_owner: Option<String>,
    /// 按对象 ID 升序；含全部胜者（准入终值已定，被拒者仍在列）。
    pub movers: Vec<ObstacleMoverInfo>,
}
