//! `mod test`：对 Mod 根下自带场景的包逐个运行断言并汇总。
//!
//! 每个包含 scenario.json 的包走完整 `sim run` 路径（真实注册、结算
//! 与断言），无场景的包记为 skipped；汇总报告内联各场景的机器结果。
//! 场景按包的真实目录定位（目录名可与 manifest id 不同），退出码取
//! 各场景退出码的最大值：0 全过、1 装载失败、2 运行或断言失败。

use crate::report::write_report;
use crate::sim;
use serde_json::{Value, json};
use std::path::PathBuf;

pub struct Options {
    /// Mod 根目录；其直接子目录按包发现，无场景的包记 skipped。
    pub root: PathBuf,
    /// 每个场景推进的 tick 数；默认 3 与模板场景断言匹配。
    pub ticks: u32,
    /// 二进制投影通道（与 sim run --kkpv 同义）。
    pub kkpv: bool,
    /// 开启 Mod 运行 trace（与 sim run --mod-trace 同义）。
    pub mod_trace: bool,
    /// 汇总报告输出路径（.json）。
    pub report: PathBuf,
}

pub fn run(options: Options) -> i32 {
    if let Err(error) = crate::output_guard::json_report(&options.report, &options.root, None) {
        eprintln!("mod test: {error:#}");
        return 1;
    }
    match test(&options) {
        Ok((code, report)) => {
            if let Err(error) = write_report(&options.report, &report) {
                eprintln!("mod test 报告写入失败: {error:#}");
                return 1;
            }
            print_summary(&report, &options.report);
            code
        }
        Err(error) => {
            // 装载期失败同样落盘汇总，与 mod inspect 的 setup_failed 惯例一致。
            let report = json!({
                "report_version": 1,
                "tool": "kkscreeps mod test",
                "root": options.root,
                "kkpv": options.kkpv,
                "status": "setup_failed",
                "error": format!("{error:#}"),
            });
            if let Err(write_error) = write_report(&options.report, &report) {
                eprintln!("mod test 报告写入失败: {write_error:#}");
            }
            eprintln!("mod test: {error:#}");
            1
        }
    }
}

fn print_summary(report: &Value, report_path: &std::path::Path) {
    for result in report["results"].as_array().unwrap_or(&Vec::new()) {
        let scenario = result["scenario"].as_str().unwrap_or_default();
        let suffix = if scenario.is_empty() {
            String::new()
        } else {
            format!("（{scenario}）")
        };
        println!(
            "mod test: {} {}{suffix}",
            result["id"].as_str().unwrap_or("?"),
            result["outcome"].as_str().unwrap_or("?"),
        );
    }
    println!(
        "mod test：{}，报告 {}",
        report["summary"].as_str().unwrap_or(""),
        report_path.display()
    );
}

fn test(options: &Options) -> anyhow::Result<(i32, Value)> {
    use kk_mod_host::discover_packages;
    anyhow::ensure!(
        options.root.is_dir(),
        "Mod 根 {} 不是目录",
        options.root.display()
    );
    let packages = discover_packages(&options.root)?;
    anyhow::ensure!(
        !packages.is_empty(),
        "Mod 根 {} 下没有发现包",
        options.root.display()
    );
    let mut results = Vec::new();
    let mut worst = 0i32;
    let mut passed = 0usize;
    let mut skipped = 0usize;
    // 逐场景串行：包数量少，且避免并发临时报告名冲突。场景按包的
    // 真实目录定位——目录名可与 manifest id 不同，改名不改变测试对象。
    for (id, package) in &packages {
        let scenario = package.root.join("scenario.json");
        if !scenario.is_file() {
            skipped += 1;
            results.push(json!({
                "id": id,
                "outcome": "skipped",
                "reason": "包内没有 scenario.json",
            }));
            continue;
        }
        let temporary = TemporaryReport::new()?;
        let detail_path = temporary.0.join("scene.json");
        let code = sim::run(sim::Options {
            scenario: scenario.clone(),
            ticks: options.ticks,
            concurrency: 1,
            kkpv: options.kkpv,
            report: detail_path.clone(),
            save_checkpoint: None,
            load_checkpoint: None,
            mod_trace: options.mod_trace,
            input_log: None,
            verify_replay: false,
        });
        let detail: Value = serde_json::from_slice(&std::fs::read(&detail_path)?)?;
        let outcome = if code == 0 {
            passed += 1;
            "passed"
        } else {
            worst = worst.max(code);
            "failed"
        };
        results.push(json!({
            "id": id,
            "outcome": outcome,
            "scenario": scenario,
            "exit_code": code,
            "detail": detail,
        }));
    }
    let failed = results.len() - passed - skipped;
    let summary = format!(
        "{passed} 通过 / {failed} 失败 / {skipped} 跳过（ticks={}，多包失败取最严重退出码）",
        options.ticks
    );
    Ok((
        worst,
        json!({
            "report_version": 1,
            "tool": "kkscreeps mod test",
            "root": options.root,
            "kkpv": options.kkpv,
            "mod_trace": options.mod_trace,
            "summary": summary,
            "results": results,
        }),
    ))
}

/// 原子占用私有临时目录，避免 PID 重用时读取旧结果或替换他人的临时报告。
struct TemporaryReport(PathBuf);
impl TemporaryReport {
    fn new() -> anyhow::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "kkscreeps-mod-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("无法占用场景报告临时目录")
    }
}
impl Drop for TemporaryReport {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
