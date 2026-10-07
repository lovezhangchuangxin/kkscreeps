//! 驱动 EmbedEngine 运行多玩家负载，采集各阶段耗时并输出 JSON 报告。
//!
//! 每玩家拥有独立房间，使用玩家 API 孵化、采集和升级。预热后清空耗时样本，
//! 报告投影、执行、结算和整次调用的统计，定义沿用 kk-runtime::EmbedEngine；
//! 另测采集窗口的进程 CPU 与 tick 边界 RSS，包含采样及窗口内负载检查。
//! `--kkpv` 切换世界注入通道（默认 JSON 参考）做同负载 A/B；报告附
//! `channel` 与 `injection_bytes_sample`（p1 单玩家在计时窗外的注入
//! 体积采样，KKPV 形态为信封+束双计数）。
//! 最多记录 50,000 个采集样本，后续 tick 仍运行但不加入耗时统计。
//! 每名玩家的执行回执、成功动作计数和全部房间的边界状态均须通过自检。
//!
//! 初始 creep 寿命为 1500 tick。预热与采集合计超过其寿命时，会发生死亡、
//! 墓碑衰减和重新孵化，长窗口的工作负载不能视为始终不变。

use std::collections::BTreeMap;

use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

use super::{metadata, process};

/// 按此顺序摆放 source，玩家脚本固定采集 `FIND_SOURCES[0]`。
/// 同房对象按 ID 升序投影，因此首源必须保留在 (2,2)。
const SOURCES: [(u8, u8); 2] = [(2, 2), (1, 3)];
const SPAWN: (u8, u8) = (3, 3);
/// 初始 creep 坐标，距首源不超过 1、距控制器不超过 3。
const CREEPS: [(u8, u8); 3] = [(2, 1), (1, 2), (3, 1)];

/// 有能量时升级，空仓时采集首源；少于 6 个 creep 且 spawn 空闲时请求孵化。
/// 只选择 TOP、LEFT 出生，确保成功出生的 creep 仍在采集和升级范围内。
fn player_code() -> String {
    r#"
const counts = globalThis.benchCounts ||= {ticks:0, harvest:0, upgrade:0, spawn:0};
counts.ticks++;
const spawn = Game.spawns && Object.values(Game.spawns)[0];
if (spawn && !spawn.spawning && Object.keys(Game.creeps).length < 6) {
  if (spawn.spawnCreep(['work', 'carry', 'move'], 'w' + Game.time + '_' + spawn.name, {directions: [TOP, LEFT]}) === OK) counts.spawn++;
}
for (const name in Game.creeps) {
  const c = Game.creeps[name];
  if (c.spawning) continue;
  if (c.store.energy > 0 && c.room.controller) {
    if (c.upgradeController(c.room.controller) === OK) counts.upgrade++;
  } else {
    const src = c.room.find(FIND_SOURCES)[0];
    if (src && c.harvest(src) === OK) counts.harvest++;
  }
}
counts
"#
    .to_string()
}

/// 每名玩家单独使用一个房间，避免共享对象产生玩家间竞争。
fn room_name(i: u32) -> String {
    format!("W{i}N1")
}

/// 一个房间在采样时的 source 最低能量，以及是否全部达到容量。
struct SourceObs {
    energy_min: i64,
}

fn workload_probe(e: &EmbedEngine, room: &str) -> anyhow::Result<(i64, SourceObs)> {
    let progress = e
        .find_in_room(room, kk_core::constants::FIND_STRUCTURES)
        .iter()
        .find(|o| o.get("structureType").and_then(Value::as_str) == Some("controller"))
        .and_then(|o| o.get("progress").and_then(Value::as_i64))
        .ok_or_else(|| anyhow::anyhow!("房间 {room} 未找到 controller"))?;
    let sources: Vec<(i64, i64)> = e
        .find_in_room(room, kk_core::constants::FIND_SOURCES)
        .iter()
        .filter_map(|o| {
            let energy = o.get("energy").and_then(Value::as_i64)?;
            let cap = o.get("energyCapacity").and_then(Value::as_i64)?;
            Some((energy, cap))
        })
        .collect();
    if sources.is_empty() {
        anyhow::bail!("房间 {room} 未找到 source");
    }
    let obs = SourceObs {
        energy_min: sources.iter().map(|(e, _)| *e).min().unwrap(),
    };
    Ok((progress, obs))
}

