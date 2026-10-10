//! 性能命令族：共享采样能力独立于各负载。
mod comparison;
mod comparison_mature;
mod metadata;
pub(super) mod mod_chain;
pub(super) mod mod_player;
mod process;
pub(super) mod workload;

pub(super) use comparison::run as run_comparison;
use metadata::metadata;
