//! G25.a 持久输入日志的库面验收：三来源写入-读回等值、失败轮/重试的
//! 记段语义、篡改矩阵（逐变体命中具体错误）、续接与生命周期门。
use super::*;

use kk_runtime::embed::{
    InputLogError, Retain, ScanError, read_input_log, scan_input_log, truncate_after_cursor,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// 每个用例独立临时目录，Drop 清理；目录名含进程与序号避免并行冲突。
struct Case(PathBuf);
impl Case {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kkscreeps-input-log-{}-{}-{}",
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

/// 与写入器同法封装一行：键序依赖 serde_json 无 preserve_order 时的
/// BTreeMap 排序，写读两侧一致（见模块文档）。
fn seal_line(mut record: serde_json::Map<String, Value>) -> String {
    record.remove("sha256");
    let body = Value::Object(record.clone()).to_string();
    let hash = sha256_hex(body.as_bytes());
    record.insert("sha256".into(), json!(hash));
    Value::Object(record).to_string()
}

fn new_engine(case: &Case, log_name: &str) -> anyhow::Result<(EmbedEngine, PathBuf)> {
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

/// 三种来源各落一段：玩家轮带原始意图与写回，外部轮带 to_json 规范
/// 形态，推进轮为空载荷；读回等值且段号/来源/链完整。
pub(super) fn three_sources_round_trip_impl() -> anyhow::Result<()> {
    let case = Case::new("sources");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    let creep = engine.find_in_room("W1N1", 101)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    engine.enable_input_log(&log, json!({"case": "sources"}))?;

    // 玩家轮：Memory/RawMemory/段写回 + 一次移动意图。
    // 多语句程序（分号不能包进括号——那是语法错误）；末值作为 expr 结果。
    let code = r#"Memory.note = 'g25a'; RawMemory.segments[3] = 'seg';
                  RawMemory.setActiveSegments([3]);
                  Game.creeps.c1.move(3); true"#;
    engine.run_players(&BTreeMap::from([("p1".to_string(), code.to_string())]))?;

    // 外部轮：调用方直接结算的意图批（to_json 规范形态入日志）。
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

    // 推进轮：不执行玩家代码。
    engine.tick(2)?;

    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 4, "三种来源共四段");
    assert_eq!(data.chain.len(), 5, "链含头段");
    assert_eq!(data.header["kind"], "header");
    assert_eq!(data.header["metadata"]["case"], "sources");
    let sources: Vec<&str> = data
        .segments
        .iter()
        .map(|s| s["source"].as_str().unwrap())
        .collect();
    assert_eq!(sources, ["players", "external", "advance", "advance"]);

    let ticks: Vec<u64> = data
        .segments
        .iter()
        .filter_map(|s| s["tick"].as_u64())
        .collect();
    assert_eq!(ticks, [1, 2, 3, 4], "段按结算 tick 递增");
    for (index, segment) in data.segments.iter().enumerate() {
        assert_eq!(segment["seq"], index as u64 + 1);
        assert!(segment["identity"]["plan_hash"].is_string(), "逐段身份快照");
        assert!(segment["now_ms"].is_u64());
    }

    // 玩家段：外层键=产出沙箱句柄，内层=对象键原始报告（from_json 的
    // 直接输入），移动意图按对象 ID 落位。
    let player_segment = &data.segments[0];
    let report = &player_segment["intents"]["p1"];
    assert_eq!(
        report[&creep]["move"]["direction"], 3,
        "原始意图按对象 ID 落位"
    );
    let writeback = &player_segment["players"]["p1"];
    assert!(
        writeback["cpu_bucket"].is_object(),
        "回放直接消费的新桶整桶"
    );
    assert!(writeback["cpu_billed_ms"].is_number(), "实测计费佐证");
    // Memory 只随段携带摘要——与终态检查点的提交值对账（推进轮不
    // 改 Memory，值自玩家轮后保持不变）。
    let committed = engine.checkpoint(Value::Null)?["engine"]["players"]["p1"]["raw_memory"]
        .as_str()
        .expect("检查点玩家 Memory 在位")
        .to_string();
    assert!(
        committed.contains("g25a"),
        "RawMemory 写回候选：{committed}"
    );
    assert_eq!(
        writeback["raw_memory_sha256"],
        json!(sha256_hex(committed.as_bytes())),
        "段内摘要与终态提交值一致"
    );
    assert_eq!(writeback["segments"]["active"], json!([3]), "段写回候选");

    // 外部段：to_json 的玩家键形态，房间动作按动作名分组。
    let external = &data.segments[1];
    assert_eq!(
        external["intents"]["p1"]["room"]["createFlag"][0]["name"],
        "flag-a"
    );
    assert_eq!(external["players"].as_object().map(|m| m.len()), Some(0));

    // 推进段：意图与玩家表为空对象。
    for segment in &data.segments[2..] {
        assert_eq!(segment["intents"], json!({}));
        assert_eq!(segment["players"], json!({}));
    }
    Ok(())
}

/// 失败轮不记段；缓存轮重试成功后只记一次（重试不重复采样输入）。
pub(super) fn failed_round_and_retry_semantics_impl() -> anyhow::Result<()> {
    let case = Case::new("retry");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let codes = BTreeMap::from([("p1".to_string(), "(Memory.n = 1; true)".to_string())]);

    engine.fail_next_settle_for_test();
    let error = engine.run_players(&codes).unwrap_err();
    assert!(error.to_string().contains("故障注入"), "{error:#}");
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 0, "失败轮不记段（只有头段）");

    engine.run_players(&codes)?;
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 1, "重试成功只记一次");
    assert_eq!(data.segments[0]["tick"], 1, "重试轮仍执行缓存时的 tick");

    engine.run_players(&codes)?;
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 2);
    assert_eq!(data.segments[1]["tick"], 2);
    Ok(())
}

