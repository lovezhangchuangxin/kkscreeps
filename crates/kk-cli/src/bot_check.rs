//! `kkscreeps bot-check` 子命令：M0 bot 环境兼容性验收。
//!
//! 在桩环境（deno_core + CJS 加载器 + lodash 预置 + stub Game）下加载真实社区 bot，
//! 跑 N 个 tick 的 loop 循环，产出：运行结局统计、console 错误流、缺失 API 清单、
//! 桩方法调用统计。验收口径是"加载运行无崩溃 + 缺失 API 清零或登记"（roadmap M0），
//! 不要求游戏行为正确——那需要引擎（M1+）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use kk_runner::{ModuleTable, PlayerThreads, SandboxConfig, TickOutcome, TickPlan};

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
    println!(
        "bot-check: {}（{module_count} 个模块）× {ticks} ticks",
        bot_dir.display()
    );

    let mut pool = PlayerThreads::new();
    pool.spawn_player(PLAYER, SandboxConfig::default())?;
    pool.set_modules(PLAYER, table)?;

    // tick 0 播种 `Memory.creeps = {}`（begin→expr→end 原子序，写回 RawMemory）。
    // 夹具世界预置了 creep，而真实新服首 tick 无 creep——bot（tooangel）的角色
    // 检查直接读 Memory.creeps（该对象由 creep.memory 访问或既有存档创建），
    // 常态服务器上恒存在；播种把夹具对齐"bot 已运行过"的常态
    pool.run_player(PLAYER, &home_world(0), "Memory.creeps = {}; 0")?;

    let mut console_errors: Vec<String> = Vec::new();
    let mut console_head: Vec<String> = Vec::new();
    let mut missing_api: BTreeMap<String, u64> = BTreeMap::new();
    let mut stub_calls: BTreeMap<String, u64> = BTreeMap::new();
    let mut completed = 0u32;
    let mut terminated = 0u32;
    let mut skipped_penalty = 0u32;
    let mut errored_ticks = 0u32;

    for t in 1..=ticks {
        let world = home_world(t);
        let report = pool.tick(PLAYER, &world)?;
        // 按 plan 区分真实硬中断与跳帧 tick（跳帧的 outcome 也是 Terminated——
        // runPlayer 的 executed=false 契约；统计上混计会把一次硬中断记成两次）
        match (&report.outcome, &report.plan) {
            (TickOutcome::Completed { .. }, _) => completed += 1,
            (TickOutcome::Terminated, TickPlan::SkipPenalty) => skipped_penalty += 1,
            (TickOutcome::Terminated, TickPlan::Run { .. }) => terminated += 1,
        }
        // 玩家 loop 逻辑错误（bot 常自行 try/catch 后用 console.log 记录，
        // 只看 error 级会漏——player_error 是更可靠的信号）
        if report.player_error.is_some() || report.console.iter().any(|l| l.level == "error") {
            errored_ticks += 1;
        }
        if let Some(err) = &report.player_error
            && console_errors.len() < 300
        {
            console_errors.push(format!("[tick {t}] [loop] {}", err));
        }
        for line in report.console {
            if line.level == "error" && console_errors.len() < 300 {
                console_errors.push(format!("[tick {t}] {}", line.text));
            }
            if console_head.len() < 20 {
                console_head.push(format!("[tick {t}][{}] {}", line.level, line.text));
            }
        }
        merge_counts(&mut missing_api, &report.missing_api);
        merge_counts(&mut stub_calls, &report.stub_calls);
    }

    let report = json!({
        "bot": bot_dir.display().to_string(),
        "ticks_requested": ticks,
        "ticks_completed": completed,
        "ticks_terminated": terminated,
        "ticks_skipped_penalty": skipped_penalty,
        "ticks_with_console_error": errored_ticks,
        "missing_api": missing_api,
        "stub_calls_top": top(&stub_calls, 30),
        "console_errors_head": console_errors,
        "console_head": console_head,
    });
    std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;

    println!();
    println!("== bot-check 报告（{report_path}） ==");
    println!(
        "  完成 tick：{completed}/{}（硬中断 {terminated}，跳帧（桶透支停跑） {skipped_penalty}，含 console 错误的 tick {errored_ticks}）",
        ticks
    );
    println!("  缺失 API：{} 项", missing_api.len());
    for (k, v) in top(&missing_api, 20) {
        println!("    {k} × {v}");
    }
    println!(
        "  桩方法调用：{} 种，top: {:?}",
        stub_calls.len(),
        top(&stub_calls, 8)
            .into_iter()
            .map(|(k, v)| format!("{k}×{v}"))
            .collect::<Vec<_>>()
    );
    println!("  console 头部输出（前 5 条）：");
    for l in console_head.iter().take(5) {
        println!("    {}", l.lines().next().unwrap_or_default());
    }
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
/// 地形全平原——Terrain 面（getTerrain/lookFor(LOOK_TERRAIN)）经世界 JSON 的
/// rooms[].terrain 生效，tooangel 布局阶段依赖它。
fn home_world(t: u32) -> Value {
    let plains: Vec<u8> = vec![0; 2500];
    json!({
        "time": t,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "rooms": { "W1N1": { "name": "W1N1", "terrain": plains } },
        "objects": [
            { "id": "ctrl1", "type": "structure", "structureType": "controller",
              "room": "W1N1", "x": 10, "y": 10, "level": 4, "progress": 0, "progressTotal": 121500,
              "owner": "bot", "hits": 10000, "hitsMax": 10000 },
            { "id": "src1", "type": "structure", "structureType": "source",
              "room": "W1N1", "x": 15, "y": 15, "energy": 2500, "energyCapacity": 3000 },
            { "id": "min1", "type": "structure", "structureType": "mineral",
              "room": "W1N1", "x": 30, "y": 30, "mineralType": "XUH2O",
              "mineralAmount": 50000, "density": 3 },
            { "id": "spw1", "type": "spawn", "room": "W1N1", "x": 20, "y": 20, "name": "Spawn1",
              "owner": "bot", "store": { "energy": 300 }, "energyCapacity": 300 },
            { "id": "crp1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "harvester1",
              "owner": "bot", "body": ["move", "work", "carry"], "store": { "energy": 50 },
              "fatigue": 0 }
        ]
    })
}
