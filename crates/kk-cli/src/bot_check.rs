//! `kkscreeps bot-check` 子命令：M0 bot 环境兼容性验收。
//!
//! 在桩环境（deno_core + CJS 加载器 + lodash 预置 + stub Game）下加载真实社区 bot，
//! 跑 N 个 tick 的 loop 循环，产出：运行结局统计、console 错误流、缺失 API 清单、
//! 桩方法调用统计。验收口径是"加载运行无崩溃 + 缺失 API 清零或登记"（roadmap M0），
//! 不要求游戏行为正确——那需要引擎（M1+）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use kk_runner::{ModuleTable, PlayerThreads, SandboxConfig, TickOutcome};

const PLAYER: u64 = 0;

pub fn run(path: &str, ticks: u32, report_path: &str) -> i32 {
    match run_inner(path, ticks, report_path) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("bot-check 失败: {e:#}");
            1
        }
    }
}

fn run_inner(path: &str, ticks: u32, report_path: &str) -> anyhow::Result<()> {
    let bot_dir = PathBuf::from(path);
    let table = ModuleTable::from_directory(&bot_dir)?;
    let module_count = table.len();
    if module_count == 0 {
        anyhow::bail!("模块目录为空（没有 *.js 模块）");
    }
    println!("bot-check: {}（{module_count} 个模块）× {ticks} ticks", bot_dir.display());

    let mut pool = PlayerThreads::new();
    pool.spawn_player(PLAYER, SandboxConfig::default())?;
    pool.set_modules(PLAYER, table)?;

    let mut console_errors: Vec<String> = Vec::new();
    let mut missing_api: BTreeMap<String, u64> = BTreeMap::new();
    let mut stub_calls: BTreeMap<String, u64> = BTreeMap::new();
    let mut completed = 0u32;
    let mut terminated = 0u32;
    let mut errored_ticks = 0u32;

    for t in 1..=ticks {
        let world = home_world(t);
        let report = pool.tick(PLAYER, &world)?;
        match report.outcome {
            TickOutcome::Completed { .. } => completed += 1,
            TickOutcome::Terminated => terminated += 1,
        }
        let tick_had_error = report
            .console
            .iter()
            .any(|l| l.level == "error" && l.text.starts_with("Error") || l.level == "error" && l.text.contains("at kk_module"));
        if report.console.iter().any(|l| l.level == "error") {
            errored_ticks += 1;
        }
        for line in report.console {
            if line.level == "error" && console_errors.len() < 300 {
                console_errors.push(format!("[tick {t}] {}", line.text));
            }
        }
        merge_counts(&mut missing_api, &report.missing_api);
        merge_counts(&mut stub_calls, &report.stub_calls);
        let _ = tick_had_error;
    }

    let report = json!({
        "bot": bot_dir.display().to_string(),
        "ticks_requested": ticks,
        "ticks_completed": completed,
        "ticks_terminated": terminated,
        "ticks_with_console_error": errored_ticks,
        "missing_api": missing_api,
        "stub_calls_top": top(&stub_calls, 30),
        "console_errors_head": console_errors,
    });
    std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;

    println!();
    println!("== bot-check 报告（{report_path}） ==");
    println!("  完成 tick：{completed}/{}（硬中断 {terminated}，含 console 错误的 tick {errored_ticks}）", ticks);
    println!("  缺失 API：{} 项", missing_api.len());
    for (k, v) in top(&missing_api, 20) {
        println!("    {k} × {v}");
    }
    println!("  桩方法调用：{} 种，top: {:?}",
        stub_calls.len(),
        top(&stub_calls, 8).into_iter().map(|(k, v)| format!("{k}×{v}")).collect::<Vec<_>>());
    if !console_errors.is_empty() {
        println!("  console 错误（前 5 条）：");
        for e in console_errors.iter().take(5) {
            println!("    {}", e.lines().next().unwrap_or_default());
        }
    }
    pool.shutdown_all();
    Ok(())
}

fn merge_counts(target: &mut BTreeMap<String, u64>, source: &Value) {
    if let Some(obj) = source.as_object() {
        for (k, v) in obj {
            let count = v.as_u64().unwrap_or(1);
            *target.entry(k.clone()).or_insert(0) += count;
        }
    }
}

fn top(map: &BTreeMap<String, u64>, n: usize) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = map.iter().map(|(k, c)| (k.clone(), *c)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

/// 桩世界的"家"：一个被玩家拥有的房间（controller/source/spawn/creep 各一）。
fn home_world(t: u32) -> Value {
    json!({
        "time": t,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "rooms": { "W1N1": { "name": "W1N1" } },
        "objects": [
            { "id": "ctrl1", "type": "structure", "structureType": "controller",
              "room": "W1N1", "x": 10, "y": 10, "level": 4, "progress": 0, "progressTotal": 121500,
              "owner": "bot", "hits": 10000, "hitsMax": 10000 },
            { "id": "src1", "type": "structure", "structureType": "source",
              "room": "W1N1", "x": 15, "y": 15, "energy": 2500, "energyCapacity": 3000 },
            { "id": "spw1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "bot", "store": { "energy": 300 }, "energyCapacity": 300 },
            { "id": "crp1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "harvester1",
              "owner": "bot", "body": ["move", "work", "carry"], "store": { "energy": 50 },
              "fatigue": 0 }
        ]
    })
}
