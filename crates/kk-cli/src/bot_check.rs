//! 在固定世界数据中加载 bot 模块，并通过玩家沙箱反复调用入口。
//!
//! 世界每轮只更新时间；玩家意图不送入引擎结算，不能用本工具验证采集、移动等
//! 行为是否改变世界。Memory 和 JavaScript 全局变量仍在沙箱中跨轮保留。
//!
//! 退出码为 0（通过）、1（模块加载、通信或报告写入失败）、2（硬中断、
//! 未完成全部请求 tick 或记录到缺失 API）。玩家逻辑错误只记录，不影响通过判定；
//! Completed 也可能包含玩家异常，或 main 没有导出 loop 的情况。
//!
//! 缺失 API 统计只覆盖代理记录的访问，并排除预设忽略项；属性枚举不会触发该检测。
//! 报告另列 in 运算符探测和计数器记录的方法调用，不能据此证明所有 API 都兼容。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use kk_runner::{ModuleTable, PlayerThreads, RunEpoch, SandboxConfig, TickOutcome, TickPlan};

const PLAYER: u64 = 0;

/// 运行检查并写入 JSON 报告，返回进程退出码；判定条件见本模块说明。
/// `kkpv` 为 true 时世界经 KKPV 二进制通道注入（对象记录走块束，
/// 判定口径与 JSON 通道一致）。
pub fn run(path: &str, ticks: u32, report_path: &str, kkpv: bool) -> i32 {
    match run_inner(path, ticks, report_path, kkpv) {
        Ok(true) => 0,
        Ok(false) => 2,
        Err(e) => {
            eprintln!("bot-check 失败: {e:#}");
            1
        }
    }
}

fn run_inner(path: &str, ticks: u32, report_path: &str, kkpv: bool) -> anyhow::Result<bool> {
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

    // 夹具初始就有 creep，因此先建立 Memory.creeps，避免 bot 的角色检查
    // 因缺少记录容器报错。这一轮只准备 Memory，不纳入后面的 tick 统计。
    let prep_world = home_world(0);
    let prep_result = if kkpv {
        let (public, private) = kkpv_fixture_bundles(&prep_world)?;
        pool.run_player_kkpv(
            PLAYER,
            &kk_world::kkpv::kkpv_envelope_from_world_json(&prep_world),
            &kk_runner::SharedKkpvBundle::from_vec(public),
            &private,
            "Memory.creeps = {}; 0",
            RunEpoch {
                tick: 0,
                generation: 0,
            },
        )
    } else {
        pool.run_player(
            PLAYER,
            &prep_world,
            "Memory.creeps = {}; 0",
            RunEpoch {
                tick: 0,
                generation: 0,
            },
        )
    };
    prep_result?;

    let mut console_errors: Vec<String> = Vec::new();
    let mut console_head: Vec<String> = Vec::new();
    let mut console_error_folds: BTreeMap<String, u64> = BTreeMap::new();
    let mut missing_api: BTreeMap<String, u64> = BTreeMap::new();
    let mut has_probes: BTreeMap<String, u64> = BTreeMap::new();
    let mut stub_calls: BTreeMap<String, u64> = BTreeMap::new();
    let mut completed = 0u32;
    let mut terminated = 0u32;
    let mut skipped_penalty = 0u32;
    let mut errored_ticks = 0u32;

    for t in 1..=ticks {
        let world = home_world(t);
        let report = if kkpv {
            let envelope = kk_world::kkpv::kkpv_envelope_from_world_json(&world);
            let (public, private) = kkpv_fixture_bundles(&world)?;
            pool.tick_kkpv(
                PLAYER,
                &envelope,
                &kk_runner::SharedKkpvBundle::from_vec(public),
                &private,
            )?
        } else {
            pool.tick(PLAYER, &world)?
        };
        // 跳帧和执行后的硬中断都报告 Terminated，必须结合 plan 分开计数。
        match (&report.outcome, &report.plan) {
            (TickOutcome::Completed { .. }, _) => completed += 1,
            (TickOutcome::Terminated, TickPlan::SkipPenalty) => skipped_penalty += 1,
            (TickOutcome::Terminated, TickPlan::Run { .. }) => terminated += 1,
        }
        // Completed 只表示宿主入口返回；玩家异常和 Memory 写回失败须看 player_error。
        // bot 自行捕获并用 console.log 记录的异常不计入 errored_ticks。
        if report.player_error.is_some() || report.console.iter().any(|l| l.level == "error") {
            errored_ticks += 1;
        }
        if let Some(err) = &report.player_error {
            if console_errors.len() < 300 {
                console_errors.push(format!("[tick {t}] [loop] {}", err));
            }
            *console_error_folds
                .entry(format!("[loop] {}", normalize_line(err)))
                .or_insert(0) += 1;
        }
        // 归一化计数覆盖所有日志级别，不受原文保存数量限制。
        // console_errors_folded 因而也包含 log 等级的行，便于查看 bot 自行捕获的异常。
        for line in report.console {
            if line.level == "error" && console_errors.len() < 300 {
                console_errors.push(format!("[tick {t}] {}", line.text));
            }
            *console_error_folds
                .entry(format!("[{}] {}", line.level, normalize_line(&line.text)))
                .or_insert(0) += 1;
            if console_head.len() < 20 {
                console_head.push(format!("[tick {t}][{}] {}", line.level, line.text));
            }
        }
        // 直接访问不存在的属性计入 missing；in 检查可能只是能力探测，
        // hasProbes 单独报告，不参与通过判定。
        merge_counts(&mut missing_api, &report.missing_api["missing"]);
        merge_counts(&mut has_probes, &report.missing_api["hasProbes"]);
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
        "has_probes": has_probes,
        "stub_calls_top": top(&stub_calls, 30),
        "console_errors_head": console_errors,
        "console_errors_folded": top(&console_error_folds, 10),
        "console_head": console_head,
    });
    std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;

    println!();
    println!("== bot-check 报告（{report_path}） ==");
    println!(
        "  完成 tick：{completed}/{}（硬中断 {terminated}，跳帧（桶透支停跑） {skipped_penalty}，含 console 错误的 tick {errored_ticks}）",
        ticks
    );
    println!(
        "  缺失 API：{} 项（另有 has 探测 {} 项，低置信不计验收）",
        missing_api.len(),
        has_probes.len()
    );
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
        println!("  console 错误（前 5 条，折叠 top 3）：");
        for e in console_errors.iter().take(5) {
            println!("    {}", e.lines().next().unwrap_or_default());
        }
        for (k, v) in top(&console_error_folds, 3) {
            println!("    ×{v} {k}");
        }
    }
    pool.shutdown_all();

    let verdict = verdict(
        terminated,
        completed,
        skipped_penalty,
        ticks,
        missing_api.len(),
    );
    match &verdict {
        Verdict::Pass => println!("  验收判定：通过"),
        Verdict::Fail(reasons) => println!("  验收判定：未通过 —— {}", reasons.join("；")),
    }
    Ok(verdict == Verdict::Pass)
}

