//! bin `kk-storage-bench`：微基准编排——固定 seed 夹具 → 双引擎 × 双持久化
//! 档位 × 场景矩阵 → 机器可读 JSON + 人读摘要。
//!
//! **非正式选型**（ADR-008 第 7 条）：本工具只建立可重复的测量方法学与初轮
//! 本机数字，正式选型待 2A 落地后用真实负载跑。
//!
//! 用法（引擎依赖须显式开 feature）：
//!
//! ```text
//! cargo run -p kk-storage-bench --release --features redb,libmdbx -- \
//!   --tier t3 --rounds 5 --json target/storage-bench/report.json
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use kk_core::rng::SeededRng;

use kk_storage_bench::engine::{
    DurabilityProfile, StorageEngine, TableKind, available_engines, open_engine,
};
use kk_storage_bench::fixtures::{self, Tier, WorldFixture};
use kk_storage_bench::harness::{
    BenchReport, ReportMeta, ScenarioResult, latency_stats, round_stats,
};

/// 命令行配置（手动解析——基准工具不引 clap，依赖面保持最小）。
struct Config {
    tier: Tier,
    rounds: usize,
    warmup: usize,
    read_ops: usize,
    profiles: Vec<DurabilityProfile>,
    engines: Vec<String>,
    json: PathBuf,
    data_dir: PathBuf,
    keep: bool,
}

fn next_value<I: Iterator<Item = String>>(args: &mut I, flag: &str) -> String {
    args.next().unwrap_or_else(|| panic!("缺少 {flag} 的值"))
}

fn parse_args() -> anyhow::Result<Config> {
    let mut args = std::env::args().skip(1);
    let mut tier = Tier::T3;
    let mut rounds = 5usize;
    let mut warmup = 1usize;
    let mut read_ops = 500usize;
    let mut profiles = vec![DurabilityProfile::Durable, DurabilityProfile::Relaxed];
    let mut engines: Option<Vec<String>> = None;
    let mut json = PathBuf::from("target/storage-bench/report.json");
    let mut data_dir = PathBuf::from("target/storage-bench/data");
    let mut keep = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tier" => {
                tier = Tier::parse(&next_value(&mut args, "--tier"))
                    .context("未知 --tier：可选 t1/t2/t3")?;
            }
            "--rounds" => rounds = next_value(&mut args, "--rounds").parse()?,
            "--warmup" => warmup = next_value(&mut args, "--warmup").parse()?,
            "--read-ops" => read_ops = next_value(&mut args, "--read-ops").parse()?,
            "--profiles" => {
                profiles = next_value(&mut args, "--profiles")
                    .split(',')
                    .map(|s| DurabilityProfile::parse(s).with_context(|| format!("未知档位 {s}")))
                    .collect::<anyhow::Result<_>>()?;
            }
            "--engines" => {
                engines = Some(
                    next_value(&mut args, "--engines")
                        .split(',')
                        .map(str::to_string)
                        .collect(),
                );
            }
            "--json" => json = PathBuf::from(next_value(&mut args, "--json")),
            "--data-dir" => data_dir = PathBuf::from(next_value(&mut args, "--data-dir")),
            "--keep" => keep = true,
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            other => bail!("未知参数 {other:?}（--help 看用法）"),
        }
    }

    let available = available_engines();
    let engines = engines.unwrap_or_else(|| available.iter().map(|s| s.to_string()).collect());
    let missing: Vec<&str> = engines
        .iter()
        .map(String::as_str)
        .filter(|e| !available.contains(e))
        .collect();
    if !missing.is_empty() {
        bail!(
            "引擎不可用（feature 未启用）：{}；可用：{}。构建加 --features redb,libmdbx",
            missing.join(", "),
            available.join(", ")
        );
    }
    Ok(Config {
        tier,
        rounds,
        warmup,
        read_ops,
        profiles,
        engines,
        json,
        data_dir,
        keep,
    })
}