/// 日志写入失败：本轮世界已提交、错误如实上报且日志停用（不再续写）。
pub(super) fn log_io_failure_commits_round_and_detaches_impl() -> anyhow::Result<()> {
    let case = Case::new("io");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let codes = BTreeMap::from([("p1".to_string(), "(Memory.n = 1; true)".to_string())]);

    engine.fail_next_log_write_for_test();
    let error = engine.run_players(&codes).unwrap_err();
    let message = format!("{error:#}");
    assert!(
        message.contains("已提交"),
        "错误必须说明世界已提交：{message}"
    );
    assert!(message.contains("不得重试"), "{message}");
    assert_eq!(
        error
            .downcast_ref::<kk_runtime::embed::CommittedLogError>()
            .unwrap()
            .committed_time,
        2
    );
    assert_eq!(engine.get_game_time(), 2, "世界事实上已推进");
    assert_eq!(engine.input_log_segments(), None, "日志已停用摘除");

    // 停用后继续运行不写段也不报错；磁盘日志保持头段。
    engine.run_players(&codes)?;
    assert_eq!(engine.input_log_segments(), None);
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 0);
    Ok(())
}

/// 篡改矩阵：每个变体命中具体错误；最后一行损坏收敛为 TornTail 并
/// 携带有效前缀段数（截断恢复的依据）。
pub(super) fn tamper_matrix_impl() -> anyhow::Result<()> {
    let case = Case::new("tamper");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    engine.tick(3)?;
    drop(engine);

    let original = std::fs::read_to_string(&log)?;
    let lines: Vec<&str> = original.lines().collect();
    assert_eq!(lines.len(), 4, "头段 + 三段");
    let write_case = |name: &str, text: String| {
        let path = case.path(name);
        std::fs::write(&path, text).unwrap();
        path
    };

    // 中段字节翻转（保持合法 JSON）：载荷被改、摘要不匹配。
    let flipped = lines
        .clone()
        .iter()
        .map(|l| l.replacen("\"source\":\"advance\"", "\"source\":\"advancf\"", 1))
        .collect::<Vec<_>>()
        .join("\n");
    let path = write_case("flip.jsonl", flipped + "\n");
    match read_input_log(&path) {
        Err(InputLogError::HashMismatch { seq: 1, .. }) => {}
        other => panic!("中段字节翻转应命中段 2 摘要不匹配：{other:?}"),
    }

    // 删中段：段号断链。
    let deleted = [lines[0], lines[1], lines[3]].join("\n");
    let path = write_case("delete.jsonl", deleted + "\n");
    match read_input_log(&path) {
        Err(InputLogError::SeqGap {
            expected: 2,
            found: 3,
            ..
        }) => {}
        other => panic!("删中段应命中段号断链：{other:?}"),
    }

    // 乱序（交换两个段行）：读取器先校验段号连续，乱序在此暴露。
    let reordered = [lines[0], lines[2], lines[1], lines[3]].join("\n");
    let path = write_case("reorder.jsonl", reordered + "\n");
    match read_input_log(&path) {
        Err(InputLogError::SeqGap {
            expected: 1,
            found: 2,
            ..
        }) => {}
        other => panic!("乱序应被段号检查拦截：{other:?}"),
    }

    // 链断裂：段号与自身摘要都合法，但 prev_hash 指向别处（重放旧段或
    // 跨日志拼段的形态）。
    let mut stale: serde_json::Map<String, Value> = serde_json::from_str(lines[1])?;
    stale.insert("prev_hash".into(), json!("0".repeat(64)));
    let spliced = [
        lines[0].to_string(),
        seal_line(stale),
        lines[2].to_string(),
        lines[3].to_string(),
    ]
    .join("\n");
    let path = write_case("chain-break.jsonl", spliced + "\n");
    match read_input_log(&path) {
        Err(InputLogError::ChainBreak { seq: 1, .. }) => {}
        other => panic!("断链段应命中 ChainBreak：{other:?}"),
    }

    // 撕裂尾（截断最后一行中部）：TornTail 携带完整前缀段数（前两段）。
    let torn = original[..original.len() - 9].to_string();
    let path = write_case("torn.jsonl", torn);
    match read_input_log(&path) {
        Err(InputLogError::TornTail { valid_segments: 2 }) => {}
        other => panic!("撕裂尾应携带有效前缀：{other:?}"),
    }

    // 尾行字节翻转（行完整但摘要不符）：撕裂签名同样收敛为 TornTail。
    let tail_flipped = format!(
        "{}\n{}",
        lines[..3].join("\n"),
        lines[3].replacen("\"source\":\"advance\"", "\"source\":\"advancf\"", 1)
    );
    let path = write_case("tail-flip.jsonl", tail_flipped + "\n");
    match read_input_log(&path) {
        Err(InputLogError::TornTail { valid_segments: 2 }) => {}
        other => panic!("尾行损坏应收敛 TornTail：{other:?}"),
    }

    // 换头（用另一身份日志的头段）：首个数据段链哈希无法衔接。
    let other_case = Case::new("tamper-other");
    let (mut other_engine, other_log) = new_engine(&other_case, "other.jsonl")?;
    // metadata 必须不同，否则两个头段字节相同、换头后哈希仍然衔接。
    other_engine.enable_input_log(&other_log, json!({"case": "other"}))?;
    other_engine.tick(1)?;
    drop(other_engine);
    let other_text = std::fs::read_to_string(&other_log)?;
    let other_lines: Vec<&str> = other_text.lines().collect();
    let swapped = [other_lines[0], lines[1], lines[2], lines[3]].join("\n");
    let path = write_case("swap-header.jsonl", swapped + "\n");
    match read_input_log(&path) {
        Err(InputLogError::ChainBreak { seq: 1, .. }) => {}
        other => panic!("换头应使首段断链：{other:?}"),
    }

    // 未知版本（整行自洽重封）：显式拒绝、不迁移。
    let mut header: serde_json::Map<String, Value> = serde_json::from_str(lines[0])?;
    header.insert("version".into(), json!(99));
    let forged = [
        seal_line(header),
        lines[1].to_string(),
        lines[2].to_string(),
        lines[3].to_string(),
    ]
    .join("\n");
    let path = write_case("future.jsonl", forged + "\n");
    match read_input_log(&path) {
        Err(InputLogError::UnknownVersion { version: 99, .. }) => {}
        other => panic!("未来版本应显式拒绝：{other:?}"),
    }

    // 空文件：无头段可读。
    let path = write_case("empty.jsonl", String::new());
    match read_input_log(&path) {
        Err(InputLogError::BadHeader { .. }) => {}
        other => panic!("空文件应报头段缺失：{other:?}"),
    }

    // enable 面同样拒绝损坏日志：不能从断链处静默续写。
    let flipped_path = case.path("flip.jsonl");
    let (mut engine2, _) = new_engine(&case, "unused.jsonl")?;
    assert!(
        engine2.enable_input_log(&flipped_path, json!({})).is_err(),
        "续接前必须整读校验"
    );
    Ok(())
}

