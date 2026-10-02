//! 玩家代码沙箱：执行 CommonJS 模块和表达式，收集玩家指令、Memory 与日志。
//!
//! 沙箱通过 deno_core 持有可跨 tick 复用的 V8 实例；看门狗限制墙钟执行时间，
//! 近堆上限回调中断过量的 V8 堆分配。玩家线程与计费候选由独立模块管理。

pub mod budget;
pub mod cpu_clock;
pub mod modules;
pub mod ops;
pub mod proto;
pub mod sandbox;
pub mod shared_bundle;
pub mod watchdog;

pub use budget::{Bucket, TickPlan};
pub use modules::{ModuleCode, ModuleKind, ModuleTable};
pub use ops::LogLine;
pub use proto::{
    EvalReport, JobGate, JobToken, PendingJob, PlayerThreads, RunEpoch, RunJob, RunReceipt,
    TickReport,
};
pub use sandbox::{Sandbox, SandboxConfig, TickOutcome, lifo_violations, live_sandbox_count};
pub use shared_bundle::SharedKkpvBundle;
