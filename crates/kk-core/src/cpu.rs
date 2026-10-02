//! 管理玩家 CPU 桶余额与每 tick 的执行预算，单位为毫秒。
//!
//! 正常结算增加 limit，再扣除向上取整的用量，余额可为负且不超过 cap。
//! 执行前余额为负则停跑；回充由调用方在提交时应用，begin_tick 只制定计划。
//! 用量由调用方报告；本模块不读取时钟或访问沙箱。
//! 来源：上游 driver/lib/runtime/make.js 的 CPU 预算与回充逻辑。

/// 每 tick 默认回充量，单位毫秒。
pub const DEFAULT_CPU_LIMIT: f64 = 20.0;
/// 默认桶余额上限，单位毫秒。
pub const BUCKET_CAP: f64 = 10_000.0;
/// 默认单 tick 执行预算上限，单位毫秒。
pub const MAX_PER_TICK: f64 = 500.0;

#[derive(Debug, Clone, PartialEq)]
pub enum TickPlan {
    /// 正常执行，携带本 tick 的 tickLimit（毫秒）。
    Run { tick_limit: f64 },
    /// 余额为负，本 tick 跳过玩家代码；调用方仍需提交或丢弃回充候选。
    SkipPenalty,
}

#[derive(Debug, Clone, PartialEq)]
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

    /// 根据当前余额制定执行计划，不修改余额。
    /// 余额非负时缓存 min(max_per_tick, limit + bucket) 作为本 tick 预算；
    /// 余额为负时返回 SkipPenalty，由调用方暂存回充候选。
    pub fn begin_tick(&mut self) -> TickPlan {
        if self.bucket < 0.0 {
            return TickPlan::SkipPenalty;
        }
        self.tick_limit = self.max_per_tick.min(self.bucket + self.limit);
        TickPlan::Run {
            tick_limit: self.tick_limit,
        }
    }

    /// 计算停跑轮的回充增量，不修改余额。
    /// 先比较原余额：低于 −2×limit 时恢复到 −2×limit，否则增加 limit。
    /// 不能写成 `(bucket + limit).max(-2.0 * limit)`；limit=20、bucket=−41
    /// 时那样会得到 −21，而这里应得到 −40。
    /// 来源：上游 driver/lib/runtime/make.js 的负余额回充分支。
    pub fn skip_recharge(&self) -> f64 {
        let target = if self.bucket < -2.0 * self.limit {
            -2.0 * self.limit
        } else {
            self.bucket + self.limit
        };
        target - self.bucket
    }

    /// 按当前余额重新计算并应用停跑回充。
    /// 调用方应在暂存候选后保持余额和配置不变，直到提交或丢弃候选。
    pub fn apply_recharge(&mut self) {
        self.bucket += self.skip_recharge();
    }

    /// 最近一次 Run 计划缓存的预算；尚未执行 begin_tick 时为 0，停跑时不更新。
    pub fn tick_limit(&self) -> f64 {
        self.tick_limit
    }

    /// 按当前余额计算 min(max_per_tick, limit + bucket)，不更新缓存。
    /// 独立表达式执行使用此值，避免读取尚未初始化或属于上一 tick 的预算。
    pub fn next_tick_limit(&self) -> f64 {
        self.max_per_tick.min(self.bucket + self.limit)
    }

    pub fn bucket(&self) -> f64 {
        self.bucket
    }

    /// 提交计费：bucket += limit − ceil(used_ms)，封顶 cap，允许负余额。
    /// 亚毫秒的正用量也计 1 毫秒。来源：上游 driver/lib/runtime/make.js 的计费逻辑。
    pub fn settle(&mut self, used_ms: f64) {
        self.bucket = (self.bucket + self.limit - used_ms.ceil()).min(self.cap);
    }

    /// 按最近一次 Run 计划的完整预算结算，并增加本 tick 回充。
    /// 余额非负时下一 tick 仍可运行；余额为负时由 begin_tick 判定停跑。
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

    /// 验证先比较原余额再选择恢复方式，覆盖 −3×limit 与 −2×limit 之间的余额。
    #[test]
    fn skip_recharge_floor_window_official_values() {
        for (before, after) in [
            (-41.0, -40.0),   // 窗口内：官方钳 −40（旧错误式得 −21）
            (-50.0, -40.0),   // （旧式 −30）
            (-59.0, -40.0),   // （旧式 −39）
            (-60.0, -40.0),   // 开窗下界 −60：两式合流（开窗不含该点）
            (-40.0, -20.0),   // 地板上：不钳、加 limit
            (-2000.0, -40.0), // 深负：两式合流
        ] {
            let b = Bucket::new(20.0, 10_000.0, 500.0, before);
            assert_eq!(
                b.skip_recharge(),
                after - before,
                "原桶 {before} → 目标 {after}"
            );
        }
        // 窗口轨迹：−41 → −40 → −20 → 0（恢复轮 tickLimit=20）
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, -41.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        b.apply_recharge();
        assert_eq!(b.bucket(), -40.0, "窗口内首轮回充钳回地板");
        b.apply_recharge();
        assert_eq!(b.bucket(), -20.0);
        b.apply_recharge();
        assert_eq!(b.bucket(), 0.0);
        assert!(
            matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }),
            "恢复轮 tickLimit=20（旧式轨迹 −41→−21→−1→19：恢复轮桶 19/tickLimit 39——可分辨）"
        );
    }

    #[test]
    fn negative_bucket_skips_until_recharged() {
        // begin_tick 只判定停跑；回充要等 apply_recharge 调用后才改变余额。
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
        b.settle(60.0); // 0 + 20 − 60 = −40（结算并入回充）
        assert_eq!(b.bucket(), -40.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty), "负桶停跑");
        assert_eq!(b.bucket(), -40.0, "停跑轮回充不即时落桶（候选化）");
        assert_eq!(b.skip_recharge(), 20.0, "回充增量 = limit");
        b.apply_recharge(); // 模拟统一提交点 settle_bill 并入
        assert_eq!(b.bucket(), -20.0, "停跑 tick 照常回充（提交点并入）");
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty), "-20 仍停");
        b.apply_recharge();
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
        // limit=20 时，−580 先恢复到 −40，再经两次停跑回充到 0。
        let mut b = Bucket::new(20.0, 10_000.0, 500.0, 0.0);
        b.settle(600.0); // 0 + 20 − 600 = −580
        assert_eq!(b.bucket(), -580.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        assert_eq!(b.bucket(), -580.0, "深负同样不即时落桶");
        assert_eq!(
            b.skip_recharge(),
            540.0,
            "深负钳到 −2×limit 地板（目标 −40）"
        );
        b.apply_recharge();
        assert_eq!(b.bucket(), -40.0, "深负钳到 −2×limit 地板");
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        b.apply_recharge();
        assert_eq!(b.bucket(), -20.0);
        assert!(matches!(b.begin_tick(), TickPlan::SkipPenalty));
        b.apply_recharge();
        assert_eq!(b.bucket(), 0.0);
        assert!(matches!(b.begin_tick(), TickPlan::Run { tick_limit: 20.0 }));
    }
}