/// 撕裂的字节形态：尾块切进多字节字符收敛 TornTail；前缀中段非法
/// UTF-8 报实际行号；缺换行尾块内容完整可读回但续接写入被拒。
pub(super) fn torn_tail_byte_forms_impl() -> anyhow::Result<()> {
    let case = Case::new("torn-bytes");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({"备注": "中文"}))?;
    // Memory 只入摘要（ASCII），多字节字符经意图载荷进段。
    let code = "Game.creeps.c1.say('撕裂试验'); true";
    engine.run_players(&BTreeMap::from([("p1".to_string(), code.to_string())]))?;
    engine.tick(1)?;
    drop(engine);
    let bytes = std::fs::read(&log)?;
    let text = String::from_utf8(bytes.clone())?;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "头段 + 玩家段 + 推进段");

    // 尾块切进多字节字符：截掉推进段与玩家段后半，切口落在玩家段
    // intents 载荷的多字节字符内部（未换行结尾的尾块）。
    let player_line = lines[1];
    let player_start = text.len() - lines[2].len() - 1 - player_line.len();
    let multibyte_at = player_line
        .bytes()
        .position(|b| b >= 0x80)
        .expect("玩家段 intents 含中文");
    let cut_inside = player_start + multibyte_at + 1;
    let path = case.path("utf8-torn.jsonl");
    std::fs::write(&path, &bytes[..cut_inside])?;
    match read_input_log(&path) {
        Err(InputLogError::TornTail { valid_segments: 0 }) => {}
        other => panic!("玩家段被切进多字节字符应收敛 TornTail 且前缀为 0 段：{other:?}"),
    }

    // 前缀中段非法 UTF-8：报实际行号（第 2 行）。
    let mut corrupted = bytes.clone();
    corrupted[player_start] = 0x80; // 裸续字节，非法 UTF-8 起始
    let path = case.path("utf8-mid.jsonl");
    std::fs::write(&path, corrupted)?;
    match read_input_log(&path) {
        Err(InputLogError::BadLine { line: 2 }) => {}
        other => panic!("中段非法 UTF-8 应报实际行号 2：{other:?}"),
    }

    // 缺换行尾块：内容完整则读回接受为末段；续接写入必须拒绝。
    let stripped = &bytes[..bytes.len() - 1];
    let path = case.path("no-newline.jsonl");
    std::fs::write(&path, stripped)?;
    let data = read_input_log(&path)?;
    assert_eq!(data.segments.len(), 2, "完整的无换行尾块按末段接受");
    let (mut engine2, _) = new_engine(&case, "unused3.jsonl")?;
    let rejected = engine2.enable_input_log(&path, json!({})).unwrap_err();
    assert!(
        rejected.to_string().contains("缺换行"),
        "append 会把新段拼进旧行，必须拒绝续接：{rejected:#}"
    );

    // 末行撕裂签名后跟空白块：末行口径取最后一个非空行，尾随空白不把
    // 撕裂行升级成中段损坏。切掉行尾 20 字节落在
    // sha256/花括号的 ASCII 区，构造解析失败签名且不切进多字节字符。
    let torn_line = &lines[2][..lines[2].len() - 20];
    let path = case.path("torn-blank.jsonl");
    std::fs::write(&path, format!("{}\n{torn_line}\n\n", lines[0]))?;
    match read_input_log(&path) {
        Err(InputLogError::TornTail { valid_segments: 0 }) => {}
        other => panic!("撕裂行后的空白行不应改变末行判定：{other:?}"),
    }
    let path = case.path("torn-blank-tail.jsonl");
    std::fs::write(&path, format!("{}\n{torn_line}\n  ", lines[0]))?;
    match read_input_log(&path) {
        Err(InputLogError::TornTail { valid_segments: 0 }) => {}
        other => panic!("撕裂行后的无换行空白不应改变末行判定：{other:?}"),
    }
    Ok(())
}

