//! 输入日志的读取域（v2 流式扫描）：逐行解析、增量链校验、按保留策略
//! 驻留后缀，以及目录恢复用的游标处原子截断。写入器、文件协议常量与
//! 错误族在 `embed/input_log.rs`；撕裂语义、保留策略与配对规则的完整
//! 契约见 docs/architecture/input-log.md。

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde_json::Value;

use super::input_log::{InputLogError, LOG_FORMAT, LOG_VERSION, LogCursor, sha256_hex};

/// 校验读取器：解析、逐段摘要与链校验、段号连续性与必需字段。
/// 头段（seq 0）校验格式/版本/身份形状；段的身份快照只查形状——
/// 与检查点的身份配对在 `embed/replay.rs` 的回放入口实施。
/// v2 流式扫描：`stats`/`final_seq`/`final_hash` 是全量对账与链尾
/// （与保留策略无关）；`segments`/`chain` 只含保留策略选中的段。
#[derive(Debug)]
pub struct InputLogData {
    pub header: Value,
    pub segments: Vec<Value>,
    /// (seq, sha256) 依段序；含头段（seq 0）；仅保留段入链。
    pub chain: Vec<(u64, String)>,
    pub stats: ScanStats,
    /// 最后一个通过校验的段的 seq（仅头段时为 0）；续接下一段为 +1。
    pub final_seq: u64,
    /// 最后一个通过校验的段的 sha256（含头段回退）；续接的 prev_hash。
    pub final_hash: String,
}

/// 流式扫描的全量对账统计：零保留时也能报告段数与首末 tick。
#[derive(Debug, Default)]
pub struct ScanStats {
    pub segments: u64,
    pub first_tick: Option<u64>,
    pub last_tick: Option<u64>,
}

/// 保留策略：决定扫描时驻留哪些段（链/摘要/字段校验始终覆盖全部行）。
pub enum Retain {
    /// 零保留：续接与运行后摘要只要对账统计与链尾。
    None,
    /// 全段保留：整读校验（测试与全量诊断）。
    All,
    /// 检查点游标配对（等价旧 `segments_after_cursor`）：游标命中的段
    /// 须摘要与 tick 双验证（其后缀才可回放），保留其后缀且逐段 tick
    /// 自 `time` 起连续 +1。
    FromCursor { cursor: LogCursor, time: u64 },
    /// 起点快照无游标时的配对（等价旧 `segments_from_checkpoint`）：
    /// 首个不早于 `tick` 的段必须恰等于它，其后缀保留且 tick 严格递增。
    FromTick(u64),
}

/// 扫描错误：格式族沿用 `InputLogError`；配对族是流式化后从后缀选择
/// 移入扫描的检查点配对失败。
#[derive(Debug)]
pub enum ScanError {
    Format(InputLogError),
    Pairing(String),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(error) => write!(f, "{error}"),
            Self::Pairing(message) => write!(f, "输入日志检查点配对失败：{message}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl From<InputLogError> for ScanError {
    fn from(error: InputLogError) -> Self {
        Self::Format(error)
    }
}

impl From<std::io::Error> for ScanError {
    fn from(error: std::io::Error) -> Self {
        Self::Format(InputLogError::Io(error))
    }
}

impl InputLogData {
    /// 验证游标的历史身份与世界时间后选择后缀；零段也必须通过同一配对门。
    /// 仅对 [`Retain::All`] 整读结果有效——流式扫描按保留策略裁剪 chain
    /// 与 segments，此处按 seq 建索引会错位（运行期消费方走
    /// `scan_input_log` 的 `FromCursor` 配对）。
    pub fn segments_after_cursor(&self, time: u64, cursor: &LogCursor) -> anyhow::Result<&[Value]> {
        use anyhow::Context as _;
        let index = usize::try_from(cursor.last_seq).context("日志游标 seq 超出本机范围")?;
        let (seq, hash) = self.chain.get(index).context("日志不覆盖检查点游标")?;
        anyhow::ensure!(
            *seq == cursor.last_seq && hash == &cursor.sha256,
            "检查点 log_cursor 历史哈希不匹配"
        );
        let preceding = index
            .checked_sub(1)
            .and_then(|index| self.segments.get(index))
            .context("检查点日志游标必须指向已提交段")?;
        anyhow::ensure!(
            preceding["tick"].as_u64() == Some(cursor.last_tick)
                && cursor.last_tick.checked_add(1) == Some(time),
            "检查点时间与日志游标不匹配"
        );
        let suffix = &self.segments[index..];
        let mut expected = time;
        for segment in suffix {
            anyhow::ensure!(
                segment["tick"].as_u64() == Some(expected),
                "配对缺口或 tick 回退：预期 {expected}，实际 {}",
                segment["tick"]
            );
            expected = expected.checked_add(1).context("回放时间溢出")?;
        }
        Ok(suffix)
    }
}

/// 整读校验入口（全段保留）：测试与全量诊断用；运行期消费方一律走
/// `scan_input_log` 流式扫描，避免长窗日志整驻内存。
pub fn read_input_log(path: &Path) -> Result<InputLogData, InputLogError> {
    match scan_input_log(path, Retain::All) {
        Ok(data) => Ok(data),
        Err(ScanError::Format(error)) => Err(error),
        // All 保留不触发配对检查；防御性归并为格式族错误。
        Err(ScanError::Pairing(reason)) => Err(InputLogError::BadHeader { line: 0, reason }),
    }
}

/// 流式扫描的逐行可变状态。
struct ScanState {
    header: Option<Value>,
    segments: Vec<Value>,
    chain: Vec<(u64, String)>,
    stats: ScanStats,
    last_seq: u64,
    last_sha: String,
    line_no: usize,
    // FromCursor 配对状态。
    cursor_matched: bool,
    cursor_next_tick: Option<u64>,
    // FromTick 配对状态。
    tick_matched: bool,
    last_retained_tick: Option<u64>,
}

impl ScanState {
    fn new() -> Self {
        Self {
            header: None,
            segments: Vec::new(),
            chain: Vec::new(),
            stats: ScanStats::default(),
            last_seq: 0,
            last_sha: String::new(),
            line_no: 0,
            cursor_matched: false,
            cursor_next_tick: None,
            tick_matched: false,
            last_retained_tick: None,
        }
    }

