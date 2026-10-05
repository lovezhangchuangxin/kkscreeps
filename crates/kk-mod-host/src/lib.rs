//! 可信 JS Mod 宿主：包发现、依赖排序、指纹、注册冻结与整槽执行。
//!
//! 边界：注册期声明校验与冻结，结算期 JS 整槽组合；Rust 候选访问通过
//! ChainHost 注入，不依赖 kk-engine。玩家 API 装载由 kk-runner 负责。
//!
//! 信任模型：全部 Mod 共享一个可信 realm（服主安装的代码等同服务器扩展），
//! 注册期 SDK 没有世界读写面；玩家隔离由 runner/投影层负责，与此处无关。
//!
//! ```
//! use kk_mod_host::discover_packages;
//!
//! let missing = discover_packages(std::path::Path::new("does-not-exist"));
//! assert!(missing.is_err());
//! ```

mod chain;
mod composition;
mod convert;
mod error;
mod execution_plan;
mod graph;
mod limits;
mod manifest;
mod package;
mod profile;
mod realm;
mod v8_convert;
mod write;

pub use chain::{ChainContext, ChainHost, ChainTarget};
pub use error::{ModFault, ModFaultKind, ModHostError, ModHostResult};
pub use limits::{ModExecutionLimits, ModVmLimits};
pub use manifest::{ModManifest, SDK_API_VERSION, Version, VersionReq};
pub use package::{ModPackage, discover_packages};
pub use profile::{ChainProfiler, ProfileSample};
pub use realm::{ModRealm, PlayerModulePackage, RegisterOptions, Registered};
pub use write::ChainWrite;

pub use composition::{ComposedSlot, CompositionReport, ExecutionMode, RuleSelection};
pub use execution_plan::ExecutionChain;
