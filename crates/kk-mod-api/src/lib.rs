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

pub mod buildings;
pub mod builtin;
#[path = "builtin/capabilities.rs"]
pub mod builtin_capabilities;
pub mod combat;
mod contribution;
pub mod extension;
pub mod incoming;
pub mod intent_gate;
pub mod movement;
// 兼容既有公开路径；内部消费者使用 movement 领域入口。
pub use movement::{admission, contest, move_cost, obstacle, precheck, pull_registration};
pub mod objects;
mod outcome;
pub mod player_surface;
pub mod resources;
pub mod typegen;
pub mod validate;

pub use contribution::*;
pub use outcome::{Decline, SlotOutcome};
