//! Mod 作者命令参数及选项映射。
use super::{init, inspect, services, test, types};
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum ModCommand {
    /// 在独立后台域运行真实服务包的生命周期、异步探针与请求
    Services {
        #[arg(long)]
        root: std::path::PathBuf,
        #[arg(long)]
        configs: Option<std::path::PathBuf>,
        #[arg(long)]
        select: Vec<String>,
        #[arg(long)]
        exclude: Vec<String>,
        /// 已提交检查点；省略时构造最小作者世界
        #[arg(long)]
        checkpoint: Option<std::path::PathBuf>,
        /// [{service,args}] 请求数组
        #[arg(long)]
        requests: std::path::PathBuf,
        /// namespace:name=path，显式授予常规文件只读探针，可重复
        #[arg(long)]
        probe: Vec<String>,
        #[arg(long)]
        report: std::path::PathBuf,
    },
    /// 生成作者最小包模板（对象 tick、玩家类、场景与断言）
    Init {
        /// 包根目录；模板落在 <root>/<id>/，已存在时拒绝
        #[arg(long)]
        root: std::path::PathBuf,
        /// 包 ID（同时是子目录名与命名空间前缀）
        #[arg(long)]
        id: String,
    },
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
    /// 对根下自带场景的包逐个运行断言并汇总（sim run 同一执行路径）
    Test {
        /// Mod 根目录；包内 scenario.json 参与测试，缺失记 skipped
        #[arg(long)]
        root: std::path::PathBuf,
        /// 每个场景推进的 tick 数（1 起步，默认 3 匹配模板断言）
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
        ticks: u32,
        /// 二进制投影通道（与 sim run --kkpv 同义）
        #[arg(long)]
        kkpv: bool,
        /// 开启 Mod 运行 trace（与 sim run --mod-trace 同义）
        #[arg(long)]
        mod_trace: bool,
        /// 汇总报告输出路径（JSON）
        #[arg(long)]
        report: std::path::PathBuf,
    },
    /// 从冻结计划生成玩家面 TypeScript 声明；--check 校验新鲜度
    Types {
        #[arg(long)]
        root: std::path::PathBuf,
        /// 按包 ID 分组的 JSON 配置文件
        #[arg(long)]
        configs: Option<std::path::PathBuf>,
        /// 显式选择的包 ID，可重复；依赖闭包自动加入，未选包不安装
        #[arg(long)]
        select: Vec<String>,
        /// 显式排除的包 ID，可重复；与必需依赖相撞时报错
        #[arg(long)]
        exclude: Vec<String>,
        /// 声明输出路径（.d.ts）；必须位于包根目录之外
        #[arg(long)]
        out: std::path::PathBuf,
        /// 只与磁盘声明按字节对比，不写入也不创建目录；过期或缺失退出非零
        #[arg(long)]
        check: bool,
        /// 汇总报告输出路径（JSON）
        #[arg(long)]
        report: std::path::PathBuf,
    },
}

impl ModCommand {
    pub(crate) fn run(self) -> i32 {
        match self {
            Self::Services {
                root,
                configs,
                select,
                exclude,
                checkpoint,
                requests,
                probe,
                report,
            } => services::run(services::Options {
                root,
                configs,
                select,
                exclude,
                checkpoint,
                requests,
                probes: probe,
                report,
            }),
            Self::Init { root, id } => init::run(init::Options { root, id }),
            Self::Test {
                root,
                ticks,
                kkpv,
                mod_trace,
                report,
            } => test::run(test::Options {
                root,
                ticks,
                kkpv,
                mod_trace,
                report,
            }),
            Self::Inspect {
                root,
                configs,
                rule_source,
                disable,
                select,
                exclude,
                report,
            } => inspect::run(inspect::Options {
                root,
                configs,
                rule_sources: rule_source,
                disable,
                select,
                exclude,
                report,
            }),
            Self::Types {
                root,
                configs,
                select,
                exclude,
                out,
                check,
                report,
            } => types::run(types::Options {
                root,
                configs,
                select,
                exclude,
                out,
                check,
                report,
            }),
        }
    }
}