/// 生命周期：重复挂载拒绝；干净日志可续接且段号延续；teardown 摘除
/// 日志但保留磁盘文件。
pub(super) fn resume_and_lifecycle_impl() -> anyhow::Result<()> {
    let case = Case::new("resume");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    assert!(
        engine.enable_input_log(&log, json!({})).is_err(),
        "一个分片运行期只挂一份"
    );
    engine.tick(2)?;
    assert_eq!(engine.input_log_segments(), Some(2));
    engine.teardown();
    assert_eq!(engine.input_log_segments(), None, "teardown 摘除日志");
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 2, "磁盘文件保留");

    // 重建分片同样摘除运行期日志：同引擎 teardown→create_shard 后可再挂载。
    engine.create_shard(&json!({"players": ["p1"], "rooms": [{"name": "W1N1"}]}))?;
    assert_eq!(engine.input_log_segments(), None, "重建分片摘除日志");
    engine.enable_input_log(&log, json!({}))?;
    engine.tick(1)?;
    assert_eq!(engine.input_log_segments(), Some(3));
    engine.teardown();

    // 新分片续接同一日志：先整读校验，段号从 4 延续。
    let (mut engine2, _) = new_engine(&case, "unused2.jsonl")?;
    engine2.enable_input_log(&log, json!({"case": "resume"}))?;
    // 段数按整个文件计（含续接时既有的段），与读回对账同口径。
    assert_eq!(engine2.input_log_segments(), Some(3));
    engine2.tick(1)?;
    assert_eq!(engine2.input_log_segments(), Some(4));
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 4, "磁盘上共四段");
    assert_eq!(data.segments[3]["seq"], 4);
    // 段号跨日志文件延续；世界 tick 属于新分片自己的时间线——与检查
    // 点配对恢复世界连续性是 G25.b 的规则，此处不伪造。
    assert_eq!(data.segments[3]["tick"], 1, "续接段记录新分片自己的 tick");
    Ok(())
}

