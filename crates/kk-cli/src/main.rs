//! 解析命令行参数并分派到嵌入式服务、bot 检查或性能采集工具。
//! serve、sim、test、import 当前只保留命令入口，调用时返回退出码 2。

use clap::{Parser, Subcommand};

mod bot_check;
mod embed_main;
mod perf_probe;

#[derive(Parser)]
#[command(name = "kkscreeps", about = "Screeps 服务端的 Rust 重写版", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 运行游戏服务器（尚未实现）
    Serve,
    /// 运行本地模拟（尚未实现）
    Sim,
    /// 运行断言测试套件（尚未实现）
    Test,
    /// 导入世界数据（尚未实现）
    Import,
    /// 通过 stdin/stdout 提供逐行 JSON 的嵌入式调用服务
    Embed,
    /// 在固定世界中运行 bot，输出 API 访问和错误统计
    BotCheck {
        /// bot 模块目录（支持 .js 和 .wasm 文件）
        #[arg(long)]
        path: String,
        /// 要跑的 tick 数
        #[arg(long, default_value_t = 2000)]
        ticks: u32,
        /// 报告输出路径（JSON）
        #[arg(long, default_value = "bot-check-report.json")]
        report: String,
        /// 经 KKPV 二进制通道注入世界（对象记录走块束；与 JSON 通道的
        /// 判定口径一致）
        #[arg(long)]
        kkpv: bool,
    },
    /// 运行跨引擎共用的显式负载
    CompareProbe {
        #[arg(long)]
        fixture: String,
        #[arg(long, default_value_t = 500)]
        warmup: u32,
        #[arg(long, default_value_t = 3000)]
        ticks: u32,
        #[arg(long, default_value_t = 2)]
        concurrency: usize,
        #[arg(long)]
        report: String,
    },
    /// 运行采集、升级和孵化负载，统计投影、执行、结算与整轮耗时
    PerfProbe {
        /// 采集 tick 数
        #[arg(long, default_value_t = 1000)]
        ticks: u32,
        /// 预热 tick 数（不计入汇总）
        #[arg(long, default_value_t = 100)]
        warmup: u32,
        /// 玩家数（1..32）；每玩家独立房间
        #[arg(long, default_value_t = 2)]
        players: u32,
        /// 同时执行玩家上限；省略时使用引擎默认值
        #[arg(long)]
        concurrency: Option<usize>,
        /// 报告输出路径（JSON）
        #[arg(long, default_value = "perf-probe.json")]
        report: String,
        /// 经 KKPV 二进制通道注入世界（与 JSON 参考通道同负载 A/B）
        #[arg(long)]
        kkpv: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve | Command::Sim | Command::Test | Command::Import => {
            eprintln!("该子命令尚未实装（见 docs/roadmap.md 的里程碑规划）");
            std::process::exit(2);
        }
        Command::Embed => {
            std::process::exit(crate::embed_main::run());
        }
        Command::BotCheck {
            path,
            ticks,
            report,
            kkpv,
        } => {
            std::process::exit(bot_check::run(&path, ticks, &report, kkpv));
        }
        Command::CompareProbe {
            fixture,
            warmup,
            ticks,
            concurrency,
            report,
        } => {
            std::process::exit(perf_probe::run_comparison(
                &fixture,
                warmup,
                ticks,
                concurrency,
                &report,
            ));
        }
        Command::PerfProbe {
            ticks,
            warmup,
            players,
            concurrency,
            report,
            kkpv,
        } => {
            std::process::exit(perf_probe::run(
                ticks,
                warmup,
                players,
                concurrency,
                &report,
                kkpv,
            ));
        }
    }
}
