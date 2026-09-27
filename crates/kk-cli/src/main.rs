//! `kkscreeps` 命令行入口——一个二进制，多种运行方式（docs/architecture/overall.md）。
//!
//! M0 交付的子命令：`embed`（screeps-ok 适配器的 sidecar 服务）、`bot-check`（bot 环境验收）。
//! 其余子命令为占位，随各自里程碑实装。

use clap::{Parser, Subcommand};

mod bot_check;
mod embed_main;

#[derive(Parser)]
#[command(name = "kkscreeps", about = "Screeps 服务端的 Rust 重写版", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 运行完整游戏服务器（游戏循环 + 网络接口）——M3
    Serve,
    /// 本地模拟与集成测试（无网络、内存即存储）——M2.5
    Sim,
    /// 断言测试套件（退出码供 CI 使用）——M2.5
    Test,
    /// 导入官方世界数据等运维工具——M4
    Import,
    /// screeps-ok 适配器的 sidecar：stdin/stdout 上的一行一条 JSON-RPC——M0
    Embed,
    /// bot 环境兼容性验收：桩环境下加载 bot 跑 N 个 tick，产出缺失 API 清单——M0
    BotCheck {
        /// bot 模块目录（含 main.js 或构建产物）
        #[arg(long)]
        path: String,
        /// 要跑的 tick 数
        #[arg(long, default_value_t = 2000)]
        ticks: u32,
        /// 报告输出路径（JSON）
        #[arg(long, default_value = "bot-check-report.json")]
        report: String,
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
        } => {
            std::process::exit(bot_check::run(&path, ticks, &report));
        }
    }
}
