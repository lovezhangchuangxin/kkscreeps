//! 移动候选能力、JS 参数校验与宿主操作。
use super::*;

mod host;
pub use host::MovementHost;
mod request;
pub(crate) use request::{
    op_mod_chain_move_cancel, op_mod_chain_move_pending, op_mod_chain_move_request,
};
mod admission;
pub(crate) use admission::{
    op_mod_chain_admission_admitted, op_mod_chain_admission_evaluate,
    op_mod_chain_admission_set_admitted,
};
mod contest;
pub(crate) use contest::{op_mod_chain_contest_priority, op_mod_chain_contest_set_priority};
mod move_cost;
pub(crate) use move_cost::{op_mod_chain_cost_fatigue_of, op_mod_chain_cost_set_fatigue};
mod obstacle;
pub(crate) use obstacle::{
    op_mod_chain_obstacle_block, op_mod_chain_obstacle_blocked, op_mod_chain_obstacle_evaluate,
};
mod precheck;
pub(crate) use precheck::{
    op_mod_chain_precheck_allow_static, op_mod_chain_precheck_block, op_mod_chain_precheck_blocked,
};
mod pull_registration;
pub(crate) use pull_registration::{op_mod_chain_pull_denied, op_mod_chain_pull_deny};
