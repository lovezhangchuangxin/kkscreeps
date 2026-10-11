//! 本地文件持久化原语：原子发布、信封校验与目录单写者租约。
//!
//! 本 crate 只提供“写出完整文件并使其耐久”的机制，不解释游戏规则、
//! 不依赖 runtime/V8。检查点、inbox、effects 等各写入域的格式与恢复
//! 政策由上层（kk-runtime service 等）定义；发布协议为：同目录新临时
//! 文件 → 完整写入 → 文件同步 → rename → 父目录同步 → 才记为成功。
//!
//! 检查点与分布式存储的目标设计见 docs/architecture/storage.md，
//! 服务目录布局与恢复政策见 docs/plans/service-playable-batches.md。
pub mod envelope;
pub mod lease;
pub mod publish;

pub use envelope::{EnvelopeSpec, ReadError};
pub use lease::WriterLease;
pub use publish::{
    PublishStage, StaleCleanup, publish, publish_json, remove_stale_temporaries, sync_directory,
};
