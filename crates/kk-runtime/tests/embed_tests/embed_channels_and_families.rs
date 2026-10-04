//! 验证玩家意图从沙箱传到结算，再出现在下一轮玩家视图中的完整过程。
//! 场景由 `embed_engine_suite` 顺序调用。

use super::*;

#[path = "embed_channels_and_families/kkpv.rs"]
mod kkpv;
pub(super) use kkpv::*;

#[path = "embed_channels_and_families/families.rs"]
mod families;
pub(super) use families::*;
