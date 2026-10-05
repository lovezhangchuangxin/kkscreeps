//! 从版本化场景加载真实 bot，复用 EmbedEngine 执行、结算和提交。
//! 报告读取已提交状态；玩家错误后的该轮可能已提交部分合法意图。

use crate::report::write_report;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Context;
use kk_runner::{ModuleKind, ModuleTable};
use kk_runtime::EmbedEngine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod mods;

pub struct Options {
    pub scenario: PathBuf,
    pub ticks: u32,
    pub concurrency: usize,
    pub kkpv: bool,
    pub report: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    version: u32,
    shard: Shard,
    bots: Vec<Bot>,
    #[serde(default)]
    mods: Option<mods::Mods>,
    #[serde(default)]
    objects: Vec<Object>,
    #[serde(default)]
    assertions: Vec<Assertion>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Shard {
    players: Vec<Value>,
    rooms: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bot {
    player: String,
    modules: PathBuf,
    #[serde(default = "main_entry")]
    entry: String,
    #[serde(default = "empty_memory")]
    memory: Value,
}

fn main_entry() -> String {
    "main".into()
}
fn empty_memory() -> Value {
    json!({})
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    alias: String,
    room: String,
    kind: String,
    spec: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Comparison {
    Eq,
    Gte,
    DeltaGte,
    Exists,
    Missing,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Assertion {
    pointer: String,
    op: Comparison,
    #[serde(default)]
    value: Value,
}

#[derive(Default, Serialize)]
struct PlayerStats {
    successful_ticks: u32,
    error_ticks: u32,
    not_evaluated_ticks: u32,
    errors_head: Vec<Value>,
    console_head: Vec<Value>,
}

/// 0 表示全部轮次及断言成功；2 表示运行或断言失败；1 表示输入或 I/O 错误。
pub fn run(options: Options) -> i32 {
    if options
        .report
        .extension()
        .is_none_or(|extension| extension != "json")
    {
        eprintln!("sim run 失败：报告文件必须使用 .json 后缀");
        return 1;
    }
    // 报告不能覆盖场景输入。该错误不能再向同一路径写失败报告。
    if options.report.exists()
        && options.scenario.exists()
        && std::fs::canonicalize(&options.report).ok()
            == std::fs::canonicalize(&options.scenario).ok()
    {
        eprintln!("sim run 失败：报告路径不能与场景文件相同");
        return 1;
    }
    let (report, code) = match execute(&options) {
        Ok(report) => {
            let code = if report["success"] == true { 0 } else { 2 };
            (report, code)
        }
        Err(error) => (
            json!({
                "report_version": 1, "success": false, "status": "setup_failed",
                "scenario": options.scenario, "ticks_requested": options.ticks,
                "world_ticks_committed": 0, "error": format!("{error:#}"),
            }),
            1,
        ),
    };
    if let Err(error) = write_report(&options.report, &report) {
        eprintln!("sim run 报告写入失败：{error:#}");
        return 1;
    }
    println!(
        "sim run: {}，已提交 {}/{} tick，报告 {}",
        report["status"].as_str().unwrap_or("failed"),
        report["world_ticks_committed"],
        options.ticks,
        options.report.display()
    );
    if code != 0 {
        eprintln!(
            "{}",
            report["error"]
                .as_str()
                .unwrap_or("运行或场景断言未通过，详见报告")
        );
    }
    code
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn executable_digest() -> anyhow::Result<String> {
    let mut file = std::fs::File::open(std::env::current_exe()?)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
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

fn validate(scene: &Scene) -> anyhow::Result<()> {
    anyhow::ensure!(
        scene.version == 1,
        "不支持的场景 version {}（仅支持 1）",
        scene.version
    );
    let mut players = BTreeSet::new();
    for player in &scene.shard.players {
        let name = player
            .as_str()
            .or_else(|| player["name"].as_str())
            .unwrap_or("");
        anyhow::ensure!(
            !name.is_empty() && players.insert(name),
            "玩家名为空或重复：{name}"
        );
    }
    anyhow::ensure!(!players.is_empty(), "场景至少需要一名玩家");
    let mut bots = BTreeSet::new();
    for bot in &scene.bots {
        anyhow::ensure!(
            bots.insert(bot.player.as_str()),
            "重复 bot 玩家：{}",
            bot.player
        );
        anyhow::ensure!(!bot.entry.is_empty(), "bot entry 不能为空");
        anyhow::ensure!(
            bot.memory.is_object(),
            "玩家 {} 的初始 memory 必须是对象",
            bot.player
        );
    }
    anyhow::ensure!(
        bots == players,
        "bots 必须恰好覆盖 shard.players 中的每名玩家"
    );
    let mut aliases = BTreeSet::new();
    for object in &scene.objects {
        anyhow::ensure!(
            !object.alias.is_empty() && aliases.insert(&object.alias),
            "对象 alias 为空或重复：{}",
            object.alias
        );
        anyhow::ensure!(
            object.spec.is_object(),
            "对象 {} 的 spec 必须是对象",
            object.alias
        );
    }
    for assertion in &scene.assertions {
        anyhow::ensure!(
            valid_pointer(&assertion.pointer),
            "非法 JSON Pointer：{}",
            assertion.pointer
        );
        if matches!(assertion.op, Comparison::Gte | Comparison::DeltaGte) {
            anyhow::ensure!(
                assertion.value.as_f64().is_some(),
                "数值断言 {} 的 value 必须是数字",
                assertion.pointer
            );
        }
    }
    Ok(())
}

fn valid_pointer(pointer: &str) -> bool {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return false;
    }
    let mut chars = pointer.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' && !matches!(chars.next(), Some('0' | '1')) {
            return false;
        }
    }
    true
}

fn execute(options: &Options) -> anyhow::Result<Value> {
    anyhow::ensure!(options.ticks > 0, "ticks 必须大于 0");
    anyhow::ensure!(
        (1..=32).contains(&options.concurrency),
        "concurrency 必须为 1..=32"
    );
    let path = std::fs::canonicalize(&options.scenario).context("读取场景路径")?;
    let bytes = std::fs::read(&path).context("读取场景")?;
    let scene: Scene = serde_json::from_slice(&bytes).context("解析场景 JSON")?;
    validate(&scene)?;
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
    let mut engine = EmbedEngine::new();
    engine.set_player_concurrency(options.concurrency)?;
    engine.set_kkpv_channel(options.kkpv);
    let shard = serde_json::to_value(&scene.shard)?;
    let mods_metadata = if let Some(input) = &scene.mods {
        let loaded = mods::load(base, input)?;
        engine.create_shard_with_mods(&shard, &loaded.packages, loaded.options)?;
        loaded.metadata
    } else {
        engine.create_shard(&shard)?;
        json!({"packages": {}})
    };
    let mod_names: Vec<String> = mods_metadata["packages"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut aliases = BTreeMap::new();
    for object in &scene.objects {
        let id = mods::place(&mut engine, object)
            .with_context(|| format!("放置对象 {}", object.alias))?;
        aliases.insert(object.alias.clone(), id);
    }
    let mut codes = BTreeMap::new();
    let mut stats = BTreeMap::new();
    for bot in &scene.bots {
        engine.set_player_modules(&bot.player, tables.remove(&bot.player).unwrap())?;
        engine.set_player_memory(&bot.player, &bot.memory)?;
        let entry = serde_json::to_string(&bot.entry)?;
        // 不吞掉缺少 loop 的错误；loop 的返回值不属于游戏提交状态。
        codes.insert(bot.player.clone(), format!("(() => {{ const bot = require({entry}); if (!bot || typeof bot.loop !== 'function') throw new Error('bot entry must export loop'); bot.loop(); return true; }})()"));
        stats.insert(bot.player.clone(), PlayerStats::default());
    }
    let initial = observe(&engine, &scene, &aliases, &mod_names);
    let start = std::time::Instant::now();
    let mut failure = None;
    let mut attempted = 0;
    for _ in 0..options.ticks {
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
    }
    let run_wall_ms = start.elapsed().as_secs_f64() * 1000.0;
    let final_state = observe(&engine, &scene, &aliases, &mod_names);
    let assertions: Vec<_> = scene
        .assertions
        .iter()
        .map(|a| evaluate(a, &initial, &final_state))
        .collect();
    let committed = engine.get_game_time();
    let success = failure.is_none()
        && committed == u64::from(options.ticks)
        && assertions.iter().all(|a| a["passed"] == true);
    let status = if failure.is_some() {
        "run_failed"
    } else if success {
        "passed"
    } else {
        "assertion_failed"
    };
    Ok(json!({
        "report_version": 1, "success": success, "status": status,
        "engine_version": env!("CARGO_PKG_VERSION"), "executable_sha256": executable_hash,
        "scenario": path, "scenario_sha256": digest(&bytes), "modules": modules,
        "mods": mods_metadata, "plan_hash": engine.contribution_plan_hash().hex(),
        "composition": engine.mod_composition(),
        "disabled_mods": engine.disabled_mods(),
        "channel": if options.kkpv { "kkpv" } else { "json" }, "concurrency": options.concurrency,
        "ticks_requested": options.ticks, "ticks_attempted": attempted, "world_ticks_committed": committed,
        "run_wall_ms": run_wall_ms, "phases": engine.perf_report(),
        "failure": failure, "players": stats, "aliases": aliases, "assertions": assertions,
        "mod_fault": engine.last_mod_settlement_fault().map(|fault| json!({
            "tick": fault.tick, "room": fault.room, "target": fault.target, "error": fault.error,
        })),
        "initial": initial, "final": final_state,
    }))
}

fn observe(
    engine: &EmbedEngine,
    scene: &Scene,
    aliases: &BTreeMap<String, String>,
    mod_names: &[String],
) -> Value {
    let players: BTreeMap<_, _> = scene.bots.iter().map(|bot| {
        let state = engine.player_state(&bot.player).unwrap();
        (bot.player.clone(), json!({"gcl": state.gcl, "rooms": state.rooms,
            "memory": serde_json::from_str::<Value>(&state.raw_memory).ok(), "raw_memory": state.raw_memory,
            "cpu_bucket": state.cpu.bucket()}))
    }).collect();
    let objects: BTreeMap<_, _> = aliases
        .iter()
        .map(|(name, id)| (name, engine.get_object(id)))
        .collect();
    let rooms: BTreeMap<_, _> = scene
        .shard
        .rooms
        .iter()
        .map(|room| {
            let name = room["name"].as_str().unwrap();
            let controller = engine
                .find_in_room(name, kk_core::constants::FIND_STRUCTURES)
                .into_iter()
                .find(|o| o["structureType"] == "controller");
            (name, json!({"controller": controller}))
        })
        .collect();
    let states: BTreeMap<_, _> = mod_names
        .iter()
        .map(|name| (name, engine.engine_mod_state(name)))
        .collect();
    json!({"time": engine.get_game_time(), "players": players, "objects": objects, "rooms": rooms, "world": engine.world_snapshot(), "mod_objects": engine.mod_object_snapshots(), "mod_states": states})
}

fn evaluate(assertion: &Assertion, initial: &Value, final_state: &Value) -> Value {
    let actual = final_state.pointer(&assertion.pointer);
    let before = initial.pointer(&assertion.pointer);
    let delta = actual.zip(before).and_then(|(a, b)| difference(a, b));
    let passed = match assertion.op {
        Comparison::Eq => actual == Some(&assertion.value),
        Comparison::Gte => actual.is_some_and(|a| at_least(a, &assertion.value)),
        Comparison::DeltaGte => actual.zip(before).is_some_and(|(a, b)| {
            match (integer(a), integer(b), integer(&assertion.value)) {
                (Some(a), Some(b), Some(expected)) => a - b >= expected,
                _ => delta
                    .as_ref()
                    .is_some_and(|d| at_least(d, &assertion.value)),
            }
        }),
        Comparison::Exists => actual.is_some_and(|v| !v.is_null()),
        Comparison::Missing => actual.is_none_or(Value::is_null),
    };
    json!({"pointer": assertion.pointer, "op": assertion.op, "expected": assertion.value,
        "before": before, "actual": actual, "delta": delta, "passed": passed})
}

fn integer(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

/// JSON 整数保留精度；混合小数比较拒绝将过大的整数舍入为同一 f64。
fn safe_float(value: &Value) -> Option<f64> {
    if integer(value).is_some_and(|n| n.abs() > 9_007_199_254_740_991) {
        return None;
    }
    value.as_f64().filter(|n| n.is_finite())
}

fn at_least(actual: &Value, expected: &Value) -> bool {
    match (integer(actual), integer(expected)) {
        (Some(a), Some(b)) => a >= b,
        _ => safe_float(actual)
            .zip(safe_float(expected))
            .is_some_and(|(a, b)| a >= b),
    }
}

fn difference(actual: &Value, before: &Value) -> Option<Value> {
    if let (Some(a), Some(b)) = (integer(actual), integer(before)) {
        let delta = a - b;
        return Some(if let Ok(n) = i64::try_from(delta) {
            json!(n)
        } else if let Ok(n) = u64::try_from(delta) {
            json!(n)
        } else {
            json!(delta.to_string())
        });
    }
    safe_float(actual)
        .zip(safe_float(before))
        .map(|(a, b)| json!(a - b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_integer_thresholds_do_not_round_into_false_passes() {
        let final_state = json!({"points": 9_007_199_254_740_992_u64});
        let initial = json!({"points": 9_007_199_254_740_991_u64});
        let assertion = Assertion {
            pointer: "/points".into(),
            op: Comparison::Gte,
            value: json!(9_007_199_254_740_993_u64),
        };
        assert_eq!(
            evaluate(&assertion, &initial, &final_state)["passed"],
            false
        );
        let assertion = Assertion {
            op: Comparison::DeltaGte,
            value: json!(2),
            ..assertion
        };
        assert_eq!(
            evaluate(&assertion, &initial, &final_state)["passed"],
            false
        );
        let assertion = Assertion {
            value: json!(1),
            ..assertion
        };
        assert_eq!(evaluate(&assertion, &initial, &final_state)["passed"], true);
        assert_eq!(evaluate(&assertion, &initial, &final_state)["delta"], 1);
    }

    #[test]
    fn escaped_aliases_and_missing_numeric_values_are_distinct() {
        let state = json!({"objects": {"a/b~c": null}, "n": "1"});
        let assertion = Assertion {
            pointer: "/objects/a~1b~0c".into(),
            op: Comparison::Eq,
            value: Value::Null,
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], true);
        let assertion = Assertion {
            pointer: "/objects/missing".into(),
            ..assertion
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], false);
        let assertion = Assertion {
            pointer: "/n".into(),
            op: Comparison::Gte,
            value: json!(0),
        };
        assert_eq!(evaluate(&assertion, &state, &state)["passed"], false);
    }
}