/// v2 流式扫描：零保留只校验与对账；按 tick/按游标的后缀选择与整读版
/// 等值；配对失败（未覆盖/哈希不符/头段游标）与旧后缀选择同形。
pub(super) fn streaming_scan_impl() -> anyhow::Result<()> {
    let case = Case::new("scan");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let code = "Memory.n = (Memory.n || 0) + 1; true";
    engine.run_players(&BTreeMap::from([("p1".to_string(), code.to_string())]))?;
    engine.tick(2)?;
    let cursor = engine.input_log_cursor().unwrap();
    engine.run_players(&BTreeMap::from([("p1".to_string(), code.to_string())]))?;
    engine.tick(1)?;
    drop(engine);

    let full = read_input_log(&log)?;
    assert_eq!(full.segments.len(), 5, "玩家+推进+推进+玩家+推进");

    // 零保留：数据段不驻留（链只含头段项），校验与统计仍覆盖全部行。
    let stats_only = scan_input_log(&log, Retain::None)?;
    assert!(stats_only.segments.is_empty());
    assert_eq!(stats_only.chain.len(), 1, "零保留只驻留头段链项");
    assert_eq!(stats_only.chain[0].0, 0);
    assert_eq!(stats_only.stats.segments, 5);
    assert_eq!(stats_only.stats.first_tick, Some(1));
    assert_eq!(stats_only.stats.last_tick, Some(5));
    assert_eq!(stats_only.final_seq, 5);
    assert_eq!(stats_only.final_hash, full.chain.last().unwrap().1);

    // 按 tick 的后缀与整读版 segments_from_checkpoint 等值。
    let from_tick = scan_input_log(&log, Retain::FromTick(4))?;
    assert_eq!(
        from_tick.segments,
        kk_runtime::embed::segments_from_checkpoint(4, &full.segments)?,
        "FromTick 后缀与整读版配对等值"
    );

    // 按游标的后缀与整读版 segments_after_cursor 等值；段内摘要保留。
    let from_cursor = scan_input_log(
        &log,
        Retain::FromCursor {
            cursor: cursor.clone(),
            time: 4,
        },
    )?;
    assert_eq!(
        from_cursor.segments,
        full.segments_after_cursor(4, &cursor)?,
        "FromCursor 后缀与整读版配对等值"
    );
    assert!(
        from_cursor.segments[0]["players"]["p1"]["raw_memory_sha256"]
            .as_str()
            .is_some_and(|digest| digest.len() == 64),
        "玩家段保留 Memory 摘要"
    );

    // 未覆盖：时间/游标超出日志末尾。
    assert!(scan_input_log(&log, Retain::FromTick(6)).is_err());
    let beyond = kk_runtime::embed::LogCursor {
        last_seq: 9,
        last_tick: 9,
        sha256: "0".repeat(64),
    };
    assert!(
        scan_input_log(
            &log,
            Retain::FromCursor {
                cursor: beyond,
                time: 10
            }
        )
        .is_err()
    );

    // 游标历史哈希不符：配对失败。
    let mut wrong_hash = cursor.clone();
    wrong_hash.sha256 = "1".repeat(64);
    assert!(
        scan_input_log(
            &log,
            Retain::FromCursor {
                cursor: wrong_hash,
                time: 4
            }
        )
        .is_err(),
        "游标哈希不符应拒绝"
    );

    // 头段不是可提交段：游标指向 seq 0 直接拒绝。
    let header_cursor = kk_runtime::embed::LogCursor {
        last_seq: 0,
        last_tick: 0,
        sha256: full.chain[0].1.clone(),
    };
    assert!(
        scan_input_log(
            &log,
            Retain::FromCursor {
                cursor: header_cursor,
                time: 1
            }
        )
        .is_err(),
        "游标不得指向头段"
    );

    // —— 配对错误族经流式路径钉形：整链重封 + 手工 tick 异常 ——
    // 重封保持段身份/链衔接自洽，只改 tick，使错误确定归因配对检查。
    let text = std::fs::read_to_string(&log)?;
    let header_line = text.lines().next().unwrap().to_string();
    let header_sha = full.chain[0].1.clone();
    let build = |ticks: &[u64]| -> String {
        let mut prev = json!(header_sha);
        let mut out = header_line.clone();
        for (mut record, tick) in full
            .segments
            .iter()
            .map(|s| s.as_object().unwrap().clone())
            .zip(ticks)
        {
            record.insert("tick".into(), json!(tick));
            record.insert("prev_hash".into(), prev);
            let sealed = seal_line(record);
            prev = json!(
                serde_json::from_str::<Value>(&sealed).unwrap()["sha256"]
                    .as_str()
                    .unwrap()
                    .to_string()
            );
            out.push('\n');
            out.push_str(&sealed);
        }
        out.push('\n');
        out
    };
    // 从重建文本取指定段身份（第 0 行是头段，行号即 seq）。
    let cursor_at = |text: &str, seq: usize, tick: u64| -> kk_runtime::embed::LogCursor {
        let sha256 = text
            .lines()
            .nth(seq)
            .and_then(|line| serde_json::from_str::<Value>(line).ok())
            .and_then(|value| value["sha256"].as_str().map(str::to_string))
            .expect("重建日志行可解析");
        kk_runtime::embed::LogCursor {
            last_seq: seq as u64,
            last_tick: tick,
            sha256,
        }
    };
    let write_case = |name: &str, text: &str| -> PathBuf {
        let path = case.path(name);
        std::fs::write(&path, text).unwrap();
        path
    };

    // 控制组：重封不改 tick，流式配对通过（异常须归因 tick 而非重封本身）。
    let control = build(&[1, 2, 3, 4, 5]);
    let path = write_case("rechain-control.jsonl", &control);
    scan_input_log(
        &path,
        Retain::FromCursor {
            cursor: cursor_at(&control, 3, 3),
            time: 4,
        },
    )?;
    scan_input_log(&path, Retain::FromTick(4))?;

    // 游标段之后跳拍：后缀 tick 缺口。
    let gapped = build(&[1, 2, 3, 5, 6]);
    let path = write_case("rechain-gap.jsonl", &gapped);
    match scan_input_log(
        &path,
        Retain::FromCursor {
            cursor: cursor_at(&gapped, 3, 3),
            time: 4,
        },
    ) {
        Err(ScanError::Pairing(reason)) => {
            assert!(reason.contains("配对缺口或 tick 回退"), "{reason}");
        }
        other => panic!("游标后缀跳拍应报配对缺口：{other:?}"),
    }

    // 首个可回放段晚于检查点时间：起点配对缺口。
    let jumped = build(&[1, 2, 4, 4, 5]);
    let path = write_case("rechain-jump.jsonl", &jumped);
    match scan_input_log(&path, Retain::FromTick(3)) {
        Err(ScanError::Pairing(reason)) => {
            assert!(reason.contains("首个可回放段"), "{reason}");
        }
        other => panic!("起点跳拍应报配对缺口：{other:?}"),
    }

    // 选区内同 tick 重复：tick 回退。
    let regressed = build(&[1, 2, 3, 3, 4]);
    let path = write_case("rechain-regress.jsonl", &regressed);
    match scan_input_log(&path, Retain::FromTick(2)) {
        Err(ScanError::Pairing(reason)) => {
            assert!(reason.contains("tick 回退"), "{reason}");
        }
        other => panic!("选区内回退应报 tick 回退：{other:?}"),
    }
    Ok(())
}

