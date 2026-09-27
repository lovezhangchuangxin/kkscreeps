//! CPU 预算桶（bucket）经济——官方语义的 Rust 化（spike bucket_sim 实验的移植）。
//!
//! 官方规则（engine/driver 源码取证，见 sandbox.md）：
//! - 每 tick 回充 `limit`（默认 20），桶容量上限 10000；
//! - `Game.cpu.tickLimit = min(500, bucket + limit)`（回充后计）；
//! - 执行超过 tickLimit → 看门狗硬中断，按整份 tickLimit 计费，下一 tick 惩罚性跳过
//!   （skipTicksPenalty）；
//! - 正常结算 `bucket -= 实际用量 + 0.2×intents`（M0 桩环境无 intent 计费，M2 沙箱补全）。
//!
//! M0 计时口径为墙钟毫秒（与 bucket_sim 一致）；正式实现换线程 CPU 时钟
//! （CLOCK_THREAD_CPUTIME_ID，纳秒精度，02 报告已实测），机制不变。

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
    /// 硬超时惩罚：本 tick 跳过玩家代码执行（官方 skipTicksPenalty）。
    SkipPenalty,
}

#[derive(Debug)]
pub struct Bucket {
    pub limit: f64,
    pub cap: f64,
    pub max_per_tick: f64,
    bucket: f64,
    skip_next: bool,
    tick_limit: f64,
}

impl Bucket {
    pub fn new(limit: f64, cap: f64, max_per_tick: f64, initial: f64) -> Self {
        Self {
            limit,
            cap,
            max_per_tick,
            bucket: initial.min(cap),
            skip_next: false,
            tick_limit: 0.0,
        }
    }

    /// 每 tick 开始：回充并给出本 tick 的执行计划。
    pub fn begin_tick(&mut self) -> TickPlan {
        if self.skip_next {
            self.skip_next = false;
            return TickPlan::SkipPenalty;
        }
        self.bucket = (self.bucket + self.limit).min(self.cap);
        self.tick_limit = self.max_per_tick.min(self.bucket);
        TickPlan::Run {
            tick_limit: self.tick_limit,
        }
    }

    /// 本 tick 预算（回充后），即玩家读到的 `Game.cpu.tickLimit`。
    pub fn tick_limit(&self) -> f64 {
        self.tick_limit
    }

    pub fn bucket(&self) -> f64 {
        self.bucket
    }

    /// 正常结算。
    pub fn settle(&mut self, used_ms: f64) {
        self.bucket = (self.bucket - used_ms).max(0.0);
    }

    /// 硬中断结算：按整份 tickLimit 计费并惩罚下一 tick。
    pub fn settle_terminated(&mut self) {
        self.bucket = (self.bucket - self.tick_limit).max(0.0);
        self.skip_next = true;
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
    fn termination_charges_full_and_skips() {
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 100.0);
        assert!(matches!(
            b.begin_tick(),
            TickPlan::Run { tick_limit: 120.0 }
        ));
        b.settle_terminated();
        assert_eq!(b.bucket(), 0.0, "按整份 tickLimit 计费");
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }),
            "惩罚只停一 tick"
        );
    }
}
