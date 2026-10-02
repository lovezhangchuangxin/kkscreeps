//! 驱动 EmbedEngine 运行多玩家负载，采集各阶段耗时并输出 JSON 报告。
//!
//! 每玩家拥有独立房间，使用玩家 API 孵化、采集和升级。预热后清空耗时样本，
//! 报告投影、执行、结算和整次调用的统计，定义沿用 kk-runtime::EmbedEngine；
//! 另测采集窗口的进程 CPU 与 tick 边界 RSS，包含采样及窗口内负载检查。
//! 最多记录 50,000 个采集样本，后续 tick 仍运行但不加入耗时统计。
//! 负载自检只检查首房的控制器进度和 source 能量；通过不代表每房均得到验证。
//!
//! 初始 creep 寿命为 1500 tick。预热与采集合计超过其寿命时，会发生死亡、
//! 墓碑衰减和重新孵化，长窗口的工作负载不能视为始终不变。

use std::collections::BTreeMap;

use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

mod process;

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
const spawn = Game.spawns && Object.values(Game.spawns)[0];
if (spawn && !spawn.spawning && Object.keys(Game.creeps).length < 6) {
  spawn.spawnCreep(['work', 'carry', 'move'], 'w' + Game.time + '_' + spawn.name, {directions: [TOP, LEFT]});
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

/// 每名玩家单独使用一个房间，避免共享对象产生玩家间竞争。
fn room_name(i: u32) -> String {
    format!("W{i}N1")
}

/// 一个房间在采样时的 source 最低能量，以及是否全部达到容量。
struct SourceObs {
    energy_min: i64,
    all_at_capacity: bool,
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
        all_at_capacity: sources.iter().all(|(e, c)| e >= c),
    };
    Ok((progress, obs))
}

pub fn run(
    ticks: u32,
    warmup: u32,
    players: u32,
    concurrency: Option<usize>,
    report_path: &str,
) -> i32 {
    match run_inner(ticks, warmup, players, concurrency, report_path) {
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
        "perf-probe: {players} 玩家 × 每房（2 source + spawn + 3 creep），预热 {warmup} + 采集 {ticks} ticks"
    );
    for _ in 0..warmup {
        e.run_players(&codes)?;
    }
    // 在采集窗口前读取首房状态，并清空预热耗时样本。
    let (progress_before, obs_before) = workload_probe(&e, &room_name(0))?;
    let mut harvest_observed = !obs_before.all_at_capacity;
    e.perf_reset();
    let mut process = process::ProcessProbe::start()?;
    for _ in 0..ticks {
        e.run_players(&codes)?;
        if !harvest_observed {
            let (_, obs) = workload_probe(&e, &room_name(0))?;
            harvest_observed = !obs.all_at_capacity;
        }
        process.observe()?;
    }
    let process_report = process.finish(ticks)?;

    // 只检查首房：控制器进度须增加；合计至少 100 tick 时，还须观察到 source 未满。
    // 检查采集期间的状态，不能只看两个边界：source 再生可使它们同时满容量。
    // 一旦观察到未满便停止额外查询；查询在 run_players 的耗时样本之外。
    let (progress_after, obs_after) = workload_probe(&e, &room_name(0))?;
    let progress_delta = progress_after - progress_before;
    let elapsed_ticks = u64::from(warmup) + u64::from(ticks);
    let window_long_enough = elapsed_ticks >= 100;
    if progress_delta <= 0 {
        anyhow::bail!(
            "工作负载自检失败：upgrade 臂空转（progress 增量 {progress_delta}、\
             source 最低能量采集前 {} → 采集后 {}）——动作未产生 intent，\
             检查距离预算注释与脚本分支",
            obs_before.energy_min,
            obs_after.energy_min
        );
    }
    if window_long_enough && !harvest_observed {
        anyhow::bail!(
            "工作负载自检失败：未观察到采集（预热与采集合计 {elapsed_ticks} tick，\
             采集期间首房 source 始终满容量）——检查 harvest 分支与源放置序"
        );
    }

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
            "source_energy_min_before": obs_before.energy_min,
            "source_energy_min_after": obs_after.energy_min,
        },
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

fn metadata() -> Value {
    let command = |program: &str, args: &[&str]| {
        std::process::Command::new(program)
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logical_cpus": std::thread::available_parallelism().ok().map(|n| n.get()),
        "system": command("uname", &["-a"]),
        "rustc_on_path": command("rustc", &["--version"]),
        "git_commit": command("git", &["rev-parse", "HEAD"]),
        "git_status": command("git", &["status", "--porcelain"]),
        "debug_assertions": cfg!(debug_assertions),
        "unix_time_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis()),
    })
}

