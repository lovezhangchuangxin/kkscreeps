//! 持久输入与玩家结果日志（G25.a 协议）：每轮**成功提交后**追加一段
//! 可回放输入。段包含：轮身份（tick/generation/来源）、墙钟 now_ms、
//! 意图（玩家轮为沙箱原始 JSON、外部轮为 `IntentBatch::to_json` 规范
//! 形态）、实际消费的玩家写回（CPU 新桶整桶、RawMemory、段请求）与
//! cpu_billed_ms 佐证。失败/丢弃轮不记录；缓存轮重试按首次输入只记
//! 一次。诊断面（expr/console）不入日志——last_console 已随检查点。
//!
//! 文件协议：单文件 JSONL，逐段 fsync；行=JSON 对象（键序由
//! serde_json 的 BTreeMap 决定，写读两侧同序，摘要按「去掉 sha256 后
//! 整行再序列化」复算）。链式 prev_hash=上一行（含其 sha256）的
//! SHA-256；撕裂尾/坏段/缺段/乱序/身份字段缺失可检。写序在引擎提交
//! 之后：崩溃丢尾段（RPO=1 轮），检查点不得领先日志头（G25.b 配对
//! 规则）。回放的 CPU 口径：直接用记录的新桶，不从实测值重推。
//!
//! 契约见 docs/architecture/input-log.md；写入与校验读取在本模块，
//! 段重建与回放编排在 `embed/replay.rs`（G25.b）。

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub(crate) const LOG_FORMAT: &str = "kkscreeps-input-log";
pub(crate) const LOG_VERSION: u64 = 1;

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 一段的身份快照：段自描述，交换/篡改身份可被逐段发现。
fn identity_map(
    plan_hash: &str,
    strategy: Option<&str>,
    disabled: &BTreeMap<String, Value>,
) -> Value {
    json!({"plan_hash": plan_hash, "execution_strategy": strategy, "disabled_mods": disabled})
}

/// 单玩家的回放输入记录。
pub(crate) struct PlayerLogEntry<'a> {
    pub cpu_bucket: &'a kk_core::cpu::Bucket,
    pub cpu_billed_ms: Option<f64>,
    pub raw_memory: Option<&'a str>,
    pub segments_out: Option<&'a Value>,
}

pub(crate) struct InputLog {
    file: std::fs::File,
    seq: u64,
    prev_hash: String,
    segments_written: u64,
}

