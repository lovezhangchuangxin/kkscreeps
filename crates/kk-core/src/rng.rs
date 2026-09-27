//! 带种子的确定性随机数（PCG32）。
//!
//! 结算的随机来源统一走本类型，种子保存在世界数据里（game-loop.md 关键设计二：
//! "从任意历史时间点 + 指令序列可精确重放"的前提）。玩家代码里的 `Math.random()`
//! 不纳入引擎种子化（官方行为保持）。

/// PCG-XSH-RR 64/32（O'Neill 参考实现的 `pcg32_srandom_r(seed, seed)` 同款播种：
/// `state=0; inc=(seq<<1)|1; next(); state+=initstate; next()`）。种子进 state
/// 与 inc 两个途径，与参考实现逐值一致——回放兼容以已知答案测试锁定。
///
/// M1 结算路径暂无随机规则（掉落散布、NPC、stronghold 在后续里程碑），先落地
/// 原语避免将来换 RNG 破坏回放兼容。
#[derive(Debug, Clone)]
pub struct SeededRng {
    state: u64,
    inc: u64,
}

impl SeededRng {
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

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        // XSH-RR 输出函数
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// [0, bound) 均匀分布（Lemire 乘法归约，无模偏差）。
    pub fn below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0);
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
        // 不同种子序列不同（固定断言防实现漂移）
        let mut d = SeededRng::new(1);
        assert_ne!(SeededRng::new(2).next_u32(), d.next_u32());
        // below 的边界 bound
        assert_eq!(c.below(1), 0);
        for bound in [2u32, 3, 10, 1 << 31] {
            for _ in 0..100 {
                assert!(a.below(bound) < bound);
            }
        }
    }

    /// 已知答案：canonical pcg32_srandom_r(seed, seed) 的首输出（回放兼容锚点，
    /// 播种方式任何改动都会在此显形）。期望值由参考算法在 Node 中独立复算。
    #[test]
    fn known_answer_vectors() {
        let mut a = SeededRng::new(42);
        assert_eq!(a.next_u32(), 1085446021);
        assert_eq!(a.next_u32(), 176895750);
        assert_eq!(SeededRng::new(1).next_u32(), 3380776849);
        assert_eq!(SeededRng::new(0).next_u32(), 3837872008);
    }
}
