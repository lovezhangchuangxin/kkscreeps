//! 包管理边界：快照字节、manifest、依赖/选择与只读模块加载。
//! Realm 消费已发现的包和装载接口；包内模块始终读取同一份快照。
mod dependencies;
mod loader;
mod manifest;
mod selection;
mod snapshot;

pub(crate) use dependencies::dependency_order;
pub(crate) use loader::{PackageModuleLoader, SDK_URL};
pub use manifest::{ModManifest, SDK_API_VERSION, Version, VersionReq};
pub(crate) use selection::resolve_selection;
pub use selection::{ContentSelection, SelectionReport};
pub use snapshot::{ModPackage, discover_packages};
