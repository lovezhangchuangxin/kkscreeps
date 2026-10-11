//! 常驻服务域：目录基座、身份、检查点发布/选择与（后续切片的）宿主调度。
//!
//! 现行范围（S1.1）：文件与目录基座——发布原语在 kk-storage，本模块定义
//! 服务目录布局（[`dir`]、目录身份 [`identity`]、检查点内服务元数据
//! [`metadata`] 与无日志恢复选择 [`checkpoints`]）。宿主调度、命令边界
//! 与 outbox 随 S1.2/S2 切片接入；方案见
//! docs/plans/service-playable-batches.md。
pub mod checkpoints;
pub mod dir;
pub mod identity;
pub mod metadata;

pub use dir::ServiceDir;
pub use identity::{SERVICE_FORMAT_VERSION, ServiceIdentity};
