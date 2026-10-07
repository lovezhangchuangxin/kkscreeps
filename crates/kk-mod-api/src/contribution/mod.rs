//! 贡献计划的声明、合并、排序、冻结与身份；不访问世界或执行处理器。
//! 领域目录及宿主消费者使用 crate 根的公共契约，构建步骤只在本模块内协作。
mod definition;
mod fingerprint;
mod freeze;
mod handlers;
mod plan;
mod rules;

pub(crate) use definition::valid_namespace;
pub use definition::*;
pub use fingerprint::domain_digest;
pub use freeze::{FreezeError, freeze, freeze_with_builtin};
pub use plan::*;
