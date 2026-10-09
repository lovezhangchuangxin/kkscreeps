//! G25.b 结算回放的库面验收：三来源直跑 vs 回放整份检查点逐位等值、
//! dice RNG 场景重放、配对规则矩阵、身份与 tick 门拒绝。
use super::*;

use kk_runtime::embed::{read_input_log, segments_from_checkpoint};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runner::ModuleTable;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Case(PathBuf);
impl Case {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-replay-{}-{}-{}",
            tag,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Case {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 与写入器同法重封一行（键序同 BTreeMap 口径），供伪造段测试。
/// 伪造第 index 行并把其后所有行的 prev_hash 级联到新摘要（保链）。
fn forge_cascade(
    lines: &mut [String],
    index: usize,
    mutate: impl FnOnce(&mut serde_json::Map<String, Value>),
) {
    let mut record: serde_json::Map<String, Value> =
        serde_json::from_str(&lines[index]).expect("行是合法 JSON");
    mutate(&mut record);
    let (line, mut hash) = seal_line(record);
    lines[index] = line;
    for next in lines.iter_mut().skip(index + 1) {
        let mut record: serde_json::Map<String, Value> =
            serde_json::from_str(next).expect("行是合法 JSON");
        record.insert("prev_hash".into(), json!(hash));
        let (line, new_hash) = seal_line(record);
        *next = line;
        hash = new_hash;
    }
}

fn seal_line(mut record: serde_json::Map<String, Value>) -> (String, String) {
    record.remove("sha256");
    let body = Value::Object(record.clone()).to_string();
    let hash = sha256_hex(body.as_bytes());
    record.insert("sha256".into(), json!(hash));
    (Value::Object(record).to_string(), hash)
}

/// 等值比较前剥离的键：last_console 由沙箱执行产生（回放无沙箱），
/// metadata 是调用方自定载荷。
fn strip_volatile(mut value: Value) -> Value {
    let object = value.as_object_mut().expect("检查点是对象");
    object.remove("last_console");
    object.remove("metadata");
    value
}

fn plain_engine(case: &Case, log_name: &str) -> anyhow::Result<(EmbedEngine, PathBuf)> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 3}]
    }))?;
    engine.place_object(
        "W1N1",
        "creep",
        &json!({"pos": [10, 10], "owner": "p1", "name": "c1", "body": ["move"]}),
    )?;
    let path = case.path(log_name);
    Ok((engine, path))
}

/// 直跑（players/external/advance 三来源）记日志后，从起点快照恢复
/// 逐段重放，整份检查点逐位等值；回放引擎不写新段。
pub(super) fn three_sources_replay_matches_live_impl() -> anyhow::Result<()> {
    let case = Case::new("sources");
    let (mut engine, log) = plain_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({"case": "sources"}))?;
    let start = engine.checkpoint(json!({"kind": "replay-start"}))?;

    // 段写回必须走真实路径：回放侧经 from_runner 重放的 segments 驱动
    // 玩家段状态，重建丢失 segments 会破坏等值。
    let write = "Memory.note = 'round-a'; RawMemory.segments[3] = 'seg'; RawMemory.setActiveSegments([3]); InterShardMemory.setLocal('replay-ism'); Game.creeps.c1.move(3); true";
    engine.run_players(&BTreeMap::from([("p1".to_string(), write.to_string())]))?;
    // 第二轮玩家不写 Memory：写回仍落段（Memory 序列化恒有），重放按
    // 记录值逐位复现——覆盖「同句柄多轮、每轮一次 extend」的路径。
    engine.run_players(&BTreeMap::from([("p1".to_string(), "true".to_string())]))?;
    let mut batch = kk_engine::intent::IntentBatch::default();
    batch.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::CreateFlag {
            room: "W1N1".into(),
            x: 5,
            y: 5,
            name: "flag-a".into(),
            color: 0,
            secondary_color: 0,
        },
    );
    engine.settle_external(batch)?;
    engine.tick(1)?;
    let final_live = engine.checkpoint(Value::Null)?;

    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 4);
    let start_time = start["engine"]["world"]["time"]
        .as_u64()
        .expect("起点带世界时间");
    let segments = segments_from_checkpoint(start_time, &data.segments)?;
    assert_eq!(segments.len(), 4, "起点在首轮前：全部段可回放");
    let header = data.header.clone();

    // V8 栈序：先销毁直跑引擎再建回放引擎。
    drop(engine);
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start, None)?;
    replay.replay_input_rounds(&header, segments)?;
    assert_eq!(
        strip_volatile(replay.checkpoint(Value::Null)?),
        strip_volatile(final_live.clone()),
        "回放终态必须与直跑终态逐位相等"
    );
    assert_eq!(replay.input_log_segments(), None, "回放引擎不写新段");
    Ok(())
}