/// 对照入口直接读取共享夹具，不复用旧 perf-probe 的隐式地形和对象默认值。
pub fn run_comparison(
    fixture: &str,
    warmup: u32,
    ticks: u32,
    concurrency: usize,
    report: &str,
) -> i32 {
    let run = || -> anyhow::Result<()> {
        anyhow::ensure!(ticks > 0 && ticks <= 50_000, "采集 ticks 须为 1..=50000");
        anyhow::ensure!(warmup <= 50_000, "预热 ticks 不得超过 50000");
        let fixture: Value = serde_json::from_slice(&std::fs::read(fixture)?)?;
        let players = fixture["shard"]["players"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("缺少玩家数组"))?;
        anyhow::ensure!(
            !players.is_empty() && players.len() <= 32,
            "玩家数须为 1..=32"
        );
        let ids: std::collections::BTreeSet<_> = players.iter().filter_map(Value::as_str).collect();
        anyhow::ensure!(ids.len() == players.len(), "玩家须为不重复的字符串");
        anyhow::ensure!(
            fixture["shard"]["rooms"]
                .as_array()
                .is_some_and(|rooms| !rooms.is_empty()),
            "缺少非空房间数组"
        );
        let mut e = EmbedEngine::new();
        e.set_player_concurrency(concurrency)?;
        // xxscreeps 的生产 runner 固定 limit=100；对照夹具显式统一预算。
        let cpu_limit = fixture["cpu_limit"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("缺少 cpu_limit"))?;
        anyhow::ensure!(cpu_limit == 100.0, "对照预算 cpu_limit 须为 100");
        for id in &ids {
            e.set_sandbox_config_for_test(
                id,
                kk_runner::SandboxConfig {
                    cpu_limit,
                    ..kk_runner::SandboxConfig::default()
                },
            );
        }
        e.create_shard(&fixture["shard"])?;
        for object in fixture["objects"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("缺少对象数组"))?
        {
            e.place_object(
                object["room"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("缺少 room"))?,
                object["kind"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("缺少 kind"))?,
                &object["spec"],
            )?;
        }
        let code = fixture["code"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("缺少 code"))?;
        let codes: BTreeMap<String, String> = players
            .iter()
            .map(|p| {
                Ok((
                    p.as_str()
                        .ok_or_else(|| anyhow::anyhow!("玩家须为字符串"))?
                        .into(),
                    code.into(),
                ))
            })
            .collect::<anyhow::Result<_>>()?;
        let rooms = fixture["shard"]["rooms"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("缺少房间数组"))?;
        let observe = |engine: &EmbedEngine| -> anyhow::Result<Vec<Value>> {
            rooms.iter().map(|r| {
                let name = r["name"].as_str().ok_or_else(|| anyhow::anyhow!("缺少房间名"))?;
                let structures = engine.find_in_room(name, kk_core::constants::FIND_STRUCTURES);
                let controller = structures.iter().find(|o| o["structureType"] == "controller")
                    .ok_or_else(|| anyhow::anyhow!("房间 {name} 缺少控制器"))?;
                let mut sources: Vec<Value> = engine.find_in_room(name, kk_core::constants::FIND_SOURCES)
                    .iter().map(|o| json!({
                        "x":o["pos"]["x"], "y":o["pos"]["y"],
                        "energy":o["energy"], "capacity":o["energyCapacity"]
                    })).collect();
                sources.sort_by_key(|o| (o["y"].as_i64(),o["x"].as_i64()));
                let mut creeps: Vec<Value> = engine.find_in_room(name, kk_core::constants::FIND_CREEPS)
                    .iter().map(|o| json!({
                        "name":o["name"], "x":o["pos"]["x"], "y":o["pos"]["y"],
                        "energy":o["store"]["energy"].as_i64().unwrap_or(0), "hits":o["hits"],
                        "body":o["body"].as_array().map(|parts| parts.iter().map(|p|p["type"].clone()).collect::<Vec<_>>()),
                        "spawning":o["spawning"]
                    })).collect();
                creeps.sort_by_key(|o| o["name"].as_str().unwrap_or("").to_string());
                let mut spawns: Vec<Value> = structures.iter().filter(|o| o["structureType"] == "spawn")
                    .map(|o| json!({
                        "name":o["name"], "x":o["pos"]["x"], "y":o["pos"]["y"],
                        "hits":o["hits"], "energy":o["store"]["energy"].as_i64().unwrap_or(0),
                        "spawning":!o["spawning"].is_null()
                    })).collect();
                spawns.sort_by_key(|o| o["name"].as_str().unwrap_or("").to_string());
                Ok(json!({"room":name,"progress":controller["progress"],"level":controller["level"],
                    "sources":sources,"creeps":creeps,"spawns":spawns}))
            }).collect()
        };
        let initial = observe(&e)?;
        let step = |engine: &mut EmbedEngine| -> anyhow::Result<()> {
            for (player, result) in engine.run_players(&codes)? {
                anyhow::ensure!(
                    result.expr["ok"] == true,
                    "玩家 {player} 执行失败：{}",
                    result.expr
                );
            }
            Ok(())
        };
        for _ in 0..warmup {
            step(&mut e)?;
        }
        let before = observe(&e)?;
        e.perf_reset();
        let mut process = process::ProcessProbe::start()?;
        let mut wall = Vec::with_capacity(ticks as usize);
        for _ in 0..ticks {
            let start = std::time::Instant::now();
            step(&mut e)?;
            wall.push(start.elapsed().as_secs_f64() * 1000.0);
            process.observe()?;
        }
        let process = process.finish(ticks)?;
        wall.sort_by(f64::total_cmp);
        let quantile = |q: f64| wall[((wall.len() - 1) as f64 * q).round() as usize];
        let mut phases = e.perf_report();
        let engine_wall = phases["phases"]["wall"].clone();
        phases["phases"]["wall"] = json!({"count":wall.len(),"p50_ms":quantile(0.5),
            "p95_ms":quantile(0.95),"p99_ms":quantile(0.99),"max_ms":wall.last()});
        let after = observe(&e)?;
        for (a, b) in before.iter().zip(&after) {
            anyhow::ensure!(
                b["progress"].as_i64() > a["progress"].as_i64(),
                "房间 {} 未升级",
                a["room"]
            );
        }
        std::fs::write(
            report,
            serde_json::to_vec_pretty(&json!({
                "engine":"kkscreeps","players":players.len(),"warmup":warmup,"ticks":ticks,
                "concurrency":e.player_concurrency(),"metadata":metadata(),"process":process,
                "cpu_budget":{"limit":cpu_limit,"bucket":10000,"tick_limit":500},
                "phases":phases,"embed_engine_wall":engine_wall,"initial":initial,"before":before,"after":after,
                "completion":players.iter().filter_map(Value::as_str).map(|id| (id.to_string(),e.capture_console_logs(id))).collect::<BTreeMap<_,_>>()
            }))?,
        )?;
        Ok(())
    };
    match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("compare-probe: {e:#}");
            1
        }
    }
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
        run_inner(309, 0, 1, None, &path.to_string_lossy())?;
        let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        std::fs::remove_file(path)?;
        assert!(report["workload"]["progress_delta"].as_i64().unwrap() > 0);
        assert_eq!(report["workload"]["source_energy_min_before"], 3000);
        assert_eq!(report["workload"]["source_energy_min_after"], 3000);
        assert_eq!(report["phases"]["ticks"], 309);
        Ok(())
    }

    #[test]
    fn comparison_uses_shared_budget_and_reads_positions() -> anyhow::Result<()> {
        let fixture_path =
            std::env::temp_dir().join(format!("kk-comparison-input-{}.json", std::process::id()));
        let report_path = fixture_path.with_extension("report.json");
        let fixture = json!({
            "cpu_limit":100,
            "shard":{"players":["100"],"rooms":[{"name":"W0N1","owner":"100","rcl":4}]},
            "objects":[{"room":"W0N1","kind":"creep","spec":{
                "pos":[2,1],"name":"worker","owner":"100","body":["work","carry","move"],"store":{"energy":50}}}],
            "code":"if (Game.cpu.limit !== 100 || Game.cpu.tickLimit !== 500) throw new Error('budget'); Game.creeps.worker.upgradeController(Game.rooms.W0N1.controller);"
        });
        std::fs::write(&fixture_path, serde_json::to_vec(&fixture)?)?;
        let status = run_comparison(
            &fixture_path.to_string_lossy(),
            0,
            10,
            2,
            &report_path.to_string_lossy(),
        );
        assert_eq!(status, 0);
        let report: Value = serde_json::from_slice(&std::fs::read(&report_path)?)?;
        assert_eq!(report["initial"][0]["creeps"][0]["x"], 2);
        assert_eq!(report["after"][0]["progress"], 10);
        assert_eq!(report["after"][0]["creeps"][0]["energy"], 40);
        assert_eq!(report["phases"]["phases"]["wall"]["count"], 10);
        std::fs::remove_file(fixture_path)?;
        std::fs::remove_file(report_path)?;
        Ok(())
    }

    #[test]
    fn comparison_rejects_room_without_controller() -> anyhow::Result<()> {
        let path =
            std::env::temp_dir().join(format!("kk-comparison-invalid-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({
                "cpu_limit":100,"shard":{"players":["100"],"rooms":[{"name":"W0N1","controller":false}]},
                "objects":[],"code":""
            }))?,
        )?;
        let report = path.with_extension("report.json");
        assert_eq!(
            run_comparison(&path.to_string_lossy(), 0, 1, 2, &report.to_string_lossy()),
            1
        );
        assert!(!report.exists());
        std::fs::remove_file(path)?;
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