/// truncate_after_cursor 直测：分支放弃的字节形态、游标即尾无操作、
/// 超尾/身份不符拒绝、陈旧临时文件的可读错误与重试、含空行日志的段号
/// 正确性，以及截断后续写全链校验。
pub(super) fn truncate_after_cursor_impl() -> anyhow::Result<()> {
    let case = Case::new("truncate");
    let (mut engine, log) = new_engine(&case, "input.jsonl")?;
    engine.enable_input_log(&log, json!({}))?;
    let code = "Memory.n = (Memory.n || 0) + 1; true";
    engine.run_players(&BTreeMap::from([("p1".to_string(), code.to_string())]))?;
    engine.tick(3)?;
    let mid_cursor = engine.input_log_cursor().unwrap();
    engine.tick(1)?;
    let tail_cursor = engine.input_log_cursor().unwrap();
    drop(engine);

    let original = std::fs::read_to_string(&log)?;
    let lines: Vec<&str> = original.lines().collect();
    assert_eq!(lines.len(), 6, "头段 + 玩家段 + 四个推进段");

    // 游标即日志尾：无操作，字节不变。
    truncate_after_cursor(&log, &tail_cursor)?;
    assert_eq!(std::fs::read_to_string(&log)?, original, "尾游标不改动文件");

    // 超出日志末段：拒绝。
    let beyond = kk_runtime::embed::LogCursor {
        last_seq: 9,
        last_tick: 9,
        sha256: "0".repeat(64),
    };
    let rejected = truncate_after_cursor(&log, &beyond).unwrap_err();
    assert!(
        rejected.to_string().contains("超出日志末段"),
        "超尾游标应拒绝：{rejected:#}"
    );

    // 游标身份不符（sha 属段 4、seq 写 3）：复制自证拒绝，日志不动。
    let mismatched = kk_runtime::embed::LogCursor {
        last_seq: 3,
        last_tick: 3,
        sha256: mid_cursor.sha256.clone(),
    };
    let rejected = truncate_after_cursor(&log, &mismatched).unwrap_err();
    assert!(
        rejected.to_string().contains("身份不匹配"),
        "身份不符应拒绝：{rejected:#}"
    );
    assert_eq!(std::fs::read_to_string(&log)?, original, "拒绝时不动日志");

    // 中途游标：保留头段+游标段，丢弃废弃分支，末行带换行。
    truncate_after_cursor(&log, &mid_cursor)?;
    let truncated = std::fs::read_to_string(&log)?;
    assert_eq!(truncated, format!("{}\n", lines[..5].join("\n")));
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 4, "保留四个数据段");
    assert_eq!(data.chain.last().unwrap().1, mid_cursor.sha256);
    assert_eq!(data.stats.last_tick, Some(mid_cursor.last_tick));

    // 截断后续写：新分片挂载已截断日志，段号/链从游标延续。
    let (mut engine2, _) = new_engine(&case, "unused.jsonl")?;
    engine2.enable_input_log(&log, json!({}))?;
    engine2.tick(1)?;
    engine2.teardown();
    let data = read_input_log(&log)?;
    assert_eq!(data.segments.len(), 5, "续写一段");
    assert_eq!(data.segments[4]["seq"], 5, "段号从游标+1 延续");

    // 陈旧截断临时文件（崩溃窗口遗留的孤儿）：预扫描通过后先摘除，
    // 截断一次成功——恢复路径不付出带误导性错误的失败尝试。
    let mut stale = log.clone().into_os_string();
    stale.push(".truncate-tmp");
    let stale = std::path::PathBuf::from(stale);
    std::fs::write(&stale, b"stale")?;
    truncate_after_cursor(&log, &mid_cursor)?;
    assert!(!stale.exists(), "孤儿临时文件被预清理摘除");
    let data = scan_input_log(&log, Retain::None)?;
    assert_eq!(data.final_seq, 4, "清理后截断照常生效");

    // 含空行日志（五段全量，截到段 4 走复制路径）：空白行原样保留在
    // 截断前缀中、不计段；段号不受影响。
    let blank_log = case.path("blank.jsonl");
    let blank_text = format!("{}\n", lines.join("\n\n"));
    std::fs::write(&blank_log, &blank_text)?;
    let cursor = kk_runtime::embed::LogCursor {
        last_seq: 4,
        last_tick: 4,
        sha256: mid_cursor.sha256.clone(),
    };
    truncate_after_cursor(&blank_log, &cursor)?;
    let truncated = std::fs::read_to_string(&blank_log)?;
    assert!(
        truncated.ends_with('\n') && truncated.matches("\n\n").count() >= 4,
        "前缀空白行原样保留且末行带换行：{truncated:?}"
    );
    let data = scan_input_log(&blank_log, Retain::None)?;
    assert_eq!(data.stats.segments, 4, "空行不改变段计数");
    assert_eq!(data.final_seq, 4);
    Ok(())
}

