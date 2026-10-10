//! 结算回放内核（G25.b）：把输入日志段重建成一轮结算的完整输入
//! （意图批、记录墙钟、玩家写回），并实施检查点与日志的配对规则。
//! 回放**不重跑玩家沙箱**——意图与写回是记录的输入（`from_json` 重走
//! 清洗证明清洗规则无漂移；CPU 直接消费记录的新桶，ADR-013 决策 5），
//! 也不写新日志段（链只属于产生它的那轮）。
//!
//! 契约见 docs/architecture/input-log.md 的回放节。

use std::collections::BTreeMap;

use anyhow::Context as _;
use serde_json::Value;

use kk_engine::intent::IntentBatch;
use kk_engine::{PlayerWriteback, SegmentWriteback, TickInput};

/// 重建出的一轮回放输入。
pub struct ReplayRound {
    pub batch: IntentBatch,
    pub input: TickInput,
    pub writebacks: BTreeMap<String, PlayerWriteback>,
    pub processor_random: Option<Vec<f64>>,
}

/// 配对规则：选出从检查点（世界时间 `checkpoint_time`）起可回放的段。
///
/// 被跳过的前缀必须整段早于检查点时间；首个不早于它的段必须恰好等于
/// 检查点时间——更晚即为配对缺口（日志曾在中途被摘除后从新分片续接，
/// 世界连续性断裂，静默跳段会重放出错误的历史）。没有任何覆盖段时
/// 报错（检查点在日志末尾之后，无从重放）。
pub fn segments_from_checkpoint(
    checkpoint_time: u64,
    segments: &[Value],
) -> anyhow::Result<&[Value]> {
    let mut start = segments.len();
    for (index, segment) in segments.iter().enumerate() {
        let tick = segment["tick"].as_u64().context("段缺少 tick 字段")?;
        if tick >= checkpoint_time {
            start = index;
            break;
        }
    }
    if start == segments.len() {
        anyhow::bail!(
            "日志不覆盖检查点时间 {checkpoint_time} 之后的轮次（全部 {} 段均不晚于它）",
            segments.len()
        );
    }
    let first = segments[start]["tick"].as_u64().expect("上面已读过");
    anyhow::ensure!(
        first == checkpoint_time,
        "配对缺口：首个可回放段 tick={first} 晚于检查点时间 {checkpoint_time}（日志曾摘除后从新分片续接，世界连续性断裂）——需使用与日志连续的检查点"
    );
    // 选区内 tick 必须严格递增：日志允许从新分片续接（G25.a 契约），
    // 但回放配对选中了跨越接缝的区间时，在选区起点就报真实原因，
    // 不等逐段 tick 硬门在接缝处误报「宿主变异或日志损坏」。
    for pair in segments[start..].windows(2) {
        let (prev, next) = (
            pair[0]["tick"].as_u64().context("段缺少 tick 字段")?,
            pair[1]["tick"].as_u64().context("段缺少 tick 字段")?,
        );
        anyhow::ensure!(
            next > prev,
            "选区内 tick 回退（{prev}→{next}）：日志在世界回退后被续接，跨接缝不可回放——用与新分支连续的检查点，或从接缝后的检查点重新配对"
        );
    }
    Ok(&segments[start..])
}

