//! CLI 入口只负责选取命令；参数与各领域实现由对应模块拥有。
use args::{Cli, Command};
use clap::Parser;

mod args;
mod bot_check;
mod embed_main;
mod export_fixtures;
mod mod_tools;
mod probes;
mod sim;
mod support;

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Mod { command } => std::process::exit(command.run()),
        Command::Serve | Command::Test | Command::Import => {
            eprintln!("该子命令尚未实装（见 docs/roadmap.md 的里程碑规划）");
            std::process::exit(2);
        }
        Command::Embed => {
            std::process::exit(crate::embed_main::run());
        }
        Command::Sim { command } => std::process::exit(command.run()),
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
            std::process::exit(probes::run_comparison(
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
            std::process::exit(probes::mod_chain::run(probes::mod_chain::Options {
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
            std::process::exit(probes::mod_player::run(probes::mod_player::Options {
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
            std::process::exit(probes::workload::run(
                ticks,
                warmup,
                players,
                concurrency,
                &report,
                kkpv,
            ));
        }
        Command::ExportFixtures { out, ticks } => {
            std::process::exit(export_fixtures::run(&out, ticks));
        }
    }
}