fn print_help() {
    println!(
        "kk-storage-bench —— 存储选型微基准（M2-C3，非正式选型）\n\
         \n\
         用法：cargo run -p kk-storage-bench --release --features redb,libmdbx -- [--flags]\n\
         \n\
         --tier t1|t2|t3      规模档（默认 t3：100 房/100 玩家；t1 单房；t2 12 房）\n\
         --rounds N           计时轮数（默认 5，取中位）\n\
         --warmup N           预热轮数（默认 1，不计时）\n\
         --read-ops N         点读场景每轮读键数（默认 500）\n\
         --profiles a,b       durable,relaxed（默认两者）\n\
         --engines a,b        redb,libmdbx（默认全部可用项）\n\
         --json PATH          机器可读报告输出（默认 target/storage-bench/report.json）\n\
         --data-dir DIR       引擎数据目录（默认 target/storage-bench/data）\n\
         --keep               保留数据目录（默认跑完删除）"
    );
}

/// epoch → UTC 年月日时分秒（civil_from_days 算法；仅供人读的时间戳，不引 chrono）。
fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + i64::from(m <= 2);
    let rem = secs % 86400;
    format!(
        "{y:04}-{:02}-{d:02}T{:02}:{:02}:{:02}Z",
        m,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 单引擎 × 单档位的场景运行器：一组场景共写一份引擎数据目录（互不干扰的
/// 表），每个场景产出一条 [`ScenarioResult`]。
struct Runner<'a> {
    engine_name: String,
    profile: DurabilityProfile,
    rounds: usize,
    warmup: usize,
    read_ops: usize,
    results: &'a mut Vec<ScenarioResult>,
}

