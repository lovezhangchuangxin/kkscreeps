//! 玩家投影、Mod 玩家 API、原生动作和统一结算的连续负载采集。
//! 每轮检查全部玩家与对象；进程窗口包含这些检查，阶段计时不包含。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, ensure};
use kk_mod_host::{ModVmLimits, RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{metadata, process::ProcessProbe};

pub(crate) struct Options {
    pub players: u32,
    pub concurrency: usize,
    pub warmup: u32,
    pub ticks: u32,
    pub fixtures: String,
    pub kkpv: bool,
    pub report: String,
}

struct Player {
    handle: String,
    room: String,
    counter: String,
    creep: String,
    controller: String,
    source: String,
    source_energy: i64,
    source_regeneration: Option<u64>,
    upgrades: u32,
    harvests: u32,
    energy: i64,
}

pub(crate) fn run(options: Options) -> i32 {
    let result = measure(&options).and_then(|report| {
        std::fs::write(&options.report, serde_json::to_string_pretty(&report)?)?;
        Ok(())
    });
    match result {
        Ok(()) => {
            println!("mod-player-probe：{}", options.report);
            0
        }
        Err(error) => {
            eprintln!("mod-player-probe 失败: {error:#}");
            1
        }
    }
}

/// 满员阶梯的网格房名：第 i 个玩家的房 = W{i%20+1}N{i/20+1}（RoomName
/// 约定 W/N 取负减一，最大坐标 |xx|=21、|yy|=17，远离 ±127 世界边界；
/// i∈0..320 单射）。
fn room_name(i: usize) -> String {
    format!("W{}N{}", i % 20 + 1, i / 20 + 1)
}

fn player_code(room: &str) -> String {
    format!(
        r#"(function(){{
      const room=Game.rooms['{room}'];
      const counter=room.find(FIND_COUNTERS)[0];
      if (!(counter instanceof Counter) || room.lookForAt(LOOK_COUNTERS,10,12)[0]!==counter)
        throw Error('Mod class/query mismatch');
      globalThis.rounds=(globalThis.rounds||0)+1;
      const previous=Memory.rounds||0;
      Memory.rounds=globalThis.rounds;
      const creep=Object.values(Game.creeps)[0];
      const action=creep.store.energy>0?'upgrade':'harvest';
      const native=action==='upgrade'?creep.upgradeController(room.controller)
        :creep.harvest(room.find(FIND_SOURCES)[0]);
      return {{round:globalThis.rounds,previous,counter:counter.value,
        increment:counter.increment(),action,native}};
    }})()"#
    )
}

fn snapshot(engine: &EmbedEngine, players: &[Player]) -> Value {
    json!({
        "time":engine.get_game_time(), "generation":engine.engine_generation(),
        "mod_state":engine.engine_mod_state("workload"),
        "players":players.iter().map(|p| json!({
            "handle":p.handle,"room":p.room,"upgrades":p.upgrades,"harvests":p.harvests,
            "counter":engine.get_object(&p.counter),"creep":engine.get_object(&p.creep),
            "controller":engine.find_in_room(&p.room,kk_core::constants::FIND_STRUCTURES)
                .into_iter().find(|o|o["structureType"]=="controller"),
            "sources":engine.find_in_room(&p.room,kk_core::constants::FIND_SOURCES),
        })).collect::<Vec<_>>(),
    })
}

