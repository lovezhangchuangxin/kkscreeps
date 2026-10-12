//! 常驻服务域：目录基座、身份、检查点发布/选择与宿主调度。
//!
//! S1.1 交付文件与目录基座（发布原语在 kk-storage：[`dir`]、
//! [`identity`]、[`metadata`]、[`checkpoints`]）；S1.2 交付常驻宿主
//! （[`host`] 的状态机/调度/周期保存/停止）与严格版本化运行配置
//! （[`config`]）。命令边界与 outbox 随 S2 切片接入；方案见
//! docs/plans/service-playable-batches.md。
pub mod checkpoints;
pub mod command_results;
pub mod config;
pub mod dir;
pub mod effects;
pub mod host;
pub mod identity;
pub mod inbox;
pub mod metadata;

pub use dir::ServiceDir;
pub use host::{Fault, HostParameters, Phase, ServiceHost};
pub use identity::{SERVICE_FORMAT_VERSION, ServiceIdentity};