    /// 校验并按保留策略驻留一行。`is_last` 为真时三种物理撕裂签名
    /// （解析失败/摘要不符/缺 sha256）收敛为 TornTail；截断不可能改写
    /// 段号或 prev_hash，末行的 SeqGap/ChainBreak 等保留具体变体。
    fn process_line(
        &mut self,
        bytes: &[u8],
        is_last: bool,
        retain: &Retain,
    ) -> Result<(), ScanError> {
        // 中段的非法 UTF-8 是真实损坏（撕裂只可能在缺换行的尾块）。
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text.strip_suffix('\n').unwrap_or(text),
            Err(_) => {
                return Err(ScanError::Format(InputLogError::BadLine {
                    line: self.line_no + 1,
                }));
            }
        };
        if text.trim().is_empty() {
            return Ok(());
        }
        self.line_no += 1;
        let line_no = self.line_no;
        let valid_segments = self.stats.segments;
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) if is_last => {
                return Err(ScanError::Format(InputLogError::TornTail {
                    valid_segments,
                }));
            }
            Err(_) => {
                return Err(ScanError::Format(InputLogError::BadLine { line: line_no }));
            }
        };
        let object = value
            .as_object()
            .ok_or(ScanError::Format(InputLogError::BadLine { line: line_no }))?
            .clone();
        let claimed = object
            .get("sha256")
            .and_then(Value::as_str)
            .map(str::to_string);
        let seq = object.get("seq").and_then(Value::as_u64);
        let bad = |reason: String| {
            ScanError::Format(InputLogError::BadHeader {
                line: line_no,
                reason,
            })
        };
        let fail_tail_or = |error: InputLogError| -> ScanError {
            let tear = matches!(
                &error,
                InputLogError::BadLine { .. }
                    | InputLogError::HashMismatch { .. }
                    | InputLogError::MissingField {
                        field: "sha256",
                        ..
                    }
            );
            if is_last && tear {
                ScanError::Format(InputLogError::TornTail { valid_segments })
            } else {
                ScanError::Format(error)
            }
        };
        // 摘要复算：去掉 sha256 后整对象再序列化。
        let mut body = object.clone();
        body.remove("sha256");
        let computed = sha256_hex(Value::Object(body).to_string().as_bytes());
        let claimed_sha = match claimed {
            Some(sha) => sha,
            None => {
                let error = InputLogError::MissingField {
                    seq: seq.unwrap_or(0),
                    line: line_no,
                    field: "sha256",
                };
                return Err(if self.header.is_none() {
                    bad("头段缺少 sha256".into())
                } else {
                    fail_tail_or(error)
                });
            }
        };
        if computed != claimed_sha {
            let error = InputLogError::HashMismatch {
                seq: seq.unwrap_or(0),
                line: line_no,
            };
            return Err(if self.header.is_none() {
                bad("头段摘要不匹配".into())
            } else {
                fail_tail_or(error)
            });
        }
        let mut retain_segment = false;
        if self.header.is_none() {
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
                return Err(ScanError::Format(InputLogError::UnknownVersion {
                    line: line_no,
                    version,
                }));
            }
            if seq != Some(0) {
                return Err(bad("头段 seq 必须为 0".into()));
            }
            if !object.get("prev_hash").is_some_and(|v| v.is_null()) {
                return Err(bad("头段 prev_hash 必须为 null".into()));
            }
            if object.get("identity").is_none_or(|v| !v.is_object()) {
                return Err(bad("头段缺少 identity 对象".into()));
            }
            self.header = Some(Value::Object(object));
            self.chain.push((0, claimed_sha.clone()));
            self.last_seq = 0;
            self.last_sha = claimed_sha;
            return Ok(());
        }
        // 段校验。链尾与段号连续性以扫描游标（last_sha/last_seq）校验，
        // 与整读版的 chain 末项同源。
        let seq = seq.ok_or(ScanError::Format(InputLogError::MissingField {
            seq: 0,
            line: line_no,
            field: "seq",
        }))?;
        let expected = self.last_seq + 1;
        if seq != expected {
            let error = InputLogError::SeqGap {
                expected,
                found: seq,
                line: line_no,
            };
            return Err(fail_tail_or(error));
        }
        if object.get("format").and_then(Value::as_str) != Some(LOG_FORMAT) {
            let error = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "format",
            };
            return Err(fail_tail_or(error));
        }
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);
        if version != LOG_VERSION {
            return Err(ScanError::Format(InputLogError::UnknownVersion {
                line: line_no,
                version,
            }));
        }
        if object.get("kind").and_then(Value::as_str) != Some("segment") {
            let error = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "kind=segment",
            };
            return Err(fail_tail_or(error));
        }
        let prev = object.get("prev_hash").and_then(Value::as_str);
        if prev != Some(self.last_sha.as_str()) {
            let error = InputLogError::ChainBreak { seq, line: line_no };
            return Err(fail_tail_or(error));
        }
        for field in ["tick", "generation", "now_ms"] {
            if !object.get(field).is_some_and(|v| v.is_u64()) {
                let error = InputLogError::MissingField {
                    seq,
                    line: line_no,
                    field,
                };
                return Err(fail_tail_or(error));
            }
        }
        let source = object.get("source").and_then(Value::as_str).unwrap_or("");
        if !matches!(source, "players" | "external" | "advance") {
            let error = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "source",
            };
            return Err(fail_tail_or(error));
        }
        if !object.get("identity").is_some_and(|v| v.is_object()) {
            let error = InputLogError::MissingField {
                seq,
                line: line_no,
                field: "identity",
            };
            return Err(fail_tail_or(error));
        }
        for field in ["intents", "players"] {
            if !object.get(field).is_some_and(Value::is_object) {
                let error = InputLogError::MissingField {
                    seq,
                    line: line_no,
                    field,
                };
                return Err(fail_tail_or(error));
            }
        }
        let tick = object["tick"].as_u64().expect("上方已校验 u64");
        // 检查点配对（后缀选择从整读后移入扫描）。
        match retain {
            Retain::All => retain_segment = true,
            Retain::None => {}
            Retain::FromCursor { cursor, time } => {
                if seq == cursor.last_seq {
                    if claimed_sha != cursor.sha256 {
                        return Err(ScanError::Pairing(
                            "检查点 log_cursor 历史哈希不匹配".into(),
                        ));
                    }
                    if tick != cursor.last_tick || cursor.last_tick.checked_add(1) != Some(*time) {
                        return Err(ScanError::Pairing("检查点时间与日志游标不匹配".into()));
                    }
                    self.cursor_matched = true;
                    // 游标段本身不重放：检查点已含其结算产物，后缀自下一段起。
                    self.cursor_next_tick = Some(*time);
                } else if self.cursor_matched {
                    let expected_tick = self.cursor_next_tick.expect("命中游标时已置期望 tick");
                    if tick != expected_tick {
                        return Err(ScanError::Pairing(format!(
                            "配对缺口或 tick 回退：预期 {expected_tick}，实际 {tick}"
                        )));
                    }
                    self.cursor_next_tick = Some(
                        expected_tick
                            .checked_add(1)
                            .ok_or_else(|| ScanError::Pairing("回放时间溢出".into()))?,
                    );
                    retain_segment = true;
                }
            }
            Retain::FromTick(from) => {
                if !self.tick_matched && tick >= *from {
                    if tick != *from {
                        return Err(ScanError::Pairing(format!(
                            "配对缺口：首个可回放段 tick={tick} 晚于检查点时间 {from}\
                             （日志曾摘除后从新分片续接，世界连续性断裂）——需使用与日志连续的检查点"
                        )));
                    }
                    self.tick_matched = true;
                }
                if self.tick_matched {
                    if let Some(previous) = self.last_retained_tick
                        && tick <= previous
                    {
                        return Err(ScanError::Pairing(format!(
                            "选区内 tick 回退（{previous}→{tick}）：日志在世界回退后被续接，\
                             跨接缝不可回放——用与新分支连续的检查点，或从接缝后的检查点重新配对"
                        )));
                    }
                    retain_segment = true;
                }
            }
        }
        if retain_segment {
            self.chain.push((seq, claimed_sha.clone()));
            self.segments.push(Value::Object(object));
            self.last_retained_tick = Some(tick);
        }
        self.stats.segments += 1;
        if self.stats.first_tick.is_none() {
            self.stats.first_tick = Some(tick);
        }
        self.stats.last_tick = Some(tick);
        self.last_seq = seq;
        self.last_sha = claimed_sha;
        Ok(())
    }
}

