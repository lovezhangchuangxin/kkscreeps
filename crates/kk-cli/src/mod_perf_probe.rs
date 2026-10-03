//! 密集 link 的纯结算基准；每个对象每 tick 必须减一次冷却，JS 四个链位各计一次。
//! 启动、预热和边界状态验证不计入窗口；不执行玩家代码或玩家投影。

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use kk_mod_host::{ChainProfiler, ModVmLimits, RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::perf_probe::{metadata, process::ProcessProbe};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Mode {
    Native,
    Empty,
    Chain,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Empty => "empty",
            Self::Chain => "chain",
        }
    }
}

pub(crate) struct Options {
    pub profile: bool,
    pub mode: Mode,
    pub rooms: u32,
    pub links_per_room: u32,
    pub warmup: u32,
    pub ticks: u32,
    pub fixtures: String,
    pub report: String,
}

pub(crate) fn run(options: Options) -> i32 {
    match measure(&options) {
        Ok(report) => {
            let result = serde_json::to_string_pretty(&report)
                .map_err(anyhow::Error::from)
                .and_then(|text| {
                    std::fs::write(&options.report, text).map_err(anyhow::Error::from)
                });
            match result {
                Ok(()) => {
                    println!("mod-perf-probe {}：{}", options.mode.name(), options.report);
                    0
                }
                Err(error) => {
                    eprintln!("mod-perf-probe 报告写入失败：{error:#}");
                    1
                }
            }
        }
        Err(error) => {
            eprintln!("mod-perf-probe 失败：{error:#}");
            1
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn limits_report(limits: &ModVmLimits) -> Value {
    json!({
        "registration_cpu_ms": limits.registration.cpu.as_millis(),
        "registration_wall_ms": limits.registration.wall.as_millis(),
        "settlement_cpu_ms": limits.settlement.cpu.as_millis(),
        "settlement_wall_ms": limits.settlement.wall.as_millis(),
        "memory_bytes": limits.memory_bytes,
    })
}

fn measure(options: &Options) -> anyhow::Result<Value> {
    anyhow::ensure!((1..=4).contains(&options.rooms), "--rooms 须为 1..4");
    anyhow::ensure!(
        (1..=400).contains(&options.links_per_room),
        "--links-per-room 须为 1..400"
    );
    anyhow::ensure!(
        (1..=50_000).contains(&options.ticks),
        "--ticks 须为 1..50000"
    );
    anyhow::ensure!(options.warmup <= 50_000, "--warmup 须为 0..50000");
    let total_ticks = options.warmup + options.ticks;
    let rooms: Vec<_> = (0..options.rooms)
        .map(|i| json!({"name":format!("W{i}N1"),"rcl":6,"owner":"p1"}))
        .collect();
    let spec = json!({"players":["p1"],"rooms":rooms});
    let limits = ModVmLimits::default();
    let profiler = options.profile.then(ChainProfiler::default);
    let mut engine = EmbedEngine::new();
    let mut package_digests = BTreeMap::new();
    if options.mode == Mode::Native {
        engine.create_shard(&spec)?;
    } else {
        let packages = discover_packages(&Path::new(&options.fixtures).join(options.mode.name()))?;
        anyhow::ensure!(
            packages.len() == 1 && packages.contains_key("perf"),
            "夹具须仅包含 perf 包"
        );
        for (name, package) in &packages {
            package_digests.insert(
                name.clone(),
                format!(
                    "{:x}",
                    Sha256::digest(serde_json::to_vec(
                        &package
                            .files
                            .iter()
                            .map(|(path, bytes)| (path.to_string_lossy(), bytes))
                            .collect::<Vec<_>>()
                    )?)
                ),
            );
        }
        engine.create_shard_with_mods(
            &spec,
            &packages,
            RegisterOptions {
                vm_limits: limits.clone(),
                profiler: profiler.clone(),
                ..Default::default()
            },
        )?;
    }
    let mut links = Vec::new();
    for room in 0..options.rooms {
        for index in 0..options.links_per_room {
            let id = engine.place_object(
                &format!("W{room}N1"),
                "structure",
                &json!({
                    "pos":[index % 50, index / 50 + 10], "structureType":"link",
                    "owner":"p1", "store":{"energy":0}, "cooldown":total_ticks,
                }),
            )?;
            links.push((id.clone(), engine.get_object(&id).unwrap()));
        }
    }
    let generation = engine.engine_generation();
    let before = verify(&engine, &links, total_ticks, 0, generation, options.mode)?;
    for _ in 0..options.warmup {
        engine.tick(1)?;
    }
    let warmed = verify(
        &engine,
        &links,
        total_ticks,
        options.warmup,
        generation,
        options.mode,
    )?;
    let mut process = ProcessProbe::start()?;
    if let Some(profiler) = &profiler {
        profiler.reset();
    }
    let mut samples = Vec::with_capacity(options.ticks as usize);
    for _ in 0..options.ticks {
        let started = Instant::now();
        engine.tick(1)?;
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        process.observe()?;
    }
    let process_report = process.finish(options.ticks)?;
    let collected = verify(
        &engine,
        &links,
        total_ticks,
        total_ticks,
        generation,
        options.mode,
    )?;
    let sum: f64 = samples.iter().sum();
    samples.sort_by(f64::total_cmp);
    let quantile = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    Ok(json!({
        "tool":"kkscreeps mod-perf-probe", "mode":options.mode.name(),
        "rooms":options.rooms, "links_per_room":options.links_per_room,
        "warmup":options.warmup, "ticks":options.ticks, "metadata":metadata(),
        "package_digests":package_digests, "vm_limits":limits_report(&limits),
        "scope":"EmbedEngine.tick; no player execution or projection",
        "fixture":"synthetic positive cooldown every tick; four audited chain positions",
        "profile": profiler.as_ref().map(ChainProfiler::snapshot),
        "wall":{"count":samples.len(),"p50_ms":quantile(0.5),"p95_ms":quantile(0.95),
            "p99_ms":quantile(0.99),"max_ms":samples.last(),"sum_ms":sum},
        "process":process_report,
        "checks":{"before":before,"after_warmup":warmed,"after_collection":collected},
    }))
}

fn verify(
    engine: &EmbedEngine,
    links: &[(String, Value)],
    initial_cooldown: u32,
    elapsed: u32,
    generation: u64,
    mode: Mode,
) -> anyhow::Result<Value> {
    anyhow::ensure!(
        engine.get_game_time() == u64::from(elapsed),
        "世界 tick 不符"
    );
    anyhow::ensure!(
        engine.engine_generation() == generation + u64::from(elapsed),
        "世界代际不符"
    );
    let mut snapshots = Vec::with_capacity(links.len());
    for (id, initial) in links {
        let actual = engine
            .get_object(id)
            .ok_or_else(|| anyhow::anyhow!("对象 {id} 缺失"))?;
        let mut expected = initial.clone();
        expected["cooldown"] = json!(initial_cooldown - elapsed);
        anyhow::ensure!(actual == expected, "对象 {id} 状态不符：{actual}");
        snapshots.push(actual);
    }
    let calls = u64::from(elapsed) * links.len() as u64;
    let expected_state = if mode == Mode::Chain && elapsed > 0 {
        json!({"before":calls,"outer":calls,"inner":calls,"after":calls})
    } else {
        Value::Null
    };
    let state = engine.engine_mod_state("perf");
    anyhow::ensure!(state == expected_state, "处理链调用次数不符：{state}");
    Ok(
        json!({"tick":elapsed,"generation":engine.engine_generation(),"verified_links":links.len(),
        "link_state_sha256":digest(&serde_json::to_vec(&snapshots)?), "namespace":state}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dense_modes_have_same_link_states_and_exact_chain_counts() -> anyhow::Result<()> {
        let mut reference = None;
        for (mode, profile) in [
            (Mode::Native, false),
            (Mode::Empty, false),
            (Mode::Chain, false),
            (Mode::Chain, true),
        ] {
            let report = measure(&Options {
                profile,
                mode,
                rooms: 2,
                links_per_room: 3,
                warmup: 2,
                ticks: 3,
                fixtures: format!(
                    "{}/../../compat/fixtures/mod-perf",
                    env!("CARGO_MANIFEST_DIR")
                ),
                report: String::new(),
            })?;
            let hash = report["checks"]["after_collection"]["link_state_sha256"].clone();
            if let Some(expected) = &reference {
                assert_eq!(&hash, expected);
            } else {
                reference = Some(hash);
            }
            if mode == Mode::Chain {
                assert_eq!(
                    report["checks"]["after_collection"]["namespace"]["outer"],
                    30
                );
            }
            if profile {
                let samples = &report["profile"];
                // 预热不进入计数；每槽六读六写和一次 native，开启诊断仍等价。
                for phase in [
                    "slot",
                    "js_call",
                    "budget_start",
                    "budget_finish",
                    "op_native",
                    "host_native",
                ] {
                    assert_eq!(samples[phase]["calls"], 18, "{phase}");
                }
                for phase in ["op_read", "host_read", "op_write", "host_write"] {
                    assert_eq!(samples[phase]["calls"], 108, "{phase}");
                    assert_eq!(samples[phase]["errors"], 0, "{phase}");
                }
            } else {
                assert!(report["profile"].is_null());
            }
            assert_eq!(report["wall"]["count"], 3);
        }
        Ok(())
    }
}