pub fn run(
    ticks: u32,
    warmup: u32,
    players: u32,
    concurrency: Option<usize>,
    report_path: &str,
    kkpv: bool,
) -> i32 {
    match run_inner(ticks, warmup, players, concurrency, report_path, kkpv) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("perf-probe 失败: {e:#}");
            1
        }
    }
}

fn run_inner(
    ticks: u32,
    warmup: u32,
    players: u32,
    concurrency: Option<usize>,
    report_path: &str,
    kkpv: bool,
) -> anyhow::Result<()> {
    if ticks == 0 {
        anyhow::bail!("采集窗口为 0（--ticks 须 ≥1；纯预热跑不是合法用法）");
    }
    anyhow::ensure!((1..=32).contains(&players), "--players 须为 1..32");
    let metadata = metadata();
    let handles: Vec<String> = (0..players).map(|i| format!("p{}", i + 1)).collect();
    let rooms: Vec<Value> = (0..players)
        .map(|i| json!({ "name": room_name(i), "rcl": 4, "owner": format!("p{}", i + 1) }))
        .collect();

    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": handles,
        "rooms": rooms,
    }))?;
    // 通道选择在分片创建后、玩家执行前生效；A/B 对照用同一负载同日各跑。
    e.set_kkpv_channel(kkpv);

    // 初始放置 3 个工作 creep，指定出生格还能容纳 2 个。
    // 出口被占用时孵化会等待；初始 creep 死亡后出口才可能再次可用。
    for i in 0..players {
        let room = room_name(i);
        let owner = format!("p{}", i + 1);
        for &(x, y) in &SOURCES {
            e.place_object(
                &room,
                "structure",
                &json!({ "pos": [x, y], "structureType": "source",
                    "energy": 3000, "energyCapacity": 3000 }),
            )?;
        }
        let (sx, sy) = SPAWN;
        e.place_object(
            &room,
            "structure",
            &json!({ "pos": [sx, sy], "structureType": "spawn",
                "owner": owner, "name": format!("Spawn{}", i + 1),
                "store": { "energy": 300 } }),
        )?;
        for &(x, y) in &CREEPS {
            e.place_object(
                &room,
                "creep",
                &json!({ "pos": [x, y], "name": format!("w{}_{x}_{y}", i + 1),
                    "owner": owner, "body": ["work", "carry", "move"],
                    "store": { "energy": 50 } }),
            )?;
        }
    }

    if let Some(limit) = concurrency {
        e.set_player_concurrency(limit)?;
    }
    let mut codes: BTreeMap<String, String> = BTreeMap::new();
    for h in &handles {
        codes.insert(h.clone(), player_code());
    }

    println!(
        "perf-probe: {players} 玩家 × 每房（2 source + spawn + 3 creep），预热 {warmup} + 采集 {ticks} ticks，通道 {}",
        if kkpv {
            "KKPV（二进制）"
        } else {
            "JSON（参考）"
        }
    );
    let observe =
        |engine: &EmbedEngine| -> anyhow::Result<Vec<Value>> {
            (0..players).map(|i| {
            let room = room_name(i);
            let (progress, source) = workload_probe(engine, &room)?;
            Ok(json!({"room":room,"progress":progress,"source_energy_min":source.energy_min,
                "sources":engine.find_in_room(&room, kk_core::constants::FIND_SOURCES),
                "creeps":engine.find_in_room(&room, kk_core::constants::FIND_CREEPS)}))
        }).collect()
        };
    let initial = observe(&e)?;
    let mut receipts = BTreeMap::new();
    let mut step = |engine: &mut EmbedEngine, round: u32| -> anyhow::Result<()> {
        let runs = engine.run_players(&codes)?;
        anyhow::ensure!(runs.len() == handles.len(), "第 {round} 轮回执缺失");
        for handle in &handles {
            let run = runs
                .get(handle)
                .ok_or_else(|| anyhow::anyhow!("缺少玩家 {handle}"))?;
            anyhow::ensure!(
                run.executed && run.expr["ok"] == true,
                "玩家 {handle} 第 {round} 轮未成功执行：{}",
                run.expr
            );
            let counters = &run.expr["value"];
            anyhow::ensure!(
                counters["ticks"] == round,
                "玩家 {handle} 执行计数不符：{counters}"
            );
            receipts.insert(handle.clone(), counters.clone());
        }
        Ok(())
    };
    for round in 1..=warmup {
        step(&mut e, round)?;
    }
    let before = observe(&e)?;
    e.perf_reset();
    let mut process = process::ProcessProbe::start()?;
    for index in 1..=ticks {
        step(&mut e, warmup + index)?;
        process.observe()?;
    }
    let process_report = process.finish(ticks)?;
    let after = observe(&e)?;
    for (index, handle) in handles.iter().enumerate() {
        let counters = &receipts[handle];
        anyhow::ensure!(
            after[index]["progress"].as_i64() > before[index]["progress"].as_i64(),
            "玩家 {handle} 的控制器未升级"
        );
        anyhow::ensure!(
            counters["upgrade"].as_u64().is_some_and(|n| n > 0),
            "玩家 {handle} 没有成功升级动作"
        );
        if warmup + ticks >= 100 {
            anyhow::ensure!(
                counters["harvest"].as_u64().is_some_and(|n| n > 0),
                "玩家 {handle} 没有成功采集动作"
            );
        }
    }
    let progress_delta =
        after[0]["progress"].as_i64().unwrap() - before[0]["progress"].as_i64().unwrap();
    let report = json!({
        "tool": "kkscreeps perf-probe",
        "players": players,
        "ticks": ticks,
        "warmup": warmup,
        "player_concurrency": e.player_concurrency(),
        "player_cpu_clock": "thread_cpu_time",
        "metadata": metadata,
        "process": process_report,
        "workload": {
            "progress_delta": progress_delta,
            "source_energy_min_before": before[0]["source_energy_min"],
            "source_energy_min_after": after[0]["source_energy_min"],
            "initial": initial, "before": before, "after": after, "actions": receipts,
        },
        "phases": e.perf_report(),
        "channel": if kkpv { "kkpv" } else { "json" },
        // p1 单玩家、采集结束后一次采样（计时窗外）；失败只降级为
        // null 并告警——测量已完成，附属诊断不得丢弃整份报告。
        "injection_bytes_sample": match e.injection_size_sample(
            handles.first().map(String::as_str).unwrap_or("p1"),
        ) {
            Ok(sample) => sample,
            Err(err) => {
                eprintln!("注入体积采样失败（报告置 null）：{err:#}");
                serde_json::Value::Null
            }
        },
    });
    std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;

    println!();
    println!("== perf-probe 报告（{report_path}） ==");
    let phases = report
        .pointer("/phases/phases")
        .cloned()
        .unwrap_or(json!({}));
    for (name, label) in [
        ("project", "世界投影构建"),
        ("run", "玩家运行"),
        ("settle", "结算"),
        ("wall", "tick 墙钟"),
    ] {
        if let Some(p) = phases.get(name).and_then(Value::as_object) {
            println!(
                "  {label:<12} p50 {:>10.3} ms | p95 {:>10.3} ms | p99 {:>10.3} ms | max {:>10.3} ms",
                p.get("p50_ms").and_then(Value::as_f64).unwrap_or(0.0),
                p.get("p95_ms").and_then(Value::as_f64).unwrap_or(0.0),
                p.get("p99_ms").and_then(Value::as_f64).unwrap_or(0.0),
                p.get("max_ms").and_then(Value::as_f64).unwrap_or(0.0),
            );
        }
    }
    let ticks_seen = report
        .pointer("/phases/ticks")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    println!("  采样 tick 数：{ticks_seen}");
    Ok(())
}