/// 流式扫描：逐行解析、增量链校验、按保留策略驻留后缀。撕裂语义与
/// 整读版同形——末行判定以**最后一个非空行**为准：其后跟的空白块不把
/// 它变成中段行（末行缺失换行且非法 UTF-8 的尾块在前缀全部校验通过后
/// 归因 TornTail；可解码的末行按末行收敛语义解析）。内存占用为
/// O(保留后缀) 而非 O(整份日志)。
pub fn scan_input_log(path: &Path, retain: Retain) -> Result<InputLogData, ScanError> {
    if let Retain::FromCursor { cursor, .. } = &retain
        && cursor.last_seq == 0
    {
        // 与整读版 segments_after_cursor 同判：头段不是可提交段。
        return Err(ScanError::Pairing("检查点日志游标必须指向已提交段".into()));
    }
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut state = ScanState::new();
    // 一行前瞻：确认存在后继非空行后，当前行才按中段处理（必带换行）。
    // 空白块不刷新 pending——末行口径与整读版同形地取最后一个非空行，
    // 撕裂签名行后的尾随空白不能把它升级成中段损坏；空白行本身是
    // process_line 的无操作，直接跳过等价。
    let mut pending: Option<Vec<u8>> = None;
    let mut torn_tail_bytes = false;
    loop {
        let mut buf = Vec::new();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        if std::str::from_utf8(&buf).is_ok_and(|text| text.trim().is_empty()) {
            continue;
        }
        if let Some(previous) = pending.take() {
            state.process_line(&previous, false, &retain)?;
        }
        pending = Some(buf);
    }
    if let Some(tail) = pending {
        if !tail.ends_with(b"\n") && std::str::from_utf8(&tail).is_err() {
            // 撕裂切进多字节字符：前缀全部校验通过后才归因（下方）。
            torn_tail_bytes = true;
        } else {
            state.process_line(&tail, true, &retain)?;
        }
    }
    if torn_tail_bytes {
        return Err(ScanError::Format(InputLogError::TornTail {
            valid_segments: state.stats.segments,
        }));
    }
    let header = state
        .header
        .ok_or(ScanError::Format(InputLogError::BadHeader {
            line: 0,
            reason: "日志为空".into(),
        }))?;
    if let Retain::FromCursor { .. } = &retain
        && !state.cursor_matched
    {
        return Err(ScanError::Pairing("日志不覆盖检查点游标".into()));
    }
    if let Retain::FromTick(from) = &retain
        && !state.tick_matched
    {
        return Err(ScanError::Pairing(format!(
            "日志不覆盖检查点时间 {from} 之后的轮次（全部 {} 段均不晚于它）",
            state.stats.segments
        )));
    }
    let ScanState {
        header: _,
        segments,
        chain,
        stats,
        last_seq,
        last_sha,
        line_no: _,
        cursor_matched: _,
        cursor_next_tick: _,
        tick_matched: _,
        last_retained_tick: _,
    } = state;
    Ok(InputLogData {
        header,
        segments,
        chain,
        stats,
        final_seq: last_seq,
        final_hash: last_sha,
    })
}

