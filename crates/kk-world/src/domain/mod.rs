//! 领域语义模块（M2B 扩面 P0，O2 落位）：从投影模块（snapshot.rs/
//! world_json.rs）纯搬移的共核 helper 家族——两投影与结算侧消费的同一
//! 领域语义单源（B1 纪律：零行为，token 级对账）。M2B 扩面新增的投影
//! 折叠（ticksToX/cooldown/destination/容量族）一律落本目录（m2b-expansion
//! 方案 §2——新增 ~190 getter 时一次性归属到位，免两次搬）。

pub mod controller;
pub mod owner;
pub mod store;

pub use controller::controller_progress_total_opt;
pub(crate) use owner::controller_owner_value;
pub use owner::object_owner;
pub(crate) use store::{apply_store_extra, store_positive_entries};