impl Runner<'_> {
    fn push(&mut self, mut result: ScenarioResult) {
        result.engine = self.engine_name.clone();
        result.profile = self.profile.name().to_string();
        self.results.push(result);
    }

    /// 批量写场景（检查点形态）：warmup + rounds 轮，每轮一个写事务写全量。
    /// warmup 轮号从 `rounds` 起排——与计时轮（0..rounds）的数据集**不相交**：
    /// 同值重写会被 mdbx 判为无脏页事务而 no-op commit（初轮实测教训），
    /// warmup 与计时首轮同值会把假绿样本混进计时分布。
    fn bulk_write(
        &mut self,
        engine: &mut dyn StorageEngine,
        scenario: &str,
        table: TableKind,
        dataset_name: &str,
        make: &dyn Fn(u64) -> fixtures::Dataset,
        note: &str,
    ) -> anyhow::Result<()> {
        for w in 0..self.warmup {
            engine.write_batch(table, &make((self.rounds + w) as u64).pairs)?;
        }
        let mut ms = Vec::with_capacity(self.rounds);
        let mut last = make(0);
        for r in 0..self.rounds {
            let ds = make(r as u64);
            let start = Instant::now();
            engine.write_batch(table, &ds.pairs)?;
            ms.push(start.elapsed().as_secs_f64() * 1e3);
            last = ds;
        }
        let rs = round_stats(&ms);
        let bytes = last.total_bytes();
        let file_len = Self::finish(engine)?;
        self.push(ScenarioResult {
            scenario: scenario.to_string(),
            engine: String::new(),
            profile: String::new(),
            table: table.name().to_string(),
            dataset: dataset_name.to_string(),
            dataset_keys: last.pairs.len(),
            dataset_bytes: bytes,
            latency: None,
            per_round: Some(rs.clone()),
            bytes_per_sec: Some(bytes as f64 / (rs.median_ms / 1e3).max(f64::EPSILON)),
            ops_per_sec: Some(1e3 / rs.median_ms.max(f64::EPSILON)),
            db_file_len: file_len,
            note: Some(note.to_string()),
        });
        Ok(())
    }

    /// 逐键提交场景：每键独立事务；warmup 轮丢弃、计时轮样本合并。warmup
    /// 轮号同样从 `rounds` 起排（与计时轮数据集不相交，理由见 [`Self::bulk_write`]）。
    fn per_key_txn(
        &mut self,
        engine: &mut dyn StorageEngine,
        scenario: &str,
        table: TableKind,
        dataset_name: &str,
        make: &dyn Fn(u64) -> fixtures::Dataset,
        note: &str,
    ) -> anyhow::Result<()> {
        for w in 0..self.warmup {
            engine.write_per_key_txn(table, &make((self.rounds + w) as u64).pairs)?;
        }
        let mut micros = Vec::new();
        let mut last = make(0);
        for r in 0..self.rounds {
            let ds = make(r as u64);
            micros.extend(engine.write_per_key_txn(table, &ds.pairs)?);
            last = ds;
        }
        let ls = latency_stats(&micros);
        let file_len = Self::finish(engine)?;
        self.push(ScenarioResult {
            scenario: scenario.to_string(),
            engine: String::new(),
            profile: String::new(),
            table: table.name().to_string(),
            dataset: dataset_name.to_string(),
            dataset_keys: last.pairs.len(),
            dataset_bytes: last.total_bytes(),
            latency: Some(ls.clone()),
            per_round: None,
            bytes_per_sec: None,
            // p50 派生（非 mean）：逐条提交的样本带 fsync 风暴重尾，mean 会被
            // 单次离群拉偏
            ops_per_sec: Some(1e6 / ls.p50_us.max(f64::EPSILON)),
            db_file_len: file_len,
            note: Some(note.to_string()),
        });
        Ok(())
    }

    /// 点读场景：先全量预置（不计时），再逐轮随机抽键（有放回、均匀）。
    /// 读回字节总数与按抽样键算出的期望合计对账（合计口径而非逐字节——
    /// 缺键在引擎内即报错）。
    fn point_read(
        &mut self,
        engine: &mut dyn StorageEngine,
        scenario: &str,
        table: TableKind,
        dataset: &fixtures::Dataset,
    ) -> anyhow::Result<()> {
        engine.write_batch(table, &dataset.pairs)?;
        let value_len: HashMap<Vec<u8>, u64> = dataset
            .pairs
            .iter()
            .map(|(k, v)| (k.clone(), v.len() as u64))
            .collect();
        let mut rng = SeededRng::new(0xC3_C0FFEE);
        let mut micros = Vec::with_capacity(self.read_ops * self.rounds);
        for r in 0..self.warmup + self.rounds {
            let keys: Vec<Vec<u8>> = (0..self.read_ops)
                .map(|_| {
                    dataset.pairs[rng.below(dataset.pairs.len() as u32) as usize]
                        .0
                        .clone()
                })
                .collect();
            let round_expected: u64 = keys.iter().map(|k| value_len[k]).sum();
            let refs: Vec<&[u8]> = keys.iter().map(|k| k.as_slice()).collect();
            let (samples, bytes) = engine.read_point(table, &refs)?;
            if r >= self.warmup {
                micros.extend(samples);
            }
            if bytes != round_expected {
                bail!(
                    "{scenario}/{} 第 {r} 轮读回 {bytes} 字节 != 期望 {round_expected}",
                    self.engine_name
                );
            }
        }
        let ls = latency_stats(&micros);
        let file_len = Self::finish(engine)?;
        self.push(ScenarioResult {
            scenario: scenario.to_string(),
            engine: String::new(),
            profile: String::new(),
            table: table.name().to_string(),
            dataset: dataset.name.clone(),
            dataset_keys: dataset.pairs.len(),
            dataset_bytes: dataset.total_bytes(),
            latency: Some(ls.clone()),
            per_round: None,
            bytes_per_sec: None,
            ops_per_sec: Some(1e6 / ls.p50_us.max(f64::EPSILON)),
            db_file_len: file_len,
            note: Some(format!(
                "每键独立只读事务；每轮 {} 键（有放回均匀抽取），读回字节总数按抽样期望合计对账",
                self.read_ops
            )),
        });
        Ok(())
    }

    /// 前缀扫描场景（每轮条数断言——扫描正确性随基准自验）。
    fn scan(
        &mut self,
        engine: &mut dyn StorageEngine,
        scenario: &str,
        table: TableKind,
        dataset: &fixtures::Dataset,
        prefix: &[u8],
    ) -> anyhow::Result<()> {
        engine.write_batch(table, &dataset.pairs)?;
        let expect_keys = dataset
            .pairs
            .iter()
            .filter(|(k, _)| k.starts_with(prefix))
            .count();
        let mut ms = Vec::with_capacity(self.rounds);
        let mut last_bytes = 0u64;
        for r in 0..self.warmup + self.rounds {
            let (elapsed, count, bytes) = engine.scan_prefix(table, prefix)?;
            // 每轮校验条数（不只看末轮——任何一轮的错误结果都不许进计时）
            if count != expect_keys {
                bail!(
                    "{scenario}/{} 第 {r} 轮扫描条数 {count} != 预期 {expect_keys}",
                    self.engine_name
                );
            }
            if r >= self.warmup {
                ms.push(elapsed);
                last_bytes = bytes;
            }
        }
        let rs = round_stats(&ms);
        let file_len = Self::finish(engine)?;
        self.push(ScenarioResult {
            scenario: scenario.to_string(),
            engine: String::new(),
            profile: String::new(),
            table: table.name().to_string(),
            dataset: dataset.name.clone(),
            dataset_keys: dataset.pairs.len(),
            dataset_bytes: dataset.total_bytes(),
            latency: None,
            per_round: Some(rs.clone()),
            bytes_per_sec: Some(last_bytes as f64 / (rs.median_ms / 1e3).max(f64::EPSILON)),
            ops_per_sec: None,
            db_file_len: file_len,
            note: Some(format!(
                "前缀 {:?}（空 = 全表），命中 {expect_keys} 键（每轮断言）",
                String::from_utf8_lossy(prefix)
            )),
        });
        Ok(())
    }

    /// 冲刷 + 记录文件体积（文件体积是选型判据之一）。
    fn finish(engine: &dyn StorageEngine) -> anyhow::Result<u64> {
        engine.flush()?;
        engine.file_len()
    }
}

