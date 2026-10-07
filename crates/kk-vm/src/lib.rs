//! V8 执行的通用资源机制，不包含游戏规则、玩家 API 或 Mod SDK。
pub mod budget;
pub mod cpu_clock;
pub mod isolate_order;
pub mod watchdog;

#[cfg(feature = "test-support")]
pub mod test_support;
