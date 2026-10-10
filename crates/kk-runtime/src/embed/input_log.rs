//! 持久输入与玩家结果日志（G25.a 协议，v2）：每轮**成功提交后**追加一段
//! 可回放输入。段包含：轮身份（tick/generation/来源）、墙钟 now_ms、
//! 意图（玩家轮为沙箱原始 JSON、外部轮为 `IntentBatch::to_json` 规范
//! 形态）、实际消费的玩家写回（CPU 新桶整桶、**RawMemory 的 sha256
//! 摘要**、段请求）与 cpu_billed_ms 佐证。v2 起玩家 Memory 整串不再
//! 入段——它是玩家输出回显而非结算输入，全量记录使日志体积随拍数
//! 无界增长（实测 30KB/拍中 Memory 占 25KB）；回放端 Memory 保持
//! 检查点值，核对以「终态 Memory 摘要 == 窗口内最后非空摘要」验证。
//! 失败/丢弃轮不记录；缓存轮重试按首次输入只记一次。诊断面
//! （expr/console）不入日志——last_console 已随检查点。
//!
//! 文件协议：单文件 JSONL，逐段 fsync；行=JSON 对象（键序由
//! serde_json 的 BTreeMap 决定，写读两侧同序，摘要按「去掉 sha256 后
//! 整行再序列化」复算）。链式 prev_hash=上一行（含其 sha256）的
//! SHA-256；撕裂尾/坏段/缺段/乱序/身份字段缺失可检。写序在引擎提交
//! 之后：崩溃丢尾段（RPO=1 轮），检查点不得领先日志头（G25.b 配对
//! 规则）。回放的 CPU 口径：直接用记录的新桶，不从实测值重推。
//!
//! 读取为**流式扫描**（v2，见 `embed/input_log_scan.rs`）：按保留策略
//! 只驻留所需后缀，长窗日志不再整读进内存。
//!
//! 契约见 docs/architecture/input-log.md；写入在本模块，校验读取与截断
//! 在 `embed/input_log_scan.rs`，段重建与回放编排在 `embed/replay.rs`（G25.b）。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::input_log_scan::{Retain, ScanError, scan_input_log};

pub(crate) const LOG_FORMAT: &str = "kkscreeps-input-log";
pub(crate) const LOG_VERSION: u64 = 2;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub(super) fn has_initial_state_digest(header: &Value) -> bool {
    header["identity"]["initial_state_sha256"]
        .as_str()
        .is_some_and(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// 一段的身份快照：段自描述，交换/篡改身份可被逐段发现。
fn identity_map(
    plan_hash: &str,
    strategy: Option<&str>,
    disabled: &BTreeMap<String, Value>,
) -> Value {
    json!({"plan_hash": plan_hash, "execution_strategy": strategy, "disabled_mods": disabled})
}

/// 单玩家的回放输入记录。v2：Memory 只携带摘要（值本身不入段）；
/// 摘要在构造点计算，字段持有所有权避免临时借用。
pub(crate) struct PlayerLogEntry<'a> {
    pub cpu_bucket: &'a kk_core::cpu::Bucket,
    pub cpu_billed_ms: Option<f64>,
    pub raw_memory_sha256: Option<String>,
    pub ism_local: Option<&'a str>,
    pub segments_out: Option<&'a Value>,
}

pub(crate) struct InputLog {
    file: std::fs::File,
    seq: u64,
    prev_hash: String,
    segments_written: u64,
    last_tick: Option<u64>,
}

/// 检查点与日志历史的配对凭据；哈希绑定此前的整条链，不能仅凭 tick 配对。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogCursor {
    pub last_seq: u64,
    pub last_tick: u64,
    pub sha256: String,
}

/// 世界已提交但耐久日志写入失败；调用方可下转识别，不能重新结算此轮。
#[derive(Debug)]
pub struct CommittedLogError {
    pub committed_time: u64,
    pub reason: String,
}
impl std::fmt::Display for CommittedLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "输入日志写入失败，日志已停用；本轮结算已提交、不得重试（世界时间 {}）：{}",
            self.committed_time, self.reason
        )
    }
}
impl std::error::Error for CommittedLogError {}

