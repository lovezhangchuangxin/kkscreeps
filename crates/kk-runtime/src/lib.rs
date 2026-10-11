//! 协调玩家沙箱执行与世界结算，为嵌入式调用方提供运行接口。
//!
//! 执行器返回候选指令、Memory 和用量报告；协调器计算 CPU 桶候选，
//! 与 Memory、segments、世界一起发布。结算失败缓存候选，重试不再次执行代码。
//! 玩家 V8 全局变量不随世界回退；执行阶段线程故障阻止自动重派，需要显式处置。
//! Mod VM 在结算返回错误后销毁，按固定启动快照恢复后才允许下一次执行。

pub mod checkpoint;
pub mod embed;
pub mod kkpv_store;
mod mod_trace;
mod mod_vm;
mod run;
pub mod service;
pub mod world_source;

pub use embed::{EmbedEngine, PlayerRun};
pub use kk_runner::RunEpoch;
pub use mod_trace::{ModTraceEntry, ModTraceEvent};
pub use mod_vm::{ModAutoDisable, ModFaultStreak, ModSettlementFault};
pub use run::{RunOutcome, RunResult, RunSpec};
pub use world_source::WorldSource;

mod executor;
pub use executor::{PendingExecution, PlayerExecutor, ThreadExecutor};
