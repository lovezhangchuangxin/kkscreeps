//! 计时与统计：固定 warmup + 多轮取中位的跑法（对齐 performance-acceptance.md
//! 「预热后采集、独立多轮、报中位与分位、不只挑最好一次」的口径，微基准
//! 规模下预热从 500 tick 等比缩到 warmup 轮），机器可读 JSON 报告模型同层。

use serde::Serialize;

/// 每操作延迟分布（µs）。
#[derive(Debug, Clone, Serialize)]
pub struct LatencyStats {
    pub count: usize,
    pub mean_us: f64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub min_us: f64,
    pub max_us: f64,
}

/// 就近取秩分位（nearest-rank：第 ⌈p·N⌉ 个次序统计量）。样本 < 1 返回 0。
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p * sorted.len() as f64).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

/// 汇总延迟样本。
pub fn latency_stats(micros: &[f64]) -> LatencyStats {
    let mut sorted = micros.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("样本含 NaN"));
    let mean = if micros.is_empty() {
        0.0
    } else {
        micros.iter().sum::<f64>() / micros.len() as f64
    };
    LatencyStats {
        count: micros.len(),
        mean_us: mean,
        p50_us: percentile(&sorted, 0.50),
        p95_us: percentile(&sorted, 0.95),
        p99_us: percentile(&sorted, 0.99),
        min_us: sorted.first().copied().unwrap_or(0.0),
        max_us: sorted.last().copied().unwrap_or(0.0),
    }
}

/// 每轮聚合值（如一轮检查点的总毫秒）的跨轮分布。
#[derive(Debug, Clone, Serialize)]
pub struct RoundStats {
    pub rounds: usize,
    pub median_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

/// 汇总每轮毫秒样本，另给出基于中位轮的吞吐（每秒轮数据字节数）。
pub fn round_stats(ms_per_round: &[f64]) -> RoundStats {
    let mut sorted = ms_per_round.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("样本含 NaN"));
    RoundStats {
        rounds: ms_per_round.len(),
        median_ms: percentile(&sorted, 0.50),
        min_ms: sorted.first().copied().unwrap_or(0.0),
        max_ms: sorted.last().copied().unwrap_or(0.0),
    }
}

/// 机器可读报告的元信息。
#[derive(Debug, Clone, Serialize)]
pub struct ReportMeta {
    /// 报告定位声明：非正式选型（ADR-008 第 7 条——正式选型待 2A 后真实负载）。
    pub note: String,
    pub tier: String,
    pub rounds: usize,
    pub warmup: usize,
    pub read_ops: usize,
    pub durability_profiles: Vec<String>,
    pub engines: Vec<String>,
    pub os: String,
    pub arch: String,
    /// 生成时刻（本地时区 ISO-8601 近似；仅供人读，不参与比较）。
    pub generated_at: String,
}

/// 单场景单引擎单档位的结果。
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioResult {
    pub scenario: String,
    pub engine: String,
    pub profile: String,
    pub table: String,
    /// 数据集名（含规模信息，如 `rooms@t3`）。
    pub dataset: String,
    pub dataset_keys: usize,
    pub dataset_bytes: u64,
    /// 每操作延迟场景的分布。
    pub latency: Option<LatencyStats>,
    /// 每轮聚合场景的跨轮分布。
    pub per_round: Option<RoundStats>,
    /// 基于中位轮的数据吞吐（字节/秒；每操作场景为空）。
    pub bytes_per_sec: Option<f64>,
    /// 每操作场景的操作吞吐（op/s），按 p50 派生——逐条提交样本带 fsync
    /// 重尾，mean 会被单次离群拉偏。
    pub ops_per_sec: Option<f64>,
    /// 场景末引擎数据文件字节数。
    pub db_file_len: u64,
    /// 口径附注（如逐条提交的事务边界含义）。
    pub note: Option<String>,
}

/// 完整报告。
#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub meta: ReportMeta,
    pub results: Vec<ScenarioResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        // 1..=100：p50=50、p95=95、p99=99（就近取秩）
        let data: Vec<f64> = (1..=100).map(f64::from).collect();
        let s = latency_stats(&data);
        assert_eq!(s.p50_us, 50.0);
        assert_eq!(s.p95_us, 95.0);
        assert_eq!(s.p99_us, 99.0);
        assert_eq!(s.mean_us, 50.5);
        assert_eq!(s.min_us, 1.0);
        assert_eq!(s.max_us, 100.0);
        assert_eq!(s.count, 100);
    }

    #[test]
    fn stats_empty_and_small() {
        let s = latency_stats(&[]);
        assert_eq!((s.count, s.p50_us), (0, 0.0));
        let s = latency_stats(&[3.5]);
        assert_eq!((s.p50_us, s.p95_us, s.max_us), (3.5, 3.5, 3.5));
        let r = round_stats(&[10.0, 2.0, 6.0]);
        assert_eq!((r.median_ms, r.min_ms, r.max_ms), (6.0, 2.0, 10.0));
    }
}