#[derive(Debug)]
pub enum InputLogError {
    Io(std::io::Error),
    HeaderIdentityMismatch {
        field: &'static str,
    },
    /// 行不是合法 JSON（含撕裂尾：最后一行不完整）。
    BadLine {
        line: usize,
    },
    /// 头段缺失/重复/形状不符。
    BadHeader {
        line: usize,
        reason: String,
    },
    UnknownVersion {
        line: usize,
        version: u64,
    },
    SeqGap {
        expected: u64,
        found: u64,
        line: usize,
    },
    HashMismatch {
        seq: u64,
        line: usize,
    },
    ChainBreak {
        seq: u64,
        line: usize,
    },
    MissingField {
        seq: u64,
        line: usize,
        field: &'static str,
    },
    /// 最后一行不完整或校验失败：前缀有效段数随错误携带。
    TornTail {
        valid_segments: u64,
    },
}

impl std::fmt::Display for InputLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "输入日志 I/O 错误：{e}"),
            Self::HeaderIdentityMismatch { field } => {
                write!(f, "输入日志头段身份不符：{field}；拒绝续接")
            }
            Self::BadLine { line } => write!(f, "输入日志第 {line} 行不是合法 JSON"),
            Self::BadHeader { line, reason } => {
                write!(f, "输入日志第 {line} 行头段非法：{reason}")
            }
            Self::UnknownVersion { line, version } => {
                write!(
                    f,
                    "输入日志第 {line} 行版本 {version} 不受支持（当前 {LOG_VERSION}；无迁移）"
                )
            }
            Self::SeqGap {
                expected,
                found,
                line,
            } => write!(
                f,
                "输入日志第 {line} 行段号断链：预期 {expected}，实际 {found}"
            ),
            Self::HashMismatch { seq, line } => {
                write!(
                    f,
                    "输入日志第 {line} 行（段 {seq}）摘要不匹配：载荷被改动或损坏"
                )
            }
            Self::ChainBreak { seq, line } => write!(
                f,
                "输入日志第 {line} 行（段 {seq}）链哈希断链：与前段摘要不衔接"
            ),
            Self::MissingField { seq, line, field } => {
                write!(f, "输入日志第 {line} 行（段 {seq}）缺少必需字段 {field}")
            }
            Self::TornTail { valid_segments } => write!(
                f,
                "输入日志尾段不完整（撕裂或损坏）：前缀 {valid_segments} 个有效段"
            ),
        }
    }
}

impl std::error::Error for InputLogError {}

impl From<std::io::Error> for InputLogError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl InputLog {
    /// 新建日志（文件必须不存在）或在校验通过后续接既有日志。
    /// 续接要求既有日志完整无撕裂：从断链处续写会静默丢弃可回放历史。
    pub(crate) fn open(path: &Path, header: Map<String, Value>) -> Result<Self, InputLogError> {
        let (file, seq, prev_hash, segments_written, last_tick) = if path.exists() {
            // 续接只驻留校验状态：v2 起整读改流式扫描，长历史不再整驻
            // 内存。链校验/撕裂检测覆盖全部行，与整读版同强。
            let data = scan_input_log(path, Retain::None).map_err(|error| match error {
                ScanError::Format(error) => error,
                // None 保留不产生配对失败；防御性归并回格式族。
                ScanError::Pairing(reason) => InputLogError::BadHeader { line: 0, reason },
            })?;
            for field in ["seed", "engine_version", "host_protocol"] {
                if data.header["identity"][field].is_null()
                    || data.header["identity"][field] != header["identity"][field]
                {
                    return Err(InputLogError::HeaderIdentityMismatch { field });
                }
            }
            if !has_initial_state_digest(&data.header) {
                return Err(InputLogError::HeaderIdentityMismatch {
                    field: "initial_state_sha256",
                });
            }
            // 续接安全门：末行必须换行结尾。append 模式下缺换行会把新段
            // 拼进旧末行、静默毁掉整个日志；这是撕裂写的形态，先修复再续接。
            ensure_trailing_newline(path)?;
            // 头段 seq 为 0、数据段自 1 连续：链尾 seq 即既有数据段数，
            // 下一段 seq = final_seq + 1。
            let existing = data.final_seq;
            let file = std::fs::OpenOptions::new().append(true).open(path)?;
            (
                file,
                existing,
                data.final_hash,
                existing,
                data.stats.last_tick,
            )
        } else {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(path)?;
            let (line, hash) = seal(header);
            write_and_sync(&mut file, &line)?;
            (file, 0, hash, 0, None)
        };
        Ok(Self {
            file,
            seq,
            prev_hash,
            segments_written,
            last_tick,
        })
    }

