//! 引擎 tick 的显式阶段与运行状态机（A1 批，mod-plane-batches / 06 §2.2）。
//!
//! 一次 tick 拆为五个可注入故障的段边界：**prepare → settle → market →
//! validate → publish**。驱动（[`Engine::tick_with_intents_with_input`](crate::settle::Engine)）
//! 经 [`StageCursor`] 推进并记录段位；任一段失败 → [`RunState::NeedsRecovery`]，
//! 全部权威状态（世界/玩家候选/市场快照桥）保持上一 tick。
//!
//! 演进锚点：A2 把市场改写为真候选后，Market 段的快照桥撤销；A3 把
//! [`TickCandidates`]/[`SettledRooms`] 的所有权形态演进为 OwnedTickTxn。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::intent::IntentBatch;
use crate::settle::PlayerState;
use crate::settle::SettleError;
use crate::settle::market::MarketState;
use kk_world::RoomState;

/// 段边界（故障注入与状态机记录的最小单位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickStage {
    /// 输入规整：id 基线、玩家候选克隆、safeMode 门④、房间名序。
    Prepare,
    /// 逐房候选结算（clone → settle_room → 收集；内部另有 per-room 注入面）。
    Settle,
    /// 市场全局阶段（官方 main.js global 阶段镜像，运行在房间候选之上）。
    Market,
    /// 候选结构校验（tripwire：房间覆盖/id 单调/玩家键不减）。
    Validate,
    /// 一次发布：房间候选写回 + notify/user.rooms 落账 + id/事件/时间推进。
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

/// 引擎运行状态机（A1）。`Ready`/`Committed` 同为「可开新一轮」的净态；
/// `Running`/`Prepared` 是单次调用内的瞬态（同步引擎下不跨调用存在——
/// 跨调用见到即为状态机接线 bug，驱动显式拒绝）；段失败落
/// `NeedsRecovery` 后世界保持失败轮的上一 tick 原样。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    Ready,
    /// 上一次成功发布的终点（携带提交后的 tick）。
    Committed {
        tick: u64,
    },
    /// prepare→validate 的候选构建域（调用内瞬态）。
    Running {
        tick: u64,
    },
    /// 候选完成且通过校验、publish 之前（调用内瞬态）。
    Prepared {
        tick: u64,
    },
    /// 某段失败：世界保持在 `tick`/`generation` 记录的上一 tick 原样——
    /// 只放行「世界未被外部变异」的重试（同候选重结算）或显式放弃恢复。
    NeedsRecovery {
        tick: u64,
        generation: u64,
        stage: TickStage,
        reason: String,
    },
}

/// prepare 段产物：候选构建域的全部拥有数据（A3 OwnedTickTxn 的前身——
/// players/next_id 从引擎共享借用改为随候选流动；市场域暂留引擎侧 +
/// 快照桥，A2 真候选化后并入）。
pub(crate) struct TickCandidates {
    pub(crate) time: u64,
    pub(crate) room_names: Vec<String>,
    pub(crate) next: u64,
    pub(crate) players: BTreeMap<String, PlayerState>,
    pub(crate) batch: IntentBatch,
}

/// settle 段产物：房间候选与事件日志（房间名序）。
pub(crate) struct SettledRooms {
    pub(crate) rooms: Vec<(String, RoomState)>,
    pub(crate) events: Vec<Value>,
}

/// 段游标：记录当前段位；段入口检查故障注入；Market 段落市场快照。
pub(crate) struct StageCursor {
    fail: Option<TickStage>,
    stage: TickStage,
    pub(crate) market_snapshot: Option<MarketState>,
}

impl StageCursor {
    pub(crate) fn new(fail: Option<TickStage>) -> Self {
        Self {
            fail,
            stage: TickStage::Prepare,
            market_snapshot: None,
        }
    }

    /// 进入段：先记段位再查注入——注入 Err 的段位也已记录（NeedsRecovery
    /// 报告的 stage 即「失败发生在进入该段时」，世界在该段入口前未变异）。
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

    /// 落市场快照（进入 Market 段后、settle_market 前）。A1 候选桥：其后
    /// 任一段失败由驱动恢复快照——「任一段失败 ⇒ 市场域同样保持上一 tick」；
    /// A2 真候选化后撤销。
    pub(crate) fn snapshot_market(&mut self, market: &MarketState) {
        self.market_snapshot = Some(market.clone());
    }

    pub(crate) fn take_market_snapshot(&mut self) -> Option<MarketState> {
        self.market_snapshot.take()
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
        // 快照桥：落/取恰一次
        let market = MarketState::default();
        cursor.snapshot_market(&market);
        assert!(cursor.take_market_snapshot().is_some());
        assert!(cursor.take_market_snapshot().is_none(), "快照取走即清");
    }
}