/// 把日志截断到游标（保留头段与游标段，丢弃其后的废弃分支）：目录恢复
/// 放弃检查点之后的未确认历史，使续写段号/时间线从游标继续。与撕裂尾
/// 的人工截断不同——这是对**已通过整链校验**的历史做分支放弃，不是损
/// 坏修复；复制到游标段时自证其 (seq, sha256) 身份与游标一致，不单靠
/// 调用方先配对的纪律。游标即日志尾时为无操作。截断经临时文件+原子
/// rename 落地（崩溃不产生撕裂形态），末行补齐换行保证 append 续接
/// 安全门可通过。
pub fn truncate_after_cursor(path: &Path, cursor: &LogCursor) -> Result<(), InputLogError> {
    let data = scan_input_log(path, Retain::None).map_err(|error| match error {
        ScanError::Format(error) => error,
        ScanError::Pairing(reason) => InputLogError::BadHeader { line: 0, reason },
    })?;
    if cursor.last_seq > data.final_seq {
        return Err(InputLogError::BadHeader {
            line: 0,
            reason: format!(
                "截断游标超出日志末段（{} > {}）",
                cursor.last_seq, data.final_seq
            ),
        });
    }
    if cursor.last_seq == data.final_seq {
        return Ok(());
    }
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut temp = path.as_os_str().to_owned();
    temp.push(".truncate-tmp");
    let temp = std::path::PathBuf::from(temp);
    // 单进程 CLI：预扫描通过后的同路径临时文件必属上次截断崩溃的孤儿
    // （正常流程 rename 后即消失），先摘除——崩溃窗口后的恢复一次成功，
    // 不付出一次带错误的失败尝试。
    let _ = std::fs::remove_file(&temp);
    let result = (|| -> Result<(), InputLogError> {
        let mut writer = match std::fs::File::create_new(&temp) {
            Ok(file) => std::io::BufWriter::new(file),
            // 预清理后仍冲突：非预期并发写同一日志，指名路径让调用方
            // 排查而不是重试。
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(InputLogError::BadHeader {
                    line: 0,
                    reason: format!(
                        "截断临时文件已存在（有并发写入者？），排查后重试：{}",
                        temp.display()
                    ),
                });
            }
            Err(error) => return Err(error.into()),
        };
        // 已校验文件按非空行计数复制到游标段为止；空白行原样保留（读取
        // 器跳过它们，不影响段号）。
        let mut kept: u64 = 0;
        loop {
            let mut buf = Vec::new();
            if reader.read_until(b'\n', &mut buf)? == 0 {
                break;
            }
            let is_blank = std::str::from_utf8(&buf)
                .map(|text| text.trim().is_empty())
                .unwrap_or(false);
            writer.write_all(&buf)?;
            if !is_blank {
                kept += 1;
                if kept == cursor.last_seq + 1 {
                    // 自证截到的就是调用方所指的那段：预扫描已校验全链
                    // 自洽，这里再钉游标身份，误配的游标截不出错误前缀。
                    let (seq, sha) = line_identity(&buf).ok_or(InputLogError::BadHeader {
                        line: 0,
                        reason: "游标段行解析或自摘要复算失败".into(),
                    })?;
                    if seq != cursor.last_seq || sha != cursor.sha256 {
                        return Err(InputLogError::BadHeader {
                            line: 0,
                            reason: "截断游标与日志段身份不匹配（seq 或 sha256）".into(),
                        });
                    }
                    if !buf.ends_with(b"\n") {
                        writer.write_all(b"\n")?;
                    }
                    break;
                }
            }
        }
        // 复制没到游标段就 EOF：预扫描与复制之间日志被外部改短
        // （TOCTOU），宁可失败也不能整文件照抄却报告已截断。
        if kept != cursor.last_seq + 1 {
            return Err(InputLogError::BadHeader {
                line: 0,
                reason: "复制期间日志缩短，未到达游标段".into(),
            });
        }
        writer.flush()?;
        drop(writer);
        // 与检查点发布同规：临时文件 fsync → rename → 父目录 fsync。
        std::fs::File::open(&temp)?.sync_all()?;
        std::fs::rename(&temp, path)?;
        if let Some(parent) = path.parent()
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// 单行的 `(seq, 自摘要)` 身份，要求行内 claimed sha256 与复算值一致
/// （与扫描侧摘要复算同法）；供截断复制自证游标段身份。
fn line_identity(bytes: &[u8]) -> Option<(u64, String)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value: Value = serde_json::from_str(text.trim()).ok()?;
    let object = value.as_object()?;
    let claimed = object.get("sha256")?.as_str()?.to_string();
    let mut body = object.clone();
    body.remove("sha256");
    let computed = sha256_hex(Value::Object(body).to_string().as_bytes());
    (computed == claimed).then_some((object.get("seq")?.as_u64()?, claimed))
}
