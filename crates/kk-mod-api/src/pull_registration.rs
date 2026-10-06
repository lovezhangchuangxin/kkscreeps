//! 拉拽登记策略的有界输入；登记策略是集合收紧，无数值域扩展。
//!
//! 输入在全部拉拽意图登记完成后、移动请求预检之前整房一次提供；
//! 列表中的配对均已通过原生登记校验（相邻、非孵化、循环链拒绝）。
//! 配对内每个拉者至多一个有效目标（重复登记后到覆盖先到，映射保持
//! 双向一致）；宿主按拉者键应用否决，被否决配对等同从未登记。

use serde::Serialize;

/// 整房视野中的一条有效拉拽配对。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullPairInfo {
    pub puller_id: String,
    pub target_id: String,
    /// 拉者与被拉者当前位置；登记时点已相邻（切比雪夫距离 ≤1）。
    pub puller_x: u8,
    pub puller_y: u8,
    pub target_x: u8,
    pub target_y: u8,
    pub puller_owner: String,
    pub target_owner: String,
}

/// 房间拉拽登记输入：全部有效配对按拉者对象 ID 升序、不截断。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRegistrationInput {
    pub pulls: Vec<PullPairInfo>,
}