impl Default for Bucket {
    fn default() -> Self {
        Self::new(DEFAULT_CPU_LIMIT, BUCKET_CAP, MAX_PER_TICK, BUCKET_CAP)
    }
}

/// 宿主从已提交桶制定的本轮预算。沙箱使用快照，不能修改宿主余额。
#[derive(Debug, Clone)]
pub struct RunBudget {
    bucket: Bucket,
    plan: TickPlan,
}

impl RunBudget {
    pub fn new(committed: &Bucket) -> Self {
        let mut bucket = committed.clone();
        let plan = bucket.begin_tick();
        Self { bucket, plan }
    }
    pub fn bucket(&self) -> &Bucket {
        &self.bucket
    }
    pub fn plan(&self) -> &TickPlan {
        &self.plan
    }

    /// 将固定用量转换为待发布桶值。halt 不计费也不回充；停跑只回充。
    /// 非有限或负用量属于协议错误，不能污染持久余额。
    pub fn finish(self, used_ms: Option<f64>) -> Result<Bucket, &'static str> {
        let mut bucket = self.bucket;
        match self.plan {
            TickPlan::SkipPenalty => {
                if used_ms.is_some() {
                    return Err("停跑回执不应携带 CPU 费用");
                }
                bucket.apply_recharge();
            }
            TickPlan::Run { .. } => {
                if let Some(used) = used_ms {
                    if !used.is_finite() || used < 0.0 {
                        return Err("CPU 费用必须为有限非负值");
                    }
                    bucket.settle(used);
                }
            }
        }
        Ok(bucket)
    }
}

#[cfg(test)]
mod host_budget_tests {
    use super::*;
    #[test]
    fn fixed_budget_reports_preserve_halt_and_penalty_semantics() {
        let original = Bucket::new(20.0, 10000.0, 500.0, 0.0);
        let budget = RunBudget::new(&original);
        assert_eq!(budget.plan(), &TickPlan::Run { tick_limit: 20.0 });
        assert_eq!(budget.clone().finish(Some(0.1)).unwrap().bucket(), 19.0);
        assert_eq!(budget.clone().finish(Some(20.0)).unwrap().bucket(), 0.0);
        assert_eq!(budget.finish(None).unwrap().bucket(), 0.0);
        assert_eq!(original.bucket(), 0.0);
        let negative = Bucket::new(20.0, 10000.0, 500.0, -41.0);
        let budget = RunBudget::new(&negative);
        assert_eq!(budget.plan(), &TickPlan::SkipPenalty);
        assert_eq!(budget.finish(None).unwrap().bucket(), -40.0);
        assert_eq!(negative.bucket(), -41.0);
    }
    #[test]
    fn invalid_usage_is_rejected_without_mutating_committed_balance() {
        let original = Bucket::default();
        for used in [f64::NAN, f64::INFINITY, -0.1] {
            assert!(RunBudget::new(&original).finish(Some(used)).is_err());
        }
        let negative = Bucket::new(20.0, 10000.0, 500.0, -41.0);
        assert!(RunBudget::new(&negative).finish(Some(0.0)).is_err());
        assert_eq!(original.bucket(), 10000.0);
    }
}
