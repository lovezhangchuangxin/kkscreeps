//! 带种子的确定性伪随机数，用于需要重复生成的数据。
//!
//! 相同种子和相同调用顺序产生相同序列；修改算法或播种步骤会改变序列。
//! 本类型不自动保存状态，也不控制玩家 JavaScript 中的 `Math.random()`。
//! 玩法 realm 的 Mod 随机协议用下方 [`gameplay_draw`] 无状态派生，
//! 也不经过本类型。

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

/// 玩法 realm 随机流的无状态单抽推导（G22.b 协议）。
///
/// 每次抽取都从（世界种子、流域名、tick、域内序号）整体混合得出，
/// 不保存跨调用状态：同一 tick 的重试/回放重抽同值，检查点只需持久化
/// 种子（世界标量域）即可恢复全部随机行为。域串由调用方拼装（结算期
/// `绑定键#房间#目标ID`，注册期 `{命名空间}#init` 且 tick 恒 0），域的
/// 划分保证无关绑定/房间/对象互不挤占序列。终结器与 kk-engine
/// `settle/economy.rs` 的 MineralRng 同族：FNV-1a 域串 + SplitMix64 终结。
/// 本函数的混合常量属于冻结的玩法协议：改变它会改变所有已定场景的
/// 抽值，等价于协议变更而非实现细节。
pub fn gameplay_draw(seed: u64, domain: &str, tick: u64, index: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in domain.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let mut z = seed
        ^ h.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ tick.wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ index.wrapping_mul(0x94d0_49bb_1331_11eb);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// 把 [`gameplay_draw`] 的原始 64 位输出映射为 `[0, 1)` 双精度数，
/// 取高 53 位（JS `Math.random` 语义同口径）。
pub fn gameplay_unit(draw: u64) -> f64 {
    (draw >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod gameplay_tests {
    use super::*;

    /// 相同输入逐位相等；四个输入各自变化都会改变输出（种子/域/tick/序号
    /// 都参与混合，缺一不可）。
    #[test]
    fn deterministic_and_sensitive_to_every_input() {
        let base = gameplay_draw(7, "ns:slot#W1N1#12", 3, 0);
        assert_eq!(base, gameplay_draw(7, "ns:slot#W1N1#12", 3, 0));
        assert_ne!(base, gameplay_draw(8, "ns:slot#W1N1#12", 3, 0));
        assert_ne!(base, gameplay_draw(7, "ns:slot#W1N1#13", 3, 0));
        assert_ne!(base, gameplay_draw(7, "ns:slot#W1N1#12", 4, 0));
        assert_ne!(base, gameplay_draw(7, "ns:slot#W1N1#12", 3, 1));
    }

    /// 域串前缀不得串流：共享前缀的两个域输出不同（域以整串参与哈希）。
    #[test]
    fn domain_is_whole_string_no_prefix_stream() {
        let a = gameplay_draw(7, "ns:a#init", 0, 0);
        let b = gameplay_draw(7, "ns:ab#init", 0, 0);
        assert_ne!(a, b);
    }

    /// 固定向量钉住混合常量；协议变更（改常量/改结构）在此红灯。
    #[test]
    fn known_answer_vectors() {
        assert_eq!(gameplay_draw(0, "ns#init", 0, 0), 0xdee7_4f4c_1f5a_7ba5u64);
        assert_eq!(
            gameplay_draw(123456789, "dice:room.tick#W1N1#18446744073709551615", 42, 3),
            0x8d22_aff5_03c1_15b3u64
        );
        // 值域：unit 输出落在 [0,1)，固定向量钉住映射尾部。
        let d = gameplay_draw(1, "d", 2, 1);
        let u = gameplay_unit(d);
        assert!((0.0..1.0).contains(&u));
        assert_eq!(u, (d >> 11) as f64 / (1u64 << 53) as f64);
    }

    /// 大样本下的粗均匀性：按高 4 位分桶，任何桶不应完全缺席或独占。
    /// 这是防退化下限，不是统计检验。
    #[test]
    fn rough_uniformity_over_buckets() {
        let mut buckets = [0usize; 16];
        for i in 0..4096u64 {
            let d = gameplay_draw(99, "spread", 5, i);
            buckets[(d >> 60) as usize] += 1;
        }
        assert!(
            buckets.iter().all(|c| *c >= 128),
            "高位分桶出现显著偏斜：{buckets:?}"
        );
    }
}
