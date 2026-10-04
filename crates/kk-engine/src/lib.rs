//! 协调玩家指令、房间规则与市场规则，推进世界的一个 tick。
//!
//! Engine 接收调用方提供的指令与写回候选，在副本中结算，
//! 校验成功后将房间、玩家和市场状态一起写回内存。V8 执行与玩家
//! CPU 桶候选由 kk-runtime 计算，与 Memory 写回一同随世界候选发布。

pub mod intent;
pub mod settle;

pub use intent::MarketIntent;
pub use settle::market::{MarketOrder, MarketState, Transaction};
pub use settle::{
    Engine, PlayerState, PlayerWriteback, RunState, SegmentWriteback, SettleError, TickInput,
    TickStage, TickState, WritebackStep, settle_room,
};
