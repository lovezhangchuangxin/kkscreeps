//! CPU 预算桶（bucket）经济——官方 driver `make.js` 的 Rust 化（spike
//! bucket_sim 实验的移植）。
//!
//! 官方规则（make.js:72-83/187/206-212 逐行对照过）：
//! - 回充并入结算：`bucket += limit − used`，封顶 10000；内部值允许为负
//!   （不钳到 0，精确 0 也可达）；
//! - `tickLimit = min(500, limit + bucket)`（结算前口径）；
//! - 负桶判定在**回充之前** → 整 tick 停跑；停跑的 tick 照常回充，深负桶钳到
//!   −2×limit 地板（约 2 tick 内恢复）。桶充足时的普通超时**不跳帧**——官方
//!   skipTicksPenalty 只用于 runtime 进程硬重置，与普通超时无关；
//! - 硬超时按整份 tickLimit 计费（usedCleanTime≈tickLimit）。
//!
//! 当前计时口径为墙钟毫秒（与 bucket_sim 一致）；正式实现换线程 CPU 时钟
//! （CLOCK_THREAD_CPUTIME_ID 纳秒精度，调研 02 报告实测过），机制不变。
//! intent 计费项（+0.2×intents）随 M2 沙箱补全。

/// 每 tick 回充量（官方默认 CPU limit，单位约等于毫秒）。
pub const DEFAULT_CPU_LIMIT: f64 = 20.0;
/// 桶容量上限。
pub const BUCKET_CAP: f64 = 10_000.0;
/// 单 tick 可用时间硬上限。
pub const MAX_PER_TICK: f64 = 500.0;

#[derive(Debug, Clone, PartialEq)]
pub enum TickPlan {
    /// 正常执行，携带本 tick 的 tickLimit（毫秒）。
    Run { tick_limit: f64 },
    /// 本 tick 跳过玩家代码执行：桶为负（透支），停跑回血（官方 make.js:72-83 语义）。
    SkipPenalty,
}

#[derive(Debug)]
pub struct Bucket {
    pub limit: f64,
    pub cap: f64,
    pub max_per_tick: f64,
    bucket: f64,
    tick_limit: f64,
}

impl Bucket {
    pub fn new(limit: f64, cap: f64, max_per_tick: f64, initial: f64) -> Self {
        Self {
            limit,
            cap,
            max_per_tick,
            bucket: initial.min(cap),
            tick_limit: 0.0,
        }
    }

    /// 每 tick 开始：给出本 tick 的执行计划。
    /// 负桶 → 整 tick 停跑（停跑 tick 照常回充、深负钳到 −2×limit 地板）；
    /// 桶非负 → 执行，tickLimit = min(max_per_tick, limit + bucket)——回充在
    /// settle 时并入，这里不重复加。
    pub fn begin_tick(&mut self) -> TickPlan {
        if self.bucket < 0.0 {
            self.bucket = (self.bucket + self.limit).max(-2.0 * self.limit);
            return TickPlan::SkipPenalty;
        }
        self.tick_limit = self.max_per_tick.min(self.bucket + self.limit);
        TickPlan::Run {
            tick_limit: self.tick_limit,
        }
    }

    /// 本 tick 预算，即玩家读到的 `Game.cpu.tickLimit`。
    pub fn tick_limit(&self) -> f64 {
        self.tick_limit
    }

    /// 按官方公式现算下一窗口（`min(maxPerTick, limit + bucket)`，make.js:85-86）。
    /// 独立 runExpr 通道无 begin_tick，不能读 stale 的 `tick_limit` 字段
    /// （新沙箱为 0、tick 过的沙箱为上一 tick 残留值）。
    pub fn next_tick_limit(&self) -> f64 {
        self.max_per_tick.min(self.bucket + self.limit)
    }

    pub fn bucket(&self) -> f64 {
        self.bucket
    }

    /// 正常结算：`bucket += limit − ceil(used)`，封顶 cap、可为负、不落 0。
    /// 官方逐 tick 向上取整——亚毫秒耗时也计 1。
    pub fn settle(&mut self, used_ms: f64) {
        self.bucket = (self.bucket + self.limit - used_ms.ceil()).min(self.cap);
    }

    /// 硬中断结算：超时被沙箱内捕获，按整份 tickLimit 计费（与正常结算同路径
    /// 回充）。桶充足时不跳帧；负桶由 begin_tick 的停跑规则处理。
    pub fn settle_terminated(&mut self) {
        self.bucket = (self.bucket + self.limit - self.tick_limit.ceil()).min(self.cap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recharge_and_limit() {
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
        b.settle(15.0);
        assert_eq!(b.bucket(), 5.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 25.0 }));
    }

    #[test]
    fn cap_and_max_per_tick() {
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 10_000.0);
        assert!(matches!(
            b.begin_tick(),
            TickPlan::Run { tick_limit: 500.0 }
        ));
        b.settle(0.0);
        assert_eq!(b.bucket(), 10_000.0, "桶不超上限");
    }

    #[test]
    fn termination_charges_full_no_skip_when_bucket_left() {
        // 官方语义（make.js:206-212 + 72-83）：硬中断按整份 tickLimit 计费，
        // 桶充足（扣后 >= 0）时下一 tick 照常执行——不跳帧。
        let mut b = Bucket::new(20.0, 10_000.0, 100.0, 100.0);
        assert!(matches!(
            b.begin_tick(),
            TickPlan::Run { tick_limit: 100.0 }
        ));
        b.settle_terminated();
        assert!((b.bucket() - 20.0).abs() < 1e-9, "按整份 tickLimit 计费");
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 40.0 }),
            "桶充足时硬中断不触发跳帧（官方 cpuAvailable 语义）"
        );
    }

    #[test]
    fn negative_bucket_skips_until_recharged() {
        // 官方语义（make.js:72-83）：回充前判定负桶即整 tick 停跑；停跑 tick 照常回充
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
        b.settle(60.0); // 0 + 20 − 60 = −40（结算并入回充）
        assert_eq!(b.bucket(), -40.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty), "负桶停跑");
        assert_eq!(b.bucket(), -20.0, "停跑 tick 照常回充");
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty), "-20 仍停");
        assert_eq!(b.bucket(), 0.0);
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }),
            "回正后恢复执行，tickLimit = limit + bucket"
        );
    }

    #[test]
    fn settle_ceils_sub_millisecond_usage() {
        // 官方 make.js:193 Math.ceil：0.1ms 也计 1，亚毫秒连续 tick 不会白嫖预算
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
        b.settle(0.1);
        assert_eq!(b.bucket(), 19.0, "ceil(0.1) = 1");
        b.settle(0.0);
        assert_eq!(b.bucket(), 39.0, "ceil(0) = 0（官方对 0 不虚计）");
    }

    #[test]
    fn deep_negative_floors_at_minus_two_limits() {
        // 官方 make.js:77：深负桶在停跑回充时钳到 −2×limit 地板（约 2 tick 内恢复）
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        b.settle(600.0); // 0 + 20 − 600 = −580
        assert_eq!(b.bucket(), -580.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        assert_eq!(b.bucket(), -40.0, "深负钳到 −2×limit 地板");
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        assert_eq!(b.bucket(), -20.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        assert_eq!(b.bucket(), 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
    }
}
