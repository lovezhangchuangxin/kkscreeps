//! 带种子的确定性伪随机数，用于需要重复生成的数据。
//!
//! 相同种子和相同调用顺序产生相同序列；修改算法或播种步骤会改变序列。
//! 本类型不自动保存状态，也不控制玩家 JavaScript 中的 `Math.random()`。

/// 使用 64 位状态、输出 32 位整数的 PCG-XSH-RR 生成器。
/// 克隆复制当前状态，两个实例随后采用相同调用顺序时会产生相同输出。
///
/// 播种和单次输出对照 O'Neill 的 `pcg32_srandom_r`、`pcg32_random_r`：
/// <https://github.com/imneme/pcg-c-basic/blob/bc39cd76ac3d541e618606bcc6e1e5ba5e5e6aa3/pcg_basic.c>。
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
    inc: u64,
}

impl SeededRng {
    /// 以 seed 同时作为初始状态参数和序列选择参数播种。
    /// 对应参考实现的 `pcg32_srandom_r(seed, seed)`，包括播种时的两次状态推进。
    pub fn new(seed: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: (seed << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    /// 推进内部状态一次，并从推进前的状态计算下一个 32 位输出。
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        // 状态更新按 u64 回绕；输出对旧状态做异或移位，再按其高位决定旋转量。
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// 生成 0..bound 内的整数，使用乘法和拒绝采样避免直接取模产生偏差。
    /// 一次调用可能消费多个随机输出；bound=1 也会消费一次输出。
    /// 算法依据：Lemire, Fast Random Integer Generation in an Interval,
    /// <https://arxiv.org/abs/1805.10941>。
    ///
    /// # Panics
    /// bound 为 0 时 panic。
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0);
        // threshold = 2^32 % bound；低半部分小于它的乘积须重取随机数。
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let x = self.next_u32();
            let m = (x as u64) * (bound as u64);
            if (m as u32) >= threshold {
                return (m >> 32) as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_bounded() {
        let mut a = SeededRng::new(42);
        let mut b = SeededRng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
        let mut c = SeededRng::new(7);
        for _ in 0..1000 {
            assert!(c.below(10) < 10);
        }
        // 检查这两个固定种子的首值不同，不声称所有种子的首值都不同。
        let mut d = SeededRng::new(1);
        assert_ne!(SeededRng::new(2).next_u32(), d.next_u32());
        // 上限为 1 时唯一结果是 0；其余结果不得达到 bound。
        assert_eq!(c.below(1), 0);
        for bound in [2u32, 3, 10, 1 << 31] {
            for _ in 0..100 {
                assert!(a.below(bound) < bound);
            }
        }
    }

    /// 固定种子的已知输出，防止播种步骤或输出算法变化后仍只通过“序列可重复”测试。
    #[test]
    fn known_answer_vectors() {
        let mut a = SeededRng::new(42);
        assert_eq!(a.next_u32(), 1085446021);
        assert_eq!(a.next_u32(), 176895750);
        assert_eq!(SeededRng::new(1).next_u32(), 3380776849);
        assert_eq!(SeededRng::new(0).next_u32(), 3837872008);
    }
}
