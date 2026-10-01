//! `kkscreeps perf-probe` 子命令：M2A 性能观测点的小规模负载实测（A6 交付，
//! [performance-acceptance](../../docs/architecture/performance-acceptance.md)
//! 「阶段耗时」口径）。
//!
//! 驱动 EmbedEngine 的完整闭环（per-viewer 投影 → 真实沙箱玩家代码 → intent
//! 清洗 → 结算），玩家代码走真实 API 面（find/spawnCreep/harvest/
//! upgradeController）。产出：各阶段 p50/p95/max 表 + JSON 报告 +
//! 工作负载自检（空转形态非零退出，B0-6 登记项的结构性防复发）。
//! 这是 M2A 的架构验收通道；正式场景基线（场景 A/C × 3 实现 × ≥5 次独立
//! 运行）随 M2B 基准批次，本工具届时复用同一观测点。
//!
//! 窗口语义注记（长跑分段，登记）：初始 creep 的 ageTime=放置时刻+1500
//!（place 缺省，CREEP_LIFE_TIME）——`warmup+ticks` 超 ~1500 后窗中出现
//! TTL 死亡分段（墓碑结算/种群回落/spawn 重孵）；M2B 正式口径（预热 ≥500
//! + 采集 ≥3000）跑长窗时须注记该段或届时夹具摆长 ttl creep（place 支持）。

use std::collections::BTreeMap;

use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

/// 夹具布局（与距离预算微测 `fixture_distance_budget` 共用单源）。
/// controller 由 create_shard 硬编码在 (1,1)（embed.rs）；**source 放置序
/// 即 `FIND_SOURCES[0]` 的取序**（放置序=ObjId 升序=world JSON objects 数组
/// 序）——首源 (2,2) 是被采集的源，勿重排放置数组。出生落位语义见
/// tests::BORN_TILES。
const SOURCES: [(u8, u8); 2] = [(2, 2), (1, 3)];
const SPAWN: (u8, u8) = (3, 3);
/// 放置的 3 只工作 creep（两臂都在程内：到首源 ≤1 且到 controller ≤3）。
const CREEPS: [(u8, u8); 3] = [(2, 1), (1, 2), (3, 1)];

/// 负载玩家代码：真实 API 面（装配 + find + spawn + 动作 intent 预检）。
/// spawn 上限 6 只防种群爆炸；harvest/upgrade 走已就绪执行器。
/// directions [TOP, LEFT] 控制出生落位到 BORN_TILES（见布局注释）。
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

/// 房名按玩家序横排（W{i}N1）——单房每玩家，避免跨房视野交互（场景 C 的
/// 共享房变体随 M2B 基准 fixture 锁定）。
fn room_name(i: u32) -> String {
    format!("W{i}N1")
}

/// 工作负载观测（经公共读取面，不旁窥内部状态；只采样首房——多玩家下
/// 各房脚本与坐标同构，抽样自检）：controller 进度、source 最低能量、
/// 是否全部满容量。
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
    if ticks == 0 {
        anyhow::bail!("采集窗口为 0（--ticks 须 ≥1；纯预热跑不是合法用法）");
    }
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
    // 观测点验收的是阶段形态非绝对量级）。距离预算与出生落位语义见布局
    // 常量注释与 fixture_distance_budget 微测（B0-6 夹具空转清偿：注释
    // 曾自述「出生即工作」与引擎出生语义不符，评审 1 P1——现 directions
    // 控制落位、注释如实）。
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
    // 工作负载基线（采集前）：progress 增量与 source 采空证据的对照起点
    let (progress_before, obs_before) = workload_probe(&e, &room_name(0))?;
    let mut harvest_observed = !obs_before.all_at_capacity;
    e.perf_reset();
    for _ in 0..ticks {
        e.run_players(&codes)?;
        if !harvest_observed {
            let (_, obs) = workload_probe(&e, &room_name(0))?;
            harvest_observed = !obs.all_at_capacity;
        }
    }

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

/// 距离预算微测：放置位与出生位逐点锁预算（harvest ≤1 到某源、upgrade ≤3
/// 到 controller）、占位互斥（出生落位取首个空闲方向格，被占即失效）、
/// 被采集源=放置首源（FIND_SOURCES[0] 取序耦合）。
#[cfg(test)]
mod tests {
    use super::*;

    /// controller 由 create_shard 硬编码（embed.rs），夹具只依赖不摆放。
    const CONTROLLER: (u8, u8) = (1, 1);
    /// 出生落位：spawnCreep `directions:[TOP, LEFT]`（player_code）→ 首个
    /// 空闲方向格——(3,2)/(2,3) 两格都在程内。第三只孵出（spawn 雁流后）
    /// 时两格已占：二次扫描在 directions 外找到空位 → born_creep 判「有
    /// 出口但未选」逐 tick 顺延（spawn.rs 不 spawnstomp 重试语义）→
    /// **永不完成出生**，雏形滞留 spawn 格 (3,3)（程内，spawning=true
    /// 无动作）——实际负载=3 放置+2 出生共 5 只工作 creep（第一轮外部
    /// 复审 P2-1 勘正：旧注「兜底落 (4,2) 程外」的路径在本夹具不可达）。
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
        run_inner(309, 0, 1, &path.to_string_lossy())?;
        let report: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        std::fs::remove_file(path)?;
        assert!(report["workload"]["progress_delta"].as_i64().unwrap() > 0);
        assert_eq!(report["workload"]["source_energy_min_before"], 3000);
        assert_eq!(report["workload"]["source_energy_min_after"], 3000);
        assert_eq!(report["phases"]["ticks"], 309);
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
