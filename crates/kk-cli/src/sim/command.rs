//! 模拟命令参数及执行选项映射。
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum SimCommand {
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
        /// 开启 Mod 运行 trace（有界事件缓冲；报告附 mod_trace）
        #[arg(long)]
        mod_trace: bool,
        /// 每轮提交后追加持久输入日志段（G25.a）；文件存在时先整读校验再续接
        #[arg(long)]
        input_log: Option<std::path::PathBuf>,
        /// 运行成功后从起点快照按日志逐段重放并核对终态（需 --input-log）
        #[arg(long)]
        verify_replay: bool,
        /// 提交后世界时间为 N 整倍数的轮次保存一份完整检查点（G26.a）
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        checkpoint_every: Option<u32>,
        /// 周期检查点目录（与 --checkpoint-every 同用；--load-checkpoint 可指向它做恢复）
        #[arg(long)]
        checkpoint_dir: Option<std::path::PathBuf>,
        /// 周期检查点保留份数（缺省保留全部）
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        checkpoint_keep: Option<u32>,
    },
}

impl SimCommand {
    pub(crate) fn run(self) -> i32 {
        match self {
            Self::Run {
                scenario,
                ticks,
                concurrency,
                kkpv,
                report,
                save_checkpoint,
                load_checkpoint,
                mod_trace,
                input_log,
                verify_replay,
                checkpoint_every,
                checkpoint_dir,
                checkpoint_keep,
            } => super::run(super::Options {
                scenario,
                ticks,
                concurrency,
                kkpv,
                report,
                save_checkpoint,
                load_checkpoint,
                mod_trace,
                input_log,
                verify_replay,
                checkpoint_every,
                checkpoint_dir,
                checkpoint_keep,
            }),
        }
    }
}
