//! 解析命令行参数并分派到嵌入式服务、bot 检查或性能采集工具。
//! sim run 运行真实场景；serve、test、import 当前只保留命令入口。

use clap::{Parser, Subcommand};

mod bot_check;
mod embed_main;
mod mod_inspect;
mod mod_perf_probe;
mod mod_player_probe;
mod perf_probe;
mod report;
mod sim;

#[derive(Parser)]
#[command(name = "kkscreeps", about = "Screeps 服务端的 Rust 重写版", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// 检查 Mod 冻结组合与停用依赖闭包
    Mod {
        #[command(subcommand)]
        command: ModCommand,
    },
    /// 运行游戏服务器（尚未实现）
    Serve,
    /// 加载 bot 模块并逐轮执行真实世界结算
    Sim {
        #[command(subcommand)]
        command: SimCommand,
    },
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
    /// 固定 link 冷却负载，对比 native、空 Mod 与完整 JS 处理链
    ModPerfProbe {
        /// 开启 op/候选/预算包含计时；诊断耗时不能作为无插桩性能基线
        #[arg(long)]
        profile: bool,
        /// 保留逐 tick 墙钟、进程 CPU 与上下文切换；额外采样只用于诊断
        #[arg(long)]
        tick_diagnostics: bool,
        #[arg(long, value_enum)]
        mode: mod_perf_probe::Mode,
        #[arg(long, default_value_t = 1)]
        rooms: u32,
        #[arg(long, default_value_t = 400)]
        links_per_room: u32,
        #[arg(long, default_value_t = 500)]
        warmup: u32,
        #[arg(long, default_value_t = 3000)]
        ticks: u32,
        #[arg(long, default_value = "compat/fixtures/mod-perf")]
        fixtures: String,
        #[arg(long)]
        report: String,
    },
    /// 连续运行玩家与 Mod 联合负载，核验每轮动作并采集阶段与进程成本
    ModPlayerProbe {
        #[arg(long, default_value_t = 2)]
        players: u32,
        #[arg(long, default_value_t = 2)]
        concurrency: usize,
        #[arg(long, default_value_t = 500)]
        warmup: u32,
        #[arg(long, default_value_t = 3000)]
        ticks: u32,
        #[arg(long, default_value = "compat/fixtures/mod-player")]
        fixtures: String,
        #[arg(long)]
        kkpv: bool,
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

#[derive(Subcommand)]
enum ModCommand {
    /// 注册真实包并输出冻结链、规则来源及停用预览，不推进世界
    Inspect {
        #[arg(long)]
        root: std::path::PathBuf,
        /// 按包 ID 分组的 JSON 配置文件
        #[arg(long)]
        configs: Option<std::path::PathBuf>,
        /// key=namespace；显式选择多方规则覆盖的来源，可重复
        #[arg(long)]
        rule_source: Vec<String>,
        /// 预览停用指定包及依赖者，可重复；不卸载内容
        #[arg(long)]
        disable: Vec<String>,
        /// 显式选择的包 ID，可重复；依赖闭包自动加入，未选包不安装
        #[arg(long)]
        select: Vec<String>,
        /// 显式排除的包 ID，可重复；与必需依赖相撞时报错
        #[arg(long)]
        exclude: Vec<String>,
        #[arg(long)]
        report: std::path::PathBuf,
    },
}

#[derive(Subcommand)]
enum SimCommand {
    /// 从 JSON 场景运行玩家，检查最终已提交状态并生成报告
    Run {
        #[arg(long)]
        scenario: std::path::PathBuf,
        #[arg(long, default_value_t = 1000, value_parser = clap::value_parser!(u32).range(1..))]
        ticks: u32,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(usize))]
        concurrency: usize,
        #[arg(long)]
        kkpv: bool,
        #[arg(long)]
        report: std::path::PathBuf,
        /// 在最终已提交 tick 保存完整检查点；父目录必须已存在
        #[arg(long)]
        save_checkpoint: Option<std::path::PathBuf>,
        /// 从同一场景身份的检查点恢复；ticks 表示额外推进的轮数
        #[arg(long)]
        load_checkpoint: Option<std::path::PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Mod {
            command:
                ModCommand::Inspect {
                    root,
                    configs,
                    rule_source,
                    disable,
                    select,
                    exclude,
                    report,
                },
        } => {
            std::process::exit(mod_inspect::run(mod_inspect::Options {
                root,
                configs,
                rule_sources: rule_source,
                disable,
                select,
                exclude,
                report,
            }));
        }
        Command::Serve | Command::Test | Command::Import => {
            eprintln!("该子命令尚未实装（见 docs/roadmap.md 的里程碑规划）");
            std::process::exit(2);
        }
        Command::Embed => {
            std::process::exit(crate::embed_main::run());
        }
        Command::Sim {
            command:
                SimCommand::Run {
                    scenario,
                    ticks,
                    concurrency,
                    kkpv,
                    report,
                    save_checkpoint,
                    load_checkpoint,
                },
        } => {
            std::process::exit(sim::run(sim::Options {
                scenario,
                ticks,
                concurrency,
                kkpv,
                report,
                save_checkpoint,
                load_checkpoint,
            }));
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
        Command::ModPerfProbe {
            profile,
            tick_diagnostics,
            mode,
            rooms,
            links_per_room,
            warmup,
            ticks,
            fixtures,
            report,
        } => {
            std::process::exit(mod_perf_probe::run(mod_perf_probe::Options {
                profile,
                tick_diagnostics,
                mode,
                rooms,
                links_per_room,
                warmup,
                ticks,
                fixtures,
                report,
            }));
        }
        Command::ModPlayerProbe {
            players,
            concurrency,
            warmup,
            ticks,
            fixtures,
            kkpv,
            report,
        } => {
            std::process::exit(mod_player_probe::run(mod_player_probe::Options {
                players,
                concurrency,
                warmup,
                ticks,
                fixtures,
                kkpv,
                report,
            }));
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
