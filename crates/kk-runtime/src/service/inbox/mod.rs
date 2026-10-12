//! 耐久命令接收面（inbox.json v1）：原子发布的单一有界最新状态。
//!
//! 活跃流、nextServerSeq、已接收水位、未淘汰明细与接收索引在同一文件
//! 原子更新（tmp 整写+fsync+rename+父目录同步，经 kk-storage 发布原语）；
//! 接收成功只在文件与父目录同步后返回（received 承诺可找回）。并发
//! 模型：目录租约内的宿主单线程串行调用（无锁），多线程访问须由调用
//! 方串行化。幂等键为
//! `(principal, streamId, clientSeq)`：同规范载荷返回原接收状态，异载荷
//! 冲突；明细淘汰后统一结果过期，不重新执行。崩溃残留由发布协议消解，
//! 文件本身损坏（撕裂/外来内容）是停服条件，不回退旧值。
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub mod accept;
#[cfg(test)]
mod tests;

/// inbox.json 线格式版本。
pub const INBOX_FORMAT_VERSION: u32 = 1;

/// 信封上限：16MiB 状态预算加转义余量（payload 以 JSON 字符串嵌入信
/// 封，密集引号/控制字符可膨胀数倍；极端转义下 store 超限拒绝、
/// received 不成立，是安全方向的可用性边界——S2.2 接线时按实测收紧）。
const INBOX_SPEC: kk_storage::EnvelopeSpec = kk_storage::EnvelopeSpec {
    format: "kkscreeps-service-inbox",
    version: INBOX_FORMAT_VERSION,
    max_bytes: 64 * 1024 * 1024,
};

/// 接收准入预算；任一上限先到即拒绝准入（不驱逐既有命令）。
#[derive(Clone, Copy, Debug)]
pub struct InboxLimits {
    /// 未终态命令总数上限。
    pub max_pending_total: usize,
    /// 单主体（跨全部活跃流合计）未终态命令数上限。
    pub max_pending_per_principal: usize,
    /// 状态文件载荷字节上限。
    pub max_state_bytes: usize,
}

impl Default for InboxLimits {
    fn default() -> Self {
        InboxLimits {
            max_pending_total: 256,
            max_pending_per_principal: 8,
            max_state_bytes: 16 * 1024 * 1024,
        }
    }
}

/// 单条命令的接收与结局记录；payload 在完成屏障确认并淘汰前保留。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEntry {
    pub server_seq: u64,
    pub kind: String,
    pub schema_version: u32,
    pub payload: Value,
    /// 命令规范形摘要（kind+schemaVersion+payload 的 serde_json 序列化
    /// 字节）：幂等判别覆盖整个命令身份内容，异 kind/schemaVersion 同
    /// payload 视为载荷分歧行 Conflict，不误判为重放。
    pub command_hash: String,
    pub status: EntryStatus,
    pub received_at_ms: u64,
}

/// 对外命令状态机：received → completed(result) 或 rejected(reason)；
/// 内部中间态不是最终成功，不出现在耐久状态里。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum EntryStatus {
    Received,
    Completed { result: Value },
    Rejected { reason: String },
}

impl EntryStatus {
    pub fn is_settled(&self) -> bool {
        !matches!(self, EntryStatus::Received)
    }
}

/// 单命令流：clientSeq 连续接收、水位与有界明细。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamState {
    /// 流绑定的主体；来自宿主授予上下文，不信 payload 自报。
    pub principal: String,
    /// 关闭后拒绝新命令；retired 后整体失效不复用。
    pub closed: bool,
    pub retired: bool,
    /// 下一期望 clientSeq（已接收水位 + 1）。
    pub next_client_seq: u64,
    /// 已消费水位；S2.1 接收面恒 0，真实消费自 S2.2 起推进。
    pub applied_seq: u64,
    /// 明细保留下界：小于此值的 clientSeq 明细已淘汰（查询走结果过期）。
    pub retained_floor: u64,
    pub entries: BTreeMap<u64, CommandEntry>,
}

/// inbox.json 载荷；读取方按严格格式整体解码并核对世界身份。
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxState {
    pub inbox_format_version: u32,
    pub world_id: String,
    /// 全局接收序号分配游标；缓存重试复用原值，不复位。
    pub next_server_seq: u64,
    pub streams: BTreeMap<String, StreamState>,
}

impl InboxState {
    pub fn empty(world_id: &str) -> Self {
        InboxState {
            inbox_format_version: INBOX_FORMAT_VERSION,
            world_id: world_id.to_string(),
            next_server_seq: 1,
            streams: BTreeMap::new(),
        }
    }
}

/// 以发布协议原子写出 inbox；写入成功（含父目录同步）后 received 才成立。
pub fn store(state: &InboxState, path: &Path) -> anyhow::Result<()> {
    validate(state)?;
    kk_storage::publish_json(path, INBOX_SPEC, &serde_json::to_value(state)?, |_| Ok(()))
}

/// 读取并整体校验；文件损坏是停服条件，不回退可能丢失 received 的旧值。
pub fn load(path: &Path, world_id: &str) -> anyhow::Result<InboxState> {
    let state: InboxState = kk_storage::envelope::read_json(path, INBOX_SPEC)
        .map_err(|error| anyhow::anyhow!("读取命令收件箱 {}: {error}", path.display()))
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|error| anyhow::anyhow!("收件箱数据非法: {error}"))
        })?;
    validate(&state)?;
    anyhow::ensure!(
        state.world_id == world_id,
        "收件箱的世界身份 {} 与目录身份 {} 不一致",
        state.world_id,
        world_id
    );
    Ok(state)
}

fn validate(state: &InboxState) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.inbox_format_version == INBOX_FORMAT_VERSION,
        "inbox 格式版本 {} 不受支持（期望 {INBOX_FORMAT_VERSION}）",
        state.inbox_format_version
    );
    for (stream_id, stream) in &state.streams {
        anyhow::ensure!(!stream.principal.is_empty(), "流 {stream_id} 缺少主体绑定");
        anyhow::ensure!(stream.next_client_seq >= 1, "流 {stream_id} 的接收水位非法");
    }
    Ok(())
}

/// 未终态命令总数。
pub fn pending_total(state: &InboxState) -> usize {
    state
        .streams
        .values()
        .flat_map(|stream| stream.entries.values())
        .filter(|entry| !entry.status.is_settled())
        .count()
}

/// 主体当前的活跃（未关闭且未退役）流数。
pub fn active_streams_of(state: &InboxState, principal: &str) -> usize {
    state
        .streams
        .values()
        .filter(|stream| stream.principal == principal && !stream.closed && !stream.retired)
        .count()
}

pub fn stream_of_mut<'a>(
    state: &'a mut InboxState,
    principal: &str,
    stream_id: &str,
) -> Option<&'a mut StreamState> {
    state
        .streams
        .get_mut(stream_id)
        .filter(|stream| stream.principal == principal && !stream.retired)
}
