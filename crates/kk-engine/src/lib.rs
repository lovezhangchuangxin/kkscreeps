//! 游戏循环与规则结算（M1 起的主体工程）。
//!
//! M0 阶段定义**嵌入式 API**（`embed` 模块）：以库形式驱动引擎的接口——
//! 建分片、摆放对象、执行玩家代码、推进 tick、读取世界状态。
//! 它同时是 screeps-ok 适配器（compat/）与 `kkscreeps sim` 共用的同一接口（ADR-008 第 8 条）。

pub mod embed;
pub mod intent;
pub mod sandbox;
pub mod settle;

pub use embed::{EmbedEngine, PlayerRun};
pub use sandbox::{RunOutcome, RunResult, RunSpec, Sandbox, ScriptedSandbox, WorldView};
pub use settle::{Engine, PlayerState, SettleError, TickInput, settle_room};
