//! 汇总延迟和每轮耗时，并定义 JSON 报告字段。
//!
//! 操作延迟合并所有计时轮的样本，再计算分位数；每轮耗时单独计算中位数。
//! 预热样本由运行器排除，本模块不执行计时。

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

/// 对已排序样本取第 ⌈p·N⌉ 项（nearest-rank），空样本返回 0。
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((p * sorted.len() as f64).ceil() as usize).saturating_sub(1);
    sorted[idx.min(sorted.len() - 1)]
}

/// 汇总微秒样本，空样本的所有统计量为 0；调用方应传入有限且非负的耗时。
/// 分位数使用 nearest-rank，偶数个样本的 p50 取中间两项中较小的一项。
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

/// 汇总每轮毫秒样本，使用与 [`latency_stats`] 相同的分位数规则。
/// 本函数只统计耗时，吞吐由运行器根据数据量计算。
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
    /// 数据来源和结果适用范围的说明。
    pub note: String,
    pub tier: String,
    /// 每个场景的计时轮数。
    pub rounds: usize,
    /// 每个场景丢弃的预热轮数。
    pub warmup: usize,
    /// 点读场景每轮抽取的键数，允许重复。
    pub read_ops: usize,
    pub durability_profiles: Vec<String>,
    pub engines: Vec<String>,
    pub os: String,
    pub arch: String,
    /// UTC 生成时间，格式为 `YYYY-MM-DDTHH:MM:SSZ`，不参与性能计算。
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
    /// 完整数据集的键数；前缀扫描的实际命中条数写在 note 中。
    pub dataset_keys: usize,
    /// 完整数据集的值字节总数，不含键；前缀扫描时也不是仅命中部分的大小。
    pub dataset_bytes: u64,
    /// 每操作延迟场景的分布。
    pub latency: Option<LatencyStats>,
    /// 每轮聚合场景的跨轮分布。
    pub per_round: Option<RoundStats>,
    /// 批量写使用末轮完整值字节数，扫描使用末轮命中值字节数，
    /// 均除以中位轮耗时，单位字节/秒；每操作场景为空。
    pub bytes_per_sec: Option<f64>,
    /// 点读和逐键写为 `1e6 / p50_us`，批量写为 `1e3 / median_ms`（事务/秒）；
    /// 扫描为空。这是典型耗时的倒数，不是总操作数除以总时间的实测吞吐。
    pub ops_per_sec: Option<f64>,
    /// 场景末同步后的整个数据库文件大小，包含此前场景的数据和已分配空间。
    pub db_file_len: u64,
    /// 场景的计时与校验说明。
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