/// rebuild_round 的写回复建直测：raw_memory/segments 的 null 与 Some
/// 两态都从真实段直接钉死。正常记段轮 raw_memory 恒为字符串，null 分支
/// 服务未来/异构载荷（None=结算端保留旧值），故以字段改写直测而非
/// 构造自然轮。
pub(super) fn rebuild_writeback_null_and_some_impl() -> anyhow::Result<()> {
    let case = Case::new("rebuild");
    let (mut engine, log) = plain_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let write =
        "Memory.note = 'x'; RawMemory.segments[3] = 'seg'; RawMemory.setActiveSegments([3]); true";
    engine.run_players(&BTreeMap::from([("p1".to_string(), write.to_string())]))?;
    drop(engine);
    let data = read_input_log(&log)?;
    let segment = data.segments[0].clone();

    let round = kk_runtime::embed::rebuild_round(&segment)?;
    let writeback = &round.writebacks["p1"];
    assert!(writeback.cpu.is_some(), "CPU 新桶整桶重建");
    // 正常轮 endTick 恒带 rawMemory（字符串），Some 态是常态。
    assert!(
        writeback
            .raw_memory
            .as_deref()
            .is_some_and(|m| m.contains('x'))
    );
    assert!(writeback.segments.is_some(), "段写回经 from_runner 重放");

    // null 两态：字段置 null 后重建（rebuild 不做链校验，直接可测）。
    let mut nulled = segment.clone();
    nulled["players"]["p1"]["raw_memory"] = Value::Null;
    nulled["players"]["p1"]["segments"] = Value::Null;
    let round = kk_runtime::embed::rebuild_round(&nulled)?;
    let writeback = &round.writebacks["p1"];
    assert_eq!(
        writeback.raw_memory, None,
        "null 重建为 None（保留旧值语义）"
    );
    assert_eq!(writeback.segments, None);
    Ok(())
}

/// dice 场景（G22.b 夹具，每 tick 抽值写 state）：重放与直跑的抽值
/// 逐位一致——RNG 免记输入面靠无状态协议成立；Mod 身份快照逐段核对。
pub(super) fn dice_replay_matches_live_impl() -> anyhow::Result<()> {
    let case = Case::new("dice");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/mod-determinism")
        .canonicalize()?;
    let packages = discover_packages(&root)?;
    let mut engine = EmbedEngine::with_world_seed(7);
    engine.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        &packages,
        RegisterOptions::default(),
    )?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "exports.loop = () => {};");
    engine.set_player_modules("p1", table)?;
    engine.place_mod_object("W1N1", "dice:roller", &json!({"x": 10, "y": 10}))?;
    let log = case.path("dice.jsonl");
    engine.enable_input_log(&log, json!({"case": "dice"}))?;
    let start = engine.checkpoint(json!({"kind": "replay-start"}))?;

    let codes = BTreeMap::from([("p1".to_string(), "require('main').loop()".to_string())]);
    for _ in 0..4 {
        let results = engine.run_players(&codes)?;
        assert!(results.values().all(|r| r.expr["ok"] == true));
    }
    engine.tick(1)?;
    let final_live = engine.checkpoint(Value::Null)?;

    let data = read_input_log(&log)?;
    let start_time = start["engine"]["world"]["time"]
        .as_u64()
        .expect("起点带世界时间");
    let segments = segments_from_checkpoint(start_time, &data.segments)?;
    let header = data.header.clone();
    drop(engine);
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start, Some((&packages, &RegisterOptions::default())))?;
    replay.replay_input_rounds(&header, segments)?;
    let replay_final = replay.checkpoint(Value::Null)?;
    assert_eq!(
        strip_volatile(replay_final.clone()),
        strip_volatile(final_live.clone()),
        "含 Mod 随机流的回放终态必须逐位相等（抽值域含 state）"
    );
    assert_eq!(
        replay.engine_mod_state("dice")["draws"],
        final_live["engine"]["mod_states"]["dice"]["draws"],
        "抽值序列逐位一致（RNG 无状态协议：同输入即同抽值）"
    );
    Ok(())
}