fn step(
    engine: &mut EmbedEngine,
    codes: &BTreeMap<String, String>,
    players: &mut [Player],
    round: u32,
    initial_time: u64,
    initial_generation: u64,
) -> anyhow::Result<()> {
    let runs = engine
        .run_players(codes)
        .with_context(|| format!("第 {round} 轮"))?;
    ensure!(runs.len() == players.len(), "第 {round} 轮玩家回执缺失");
    for player in players.iter_mut() {
        let run = &runs[&player.handle];
        ensure!(
            run.executed && run.expr["ok"] == true,
            "{} 第 {round} 轮未执行或异常: {}",
            player.handle,
            run.expr
        );
        ensure!(
            run.console.is_empty(),
            "{} 第 {round} 轮出现 console: {:?}",
            player.handle,
            run.console
        );
        let value = &run.expr["value"];
        let action = if player.energy > 0 {
            "upgrade"
        } else {
            "harvest"
        };
        ensure!(
            *value
                == json!({"round":round,"previous":round-1,"counter":round-1,
            "increment":0,"action":action,"native":0}),
            "{} 第 {round} 轮玩家状态/动作不符: {value}",
            player.handle
        );
        if action == "upgrade" {
            player.upgrades += 1;
            player.energy -= 1;
        } else {
            player.harvests += 1;
            player.energy += 2;
        }
        ensure!(
            engine.get_object(&player.counter).context("counter 消失")?["fields"]["value"] == round,
            "{} 第 {round} 轮 Mod 动作未提交",
            player.handle
        );
        let creep = engine.get_object(&player.creep).context("creep 消失")?;
        // 管理 JSON 的空仓不保留 energy 键；玩家 Store API 仍返回 0。
        let actual_energy = match creep["store"].get("energy") {
            None => 0,
            Some(value) => value.as_i64().context("creep energy 非整数")?,
        };
        ensure!(
            actual_energy == player.energy,
            "{} 第 {round} 轮原生动作未提交",
            player.handle
        );
        // 固定 RCL 4、单 WORK，窗口最多 100000 轮，低于升至 RCL 5 所需进度。
        // 能量减少也可能来自 drop 等动作，必须同时验证升级目标的实际效果。
        let controller = engine
            .get_object(&player.controller)
            .context("controller 消失")?;
        ensure!(
            controller["level"] == 4 && controller["progress"] == player.upgrades,
            "{} 第 {round} 轮控制器升级效果不符",
            player.handle
        );
        if action == "harvest" {
            player.source_energy -= 2;
        }
        // 指令先采集，source tick 再按 ENERGY_REGEN_TIME 的到期轮补满。
        // 不能只验证 worker 收到能量，遗漏 source 消耗或再生错误。
        let time = initial_time + u64::from(round) - 1;
        if player.source_energy < 3000 {
            let deadline = *player
                .source_regeneration
                .get_or_insert(time + kk_core::constants::ENERGY_REGEN_TIME as u64);
            if time >= deadline - 1 {
                player.source_energy = 3000;
                player.source_regeneration = None;
            }
        }
        let source = engine.get_object(&player.source).context("source 消失")?;
        ensure!(
            source["energy"] == player.source_energy
                && source["energyCapacity"] == 3000
                && source["ticksToRegeneration"]
                    == json!(
                        player
                            .source_regeneration
                            .map(|t| t.saturating_sub(time + 1))
                    ),
            "{} 第 {round} 轮 source 消耗/再生效果不符",
            player.handle
        );
        let state = engine
            .player_state_for_test(&player.handle)
            .context("玩家状态缺失")?;
        ensure!(
            serde_json::from_str::<Value>(&state.raw_memory)? == json!({"rounds":round}),
            "{} 第 {round} 轮 Memory 未提交",
            player.handle
        );
    }
    ensure!(
        engine.get_game_time() == initial_time + u64::from(round)
            && engine.engine_generation() == initial_generation + u64::from(round),
        "世界时钟/代际不符"
    );
    ensure!(
        engine.engine_mod_state("workload")
            == json!({"increments":u64::from(round)*players.len() as u64}),
        "第 {round} 轮 Mod namespace 计数不符"
    );
    ensure!(
        !engine.has_pending_round() && !engine.needs_mod_recovery(),
        "成功轮留有待恢复状态"
    );
    Ok(())
}