#[derive(Debug)]
pub enum InputLogError {
    Io(std::io::Error),
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
        let (file, seq, prev_hash, segments_written) = if path.exists() {
            let data = read_input_log(path)?;
            // 续接安全门：末行必须换行结尾。append 模式下缺换行会把新段
            // 拼进旧末行、静默毁掉整个日志；这是撕裂写的形态，先修复再续接。
            ensure_trailing_newline(path)?;
            let last = data.chain.last().ok_or(InputLogError::BadHeader {
                line: 0,
                reason: "空文件不能续接".into(),
            })?;
            let file = std::fs::OpenOptions::new().append(true).open(path)?;
            // chain 含 seq 0 的头段：既有数据段数 = len-1，下一段 seq = len。
            let existing = data.chain.len() as u64 - 1;
            (file, existing, last.1.clone(), existing)
        } else {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .append(true)
                .open(path)?;
            let (line, hash) = seal(header);
            write_and_sync(&mut file, &line)?;
            (file, 0, hash, 0)
        };
        Ok(Self {
            file,
            seq,
            prev_hash,
            segments_written,
        })
    }

    pub(crate) fn segments_written(&self) -> u64 {
        self.segments_written
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
            "raw_memory".into(),
            entry.raw_memory.map_or(Value::Null, |v| json!(v)),
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

/// 校验读取器：解析、逐段摘要与链校验、段号连续性与必需字段。
/// 头段（seq 0）校验格式/版本/身份形状；段的身份快照只查形状——
/// 与检查点的身份配对在 `embed/replay.rs` 的回放入口实施。
#[derive(Debug)]
pub struct InputLogData {
    pub header: Value,
    pub segments: Vec<Value>,
    /// (seq, sha256) 依段序；含头段（seq 0）。
    pub chain: Vec<(u64, String)>,
}

/// 以最后一个换行为界切出完整行前缀与尾块：撕裂写只可能影响尾块
/// （写序=行体、换行、fsync），前缀中的任何损坏都是真实损坏。
fn split_tail(bytes: &[u8]) -> (&[u8], Option<&[u8]>) {
    match bytes.iter().rposition(|&b| b == b'\n') {
        Some(at) => {
            let tail = &bytes[at + 1..];
            (&bytes[..at + 1], (!tail.is_empty()).then_some(tail))
        }
        None if bytes.is_empty() => (&[], None),
        None => (&[], Some(bytes)),
    }
}

pub fn read_input_log(path: &Path) -> Result<InputLogData, InputLogError> {
    let bytes = std::fs::read(path)?;
    let (body, tail) = split_tail(&bytes);
    // 前缀中的非法 UTF-8 是中段损坏（撕裂只可能在尾块），报实际行号。
    let text = String::from_utf8(body.to_vec()).map_err(|error| {
        let valid = error.utf8_error().valid_up_to();
        let line = body[..valid].iter().filter(|&&b| b == b'\n').count() + 1;
        InputLogError::BadLine { line }
    })?;
    // 尾块：空白忽略；非法 UTF-8（撕裂切进多字节字符）标记为撕裂，待
    // 前缀全部校验通过后按完整前缀段数上报；可解码则并入行序列当末行。
    let torn_tail_bytes = tail.is_some_and(|t| String::from_utf8(t.to_vec()).is_err());
    let mut lines: Vec<std::borrow::Cow<'_, str>> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(std::borrow::Cow::Borrowed)
        .collect();
    if let Some(tail_bytes) = tail
        && let Ok(t) = String::from_utf8(tail_bytes.to_vec())
        && !t.trim().is_empty()
    {
        lines.push(std::borrow::Cow::Owned(t));
    }
    let mut header: Option<Value> = None;
    let mut segments = Vec::new();
    let mut chain: Vec<(u64, String)> = Vec::new();
    let line_total = lines.len();
    let mut line_no = 0usize;
    for raw in lines {
        line_no += 1;
        let is_last = line_no == line_total;
        let value: Value = match serde_json::from_str(raw.as_ref()) {
            Ok(v) => v,
            Err(_) if is_last => {
                return Err(InputLogError::TornTail {
                    valid_segments: segments.len() as u64,
                });
            }
            Err(_) => return Err(InputLogError::BadLine { line: line_no }),
        };
        let object = value
            .as_object()
            .ok_or(InputLogError::BadLine { line: line_no })?
            .clone();
        let claimed = object
            .get("sha256")
            .and_then(Value::as_str)
            .map(str::to_string);
        let seq = object.get("seq").and_then(Value::as_u64);
        let bad = |reason: String| InputLogError::BadHeader {
            line: line_no,
            reason,
        };
        // 只有物理撕裂的三种签名（解析失败/摘要不符/缺 sha256）在末行
        // 收敛为 TornTail：截断不可能改写段号或 prev_hash，末行的
        // SeqGap/ChainBreak 等仍是具体损坏，保留具体变体。
        let fail_tail_or = |err: InputLogError| -> InputLogError {
            let tear = matches!(
                &err,
                InputLogError::BadLine { .. }
                    | InputLogError::HashMismatch { .. }
                    | InputLogError::MissingField {
                        field: "sha256",
                        ..
                    }
            );
            if is_last && tear {
                InputLogError::TornTail {
                    valid_segments: segments.len() as u64,
                }
            } else {
                err
            }
        };
        // 摘要复算：去掉 sha256 后整对象再序列化。
        let mut body = object.clone();
        body.remove("sha256");
        let computed = sha256_hex(Value::Object(body).to_string().as_bytes());
        let claimed_sha = match claimed {
            Some(sha) => sha,
            None => {
                let err = InputLogError::MissingField {
                    seq: seq.unwrap_or(0),
                    line: line_no,
                    field: "sha256",
                };
                return Err(if header.is_none() {
                    bad("头段缺少 sha256".into())
                } else {
                    fail_tail_or(err)
                });
            }
        };
        if computed != claimed_sha {
            let err = InputLogError::HashMismatch {
                seq: seq.unwrap_or(0),
                line: line_no,
            };
            return Err(if header.is_none() {
                bad("头段摘要不匹配".into())
            } else {
                fail_tail_or(err)
            });
        }
        if header.is_none() {
            if object.get("kind").and_then(Value::as_str) != Some("header") {
                return Err(bad("首行必须是头段".into()));
            }
            if object.get("format").and_then(Value::as_str) != Some(LOG_FORMAT) {
                return Err(bad(format!("格式名不符（预期 {LOG_FORMAT}）")));
            }
            let version = object
                .get("version")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            if version != LOG_VERSION {
                return Err(InputLogError::UnknownVersion {
                    line: line_no,
                    version,
                });
            }
            if object.get("seq").and_then(Value::as_u64) != Some(0) {
                return Err(bad("头段 seq 必须为 0".into()));
            }
            if !object.get("prev_hash").is_some_and(|v| v.is_null()) {
                return Err(bad("头段 prev_hash 必须为 null".into()));
            }
            if object.get("identity").is_none_or(|v| !v.is_object()) {
                return Err(bad("头段缺少 identity 对象".into()));
            }
            header = Some(Value::Object(object));
            chain.push((0, claimed_sha));
            continue;
        }
        // 段校验。
        let seq = seq.ok_or(InputLogError::MissingField {
            seq: 0,
            line: line_no,
            field: "seq",
        })?;
        let expected = chain.len() as u64;
        if seq != expected {
            let err = InputLogError::SeqGap {
                expected,
                found: seq,
                line: line_no,
            };
            return Err(fail_tail_or(err));
        }
        if object.get("format").and_then(Value::as_str) != Some(LOG_FORMAT) {
            let err = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "format",
            };
            return Err(fail_tail_or(err));
        }
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);
        if version != LOG_VERSION {
            return Err(InputLogError::UnknownVersion {
                line: line_no,
                version,
            });
        }
        if object.get("kind").and_then(Value::as_str) != Some("segment") {
            let err = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "kind=segment",
            };
            return Err(fail_tail_or(err));
        }
        let prev = object.get("prev_hash").and_then(Value::as_str);
        if prev != Some(chain.last().expect("头段在位").1.as_str()) {
            let err = InputLogError::ChainBreak { seq, line: line_no };
            return Err(fail_tail_or(err));
        }
        for field in ["tick", "generation", "now_ms"] {
            if !object.get(field).is_some_and(|v| v.is_u64()) {
                let err = InputLogError::MissingField {
                    seq,
                    line: line_no,
                    field,
                };
                return Err(fail_tail_or(err));
            }
        }
        let source = object.get("source").and_then(Value::as_str).unwrap_or("");
        if !matches!(source, "players" | "external" | "advance") {
            let err = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "source",
            };
            return Err(fail_tail_or(err));
        }
        if !object.get("identity").is_some_and(|v| v.is_object()) {
            let err = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "identity",
            };
            return Err(fail_tail_or(err));
        }
        for field in ["intents", "players"] {
            if !object.get(field).is_some_and(Value::is_object) {
                let err = InputLogError::MissingField {
                    seq,
                    line: line_no,
                    field,
                };
                return Err(fail_tail_or(err));
            }
        }
        chain.push((seq, claimed_sha));
        segments.push(Value::Object(object));
    }
    if torn_tail_bytes {
        // 前缀全部校验通过后才归因撕裂：携带完整前缀段数供截断恢复。
        return Err(InputLogError::TornTail {
            valid_segments: segments.len() as u64,
        });
    }
    let header = header.ok_or(InputLogError::BadHeader {
        line: 0,
        reason: "日志为空".into(),
    })?;
    Ok(InputLogData {
        header,
        segments,
        chain,
    })
}
