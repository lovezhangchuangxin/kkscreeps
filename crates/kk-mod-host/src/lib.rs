//! 可信 JS Mod 宿主的装载与注册期：包发现、依赖排序、指纹、
//! 注册 realm 与冻结计划产出。
//!
//! 边界：本 crate 只做注册——声明收集、键归属校验、绑定函数登记与
//! `ContributionPlan` 冻结。处理器在结算期的执行、玩家 API 装载与
//! 生产引擎接线不在其中；无 Mod 的世界不引用本 crate，默认路径零开销。
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

mod convert;
mod error;
mod graph;
mod manifest;
mod package;
mod realm;

pub use error::{ModHostError, ModHostResult};
pub use manifest::{ModManifest, SDK_API_VERSION, Version, VersionReq};
pub use package::{ModPackage, discover_packages};
pub use realm::{ModRealm, RegisterOptions, Registered};
