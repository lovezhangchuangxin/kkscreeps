//! 玩家代码沙箱（deno_core / V8）。
//!
//! M0 阶段交付"种子运行时"：内存双闸门、看门狗掐断（代际守卫）、CommonJS 加载器、
//! lodash 预置、console 收集、`Deno.core` 遮蔽、CPU 预算桶，以及一实例一线程的承载原型。
//! spike 实验的结论与坑见 docs/research/notes/spike-deno_core.md 与 ADR-002。

pub mod budget;
pub mod modules;
pub mod ops;
pub mod sandbox;
pub mod watchdog;

pub use budget::{Bucket, TickPlan};
pub use modules::{ModuleCode, ModuleKind, ModuleTable};
pub use ops::LogLine;
pub use sandbox::{Sandbox, SandboxConfig, TickOutcome, live_sandbox_count};