/// 相同 tick/seq 不能替代历史身份；游标选择后缀可以跳过旧分支前缀。
pub(super) fn cursor_and_header_pairing_impl() -> anyhow::Result<()> {
    let case = Case::new("cursor");
    // 元数据、种子、计划与操作次数相同，起始 Memory 不同：历史根必须不同。
    let (mut root_a, root_a_log) = new_engine(&case, "root-a.jsonl")?;
    let (mut root_b, root_b_log) = new_engine(&case, "root-b.jsonl")?;
    root_a.set_player_memory("p1", &json!({"root":"a"}))?;
    root_b.set_player_memory("p1", &json!({"root":"b"}))?;
    root_a.enable_input_log(&root_a_log, Value::Null)?;
    root_b.enable_input_log(&root_b_log, Value::Null)?;
    root_a.tick(1)?;
    root_b.tick(1)?;
    let root_a_data = read_input_log(&root_a_log)?;
    let root_b_data = read_input_log(&root_b_log)?;
    let mut a_identity = root_a_data.header["identity"].clone();
    let mut b_identity = root_b_data.header["identity"].clone();
    assert_ne!(
        a_identity
            .as_object_mut()
            .unwrap()
            .remove("initial_state_sha256"),
        b_identity
            .as_object_mut()
            .unwrap()
            .remove("initial_state_sha256")
    );
    assert_eq!(a_identity, b_identity);
    assert!(
        root_b_data
            .segments_after_cursor(2, &root_a.input_log_cursor().unwrap())
            .is_err()
    );
    let (mut engine, log) = new_engine(&case, "a.jsonl")?;
    engine.enable_input_log(&log, json!({"history":"a"}))?;
    engine.tick(2)?;
    let cursor = engine.input_log_cursor().unwrap();
    let saved = engine.checkpoint(Value::Null)?;
    let original = read_input_log(&log)?;
    assert!(original.segments_after_cursor(3, &cursor)?.is_empty());
    let (mut foreign, other_log) = new_engine(&case, "b.jsonl")?;
    foreign.enable_input_log(&other_log, json!({"history":"b"}))?;
    foreign.tick(2)?;
    assert!(
        read_input_log(&other_log)?
            .segments_after_cursor(3, &cursor)
            .is_err()
    );
    engine.tick(2)?;
    assert_eq!(
        read_input_log(&log)?
            .segments_after_cursor(3, &cursor)?
            .len(),
        2
    );
    let mut wrong_tick = cursor.clone();
    wrong_tick.last_tick = 0;
    assert!(original.segments_after_cursor(2, &wrong_tick).is_err());
    // 同一文件续接新分支：旧分支 0..3，新分支从 2 开始；选新分支保存的
    // 游标时，只消费其后缀，不在旧分支中重新按 tick 寻找起点。
    engine.restore_checkpoint(saved, None)?;
    engine.enable_input_log(&log, Value::Null)?;
    assert_eq!(read_input_log(&log)?.header, original.header);
    engine.tick(1)?;
    let branch_cursor = engine.input_log_cursor().unwrap();
    engine.tick(1)?;
    assert_eq!(
        read_input_log(&log)?
            .segments_after_cursor(4, &branch_cursor)?
            .len(),
        1
    );
    assert!(
        read_input_log(&log)?
            .segments_after_cursor(3, &cursor)
            .is_err()
    );
    drop(engine);
    let header_only = case.path("header-only.jsonl");
    let mut original = EmbedEngine::with_world_seed(42);
    original.create_shard(&json!({"players":[],"rooms":[]}))?;
    original.enable_input_log(&header_only, Value::Null)?;
    drop(original);
    let bytes = std::fs::read(&header_only)?;
    let mut wrong_seed = EmbedEngine::with_world_seed(43);
    wrong_seed.create_shard(&json!({"players":[],"rooms":[]}))?;
    let error = wrong_seed
        .enable_input_log(&header_only, Value::Null)
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<InputLogError>(),
        Some(InputLogError::HeaderIdentityMismatch { field: "seed" })
    ));
    assert_eq!(std::fs::read(&header_only)?, bytes);
    assert_eq!(wrong_seed.get_game_time(), 1, "创建含热身拍");
    // 重新封装摘要也不能让没有状态根的旧头段进入当前续接/回放协议。
    let mut missing_root = read_input_log(&header_only)?.header;
    missing_root["identity"]
        .as_object_mut()
        .unwrap()
        .remove("initial_state_sha256");
    assert!(kk_runtime::embed::assert_header_paired(&missing_root, 42).is_err());
    let bytes = format!("{}\n", seal_line(missing_root.as_object().unwrap().clone()));
    std::fs::write(&header_only, &bytes)?;
    let mut same_seed = EmbedEngine::with_world_seed(42);
    same_seed.create_shard(&json!({"players":[],"rooms":[]}))?;
    assert!(matches!(
        same_seed
            .enable_input_log(&header_only, Value::Null)
            .unwrap_err()
            .downcast_ref::<InputLogError>(),
        Some(InputLogError::HeaderIdentityMismatch {
            field: "initial_state_sha256"
        })
    ));
    assert_eq!(std::fs::read_to_string(&header_only)?, bytes);
    Ok(())
}
