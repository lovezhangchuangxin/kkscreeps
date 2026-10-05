//! 玩法定义在启动时合并、校验和冻结；本 crate 不执行处理器或读取世界。
//!
//! [`builtin::link`] 提供内置 link 描述和仓库容量规则；消费者由引擎装配。
//! 冻结后的整数句柄只适用于生成它们的计划，不能作为持久化身份。
//!
//! ```
//! use kk_mod_api::{builtin, freeze_with_builtin, FreezeOptions, Key, Number};
//! let plan = freeze_with_builtin(builtin::link(), vec![], FreezeOptions::default())?;
//! let key = Key::new("builtin:link.capacity").expect("固定键合法");
//! let handle = plan.rules().resolve(&key).expect("link 容量已注册");
//! assert_eq!(plan.rules().get(handle), Some(&Number::Signed(800)));
//! # Ok::<(), kk_mod_api::FreezeError>(())
//! ```

pub mod admission;
pub mod buildings;
pub mod builtin;
pub mod combat;
pub mod contest;
mod definition;
pub mod extension;
mod fingerprint;
mod freeze;
mod handlers;
pub mod incoming;
pub mod objects;
pub mod obstacle;
mod outcome;
mod plan;
pub mod player_surface;
pub mod resources;
mod rules;
pub mod validate;

pub use definition::*;
pub use freeze::{FreezeError, freeze, freeze_with_builtin};
pub use outcome::{Decline, SlotOutcome};
pub use plan::*;
