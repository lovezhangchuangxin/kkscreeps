//! 玩法 realm 的确定性随机流 op 与宿主状态（G22.b 协议宿主侧）。
//!
//! 抽值本身由 `kk_core::rng::gameplay_draw` 无状态推导：同一（种子、域、
//! tick、序号）恒得同值，检查点只需持久化世界种子即可恢复全部随机行为。
//! 本模块的运行时状态只有两样：世界种子（realm 构造注入，随分片不变）
//! 与各流域的域内序号计数。结算轮计数在 `begin_settlement` 清零（轮作用
//! 域：失败重试同 tick 重抽同值，丢弃后同 tick 新轮亦重抽同值）；注册期
//! init 域计数只在安装窗口消费，轮清零不影响其取值。协议契约见
//! `docs/architecture/gameplay-determinism.md`。

use std::collections::BTreeMap;

use deno_core::{OpState, op2};
use deno_error::JsErrorBox;

use crate::realm::Registry;

/// 玩法随机流宿主状态；realm 构造时放入 OpState。
pub(crate) struct RngState {
    seed: u64,
    counts: BTreeMap<String, u64>,
}

impl RngState {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            seed,
            counts: BTreeMap::new(),
        }
    }

    /// 结算轮开始：清空域内序号（轮作用域重置）。
    pub(crate) fn begin_round(&mut self) {
        self.counts.clear();
    }

    /// 取本域下一个抽值并推进序号；域串与 tick 由调用方按协议拼装。
    fn next(&mut self, domain: &str, tick: u64) -> f64 {
        let index = self.counts.entry(domain.to_string()).or_insert(0);
        let value = kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(
            self.seed, domain, tick, *index,
        ));
        *index += 1;
        value
    }
}

/// 结算期随机：SDK 在绑定作用域内调用；域串（`绑定键#房间#目标ID`）与
/// tick 由 SDK 从链 ctx 拼装传入。tick 以 f64 传输（op 的 64 位整数参数
/// 需 BigInt，不值得每抽一次分配）：世界时间在 2^53 内与 u64 逐位一致，
/// 远超任何真实世界寿命。本 op 不校验域串内容——realm 为共同可信宿主，
/// 伪造域串只影响伪造者自己的抽值序列。
#[op2(fast)]
pub(crate) fn op_mod_random(
    state: &mut OpState,
    #[string] domain: String,
    tick: f64,
) -> Result<f64, JsErrorBox> {
    // 直呼路径（dispatch）的 ctx 由宿主 JSON 提供，垃圾 tick（NaN/负数/
    // 小数）经饱和转换会与 tick 0 域串别名；开发期断言拦截，合法输入
    // 口径为 2^53 内非负整数。
    debug_assert!(tick.is_finite() && tick >= 0.0 && tick.fract() == 0.0);
    let tick = tick as u64;
    Ok(state.borrow_mut::<RngState>().next(&domain, tick))
}

/// 注册期模块顶层随机：域 = `{命名空间}#init`，tick 恒 0（重装不变式：
/// 冷恢复重装与首次装载同值）。不在注册窗口调用（如密封后的顶层重跑）
/// 属协议外调用，按注册状态明确报错，不静默给值。
#[op2(fast)]
pub(crate) fn op_mod_random_init(state: &mut OpState) -> Result<f64, JsErrorBox> {
    let namespace = state.borrow::<Registry>().active_namespace()?;
    let domain = format!("{namespace}#init");
    Ok(state.borrow_mut::<RngState>().next(&domain, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 域序号推进：同域两次抽取等于 index 0/1 的直接推导；轮重置后回到 0。
    #[test]
    fn advances_domain_counter_and_resets_per_round() {
        let mut state = RngState::new(7);
        let first = state.next("ns:a#W1N1#9", 3);
        let second = state.next("ns:a#W1N1#9", 3);
        assert_eq!(
            first,
            kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(7, "ns:a#W1N1#9", 3, 0))
        );
        assert_eq!(
            second,
            kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(7, "ns:a#W1N1#9", 3, 1))
        );
        // 不同域不挤占彼此序号。
        assert_eq!(
            state.next("ns:b#W1N1#9", 3),
            kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(7, "ns:b#W1N1#9", 3, 0))
        );
        state.begin_round();
        assert_eq!(first, state.next("ns:a#W1N1#9", 3));
    }
}
