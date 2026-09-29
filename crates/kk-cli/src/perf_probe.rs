//! `kkscreeps perf-probe` 子命令：M2A 性能观测点的小规模负载实测（A6 交付，
//! [performance-acceptance](../../docs/architecture/performance-acceptance.md)
//! 「阶段耗时」口径）。
//!
//! 驱动 EmbedEngine 的完整闭环（per-viewer 投影 → 真实沙箱玩家代码 → intent
//! 清洗 → 结算），玩家代码走真实 API 面（find/spawnCreep/harvest/
//! upgradeController）。产出：各阶段 p50/p95/max 表 + JSON 报告。
//! 这是 M2A 的架构验收通道；正式场景基线（场景 A/C × 3 实现 × ≥5 次独立
//! 运行）随 M2B 基准批次，本工具届时复用同一观测点。

use std::collections::BTreeMap;

use kk_engine::EmbedEngine;
use serde_json::{Value, json};

/// 负载玩家代码：真实 API 面（装配 + find + spawn + 动作 intent 预检）。
/// spawn 上限 6 只防种群爆炸；harvest/upgrade 走已就绪执行器。
fn player_code() -> String {
    r#"
const spawn = Game.spawns && Object.values(Game.spawns)[0];
if (spawn && !spawn.spawning && Object.keys(Game.creeps).length < 6) {
  spawn.spawnCreep(['work', 'carry', 'move'], 'w' + Game.time + '_' + spawn.name, {});
}
for (const name in Game.creeps) {
  const c = Game.creeps[name];
  if (c.store.energy > 0 && c.room.controller) {
    c.upgradeController(c.room.controller);
  } else {
    const src = c.room.find(FIND_SOURCES)[0];
    if (src) c.harvest(src);
  }
}
Game.time
"#
    .to_string()
}

/// 房名按玩家序横排（W{i}N1）——单房每玩家，避免跨房视野交互（场景 C 的
/// 共享房变体随 M2B 基准 fixture 锁定）。
fn room_name(i: u32) -> String {
    format!("W{i}N1")
}

pub fn run(ticks: u32, warmup: u32, players: u32, report_path: &str) -> i32 {
    match run_inner(ticks, warmup, players, report_path) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("perf-probe 失败: {e:#}");
            1
        }
    }
}

fn run_inner(ticks: u32, warmup: u32, players: u32, report_path: &str) -> anyhow::Result<()> {
    let players = players.clamp(1, 8);
    let handles: Vec<String> = (0..players).map(|i| format!("p{}", i + 1)).collect();
    let rooms: Vec<Value> = (0..players)
        .map(|i| json!({ "name": room_name(i), "rcl": 4, "owner": format!("p{}", i + 1) }))
        .collect();

    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": handles,
        "rooms": rooms,
    }))?;

    // 每房负载：2 source + 1 spawn + 3 creep（约 400 对象的 B 场景缩比——
    // 观测点验收的是阶段形态非绝对量级）
    for i in 0..players {
        let room = room_name(i);
        let owner = format!("p{}", i + 1);
        for (x, y) in [(10u8, 10u8), (40, 40)] {
            e.place_object(
                &room,
                "structure",
                &json!({ "pos": [x, y], "structureType": "source",
                    "energy": 3000, "energyCapacity": 3000 }),
            )?;
        }
        e.place_object(
            &room,
            "structure",
            &json!({ "pos": [25, 25], "structureType": "spawn",
                "owner": owner, "name": format!("Spawn{}", i + 1),
                "store": { "energy": 300 } }),
        )?;
        for c in 0..3u8 {
            e.place_object(
                &room,
                "creep",
                &json!({ "pos": [24 + c, 26], "name": format!("w{}_{c}", i + 1),
                    "owner": owner, "body": ["work", "carry", "move"],
                    "store": { "energy": 50 } }),
            )?;
        }
    }

    let mut codes: BTreeMap<String, String> = BTreeMap::new();
    for h in &handles {
        codes.insert(h.clone(), player_code());
    }

    println!(
        "perf-probe: {players} 玩家 × 每房（2 source + spawn + 3 creep），预热 {warmup} + 采集 {ticks} ticks"
    );
    for _ in 0..warmup {
        e.run_players(&codes)?;
    }
    e.perf_reset();
    for _ in 0..ticks {
        e.run_players(&codes)?;
    }

    let report = json!({
        "tool": "kkscreeps perf-probe",
        "players": players,
        "ticks": ticks,
        "warmup": warmup,
        "phases": e.perf_report(),
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
                "  {label:<12} p50 {:>10.3} ms | p95 {:>10.3} ms | max {:>10.3} ms",
                p.get("p50_ms").and_then(Value::as_f64).unwrap_or(0.0),
                p.get("p95_ms").and_then(Value::as_f64).unwrap_or(0.0),
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
