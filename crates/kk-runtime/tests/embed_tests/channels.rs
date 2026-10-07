//! 验证玩家意图从沙箱传到结算，再出现在下一轮玩家视图中的完整过程。
//! 场景由 `embed_engine_suite` 顺序调用。

use super::*;

mod kkpv;
pub(super) use kkpv::*;

mod families;
pub(super) use families::*;
