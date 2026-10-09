//! 查询投影、沙箱投影和结算共用的对象归属、资源过滤与控制器等级规则。
//! JSON 的缺键、null 或数值缺省由各投影调用方决定。

pub mod controller;
pub(crate) mod deposit;
pub mod generic;
pub mod invader_memory;
pub mod keeper;
pub mod keeper_memory;
pub mod npc;
pub mod owner;
pub(crate) mod room;
pub mod store;

pub use controller::controller_progress_total_opt;
pub(crate) use owner::controller_owner_value;
pub use owner::object_owner;
pub(crate) use store::{apply_store_extra, store_positive_entries};