fn measure(options: &Options) -> anyhow::Result<Value> {
    ensure!(
        (1..=320).contains(&options.players),
        "--players 须为 1..320"
    );
    ensure!(
        (1..=32).contains(&options.concurrency),
        "--concurrency 须为 1..32"
    );
    ensure!(
        (1..=50_000).contains(&options.ticks),
        "--ticks 须为 1..50000"
    );
    ensure!(options.warmup <= 50_000, "--warmup 须为 0..50000");
    let total = options.warmup + options.ticks;
    // 里程碑 RSS：把进程驻留拆成「引擎+世界+N 常驻 isolate/线程」「对象
    // 放置」「热身执行后的稳态」三层，配合窗口曲线即可归因每玩家边际
    // 足迹（满员容量评估的输入；绝对值受平台分配器影响，跨平台只比
    // 边际）。
    let rss_baseline = super::process::resident_bytes()?;
    let packages = discover_packages(Path::new(&options.fixtures))?;
    ensure!(
        packages.len() == 1 && packages.contains_key("workload"),
        "夹具须仅包含 workload 包"
    );
    let package_digests: BTreeMap<_, _> = packages
        .iter()
        .map(|(id, package)| {
            let bytes = serde_json::to_vec(
                &package
                    .files
                    .iter()
                    .map(|(path, bytes)| (path.to_string_lossy(), bytes))
                    .collect::<Vec<_>>(),
            )?;
            Ok((id.clone(), format!("{:x}", Sha256::digest(bytes))))
        })
        .collect::<anyhow::Result<_>>()?;
    // 房间按 20 列网格分布：线性 W{n}N1 在大玩家数下越过世界边界（±127）。
    // 公式支持到 N16 行（约 320 玩家；列固定 20，行到 127 需把上限提到 2540
    // 并重估列宽）。
    let room_name = |i: usize| room_name(i);
    let handles: Vec<_> = (1..=options.players).map(|i| format!("p{i}")).collect();
    let rooms: Vec<_> = handles
        .iter()
        .enumerate()
        .map(|(i, h)| json!({"name":room_name(i),"rcl":4,"owner":h}))
        .collect();
    let mut engine = EmbedEngine::new();
    engine.set_kkpv_channel(options.kkpv);
    engine.set_player_concurrency(options.concurrency)?;
    let vm_limits = ModVmLimits::default();
    let player_budget = kk_runner::SandboxConfig::default();
    engine.create_shard_with_mods(
        &json!({"players":handles,"rooms":rooms}),
        &packages,
        RegisterOptions {
            vm_limits: vm_limits.clone(),
            ..Default::default()
        },
    )?;
    let rss_after_shard = super::process::resident_bytes()?;
    let mut players = Vec::new();
    let mut codes = BTreeMap::new();
    for (i, handle) in handles.into_iter().enumerate() {
        let room = room_name(i);
        let (x, y) = engine
            .get_controller_pos(&room)
            .context("controller 缺失")?;
        ensure!(
            x.abs_diff(2).max(y.abs_diff(1)) <= 3,
            "controller 超出升级距离"
        );
        let source = engine.place_object(
            &room,
            "structure",
            &json!({"pos":[2,2],"structureType":"source",
            "energy":3000,"energyCapacity":3000}),
        )?;
        let controller = engine
            .find_in_room(&room, kk_core::constants::FIND_STRUCTURES)
            .into_iter()
            .find(|o| o["structureType"] == "controller")
            .context("controller 缺失")?;
        ensure!(
            controller["level"] == 4 && controller["progress"] == 0,
            "初始控制器状态不符"
        );
        let controller = controller["id"]
            .as_str()
            .context("controller ID 缺失")?
            .to_string();
        let creep = engine.place_object(
            &room,
            "creep",
            &json!({"pos":[2,1],"owner":handle,
            "name":format!("worker{i}"),"body":["work","carry","move"],"store":{"energy":50},
            // 合成负载延长寿命，整个有限窗口保持同一 worker；不模拟孵化与死亡。
                "ticksToLive":total+1}),
        )?;
        ensure!(
            engine.get_object(&creep).context("初始 creep 缺失")?["ticksToLive"] == total + 1,
            "合成 worker 寿命未生效"
        );
        let counter = engine.place_mod_object(
            &room,
            "workload:counter",
            &json!({"owner":handle,"x":10,"y":12}),
        )?;
        codes.insert(handle.clone(), player_code(&room));
        players.push(Player {
            handle,
            room,
            counter,
            creep,
            controller,
            source,
            source_energy: 3000,
            source_regeneration: None,
            upgrades: 0,
            harvests: 0,
            energy: 50,
        });
    }
    let initial_time = engine.get_game_time();
    let initial_generation = engine.engine_generation();
    // 里程碑先采样再 snapshot：snapshot 的 serde_json 分配会驻留到函数结束，
    // 先拍快照会把放置里程碑抬高、把热身增长压低（与热身侧顺序一致）。
    let rss_after_placement = super::process::resident_bytes()?;
    let before = snapshot(&engine, &players);
    for round in 1..=options.warmup {
        step(
            &mut engine,
            &codes,
            &mut players,
            round,
            initial_time,
            initial_generation,
        )?;
    }
    let rss_after_warmup = super::process::resident_bytes()?;
    let warmed = snapshot(&engine, &players);
    let actions_before: Vec<_> = players.iter().map(|p| (p.upgrades, p.harvests)).collect();
    engine.perf_reset();
    let mut process = ProcessProbe::start()?;
    for round in options.warmup + 1..=total {
        step(
            &mut engine,
            &codes,
            &mut players,
            round,
            initial_time,
            initial_generation,
        )?;
        process.observe()?;
    }
    let process = process.finish(options.ticks)?;
    let phases = engine.perf_report();
    ensure!(phases["ticks"] == options.ticks, "阶段样本数不符");
    let after = snapshot(&engine, &players);
    let action_counts: Vec<_> = players
        .iter()
        .zip(actions_before)
        .map(|(p, (upgrades, harvests))| {
            json!({"player":p.handle,"executions":options.ticks,"mod_increments":options.ticks,
            "upgrades":p.upgrades-upgrades,"harvests":p.harvests-harvests})
        })
        .collect();
    Ok(json!({
        "tool":"kkscreeps mod-player-probe", "schema_version":2,
        "players":options.players,"player_concurrency":options.concurrency,
        "warmup":options.warmup,"ticks":options.ticks,
        "channel":if options.kkpv {"kkpv"} else {"json"},
        "scope":"run_players with projection, player API, native and Mod intents, unified commit",
        "fixture":"one long-lived worker, source and Mod counter per owned room; harvest/upgrade cycle",
        "process_scope":"all process threads; includes per-round validation and RSS sampling",
        "player_cpu_clock":"thread_cpu_time", "metadata":metadata(),
        "package_digests":package_digests,
        "vm_limits":{"registration_cpu_ms":vm_limits.registration.cpu.as_millis(),
            "registration_wall_ms":vm_limits.registration.wall.as_millis(),
            "settlement_cpu_ms":vm_limits.settlement.cpu.as_millis(),
            "settlement_wall_ms":vm_limits.settlement.wall.as_millis(),"memory_bytes":vm_limits.memory_bytes},
        "player_budget":{"limit":player_budget.cpu_limit,"bucket_cap":player_budget.bucket_cap,
            "initial_bucket":player_budget.initial_bucket,"max_per_tick":player_budget.max_per_tick},
        "player_code_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&codes)?)),
        "memory_milestones": {
            "rss_bytes_baseline": rss_baseline,
            "rss_bytes_after_shard": rss_after_shard,
            "rss_bytes_after_placement": rss_after_placement,
            "rss_bytes_after_warmup": rss_after_warmup,
            // after_shard 已含引擎基座+世界+N 玩家线程/isolate；除法把固定
            // 成本摊进每玩家值，低 N 时显著虚高（2 玩家 ≈22MB vs 300 玩家
            // ≈8.6MB）——权威每玩家边际用跨规模差分，别直接引用本字段。
            // 减法用 saturating_sub：Linux 分配器归还页面时热身期 RSS 可能
            // 净下降，回绕会污染报告。
            "rss_bytes_per_player_after_shard": rss_after_shard.saturating_sub(rss_baseline)
                / u64::from(options.players),
            "rss_bytes_growth_warmup": rss_after_warmup.saturating_sub(rss_after_placement),
        },
        "process":process,"phases":phases,
        "workload":{"before":before,"warmed":warmed,"after":after,"window_actions":action_counts},
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 网格房名在满量程内唯一且可解析为世界边界内的坐标（±127）。
    #[test]
    fn grid_room_names_stay_unique_and_inside_world_bounds() {
        let mut seen = std::collections::BTreeSet::new();
        for i in 0..320 {
            let name = room_name(i);
            assert!(seen.insert(name.clone()), "房名重复：{name}");
            let parsed = kk_core::RoomName::parse(&name).expect("房名可解析");
            assert!(
                parsed.xx.abs() <= 127 && parsed.yy.abs() <= 127,
                "{name} 越界：({}, {})",
                parsed.xx,
                parsed.yy
            );
        }
    }

    fn options() -> Options {
        Options {
            players: 2,
            concurrency: 2,
            warmup: 3,
            ticks: 60,
            fixtures: format!(
                "{}/../../compat/fixtures/mod-player",
                env!("CARGO_MANIFEST_DIR")
            ),
            kkpv: false,
            report: String::new(),
        }
    }

    #[test]
    fn channels_and_concurrency_commit_every_players_native_and_mod_actions() -> anyhow::Result<()>
    {
        let mut reference = None;
        for binary in [false, true] {
            for concurrency in [1, 2] {
                let mut opts = options();
                opts.kkpv = binary;
                opts.concurrency = concurrency;
                let report = measure(&opts)?;
                assert_eq!(report["phases"]["ticks"], 60);
                for action in report["workload"]["window_actions"].as_array().unwrap() {
                    assert_eq!(action["mod_increments"], 60);
                    assert!(action["upgrades"].as_u64().unwrap() > 0);
                    assert!(action["harvests"].as_u64().unwrap() > 0);
                }
                if let Some(reference) = &reference {
                    assert_eq!(&report["workload"], reference);
                } else {
                    reference = Some(report["workload"].clone());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn worker_survives_a_window_beyond_default_creep_lifetime() -> anyhow::Result<()> {
        let mut opts = options();
        opts.warmup = 1500;
        opts.ticks = 3;
        let report = measure(&opts)?;
        // 世界时间含 createShard 热身结算拍：1503 探测轮 + 1。
        assert_eq!(report["workload"]["after"]["time"], 1504);
        for player in report["workload"]["after"]["players"].as_array().unwrap() {
            assert_eq!(player["creep"]["ticksToLive"], 1);
            // 计数器只在玩家结算轮递增：热身拍先于对象放置，不计数。
            assert_eq!(player["counter"]["fields"]["value"], 1503);
        }
        Ok(())
    }

    #[test]
    fn no_op_intent_and_player_exception_cannot_produce_a_success_report() -> anyhow::Result<()> {
        let mut opts = options();
        let root = std::env::temp_dir().join(format!("kk-mod-player-noop-{}", std::process::id()));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        std::fs::create_dir(&root)?;
        let cleanup = Cleanup(root);
        let package = cleanup.0.join("workload");
        std::fs::create_dir(&package)?;
        let originals: BTreeMap<_, _> = ["mod.json", "player.js", "server.js"]
            .into_iter()
            .map(|file| {
                Ok((
                    file,
                    std::fs::read_to_string(Path::new(&opts.fixtures).join("workload").join(file))?,
                ))
            })
            .collect::<anyhow::Result<_>>()?;
        opts.fixtures = cleanup.0.to_string_lossy().into_owned();
        // needle 必须与 compat/fixtures/mod-player/workload 的当前文本逐字一致：
        // 该夹具经 oxfmt 统一格式（对象字面量规范成 `{ amount: 1 }`），
        // 写成压缩形式会让替换静默失效、故障注不进去，测试反而以"报告成功"失败。
        for (needle, replacement, expected) in [
            (
                "ctx.target.fields.value += ctx.args.amount;",
                "",
                "Mod 动作未提交",
            ),
            (
                "return api.intents.emit('workload:increment', this, { amount: 1 });",
                "throw Error('player-fault');",
                "player-fault",
            ),
            (
                "export default function install(api) {",
                "export default function install(api) { api.builtins.Creep.prototype.upgradeController=function(){return this.drop('energy',1);};",
                "控制器升级效果不符",
            ),
        ] {
            for (file, text) in &originals {
                std::fs::write(package.join(file), text.replace(needle, replacement))?;
            }
            let error = measure(&opts).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
        Ok(())
    }
}
