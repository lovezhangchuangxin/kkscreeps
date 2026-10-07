//! 从跨引擎显式夹具执行对照负载。
use super::{metadata, process};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// 对照入口直接读取共享夹具，不复用旧 perf-probe 的隐式地形和对象默认值。
pub(crate) fn run(fixture: &str, warmup: u32, ticks: u32, concurrency: usize, report: &str) -> i32 {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let status = run(
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
            run(&path.to_string_lossy(), 0, 1, 2, &report.to_string_lossy()),
            1
        );
        assert!(!report.exists());
        std::fs::remove_file(path)?;
        Ok(())
    }
}
