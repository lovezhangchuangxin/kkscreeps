//! 模拟命令入口：先保护输入路径，再执行场景并发布报告。
use crate::support::write_report;
use serde_json::json;
use std::path::PathBuf;

mod assertions;
mod auto_checkpoint;
mod checkpoint;
mod command;
mod execution;
pub(crate) mod mods;
mod observation;
mod replay;
pub(crate) mod scene;
pub(crate) use command::SimCommand;

pub struct Options {
    pub scenario: PathBuf,
    pub ticks: u32,
    pub concurrency: usize,
    pub kkpv: bool,
    pub report: PathBuf,
    pub save_checkpoint: Option<PathBuf>,
    pub load_checkpoint: Option<PathBuf>,
    /// 开启 Mod 运行 trace（有界；报告附 mod_trace 字段）。
    pub mod_trace: bool,
    /// 开启持久输入日志（G25.a）：每轮提交后追加一段；报告附 input_log 摘要。
    pub input_log: Option<PathBuf>,
    /// 运行成功后从起点快照按日志逐段重放并核对终态（G25.b；需 input_log）。
    pub verify_replay: bool,
    /// 每 N 轮提交后保存完整检查点到 checkpoint_dir（G26.a）。
    pub checkpoint_every: Option<u32>,
    /// 周期检查点目录；`--load-checkpoint` 指向它时走「最新可用」恢复。
    pub checkpoint_dir: Option<PathBuf>,
    /// 周期检查点保留份数（缺省保留全部）。
    pub checkpoint_keep: Option<u32>,
}

/// 0 表示全部轮次及断言成功；2 表示运行或断言失败；1 表示输入或 I/O 错误。
pub fn run(options: Options) -> i32 {
    if options
        .report
        .extension()
        .is_none_or(|extension| extension != "json")
    {
        eprintln!("sim run 失败：报告文件必须使用 .json 后缀");
        return 1;
    }
    // 报告不能覆盖场景输入。该错误不能再向同一路径写失败报告。
    if options.report.exists()
        && options.scenario.exists()
        && std::fs::canonicalize(&options.report).ok()
            == std::fs::canonicalize(&options.scenario).ok()
    {
        eprintln!("sim run 失败：报告路径不能与场景文件相同");
        return 1;
    }
    if let Err(error) = checkpoint::validate_paths(&options) {
        eprintln!("sim run 失败：{error:#}");
        return 1;
    }
    let (report, code) = match execution::execute(&options) {
        Ok(report) => {
            let code = if report["status"] == "checkpoint_failed" {
                1
            } else if report["success"] == true {
                0
            } else {
                2
            };
            (report, code)
        }
        Err(error) => (
            json!({
                "report_version": SIM_REPORT_VERSION, "success": false, "status": "setup_failed",
                "scenario": options.scenario, "ticks_requested": options.ticks,
                "world_ticks_committed": 0, "error": format!("{error:#}"),
                // v8 契约：--input-log 启用时该键非 null；setup 阶段即失败
                // 则一行段都没有（含「损坏日志拒绝续接」路径）。
                "input_log": options.input_log.as_ref().map(|path|
                    json!({"path": path, "status": "setup_failed"})),
                // v9 契约：--verify-replay 启用时该键非 null。
                "verify_replay": options.verify_replay.then(|| json!({"status": "setup_failed"})),
                // v10 契约：周期保存启用时该键非 null。
                "auto_checkpoint": options.checkpoint_every
                    .map(|_| json!({"status": "setup_failed"})),
                // v10 契约：目录形态恢复启用时该键非 null（撕裂日志/全
                // 部损坏/配对硬错都发生在 setup 期，与上面的键对称，消费
                // 方可区分「未启用」与「启用后失败」）。
                "recovery": options
                    .load_checkpoint
                    .as_ref()
                    .filter(|path| path.is_dir())
                    .map(|_| json!({"status": "setup_failed"})),
            }),
            1,
        ),
    };
    if let Err(error) = write_report(&options.report, &report) {
        eprintln!("sim run 报告写入失败：{error:#}");
        return 1;
    }
    println!(
        "sim run: {}，已提交 {}/{} tick，报告 {}",
        report["status"].as_str().unwrap_or("failed"),
        report["world_ticks_committed"],
        options.ticks,
        options.report.display()
    );
    if code != 0 {
        eprintln!(
            "{}",
            report["error"]
                .as_str()
                .unwrap_or("运行或场景断言未通过，详见报告")
        );
    }
    code
}

/// sim run 报告 schema 版本：成功与 setup_failed 两条路径同源引用，
/// 升版时不得只改一处（同通道版本分裂会误导按版本判读的消费方）。
/// v6 顶层 `mods_switches`；v7 起顶层与最终观察新增 `seed`（世界种子，
/// G22.b 随机协议的可复现身份）；v8 起顶层新增 `input_log`（--input-log
/// 启用时非 null：段数、首末 tick 与读回校验结果）；v9 起顶层新增
/// `verify_replay`（--verify-replay 启用时非 null：回放核对结果）；v10
/// 起顶层新增 `auto_checkpoint`（周期保存摘要/RPO 声明，启用时含
/// setup_failed 路径非 null）与 `recovery`（--load-checkpoint 指向目
/// 录时的恢复选择记录，目录形态启用时同样两路非 null）。
const SIM_REPORT_VERSION: u32 = 10;
