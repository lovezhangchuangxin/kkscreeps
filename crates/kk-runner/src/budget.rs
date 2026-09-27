//! CPU 预算桶（bucket）经济——官方语义的 Rust 化（spike bucket_sim 实验的移植）。
//!
//! 官方规则（engine/driver 源码取证，见 sandbox.md；@screeps/driver make.js:72-83/206-212）：
//! - 每 tick 回充 `limit`（默认 20），桶容量上限 10000；
//! - `Game.cpu.tickLimit = min(500, bucket + limit)`（回充后计）；
//! - 硬超时 → 看门狗按整份 tickLimit 计费，**桶可为负**；只有桶被扣成负数
//!   （cpuAvailable<0）时下一 tick 才停跑一次（停跑的 tick 照常回充）；桶充足的
//!   普通超时不跳帧。官方 skipTicksPenalty 是 runtime 进程硬重置专用，与普通超时无关；
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
    /// 本 tick 跳过玩家代码执行：桶为负（透支），停跑回血（官方 make.js:72-83 语义）。
    SkipPenalty,
}

/// 桶为负的判定容差（浮点误差防护；真实透支至少是 tick 级毫秒量）。
const NEG_EPSILON: f64 = 0.01;

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

    /// 每 tick 开始：回充并给出本 tick 的执行计划。
    /// 官方语义：回充后桶仍为负（cpuAvailable<0）时本 tick 停跑（停跑 tick 照常回充）。
    pub fn begin_tick(&mut self) -> TickPlan {
        self.bucket = (self.bucket + self.limit).min(self.cap);
        self.tick_limit = self.max_per_tick.min(self.bucket.max(0.0));
        if self.bucket < -NEG_EPSILON {
            return TickPlan::SkipPenalty;
        }
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

    /// 正常结算。桶可为负（官方结算不落 0，封顶只在回充时做）。
    pub fn settle(&mut self, used_ms: f64) {
        self.bucket -= used_ms;
    }

    /// 硬中断结算：按整份 tickLimit 计费。官方语义：桶充足时**不**触发下一 tick 跳帧，
    /// 只有扣成负数的部分由 `begin_tick` 的负桶停跑规则处理。
    pub fn settle_terminated(&mut self) {
        self.bucket -= self.tick_limit;
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
        // 桶被扣成负数（透支）→ 停跑并逐 tick 回血，回正后恢复执行（官方 make.js:72-83）
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        b.begin_tick(); // 回充到 20
        b.settle(60.0); // 超扣 → -40
        assert!(b.bucket() < 0.0);
        // 停跑 tick 照常回充：-40 → -20（仍停）→ 0（恢复执行，tickLimit=0）
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty), "负桶停跑");
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 0.0 }),
            "停跑 tick 照常回充，回到 0 边界即恢复执行"
        );
        b.settle_terminated(); // tickLimit=0 → 扣 0
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }),
            "回充累积后正常执行"
        );
    }
}
