//! 资源领域能力的值协议；这里只定义数据，不访问世界。
use serde::{Deserialize, Serialize};

/// 当前候选中的单资源库存与容量。遗留物仅可作为来源，容量与余量为 None。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResourceView {
    pub amount: i64,
    pub capacity: Option<i64>,
    pub free: Option<i64>,
}

/// 一次精确转账的结果；库存或容量不足时三项均为 0，不产生写入或损耗。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceTransfer {
    pub debited: i64,
    pub credited: i64,
    pub lost: i64,
}
