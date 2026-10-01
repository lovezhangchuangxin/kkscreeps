//! 记录 tick 的五个阶段和运行状态。
//!
//! 准备、房间结算、市场结算和校验都操作候选；发布阶段将候选写回内存。
//! 返回 Err 时保留已提交的世界、玩家和市场状态。这不覆盖 panic、进程崩溃
//! 或磁盘持久化，指令与墙钟输入的重试缓存由调用方管理。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::intent::IntentBatch;
use crate::settle::PlayerState;
use crate::settle::SettleError;
use kk_world::RoomState;

/// 一次 tick 的处理阶段，也是故障注入和错误报告的边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickStage {
    /// 复制玩家状态，确定对象 ID 起点和房间顺序，并复核安全模式指令。
    Prepare,
    /// 按房名顺序复制并结算房间。
    Settle,
    /// 使用结算后的房间候选执行全局市场规则。
    Market,
    /// 检查房间覆盖与顺序、对象 ID 游标及玩家键。
    Validate,
    /// 写回候选，保存通知，重建玩家房间集合，再推进时间与世界版本。
    Publish,
}

impl TickStage {
    pub fn label(&self) -> &'static str {
        match self {
            TickStage::Prepare => "prepare",
            TickStage::Settle => "settle",
            TickStage::Market => "market",
            TickStage::Validate => "validate",
            TickStage::Publish => "publish",
        }
    }
}

/// 引擎的运行状态。Ready 和 Committed 可以开始下一轮。
/// Running、Prepared 仅在同步结算调用中使用；若 panic 后残留，后续推进被拒绝。
/// NeedsRecovery 允许世界时间和版本未改变时再次结算，或显式放弃恢复。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Ready,
    /// 上一次成功发布的终点（携带提交后的 tick）。
    Committed {
        tick: u64,
    },
    /// 正在构造和处理候选，tick 是本次结算前的世界时间。
    Running {
        tick: u64,
    },
    /// 候选已通过校验，尚未写回世界。
    Prepared {
        tick: u64,
    },
    /// 结算返回错误后的状态，记录当时世界时间、版本和失败阶段。
    /// 此状态不保存指令或 TickInput，也不检查玩家、市场是否被外部修改；
    /// 重试时调用方须重新提供相同输入并保持这些状态不变。
    NeedsRecovery {
        tick: u64,
        generation: u64,
        stage: TickStage,
        reason: String,
    },
}

/// 准备阶段产生的输入、玩家副本及对象 ID 游标。
/// 房间和市场副本分别由后续阶段创建，直到发布前都不写回引擎。
pub(crate) struct TickCandidates {
    pub(crate) time: u64,
    pub(crate) room_names: Vec<String>,
    pub(crate) next: u64,
    pub(crate) players: BTreeMap<String, PlayerState>,
    pub(crate) batch: IntentBatch,
}

/// 按房名顺序保存结算后的房间副本与事件。
pub(crate) struct SettledRooms {
    pub(crate) rooms: Vec<(String, RoomState)>,
    pub(crate) events: Vec<Value>,
}

/// 记录当前阶段，并在进入指定阶段时注入错误。
pub(crate) struct StageCursor {
    fail: Option<TickStage>,
    stage: TickStage,
}

impl StageCursor {
    pub(crate) fn new(fail: Option<TickStage>) -> Self {
        Self {
            fail,
            stage: TickStage::Prepare,
        }
    }

    /// 先记录阶段再检查故障注入，使错误报告包含尚未执行的失败阶段。
    pub(crate) fn enter(&mut self, stage: TickStage) -> Result<(), SettleError> {
        self.stage = stage;
        if self.fail == Some(stage) {
            return Err(SettleError(format!(
                "故障注入：{} 段失败（set_fail_stage_for_test）",
                stage.label()
            )));
        }
        Ok(())
    }

    pub(crate) fn stage(&self) -> TickStage {
        self.stage
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 段入口注入：命中段 Err 且段位已记录；未命中段放行。
    #[test]
    fn cursor_injects_at_entered_stage() {
        let mut cursor = StageCursor::new(Some(TickStage::Market));
        cursor.enter(TickStage::Prepare).unwrap();
        cursor.enter(TickStage::Settle).unwrap();
        let err = cursor.enter(TickStage::Market).unwrap_err();
        assert!(err.to_string().contains("market"), "{err}");
        assert_eq!(cursor.stage(), TickStage::Market, "注入段位已记录");
    }
}