/// 检查负载布局，以及 source 再生不会使正常采集被误判。
#[cfg(test)]
mod tests {
    use super::*;

    /// create_shard 默认控制器坐标；本夹具使用该默认值。
    const CONTROLLER: (u8, u8) = (1, 1);
    /// TOP、LEFT 对应的出生格。两格都在动作范围内，且不与初始 creep 重叠。
    /// 两格都被占用时，孵化会逐 tick 等待，不能改为使用范围外的其他出口；
    /// 等待持续到所选出口释放，长窗口中初始 creep 死亡后可能继续出生。
    const BORN_TILES: [(u8, u8); 2] = [(3, 2), (2, 3)];

    fn chebyshev(a: (u8, u8), b: (u8, u8)) -> u32 {
        u32::from(a.0.abs_diff(b.0)).max(u32::from(a.1.abs_diff(b.1)))
    }

    /// 第 309 tick 的再生使首尾 source 都满容量；中途实际采集仍应使自检通过。
    #[test]
    fn source_regeneration_does_not_hide_harvesting() -> anyhow::Result<()> {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-perf-regeneration-{}.json",
            std::process::id()
        ));
        run_inner(309, 0, 1, None, &path.to_string_lossy(), false)?;
        let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        std::fs::remove_file(path)?;
        assert!(report["workload"]["progress_delta"].as_i64().unwrap() > 0);
        assert_eq!(report["workload"]["source_energy_min_before"], 3000);
        assert_eq!(report["workload"]["source_energy_min_after"], 3000);
        assert_eq!(report["phases"]["ticks"], 309);
        Ok(())
    }

    #[test]
    fn perf_probe_checks_all_players_in_both_channels() -> anyhow::Result<()> {
        for kkpv in [false, true] {
            let path = std::env::temp_dir().join(format!(
                "kk-quality-perf-{}-{kkpv}.json",
                std::process::id()
            ));
            run_inner(120, 0, 4, Some(2), &path.to_string_lossy(), kkpv)?;
            let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            assert_eq!(report["workload"]["after"].as_array().unwrap().len(), 4);
            for counters in report["workload"]["actions"].as_object().unwrap().values() {
                assert_eq!(counters["ticks"], 120);
                assert!(counters["harvest"].as_u64().unwrap() > 0);
                assert!(counters["upgrade"].as_u64().unwrap() > 0);
            }
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    #[test]
    fn fixture_distance_budget() {
        let working = CREEPS.iter().chain(BORN_TILES.iter());
        for &pos in working {
            assert!(
                SOURCES.iter().any(|&s| chebyshev(pos, s) <= 1),
                "creep {pos:?} 到全部源超采集距离（需 ≤1）"
            );
            assert!(
                chebyshev(pos, CONTROLLER) <= 3,
                "creep {pos:?} 到 controller 超升级距离（需 ≤3）"
            );
        }
        for &c in &CREEPS {
            for &b in &BORN_TILES {
                assert_ne!(c, b, "出生位 {b:?} 被放置位 {c:?} 占据");
            }
            assert_ne!(c, SPAWN, "放置位不得占 spawn 位");
        }
        assert_ne!(SOURCES[0], BORN_TILES[0], "首源被出生位占据");
        assert_eq!(SOURCES[0], (2, 2), "放置首源即 FIND_SOURCES[0]（被采集源）");
    }
}