    pub(crate) fn segments_written(&self) -> u64 {
        self.segments_written
    }

    pub(crate) fn cursor(&self) -> Option<LogCursor> {
        Some(LogCursor {
            last_seq: self.seq,
            last_tick: self.last_tick?,
            sha256: self.prev_hash.clone(),
        })
    }

    /// 追加一段结算输入记录；调用点必须在引擎提交成功之后。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_round(
        &mut self,
        tick: u64,
        generation: u64,
        source: &str,
        now_ms: u64,
        intents: Value,
        players: Value,
        processor_random: Option<&[f64]>,
        plan_hash: &str,
        strategy: Option<&str>,
        disabled: &BTreeMap<String, Value>,
    ) -> Result<(), InputLogError> {
        let mut record = Map::new();
        record.insert("format".into(), json!(LOG_FORMAT));
        record.insert("version".into(), json!(LOG_VERSION));
        record.insert("seq".into(), json!(self.seq + 1));
        record.insert("kind".into(), json!("segment"));
        record.insert("tick".into(), json!(tick));
        record.insert("generation".into(), json!(generation));
        record.insert("source".into(), json!(source));
        record.insert("now_ms".into(), json!(now_ms));
        record.insert("intents".into(), intents);
        record.insert("players".into(), players);
        if let Some(values) = processor_random {
            record.insert("processor_random".into(), json!(values));
        }
        record.insert(
            "identity".into(),
            identity_map(plan_hash, strategy, disabled),
        );
        record.insert("prev_hash".into(), json!(self.prev_hash));
        let (line, hash) = seal(record);
        write_and_sync(&mut self.file, &line)?;
        self.seq += 1;
        self.prev_hash = hash;
        self.segments_written += 1;
        self.last_tick = Some(tick);
        Ok(())
    }
}

/// 组装玩家表：CPU 新桶是回放直接消费的结算输入，cpu_billed_ms 只是
/// 实测佐证（跨进程不可复现，不从它重推桶值）。
pub(crate) fn players_map<'a>(entries: &BTreeMap<&str, PlayerLogEntry<'a>>) -> Value {
    let mut players = Map::new();
    for (name, entry) in entries {
        let mut m = Map::new();
        m.insert(
            "cpu_bucket".into(),
            serde_json::to_value(entry.cpu_bucket).unwrap_or(Value::Null),
        );
        m.insert(
            "cpu_billed_ms".into(),
            entry.cpu_billed_ms.map_or(Value::Null, |v| json!(v)),
        );
        m.insert(
            "raw_memory_sha256".into(),
            entry
                .raw_memory_sha256
                .as_deref()
                .map_or(Value::Null, |v| json!(v)),
        );
        m.insert(
            "ism_local".into(),
            entry.ism_local.map_or(Value::Null, |v| json!(v)),
        );
        m.insert(
            "segments".into(),
            entry.segments_out.cloned().unwrap_or(Value::Null),
        );
        players.insert((*name).to_string(), Value::Object(m));
    }
    Value::Object(players)
}

/// 校验文件以换行结尾；缺换行说明末行未完整落盘（append 会拼接毁日志）。
fn ensure_trailing_newline(path: &Path) -> Result<(), InputLogError> {
    use std::io::{Read, Seek};
    let mut file = std::fs::File::open(path)?;
    let mut last = [0u8];
    let readable = file
        .seek(std::io::SeekFrom::End(-1))
        .and_then(|_| file.read_exact(&mut last))
        .is_ok();
    if readable && last[0] != b'\n' {
        return Err(InputLogError::BadHeader {
            line: 0,
            reason: "末行缺换行（撕裂写形态）：截断到上一完整段或补齐换行后才能续接".into(),
        });
    }
    Ok(())
}

fn seal(mut record: Map<String, Value>) -> (String, String) {
    let body = Value::Object(record.clone()).to_string();
    let hash = sha256_hex(body.as_bytes());
    record.insert("sha256".into(), json!(hash));
    (Value::Object(record).to_string(), hash)
}

fn write_and_sync(file: &mut std::fs::File, line: &str) -> Result<(), InputLogError> {
    // 撕裂面：半行写入在崩溃后由读取器的 TornTail 分支检出。
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
