//! 常驻服务命令入口：init 显式初始化，run 恢复并常驻运行。
//! 机制归 kk-runtime::service（宿主/检查点/身份）；本模块只做参数、
//! 装配、信号与控制流。网络接入随 S3 切片，本批不监听任何端口。
mod control;
mod init;
mod run;

use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(crate) enum ServeCommand {
    /// 从种子场景初始化服务状态目录（显式、一次性；重复 init 须换目录或先清理）
    Init {
        /// 严格版本化服务配置 JSON
        #[arg(long)]
        config: PathBuf,
        /// 种子场景（sim 场景格式；init 后运行期不再读取）
        #[arg(long)]
        seed_scenario: PathBuf,
    },
    /// 常驻运行：无日志恢复、按配置调度、周期保存，SIGTERM/控制命令停止
    Run {
        /// 严格版本化服务配置 JSON
        #[arg(long)]
        config: PathBuf,
        /// 在 stdin/stdout 上提供逐行 JSON 控制（status/retry/stop）
        #[arg(long)]
        control_stdio: bool,
    },
}

pub(crate) fn dispatch(command: ServeCommand) -> i32 {
    match command {
        ServeCommand::Init {
            config,
            seed_scenario,
        } => init::init(&config, &seed_scenario),
        ServeCommand::Run {
            config,
            control_stdio,
        } => run::run(&config, control_stdio),
    }
}