fn fmt_us(v: f64) -> String {
    if v >= 1e6 {
        format!("{:.2}s", v / 1e6)
    } else if v >= 1000.0 {
        format!("{:.2}ms", v / 1e3)
    } else {
        format!("{v:.1}µs")
    }
}

fn fmt_bytes(v: f64) -> String {
    if v >= 1e9 {
        format!("{:.2}GB", v / 1e9)
    } else if v >= 1e6 {
        format!("{:.2}MB", v / 1e6)
    } else if v >= 1e3 {
        format!("{:.1}KB", v / 1e3)
    } else {
        format!("{v:.0}B")
    }
}

fn main() -> anyhow::Result<()> {
    let cfg = parse_args()?;
    println!("== kk-storage-bench（M2-C3 基准骨架 | 非正式选型，正式选型待 2A 后真实负载）==");
    let available = available_engines();
    if available.is_empty() {
        bail!("无可用引擎（feature 均未启用）。构建加 --features redb,libmdbx（或任一）");
    }
    println!(
        "tier={} rooms/players={}/{} rounds={} warmup={} read_ops={} engines=[{}] profiles=[{}]",
        cfg.tier.name(),
        cfg.tier.rooms(),
        cfg.tier.players(),
        cfg.rounds,
        cfg.warmup,
        cfg.read_ops,
        cfg.engines.join(","),
        cfg.profiles
            .iter()
            .map(DurabilityProfile::name)
            .collect::<Vec<_>>()
            .join(",")
    );

    // 夹具（一次构建、全场景复用；逐字节确定性由 fixtures 测试锁定）
    let fix = fixtures::build_world(cfg.tier);
    print_fixture_stats(&fix);
    let rooms_ds = fixtures::rooms_dataset(&fix, 0, cfg.tier);
    let users_ds = fixtures::users_dataset(&fix, 0, cfg.tier);
    let blobs_ds = fixtures::blobs_dataset(&fix, cfg.tier, 0);

    let run_root = cfg.data_dir.join(format!(
        "run-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
    ));
    fs::create_dir_all(&run_root)?;

    let mut results: Vec<ScenarioResult> = Vec::new();
    let rooms = |round: u64| fixtures::rooms_dataset(&fix, round, cfg.tier);
    let users = |round: u64| fixtures::users_dataset(&fix, round, cfg.tier);
    let blobs = |round: u64| fixtures::blobs_dataset(&fix, cfg.tier, round);

    for engine_name in &cfg.engines {
        for profile in &cfg.profiles {
            let dir = run_root.join(format!("{engine_name}-{profile}"));
            let mut engine = open_engine(engine_name, &dir, *profile)?;
            println!("-- {engine_name} / {profile}");
            let mut runner = Runner {
                engine_name: engine_name.clone(),
                profile: *profile,
                rounds: cfg.rounds,
                warmup: cfg.warmup,
                read_ops: cfg.read_ops,
                results: &mut results,
            };

            // 房间快照：检查点批量写 / 逐房事务写 / 点读 / 全表扫描 / 前缀扫描
            runner.bulk_write(
                engine.as_mut(),
                "rooms_checkpoint_bulk_write",
                TableKind::Rooms,
                &rooms_ds.name,
                &rooms,
                "一次检查点 = 一个大事务全量重写（值同长不同内容）",
            )?;
            runner.per_key_txn(
                engine.as_mut(),
                "rooms_per_room_txn_write",
                TableKind::Rooms,
                &rooms_ds.name,
                &rooms,
                "逐房独立事务（对照批量提交的写放大）",
            )?;
            runner.point_read(
                engine.as_mut(),
                "rooms_point_read",
                TableKind::Rooms,
                &rooms_ds,
            )?;
            runner.scan(
                engine.as_mut(),
                "rooms_scan_all",
                TableKind::Rooms,
                &rooms_ds,
                b"",
            )?;
            runner.scan(
                engine.as_mut(),
                "rooms_scan_prefix",
                TableKind::Rooms,
                &rooms_ds,
                b"W1",
            )?;

            // 玩家记录：检查点批量 + 点读
            runner.bulk_write(
                engine.as_mut(),
                "users_checkpoint_bulk_write",
                TableKind::Users,
                &users_ds.name,
                &users,
                "一次检查点 = 一个大事务重写全部玩家小记录",
            )?;
            runner.point_read(
                engine.as_mut(),
                "users_point_read",
                TableKind::Users,
                &users_ds,
            )?;

            // 大 value：批量 + 逐条事务 + 点读
            runner.bulk_write(
                engine.as_mut(),
                "blobs_bulk_write",
                TableKind::Blobs,
                &blobs_ds.name,
                &blobs,
                "一个事务写全部 Memory blob（百 KB～MB 级）",
            )?;
            runner.per_key_txn(
                engine.as_mut(),
                "blobs_per_key_txn_write",
                TableKind::Blobs,
                &blobs_ds.name,
                &blobs,
                "每 blob 独立事务（逐玩家提交形态）",
            )?;
            runner.point_read(
                engine.as_mut(),
                "blobs_point_read",
                TableKind::Blobs,
                &blobs_ds,
            )?;

            let final_len = engine.close()?;
            println!("   db file: {}", fmt_bytes(final_len as f64));
        }
    }

    // 人类可读摘要（初轮本机量级；正式对比以 JSON 报告为准）
    println!("\n== 摘要（场景 | 引擎@档位 | 指标）==");
    for r in &results {
        let metric = if let Some(per_round) = &r.per_round {
            format!(
                "{:>12} /轮  {:>10}/s  ({} 键/轮)",
                fmt_us(per_round.median_ms * 1e3),
                fmt_bytes(r.bytes_per_sec.unwrap_or(0.0)),
                r.dataset_keys
            )
        } else if let Some(lat) = &r.latency {
            format!(
                "p50 {:>9}  p99 {:>9}  {:>7.0} op/s",
                fmt_us(lat.p50_us),
                fmt_us(lat.p99_us),
                r.ops_per_sec.unwrap_or(0.0)
            )
        } else {
            "-".to_string()
        };
        println!(
            "{:<30} {:10}@{:8} {metric}",
            r.scenario, r.engine, r.profile
        );
    }

    let report = BenchReport {
        meta: ReportMeta {
            note: "非正式选型基准（M2-C3 骨架）：正式选型待 2A 落地后以真实负载跑".to_string(),
            tier: cfg.tier.name().to_string(),
            rounds: cfg.rounds,
            warmup: cfg.warmup,
            read_ops: cfg.read_ops,
            durability_profiles: cfg.profiles.iter().map(|p| p.name().to_string()).collect(),
            engines: cfg.engines.clone(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            generated_at: now_iso(),
        },
        results,
    };
    if let Some(parent) = cfg.json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&cfg.json, serde_json::to_string_pretty(&report)?)?;
    println!("\nJSON 报告：{}", cfg.json.display());

    if !cfg.keep {
        fs::remove_dir_all(&run_root).ok();
    }
    Ok(())
}

/// 夹具统计：房间快照尺寸分布（min/中位/max）与总量。
fn print_fixture_stats(fix: &WorldFixture) {
    let mut lens: Vec<usize> = fix
        .room_names
        .iter()
        .map(|r| fix.room_bytes(r, fixtures::BASE_TIME).len())
        .collect();
    lens.sort_unstable();
    let total: usize = lens.iter().sum();
    println!(
        "fixture: {} 房间快照共 {}（min {} / 中位 {} / max {}），玩家 {} 名",
        lens.len(),
        fmt_bytes(total as f64),
        fmt_bytes(lens[0] as f64),
        fmt_bytes(lens[lens.len() / 2] as f64),
        fmt_bytes(lens[lens.len() - 1] as f64),
        fix.players.len(),
    );
}
