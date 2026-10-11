//! 结算相分段诊断（`KK_SETTLE_DIAG=1` 开启）：逐拍记录五阶段墙钟构成与
//! 跨房耦合计数，供依赖调度（R09）与结算压缩的可行性取证。
//! 正式计时只加轻量 Instant 差分（每拍十余次，百纳秒级），中位数聚合
//! 在报告时进行；耦合计数随共享候选的既有借用累加，无额外锁。

/// 共享候选（TickState）上的耦合计数：房间规则触碰全局状态的频次，
/// 在 allocate_id/claim_state/add_gcl 等原位累加，随 [`crate::settle::TickState`]
/// 的候选生命周期存活一拍。
#[derive(Default, Clone, Copy, Debug)]
pub struct DiagCounters {
    pub id_allocs: u64,
    pub id_restores: u64,
    pub claim_calls: u64,
    pub gcl_adds: u64,
    pub power_adds: u64,
}

/// 单拍结算的分段纳秒与耦合计数。纳秒字段是各分段墙钟之和（分段串行，
/// 不含与编码线程的重叠）；计时只覆盖 staged_body 内的引擎分段，调用方
/// 包装（写回装配、日志）不在其中。
#[derive(Default, Clone, Copy, Debug)]
pub struct TickDiag {
    pub prepare_ns: u128,
    /// 房间候选整树克隆（world.room(name).clone() 逐房求和）。
    pub clone_ns: u128,
    /// settle_room 规则执行（克隆与钩子单列）。
    pub settle_ns: u128,
    /// 逐房结算后回调（宿主预编码触发）。
    pub hook_ns: u128,
    /// portal 收集与跨房移动应用。
    pub interroom_ns: u128,
    pub nukes_ns: u128,
    pub market_ns: u128,
    pub power_ns: u128,
    /// 玩家写回候选应用。
    pub writeback_ns: u128,
    pub validate_ns: u128,
    /// 候选收敛（into_prepared：通知装配与玩家房间集重建）。
    pub into_prepared_ns: u128,
    /// 世界提交（commit_tick_rooms：逐槽替换与旧房 drop）。
    pub commit_ns: u128,
    pub counters: DiagCounters,
    /// 本拍房间循环是否走了并行路径（串行回退重做记 false）。
    pub parallel: bool,
    pub interroom_moves: u64,
    pub portal_moves: u64,
    pub events: u64,
    pub rooms: u64,
}

impl crate::settle::Engine {
    /// 结算分段诊断是否开启（构造时按 `KK_SETTLE_DIAG` 环境变量定格）。
    pub fn settle_diag_enabled(&self) -> bool {
        self.settle_diag_on
    }

    pub(crate) fn record_settle_diag(&mut self, diag: TickDiag) {
        if self.settle_diag_on {
            self.settle_diag_samples.push(diag);
        }
    }

    /// 已收集逐拍样本的中位数分段报告（毫秒）与耦合计数总和；
    /// 无样本（未开启或已清空）时返回 None。采集只在开关开启时进行。
    pub fn settle_diag_report(&self) -> Option<serde_json::Value> {
        if self.settle_diag_samples.is_empty() {
            return None;
        }
        let samples = &self.settle_diag_samples;
        let median = |pick: &dyn Fn(&TickDiag) -> u128| -> f64 {
            let mut v: Vec<u128> = samples.iter().map(pick).collect();
            v.sort_unstable();
            v[v.len() / 2] as f64 / 1e6
        };
        let seg = |pick: &dyn Fn(&TickDiag) -> u128| -> serde_json::Value {
            serde_json::json!({ "p50_ms": median(pick) })
        };
        let sum = |pick: &dyn Fn(&TickDiag) -> u64| -> u64 { samples.iter().map(pick).sum() };
        Some(serde_json::json!({
            "ticks": samples.len(),
            "segments_p50_ms": {
                "prepare": seg(&|d| d.prepare_ns),
                "clone": seg(&|d| d.clone_ns),
                "settle": seg(&|d| d.settle_ns),
                "hook": seg(&|d| d.hook_ns),
                "interroom": seg(&|d| d.interroom_ns),
                "nukes": seg(&|d| d.nukes_ns),
                "market": seg(&|d| d.market_ns),
                "power": seg(&|d| d.power_ns),
                "writeback": seg(&|d| d.writeback_ns),
                "validate": seg(&|d| d.validate_ns),
                "into_prepared": seg(&|d| d.into_prepared_ns),
                "commit": seg(&|d| d.commit_ns),
            },
            "counters_total": {
                "id_allocs": sum(&|d| d.counters.id_allocs),
                "id_restores": sum(&|d| d.counters.id_restores),
                "claim_calls": sum(&|d| d.counters.claim_calls),
                "gcl_adds": sum(&|d| d.counters.gcl_adds),
                "power_adds": sum(&|d| d.counters.power_adds),
                "parallel_room_ticks": sum(&|d| d.parallel as u64),
                "interroom_moves": sum(&|d| d.interroom_moves),
                "portal_moves": sum(&|d| d.portal_moves),
                "events": sum(&|d| d.events),
                "rooms": sum(&|d| d.rooms),
            },
        }))
    }

    /// 清空结算分段诊断样本（对照探针排除热身拍后重开窗用）。
    pub fn settle_diag_reset(&mut self) {
        self.settle_diag_samples.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settle::Engine;
    use kk_world::World;

    /// 报告在中位数口径下给出全部分段与计数；关闭时返回 None。
    #[test]
    fn report_covers_all_segments_or_is_none() {
        let mut engine = Engine::new(World::new(0));
        assert!(engine.settle_diag_report().is_none());
        let samples = [3u128, 1, 2]; // 中位数 2
        let base = TickDiag {
            prepare_ns: samples[0],
            counters: DiagCounters {
                id_allocs: 1,
                ..DiagCounters::default()
            },
            rooms: 4,
            ..TickDiag::default()
        };
        engine.settle_diag_samples.push(base);
        let mut second = base;
        second.prepare_ns = samples[1];
        engine.settle_diag_samples.push(second);
        let mut third = base;
        third.prepare_ns = samples[2];
        engine.settle_diag_samples.push(third);
        let report = engine.settle_diag_report().expect("有样本必有报告");
        assert_eq!(report["ticks"], 3);
        assert_eq!(report["segments_p50_ms"]["prepare"]["p50_ms"], 0.000_002);
        assert_eq!(report["counters_total"]["id_allocs"], 3);
        assert_eq!(report["counters_total"]["rooms"], 12);
        engine.settle_diag_reset();
        assert!(engine.settle_diag_report().is_none());
    }
}
