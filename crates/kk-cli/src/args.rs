//! 解析命令行参数并分派到嵌入式服务、bot 检查或性能采集工具。
//! sim run 运行真实场景；serve、test、import 当前只保留命令入口。

use clap::{Parser, Subcommand};

use crate::{mod_tools::ModCommand, probes::mod_chain, sim::SimCommand};

#[derive(Parser)]
#[command(name = "kkscreeps", about = "Screeps 服务端的 Rust 重写版", version)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
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
        /// 进程内交替 A/B：每 N tick 切换预编码开关，按模式归桶上报相位
        /// 与 CPU，消除窗口级机器状态漂移。0（默认）关闭。
        #[arg(long, default_value_t = 0)]
        interleave: u32,
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
        mode: mod_chain::Mode,
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
        /// 玩家数（1..320）；房间按 20 列网格分布，每玩家独立自有房
        #[arg(long, default_value_t = 2)]
        players: u32,
        /// 每玩家自有房数（1..8）；总房数（玩家×房）须 ≤320，全部 rcl4
        #[arg(long, default_value_t = 1)]
        rooms_per_player: u32,
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
    /// 渲染器夹具导出：四张脚本世界跑 N tick 逐 tick 落 world_json（JSONL）——F2
    ExportFixtures {
        /// 输出目录（每夹具一个 <名>.jsonl）
        #[arg(long)]
        out: String,
        /// 每夹具推进的 tick 数
        #[arg(long, default_value_t = 30)]
        ticks: u32,
    },
}
