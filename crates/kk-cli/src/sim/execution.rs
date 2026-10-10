//! 装配模拟输入并编排逐轮执行、提交后保存与最终报告。
use super::{
    Options, SIM_REPORT_VERSION,
    assertions::evaluate,
    auto_checkpoint, checkpoint, mods,
    observation::observe,
    replay,
    scene::{Scene, validate},
};
use crate::support::{digest, executable_digest};
use anyhow::Context;
use kk_runner::{ModuleKind, ModuleTable};
use kk_runtime::EmbedEngine;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Default, Serialize)]
struct PlayerStats {
    successful_ticks: u32,
    error_ticks: u32,
    not_evaluated_ticks: u32,
    errors_head: Vec<Value>,
    console_head: Vec<Value>,
}

fn modules_metadata(root: &Path, table: &ModuleTable) -> Value {
    let mut names: Vec<_> = table.names().collect();
    names.sort();
    let files: Vec<_> = names
        .into_iter()
        .map(|name| {
            let (kind, bytes) = match table.kind(name).unwrap() {
                ModuleKind::Js => ("js", table.load_js(name).unwrap().as_bytes()),
                ModuleKind::Binary => ("binary", table.load_binary(name).unwrap()),
            };
            json!({"name": name, "kind": kind, "bytes": bytes.len(), "sha256": digest(bytes)})
        })
        .collect();
    json!({"directory": root, "sha256": digest(&serde_json::to_vec(&files).unwrap()), "files": files})
}

/// 应用一次场景声明的包切换并构造报告记录；装载或切换失败都记
/// `rejected` 且不中断运行（引擎语义：失败保留旧版继续结算）。
fn apply_mods_switch(
    engine: &mut EmbedEngine,
    base: &Path,
    mods: &mods::Mods,
    switch: &mods::ModsSwitch,
) -> Value {
    let outcome = mods::load(base, &switch.to_mods(mods.root.clone()))
        .and_then(|switched| engine.switch_mod_packages(&switched.packages, &switched.options));
    match outcome {
        Ok(mut summary) => {
            summary["after_tick"] = json!(switch.after_tick());
            summary["status"] = json!("applied");
            summary
        }
        Err(error) => json!({
            "after_tick": switch.after_tick(),
            "status": "rejected",
            "error": format!("{error:#}"),
        }),
    }
}