/// 配对规则矩阵：连续起点/缺口/未覆盖，全部用合成段直接钉住语义。
pub(super) fn pairing_matrix_impl() -> anyhow::Result<()> {
    let seg = |tick: u64| json!({"tick": tick});
    let segments: Vec<Value> = [0, 1, 2].map(seg).to_vec();
    let chosen = segments_from_checkpoint(0, &segments)?;
    assert_eq!(chosen.len(), 3);
    let chosen = segments_from_checkpoint(2, &segments)?;
    assert_eq!(chosen.len(), 1, "跳过前缀整段早于检查点");
    assert_eq!(chosen[0]["tick"], 2);

    let uncovered: Vec<Value> = [0, 1].map(seg).to_vec();
    let error = segments_from_checkpoint(2, &uncovered).unwrap_err();
    assert!(format!("{error:#}").contains("不覆盖"), "{error:#}");

    // 换分片续接的形态：摘除后新分片 tick 时间线重启，永远追不上检查点。
    let restarted: Vec<Value> = [0, 1, 0].map(seg).to_vec();
    let error = segments_from_checkpoint(2, &restarted).unwrap_err();
    assert!(format!("{error:#}").contains("不覆盖"), "{error:#}");

    let gap: Vec<Value> = [0, 1, 3].map(seg).to_vec();
    let error = segments_from_checkpoint(2, &gap).unwrap_err();
    assert!(
        format!("{error:#}").contains("配对缺口"),
        "首段晚于检查点=世界连续性断裂：{error:#}"
    );

    let late: Vec<Value> = [0, 2].map(seg).to_vec();
    let error = segments_from_checkpoint(1, &late).unwrap_err();
    assert!(format!("{error:#}").contains("配对缺口"), "{error:#}");

    // 选区内 tick 回退（世界回退后续接同一日志）：在配对起点就报真实
    // 原因，不把接缝留给逐段硬门误报。
    let seam: Vec<Value> = [0, 1, 0, 1].map(seg).to_vec();
    let error = segments_from_checkpoint(0, &seam).unwrap_err();
    assert!(format!("{error:#}").contains("tick 回退"), "{error:#}");
    // 接缝之后的区间自身单调：用与接缝后分支连续的检查点可正常选中。
    let chosen = segments_from_checkpoint(1, &seam[3..])?;
    assert_eq!(chosen.len(), 1);
    Ok(())
}

/// settle_replayed_round 直测：可独立驱动一轮；存在失败轮候选时被
/// no_pending_round 门拒绝。
pub(super) fn settle_replayed_round_contract_impl() -> anyhow::Result<()> {
    let case = Case::new("direct");
    let (mut engine, _log) = plain_engine(&case, "unused.jsonl")?;
    engine.settle_replayed_round(
        kk_engine::intent::IntentBatch::default(),
        kk_engine::TickInput {
            now_ms: 1,
            warmup: false,
        },
        BTreeMap::new(),
    )?;
    assert_eq!(engine.get_game_time(), 2, "直调回放入口推进一轮");

    let codes = BTreeMap::from([("p1".to_string(), "true".to_string())]);
    engine.fail_next_settle_for_test();
    assert!(engine.run_players(&codes).is_err(), "结算失败缓存失败轮");
    let error = engine
        .settle_replayed_round(
            kk_engine::intent::IntentBatch::default(),
            kk_engine::TickInput {
                now_ms: 2,
                warmup: false,
            },
            BTreeMap::new(),
        )
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("被拒"),
        "失败轮候选未处置时不得回放：{error:#}"
    );
    Ok(())
}