/// 头段身份与回放引擎配对：种子是结算期玩法随机流的派生根（不在
/// plan_hash 里），引擎版本承载协议实现——错配的日志可在逐段门全过
/// 的情况下重放出错误的抽值历史，必须在入口拒绝。
pub fn assert_header_paired(header: &Value, seed: u64) -> anyhow::Result<()> {
    let identity = header["identity"]
        .as_object()
        .context("头段缺少 identity 对象")?;
    anyhow::ensure!(
        identity.get("host_protocol").and_then(Value::as_str)
            == Some(kk_mod_host::GAMEPLAY_PROTOCOL),
        "日志宿主玩法协议不符；旧日志没有自动迁移"
    );
    anyhow::ensure!(
        super::input_log::has_initial_state_digest(header),
        "日志缺少有效起始状态摘要；旧日志没有自动迁移"
    );
    anyhow::ensure!(
        identity.get("seed").and_then(Value::as_u64) == Some(seed),
        "日志头段种子与引擎不符（抽值流将整体错误）"
    );
    anyhow::ensure!(
        identity.get("engine_version").and_then(Value::as_str) == Some(env!("CARGO_PKG_VERSION")),
        "日志头段引擎版本 {} 与当前 {} 不符（日志协议实现不同代）",
        identity
            .get("engine_version")
            .and_then(Value::as_str)
            .unwrap_or("?"),
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}

/// 段级身份快照与引擎当前身份对照：plan_hash/执行策略/停用集任一
/// 不符即拒（配错日志或包集合）。
pub(crate) fn assert_segment_identity(
    segment: &Value,
    plan_hash: &str,
    strategy: Option<&str>,
    disabled: &BTreeMap<String, Value>,
) -> anyhow::Result<()> {
    let identity = segment["identity"]
        .as_object()
        .context("段缺少 identity 对象")?;
    anyhow::ensure!(
        identity.get("plan_hash").and_then(Value::as_str) == Some(plan_hash),
        "plan_hash 不符（日志属另一包计划）"
    );
    let segment_strategy = identity.get("execution_strategy").and_then(Value::as_str);
    anyhow::ensure!(
        segment_strategy == strategy,
        "执行策略身份不符（日志属另一执行策略）"
    );
    let segment_disabled = identity
        .get("disabled_mods")
        .cloned()
        .unwrap_or(Value::Null);
    anyhow::ensure!(
        segment_disabled
            == Value::Object(
                disabled
                    .iter()
                    .map(|(id, value)| (id.clone(), value.clone()))
                    .collect()
            ),
        "停用集不符（日志记录时存在现已不同的停用）"
    );
    Ok(())
}

/// 把一段日志重建成一轮结算的输入。形状由 G25.a 写入器钉死、读取器
/// 校验：players 轮 intents 为 {句柄: 沙箱原始报告} 且 players 表键集
/// 与之相同；external 轮为 to_json 的玩家键规范形态、无写回；advance
/// 轮两者皆空对象。
pub fn rebuild_round(segment: &Value) -> anyhow::Result<ReplayRound> {
    let input = TickInput {
        warmup: false,
        now_ms: segment["now_ms"].as_u64().context("段缺少 now_ms 字段")?,
    };
    let source = segment["source"].as_str().context("段缺少 source 字段")?;
    let intents = segment["intents"]
        .as_object()
        .context("段缺少 intents 对象")?;
    let players = segment["players"]
        .as_object()
        .context("段缺少 players 对象")?;
    let mut batch = IntentBatch::default();
    let mut writebacks = BTreeMap::new();
    let processor_random = match segment.get("processor_random") {
        None => None,
        Some(Value::Array(values)) if source == "advance" => Some(
            values
                .iter()
                .map(|value| {
                    let number = value.as_f64().context("processor_random 元素须为数")?;
                    anyhow::ensure!(
                        (0.0..1.0).contains(&number),
                        "processor_random 值须在 [0,1)"
                    );
                    Ok(number)
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        ),
        _ => anyhow::bail!("processor_random 只允许在 advance 段中作为数组"),
    };
    match source {
        "players" => {
            // 写回键集必须与意图句柄集一致（run_players 对每个请求玩家
            // 都同时落两者）；不一致说明段被拼改过。
            anyhow::ensure!(
                intents.keys().eq(players.keys()),
                "players 段的 intents 句柄集与 players 键集不一致"
            );
            for (handle, report) in intents {
                // extend_from 非幂等（room/notify/market 列表会翻倍）：
                // 每句柄只 from_json 一次再合并。
                batch.extend_from(IntentBatch::from_json(handle, report));
                writebacks.insert(handle.clone(), rebuild_writeback(&players[handle])?);
            }
        }
        "external" => {
            for (player, wire) in intents {
                batch.extend_from(IntentBatch::from_json(player, wire));
            }
        }
        "advance" => {}
        other => anyhow::bail!("未知段来源 {other}"),
    }
    Ok(ReplayRound {
        batch,
        input,
        writebacks,
        processor_random,
    })
}

/// 玩家写回复建：CPU 新桶整桶反序列化（不从 cpu_billed_ms 重推——那只是
/// 实测佐证）；raw_memory 的 null/Some 区分保真（None=结算端保留旧值）；
/// segments 重放 `from_runner`（日志存的就是它的原始输入，纯函数过滤，
/// 同函数重放即逐位等价，无需逆转换）。
fn rebuild_writeback(entry: &Value) -> anyhow::Result<PlayerWriteback> {
    anyhow::ensure!(
        entry["cpu_bucket"].is_object(),
        "玩家条目缺少 cpu_bucket 对象"
    );
    let bucket: kk_core::cpu::Bucket = serde_json::from_value(entry["cpu_bucket"].clone())
        .context("cpu_bucket 反序列化失败（日志版本或桶形状不符）")?;
    let raw_memory = match &entry["raw_memory"] {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => anyhow::bail!("raw_memory 必须是字符串或 null：{other}"),
    };
    let segments = match &entry["segments"] {
        Value::Null => None,
        value => Some(SegmentWriteback::from_runner(value)),
    };
    // 旧日志无 ism_local 键（特性前不产生写入）——缺失按 None。
    let ism_local = match entry.get("ism_local") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        other => anyhow::bail!("ism_local 必须是字符串或 null：{other:?}"),
    };
    Ok(PlayerWriteback {
        cpu: Some(bucket),
        raw_memory,
        ism_local,
        segments,
    })
}