/// 周期保存的运行内统计与到点保存/保留策略/报告字段见 auto_checkpoint
/// 模块（G26.a）；本文件只保留编排调用点。
pub(super) fn execute(options: &Options) -> anyhow::Result<Value> {
    anyhow::ensure!(options.ticks > 0, "ticks 必须大于 0");
    anyhow::ensure!(
        (1..=32).contains(&options.concurrency),
        "concurrency 必须为 1..=32"
    );
    if options.verify_replay {
        anyhow::ensure!(
            options.input_log.is_some(),
            "--verify-replay 需同时启用 --input-log（回放以日志段为输入）"
        );
    }
    let path = std::fs::canonicalize(&options.scenario).context("读取场景路径")?;
    let bytes = std::fs::read(&path).context("读取场景")?;
    let scene: Scene = serde_json::from_slice(&bytes).context("解析场景 JSON")?;
    validate(&scene)?;
    // 切换改变执行层身份：切换后段与恢复引擎的身份核对必然不符，
    // 逐段身份重装编排属后续切片——显式拒绝而非静默跳过核对。
    if options.verify_replay && scene.mods_switch.is_some() {
        anyhow::bail!(
            "--verify-replay 与 mods_switch 暂不兼容（切换场景的逐段身份重装属后续切片）"
        );
    }
    // 周期保存（G26.a）：间隔与目录同进同出；切换会摘除运行期日志，
    // 切换后保存的检查点没有日志覆盖、恢复配对必败——组合显式拒绝。
    if let Some(every) = options.checkpoint_every {
        anyhow::ensure!(
            options.checkpoint_dir.is_some(),
            "--checkpoint-every 需同时指定 --checkpoint-dir"
        );
        anyhow::ensure!(
            scene.mods_switch.is_none(),
            "--checkpoint-every 与 mods_switch 暂不兼容（切换摘除运行期日志，之后的检查点无日志覆盖；组合恢复属后续切片）"
        );
        let _ = every;
    }
    anyhow::ensure!(
        options.checkpoint_dir.is_none() || options.checkpoint_every.is_some(),
        "--checkpoint-dir 需与 --checkpoint-every 同用"
    );
    anyhow::ensure!(
        options.checkpoint_keep.is_none() || options.checkpoint_every.is_some(),
        "--checkpoint-keep 需与 --checkpoint-every 同用"
    );
    // 目录形态的 --load-checkpoint 走「最新可用」恢复：必须带日志。
    if options
        .load_checkpoint
        .as_ref()
        .is_some_and(|path| path.is_dir())
    {
        anyhow::ensure!(
            options.input_log.is_some(),
            "--load-checkpoint 指向目录时需同时启用 --input-log（恢复按日志段配对重放）"
        );
    }
    if let Some(dir) = &options.checkpoint_dir {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建周期检查点目录 {}", dir.display()))?;
    }
    let base = path.parent().unwrap();
    let mut tables = BTreeMap::new();
    let mut modules = BTreeMap::new();
    for bot in &scene.bots {
        let root = std::fs::canonicalize(base.join(&bot.modules))
            .with_context(|| format!("玩家 {} 的模块目录", bot.player))?;
        let table = ModuleTable::from_directory(&root)
            .with_context(|| format!("加载玩家 {} 的模块", bot.player))?;
        anyhow::ensure!(
            table.kind(&bot.entry) == Some(ModuleKind::Js),
            "玩家 {} 缺少 JS 入口模块 {}",
            bot.player,
            bot.entry
        );
        modules.insert(bot.player.clone(), modules_metadata(&root, &table));
        tables.insert(bot.player.clone(), table);
    }
    let executable_hash = executable_digest()?;
    // 场景种子进入世界：随机流域与确定性时间的派生根（G22.b）。缺省 0
    // 与既有场景行为一致；检查点续跑时种子来自载荷，此处值只影响新世界。
    let mut engine = EmbedEngine::with_world_seed(scene.seed);
    engine.set_player_concurrency(options.concurrency)?;
    engine.set_kkpv_channel(options.kkpv);
    if let Some(bytes) = scene.player_heap_limit_bytes {
        engine.set_player_heap_limit(
            usize::try_from(bytes).context("player_heap_limit_bytes 溢出")?,
        )?;
    }
    if options.mod_trace {
        engine.enable_mod_trace();
    }
    let shard = serde_json::to_value(&scene.shard)?;
    let loaded = scene
        .mods
        .as_ref()
        .map(|input| mods::load(base, input))
        .transpose()?;
    let mods_metadata = loaded
        .as_ref()
        .map(|loaded| loaded.metadata.clone())
        .unwrap_or_else(|| json!({"packages": {}}));
    // 续跑按检查点显式推进轮数与**实际生效的切换**选择恢复用包集合：被
    // 拒绝的切换不改变身份，越过边界（player_ticks >= after_tick）不一
    // 定携带切换后身份。载荷只读一次，resume 复用同一 Value。
    // 目录形态=「最新可用」恢复（G26.a）：选出的检查点恢复世界，日志在
    // 其游标处截断（废弃分支放弃），游标之后的丢失拍并入本轮**重执行**
    // ——重执行轮与普通轮同路径记段，不消费废弃分支做结算重放。
    let mut recovery: Option<checkpoint::Recovery> = None;
    let loaded_value = match &options.load_checkpoint {
        Some(path) if path.is_dir() => {
            let log = options.input_log.as_ref().expect("校验保证日志在");
            let found = checkpoint::recover_latest(path, log)?;
            let value = found.value.clone();
            recovery = Some(found);
            Some(value)
        }
        Some(path) => Some(kk_runtime::checkpoint::read_checkpoint(path)?),
        None => None,
    };
    let mut applied_switches = loaded_value
        .as_ref()
        .map(checkpoint::applied_switches)
        .transpose()?
        .unwrap_or_default();
    let resume_switched = match (&loaded_value, &scene.mods_switch) {
        (Some(value), Some(switch)) => {
            let rounds = value["engine"]["world"]["player_ticks"]
                .as_u64()
                .context("检查点载荷缺少显式推进轮数")?;
            (rounds >= switch.after_tick() && applied_switches.contains(&switch.after_tick()))
                .then(|| {
                    mods::load(
                        base,
                        &switch
                            .to_mods(scene.mods.as_ref().expect("校验保证 mods 在").root.clone()),
                    )
                })
                .transpose()?
        }
        _ => None,
    };
    if let Some(output) = &options.save_checkpoint {
        let bot_roots = scene.bots.iter().map(|bot| base.join(&bot.modules));
        let mod_roots = loaded
            .iter()
            .flat_map(|loaded| loaded.packages.values().map(|package| package.root.clone()));
        checkpoint::protect_sources(output, bot_roots.chain(mod_roots))?;
    }
    if options.load_checkpoint.is_none() {
        if let Some(loaded) = &loaded {
            engine.create_shard_with_mods(&shard, &loaded.packages, loaded.options.clone())?;
        } else {
            engine.create_shard(&shard)?;
        }
    }
    let mod_names: Vec<String> = mods_metadata["packages"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut aliases = BTreeMap::new();
    if options.load_checkpoint.is_none() {
        let input: Vec<_> = scene
            .objects
            .iter()
            .map(|object| (object.room.as_str(), object.kind.as_str(), &object.spec))
            .collect();
        let ids = engine.place_objects(&input).context("批量放置场景对象")?;
        aliases.extend(
            scene
                .objects
                .iter()
                .zip(ids)
                .map(|(object, id)| (object.alias.clone(), id)),
        );
    }
    let mut codes = BTreeMap::new();
    // 目录恢复的丢失拍（语义见下方恢复块注释）：重执行并入本轮执行数。
    let reexecute_ticks = recovery.as_ref().map_or(0, |found| found.reexecute_ticks);
    let mut stats = BTreeMap::new();
    for bot in &scene.bots {
        if options.load_checkpoint.is_none() {
            engine.set_player_modules(&bot.player, tables.remove(&bot.player).unwrap())?;
            engine.set_player_memory(&bot.player, &bot.memory)?;
        }
        let entry = serde_json::to_string(&bot.entry)?;
        // 不吞掉缺少 loop 的错误；loop 的返回值不属于游戏提交状态。
        codes.insert(bot.player.clone(), format!("(() => {{ const bot = require({entry}); if (!bot || typeof bot.loop !== 'function') throw new Error('bot entry must export loop'); bot.loop(); return true; }})()"));
        stats.insert(bot.player.clone(), PlayerStats::default());
    }
    // 场景声明的堆上限对续跑是硬约束：工作者身份随检查点恢复，场景值
    // 无法改写恢复中的工作者；与任一恢复工作者不一致时启动前拒绝，
    // 不允许配置被静默忽略。
    if let (Some(expected), Some(value)) = (scene.player_heap_limit_bytes, &loaded_value) {
        for (name, worker) in value["workers"].as_object().into_iter().flatten() {
            anyhow::ensure!(
                worker["heap_limit_bytes"].as_u64() == Some(expected),
                "场景 player_heap_limit_bytes={expected} 与检查点工作者 {name} 的堆上限不一致"
            );
        }
    }
    let initial = if let Some(value) = loaded_value {
        let (saved_aliases, initial) = checkpoint::resume(
            &mut engine,
            value,
            resume_switched.as_ref().or(loaded.as_ref()),
            &digest(&bytes),
            &executable_hash,
            &modules,
        )?;
        aliases = saved_aliases;
        engine.set_player_concurrency(options.concurrency)?;
        engine.set_kkpv_channel(options.kkpv);
        // 目录恢复：检查点回滚与游标处截断已由 recover_latest 完成；
        // 废弃分支覆盖的丢失拍并入本轮**重执行**——候选提交原子性保证无
        // 半拍状态，世界与 Memory 恢复到同一时间点（见 checkpoint.rs 恢复
        // 语义说明）。此后新轮次照常记日志并续接段号。
        initial
    } else {
        observe(&engine, &scene, &aliases, &mod_names)
    };
    // 输入日志在分片就绪后、首轮执行前挂载；文件已存在时先整读校验
    // （撕裂/断链拒绝），续接段从此处继续。元数据只作诊断，不入链。
    if let Some(path) = &options.input_log {
        engine.enable_input_log(
            path,
            json!({
                "scenario_sha256": digest(&bytes),
                "executable_sha256": executable_hash,
                "ticks_requested": options.ticks,
            }),
        )?;
    }
    // 回放起点快照（G25.b）：内存 Value，不落盘；此刻无开放轮次且未跑
    // 任何轮，日志段将从同一起点完整覆盖本次运行。
    let replay_start = options
        .verify_replay
        .then(|| {
            engine.checkpoint(
                json!({"kind": "verify-replay-start", "log_cursor": engine.input_log_cursor()}),
            )
        })
        .transpose()?;
    let start_time = engine.get_game_time();
    let start = std::time::Instant::now();
    let mut failure = None;
    let mut attempted = 0;
    let mut switches: Vec<Value> = Vec::new();
    let mut auto = auto_checkpoint::Stats::default();
    // 目录恢复的丢失拍并入执行数：重执行段与新段同样入日志/检查点节奏。
    let total_rounds = u64::from(options.ticks) + reexecute_ticks;
    for _ in 0..total_rounds {
        let tick = engine.get_game_time();
        attempted += 1;
        let runs = match engine.run_players(&codes) {
            Ok(runs) => runs,
            Err(error) => {
                failure =
                    Some(json!({"stage": "runtime", "tick": tick, "error": format!("{error:#}")}));
                break;
            }
        };
        let mut failed_players = Vec::new();
        for (player, result) in runs {
            let stat = stats.get_mut(&player).unwrap();
            for line in result.console {
                if stat.console_head.len() < 20 {
                    stat.console_head.push(json!({"tick": tick, "text": line}));
                }
            }
            if result.executed && result.expr["ok"] == true {
                stat.successful_ticks += 1;
            } else {
                if !result.executed {
                    stat.not_evaluated_ticks += 1;
                }
                stat.error_ticks += 1;
                if stat.errors_head.len() < 20 {
                    stat.errors_head
                        .push(json!({"tick": tick, "result": result.expr}));
                }
                failed_players.push(player);
            }
        }
        if !failed_players.is_empty() {
            failure = Some(json!({"stage": "player", "tick": tick, "players": failed_players}));
            break;
        }
        // 周期检查点（G26.a）：到点保存+保留策略；失败值携带
        // stage=checkpoint，终结运行（详见 auto_checkpoint 模块）。
        if let Some(broken) = options.checkpoint_every.and_then(|every| {
            auto_checkpoint::save_if_due(
                &auto_checkpoint::Periodic {
                    engine: &engine,
                    every,
                    dir: options.checkpoint_dir.as_ref().expect("校验保证目录在"),
                    keep: options.checkpoint_keep,
                    scenario_sha256: &digest(&bytes),
                    executable_sha256: &executable_hash,
                    modules: &modules,
                    aliases: &aliases,
                    initial: &initial,
                },
                &mut auto,
            )
        }) {
            failure = Some(broken);
            break;
        }
        // 已提交边界应用场景声明的包切换：恰在提交完第 after_tick 轮后。
        // 拒绝不使运行失败——引擎保证旧计划继续结算，rejected 记录入报告。
        if let Some(switch) = &scene.mods_switch
            && engine.committed_rounds() == switch.after_tick()
        {
            switches.push(apply_mods_switch(
                &mut engine,
                base,
                scene.mods.as_ref().expect("校验保证 mods 在"),
                switch,
            ));
        }
    }
    let run_wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    let final_state = observe(&engine, &scene, &aliases, &mod_names);
    let assertions: Vec<_> = scene
        .assertions
        .iter()
        .map(|a| evaluate(a, &initial, &final_state))
        .collect();
    applied_switches.extend(
        switches
            .iter()
            .filter(|record| record["status"] == "applied")
            .filter_map(|record| record["after_tick"].as_u64())
            .filter(|tick| !applied_switches.contains(tick))
            .collect::<Vec<_>>(),
    );
    if failure.is_none()
        && let Some(path) = &options.save_checkpoint
        && let Err(error) = checkpoint::save(
            &engine,
            path,
            &digest(&bytes),
            &executable_hash,
            &modules,
            &aliases,
            &initial,
            &applied_switches,
            // 终态保存也带游标：文件内段数即最后 seq（日志挂载时）。
            engine.input_log_cursor(),
        )
    {
        failure = Some(
            json!({"stage": "checkpoint", "tick": engine.get_game_time(), "error": format!("{error:#}")}),
        );
    }
    let committed = engine.get_game_time() - start_time;
    // 目录恢复的丢失拍以重执行补齐：提交数须等于「请求 + 重执行」。
    let success = failure.is_none()
        && committed == u64::from(options.ticks) + reexecute_ticks
        && assertions.iter().all(|a| a["passed"] == true);
    let status = if failure
        .as_ref()
        .is_some_and(|failure| failure["stage"] == "checkpoint")
    {
        "checkpoint_failed"
    } else if failure.is_some() {
        "run_failed"
    } else if success {
        "passed"
    } else {
        "assertion_failed"
    };
    // v10 字段：周期保存摘要（RPO 声明兑现「报告所选耐久策略」）与目录
    // 恢复记录；装配见 auto_checkpoint 模块，此处只保留挂点。
    let auto_checkpoint_record = options.checkpoint_every.map(|every| {
        auto_checkpoint::record(
            every,
            options.checkpoint_dir.as_deref(),
            options.checkpoint_keep,
            &auto,
        )
    });
    let recovery_record = recovery.as_ref().map(auto_checkpoint::recovery_record);
    let mut report = json!({
        "report_version": SIM_REPORT_VERSION, "success": success, "status": status,
        "engine_version": env!("CARGO_PKG_VERSION"), "executable_sha256": executable_hash,
        "scenario": path, "scenario_sha256": digest(&bytes), "modules": modules,
        "mods": mods_metadata, "mods_switches": switches,
        "plan_hash": engine.contribution_plan_hash().hex(),
        "composition": engine.mod_composition(),
        "execution_strategy": engine.mod_execution_strategy(),
        "disabled_mods": engine.disabled_mods(),
        "channel": if options.kkpv { "kkpv" } else { "json" }, "concurrency": options.concurrency,
        "player_heap_limit_bytes": scene
            .player_heap_limit_bytes
            .unwrap_or(kk_runner::DEFAULT_HEAP_LIMIT as u64),
        "ticks_requested": options.ticks, "ticks_attempted": attempted, "world_ticks_committed": committed,
        "seed": engine.world_seed(), "start_time": start_time, "end_time": engine.get_game_time(),
        "checkpoint_loaded": options.load_checkpoint,
        "checkpoint_saved": if failure.is_none() { options.save_checkpoint.as_ref() } else { None },
        "run_wall_ms": run_wall_ms, "phases": engine.perf_report(),
        "input_log": options.input_log.as_ref().map(|path| replay::input_log_summary(&engine, path)),
        "auto_checkpoint": auto_checkpoint_record, "recovery": recovery_record,
        "failure": failure, "players": stats, "aliases": aliases, "assertions": assertions,
        "mod_fault": engine.last_mod_settlement_fault().map(|fault| json!({
            "tick": fault.tick, "room": fault.room, "target": fault.target, "error": fault.error,
        })),
        // 诊断快照：只含定位信息与结果摘要，不含参数/字段值/玩家
        // Memory，不构成确定性回放日志——复现依赖场景身份与包目录重跑。
        "mod_trace": options.mod_trace.then(|| {
            match engine.mod_trace_snapshot() {
                Some((entries, dropped)) => json!({
                    "entries": entries,
                    "dropped": dropped,
                    "note": "诊断快照，非确定性回放日志；复现=同场景（scenario_sha256）+同包计划（plan_hash）+同二进制（executable_sha256）重跑",
                }),
                None => json!({"entries": [], "dropped": 0,
                    "note": "分片未装配 Mod（无 Mod 计划），无事件可记"}),
            }
        }),
        "initial": initial, "final": final_state,
    });
    // 回放核对放在报告主体之后：restore 会整体替换引擎实例，perf 等
    // 引擎侧字段必须先取完。
    if options.verify_replay {
        report["verify_replay"] = match (&replay_start, &failure, &options.input_log) {
            (Some(start), None, Some(log)) => replay::verify_in_process(
                &mut engine,
                start,
                log,
                loaded
                    .as_ref()
                    .map(|loaded| (&loaded.packages, &loaded.options)),
            ),
            (_, Some(_), _) => json!({"status": "skipped", "reason": "运行失败，未核对"}),
            _ => json!({"status": "skipped", "reason": "起点快照缺失"}),
        };
        // 核对失败即结论失败：显式请求的验证不允许静默假阴性（按
        // status/success/退出码判读的调用方必须看见 replay_failed）。
        if report["verify_replay"]["status"] == "failed" {
            report["status"] = json!("replay_failed");
            report["success"] = json!(false);
            report["error"] = report["verify_replay"]["error"].clone();
        }
    }
    Ok(report)
}