/// 身份与 tick 门：重封伪造段分别命中身份快照不符与 tick 硬门。
pub(super) fn identity_and_tick_gates_impl() -> anyhow::Result<()> {
    let case = Case::new("gates");
    let (mut engine, log) = plain_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let start = engine.checkpoint(Value::Null)?;
    let codes = BTreeMap::from([("p1".to_string(), "(Memory.n = 1; true)".to_string())]);
    engine.run_players(&codes)?;
    engine.tick(2)?;
    drop(engine);

    let mut lines: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .map(str::to_string)
        .collect();

    // 伪造第 2 段身份（重封并把后段 prev_hash 级联到新摘要，保链）：
    // 回放第 1 轮后命中身份门。
    let mut forged: serde_json::Map<String, Value> = serde_json::from_str(&lines[2])?;
    forged.insert(
        "identity".into(),
        json!({"plan_hash": "deadbeef", "execution_strategy": null, "disabled_mods": {}}),
    );
    let (line, hash) = seal_line(forged);
    lines[2] = line;
    let mut next: serde_json::Map<String, Value> = serde_json::from_str(&lines[3])?;
    next.insert("prev_hash".into(), json!(hash));
    lines[3] = seal_line(next).0;
    let path = case.path("identity.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start.clone(), None)?;
    let data = read_input_log(&path)?;
    let error = replay
        .replay_input_rounds(&data.header, segments_from_checkpoint(1, &data.segments)?)
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("身份快照"), "{message}");

    // 身份门的另两个子域：执行策略与停用集（末段伪造，无级联）。
    for (field, value) in [
        ("execution_strategy", json!("deadbeef-strategy")),
        ("disabled_mods", json!({"some-mod": true})),
    ] {
        let mut lines: Vec<String> = std::fs::read_to_string(&log)?
            .lines()
            .map(str::to_string)
            .collect();
        let mut forged: serde_json::Map<String, Value> = serde_json::from_str(&lines[3])?;
        forged
            .get_mut("identity")
            .and_then(Value::as_object_mut)
            .expect("段带 identity")
            .insert(field.into(), value);
        lines[3] = seal_line(forged).0;
        let path = case.path("identity-subdomain.jsonl");
        std::fs::write(&path, lines.join("\n") + "\n")?;
        let mut replay = EmbedEngine::new();
        replay.restore_checkpoint(start.clone(), None)?;
        let data = read_input_log(&path)?;
        let error = replay
            .replay_input_rounds(&data.header, segments_from_checkpoint(1, &data.segments)?)
            .unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("身份快照"), "{}（{}）", message, field);
    }

    // generation 强校验：独立文案（非 tick 硬门）命中。
    let mut lines: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .map(str::to_string)
        .collect();
    let mut forged: serde_json::Map<String, Value> = serde_json::from_str(&lines[3])?;
    let generation = forged
        .get("generation")
        .and_then(Value::as_u64)
        .expect("段带 generation");
    forged.insert("generation".into(), json!(generation + 1));
    lines[3] = seal_line(forged).0;
    let path = case.path("generation.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start.clone(), None)?;
    let data = read_input_log(&path)?;
    let error = replay
        .replay_input_rounds(&data.header, segments_from_checkpoint(1, &data.segments)?)
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("宿主世界变异") && message.contains("generation"),
        "{message}"
    );

    // 伪造末段 tick：从原始日志重新取行（上一变体已改动 lines[2]）。
    // 重封即保链——末段无后段需级联：第 2 轮命中 tick 硬门。
    let mut lines: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .map(str::to_string)
        .collect();
    let mut forged: serde_json::Map<String, Value> = serde_json::from_str(&lines[3])?;
    let tick = forged
        .get("tick")
        .and_then(Value::as_u64)
        .expect("段带 tick");
    forged.insert("tick".into(), json!(tick + 7));
    lines[3] = seal_line(forged).0;
    let path = case.path("tick.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start.clone(), None)?;
    let data = read_input_log(&path)?;
    let error = replay
        .replay_input_rounds(&data.header, segments_from_checkpoint(1, &data.segments)?)
        .unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("与世界时间"), "{message}");

    // 头段身份门：错种子的日志在入口被拒（种子是随机流派生根，不在
    // 逐段身份里）；伪造头段需级联全部后段的 prev_hash 保链。
    let mut lines: Vec<String> = std::fs::read_to_string(&log)?
        .lines()
        .map(str::to_string)
        .collect();
    forge_cascade(&mut lines, 0, |header| {
        header
            .get_mut("identity")
            .and_then(Value::as_object_mut)
            .expect("头段带 identity")
            .insert("seed".into(), json!(999));
    });
    let path = case.path("wrong-seed.jsonl");
    std::fs::write(&path, lines.join("\n") + "\n")?;
    let data = read_input_log(&path)?;
    assert_eq!(data.segments.len(), 3, "级联重封保链");
    let mut replay = EmbedEngine::new();
    replay.restore_checkpoint(start.clone(), None)?;
    let error = replay
        .replay_input_rounds(&data.header, segments_from_checkpoint(1, &data.segments)?)
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("种子"),
        "错种子日志必须入口拒绝：{error:#}"
    );
    Ok(())
}
