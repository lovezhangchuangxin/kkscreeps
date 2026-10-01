//! 协调玩家指令、房间规则与市场规则，推进世界的一个 tick。
//!
//! 指令模型和沙箱结果类型用于连接玩家执行器；Engine 在副本中结算，
//! 校验成功后将房间、玩家和市场状态一起写回内存。V8 执行与玩家
//! Memory、CPU 计费的提交由 kk-runtime 的 EmbedEngine 协调。

pub mod intent;
pub mod sandbox;
pub mod settle;

pub use intent::MarketIntent;
pub use sandbox::{RunOutcome, RunResult, RunSpec, Sandbox, ScriptedSandbox, WorldView};
pub use settle::market::{MarketOrder, MarketState, Transaction};
pub use settle::{Engine, PlayerState, RunState, SettleError, TickInput, TickStage, settle_room};