/// 是否存在硬中断、未完成的 tick 或记录到的缺失 API。
/// 玩家错误和普通日志不参与该判定，详见模块说明。
#[derive(Debug, PartialEq)]
enum Verdict {
    Pass,
    Fail(Vec<String>),
}

fn verdict(
    terminated: u32,
    completed: u32,
    skipped_penalty: u32,
    ticks: u32,
    missing_api_len: usize,
) -> Verdict {
    let failures: Vec<String> = [
        (terminated > 0, format!("硬中断 {terminated} 次")),
        (
            completed < ticks,
            format!("完成 {completed}/{ticks}（含跳帧 {skipped_penalty}）"),
        ),
        (
            missing_api_len > 0,
            format!("缺失 API {missing_api_len} 项"),
        ),
    ]
    .into_iter()
    .filter(|(fail, _)| *fail)
    .map(|(_, reason)| reason)
    .collect();
    if failures.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail(failures)
    }
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

/// 把连续 ASCII 数字替换为 #，将仅 tick、坐标或数量不同的日志合并计数。
/// 这也会合并数字不同的对象名，因此分组用于诊断，不能代替原始日志。
fn normalize_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_digits = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(ch);
        }
    }
    out
}

/// 手搓夹具世界的 KKPV 两束（公开地形束 + 私有对象束）。计划/受众摘要
/// 用零占位——bot-check 的桩世界不与引擎块混用，两个哈希在此只承担
/// 结构字段，不是授权。
fn kkpv_fixture_bundles(world: &Value) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let public = kk_world::kkpv::kkpv_public_bundle_from_world_json(world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具世界 KKPV 公开束转换失败：{e}"))?;
    let private = kk_world::kkpv::kkpv_bundle_from_world_json(world, [0; 32], [0; 32])
        .map_err(|e| anyhow::anyhow!("夹具世界 KKPV 私有束转换失败：{e}"))?;
    Ok((public, private))
}

/// 构造固定房间和对象，每轮只改变 time。地形为 2500 个平原格，
/// 玩家 API 从 rooms 的 terrain 字段读取；CPU 占位字段由沙箱实际预算覆盖。
fn home_world(t: u32) -> Value {
    let plains: Vec<u8> = vec![0; 2500];
    json!({
        "time": t,
        "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
        "user": "bot",
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
            { "id": "crp1", "type": "creep", "room": "W1N1", "x": 25, "y": 25, "name": "harvester1", "owner": "bot",
              "owner": "bot", "body": ["move", "work", "carry"], "store": { "energy": 50 },
              "fatigue": 0 }
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::{Verdict, normalize_line, verdict};

    /// 跳帧导致完成数量不足，硬中断和记录到的缺失 API 各自使判定失败。
    #[test]
    fn verdict_thresholds() {
        assert_eq!(verdict(0, 100, 0, 100, 0), Verdict::Pass);
        // 跳帧：completed < ticks → 失败（消息明示跳帧数）
        assert_eq!(
            verdict(0, 99, 1, 100, 0),
            Verdict::Fail(vec!["完成 99/100（含跳帧 1）".into()])
        );
        assert_eq!(
            verdict(2, 98, 0, 100, 0),
            Verdict::Fail(vec!["硬中断 2 次".into(), "完成 98/100（含跳帧 0）".into()])
        );
        assert_eq!(
            verdict(0, 100, 0, 100, 3),
            Verdict::Fail(vec!["缺失 API 3 项".into()])
        );
    }

    #[test]
    fn normalize_folds_digit_runs() {
        assert_eq!(normalize_line("pos 12,34 energy 7"), "pos #,# energy #");
        assert_eq!(normalize_line("12345"), "#");
        // 非 ASCII 逐字符透传（中文错误消息不损坏）
        assert_eq!(normalize_line("房间 W1N1 失败"), "房间 W#N# 失败");
        assert_eq!(normalize_line(""), "");
    }
}
