//! 处理链的正常结局；异常仍由调用接口的 Err 返回。

use crate::Key;

/// 完成包括 wrap 主动取消内层；正常拒绝只丢弃本次动作，不能丢弃整轮。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotOutcome {
    Completed,
    Declined(Decline),
}

/// 拒绝归属取自实际处理器绑定，代码由 Mod 明确返回。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decline {
    pub binding: Key,
    pub code: String,
}
