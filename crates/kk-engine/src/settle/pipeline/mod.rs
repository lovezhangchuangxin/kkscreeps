//! 世界 tick 候选的准备、阶段推进、写回、校验和统一发布。
//! 房间玩法规则留在 settle 的领域模块；候选内部只供本流水线使用。
mod engine_tick;
mod stages;
mod txn;
mod validation;
mod writeback;

pub(super) use stages::StageCursor;
pub use stages::{RunState, TickStage};
pub use txn::TickState;
pub use writeback::{PlayerWriteback, SegmentWriteback, WritebackStep};
