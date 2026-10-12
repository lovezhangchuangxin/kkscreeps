//! 命令接收事务：信封校验、幂等/冲突判别、跳号拒绝与预算准入。
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    CommandEntry, EntryStatus, InboxLimits, InboxState, StreamState, active_streams_of,
    pending_total, stream_of_mut,
};

/// 每主体活跃流上限（宿主签发，不可配置）。
pub const MAX_STREAMS_PER_PRINCIPAL: usize = 4;

/// 接收判定结果；Duplicate 不重复分配 serverSeq、不重复计数。
#[derive(Debug)]
pub enum AcceptOutcome {
    /// 新接收：received 成立的前提是调用方随后 store 成功。
    Received { server_seq: u64 },
    /// 同身份同规范载荷的重放：返回原接收状态（含 completed 结果或
    /// rejected 原因）与序号，不重复分配。
    Duplicate {
        server_seq: u64,
        status: EntryStatus,
    },
    /// 同身份异载荷：冲突，原命令不受影响。
    Conflict,
    /// 明细已淘汰的旧身份：统一结果过期，不重新执行、不补造结果。
    ResultExpired,
    /// 明确拒绝（跳号/预算/流状态/信封非法）；不持久化（区别于命令级
    /// rejected 终态——验证拒绝持久化自 S2.2 消费面起）。
    Refused { reason: String },
}

/// 接收一条命令：信封整体校验后按 `(principal, streamId, clientSeq)`
/// 幂等键判别。成功路径只改内存状态，调用方负责 store 落盘后才对外
/// 返回 received；store 失败即整体未发生（内存改动由调用方丢弃）。
pub fn accept(
    state: &mut InboxState,
    envelope: &Value,
    limits: &InboxLimits,
    now_ms: u64,
) -> AcceptOutcome {
    match accept_inner(state, envelope, limits, now_ms) {
        Ok(outcome) => outcome,
        Err(reason) => AcceptOutcome::Refused { reason },
    }
}

fn accept_inner(
    state: &mut InboxState,
    envelope: &Value,
    limits: &InboxLimits,
    now_ms: u64,
) -> Result<AcceptOutcome, String> {
    let world_id = str_field(envelope, "worldId")?;
    if world_id != state.world_id {
        return Err(format!("信封世界身份 {world_id} 与目录不符"));
    }
    let principal = str_field(envelope, "principal")?;
    if principal.is_empty() {
        return Err("主体为空".into());
    }
    let stream_id = str_field(envelope, "streamId")?;
    if stream_id.is_empty() {
        return Err("流身份为空".into());
    }
    let client_seq = envelope["clientSeq"]
        .as_u64()
        .ok_or("clientSeq 必须是非负整数")?;
    if client_seq == 0 {
        return Err("clientSeq 从 1 起".into());
    }
    let kind = str_field(envelope, "kind")?;
    if kind.is_empty() {
        return Err("命令 kind 为空".into());
    }
    let schema_version_raw = envelope["schemaVersion"]
        .as_u64()
        .ok_or("schemaVersion 必须是非负整数")?;
    if schema_version_raw > u32::MAX as u64 {
        return Err(format!("schemaVersion {schema_version_raw} 超出 u32 域"));
    }
    let schema_version = schema_version_raw as u32;
    let payload = &envelope["payload"];
    if payload.is_null() {
        return Err("payload 缺失".into());
    }

    let stream = stream_of_mut(state, principal, stream_id)
        .ok_or("流不存在或已退役；先经宿主签发流再提交命令")?;
    // 幂等键判别先于流关闭检查：关闭后旧身份仍可重放/查询直到退役
    // （丢响应的结果找回不因关闭而失效），closed 只拦新接收。
    if let Some(entry) = stream.entries.get(&client_seq) {
        return if entry.command_hash == command_hash(kind, schema_version, payload) {
            Ok(AcceptOutcome::Duplicate {
                server_seq: entry.server_seq,
                status: entry.status.clone(),
            })
        } else {
            Ok(AcceptOutcome::Conflict)
        };
    }
    if client_seq < stream.retained_floor {
        return Ok(AcceptOutcome::ResultExpired);
    }
    if client_seq < stream.next_client_seq {
        // 水位以下但不在明细：曾接收且已淘汰（retained_floor 未推进的
        // 窗口内也可能因终态淘汰出现，统一结果过期语义）。
        return Ok(AcceptOutcome::ResultExpired);
    }
    if stream.closed {
        return Err("流已关闭，拒绝新命令".into());
    }
    if client_seq > stream.next_client_seq {
        return Err(format!(
            "clientSeq {client_seq} 跳号（期望 {}）；保存原身份直到收到明确结果",
            stream.next_client_seq
        ));
    }
    // 新接收：预算准入（条数与状态字节数，任一超限拒绝，不驱逐既有）。
    // 条数按主体跨流合计（方案：单玩家 8 条）。
    let principal_pending = state
        .streams
        .values()
        .filter(|candidate| candidate.principal == principal)
        .flat_map(|candidate| candidate.entries.values())
        .filter(|entry| !entry.status.is_settled())
        .count();
    if principal_pending + 1 > limits.max_pending_per_principal {
        return Err(format!(
            "主体 {principal} 未完成命令数（跨流合计）超上限 {}",
            limits.max_pending_per_principal
        ));
    }
    if pending_total(state) + 1 > limits.max_pending_total {
        return Err(format!(
            "收件箱未完成命令总数超上限 {}",
            limits.max_pending_total
        ));
    }
    // 字节预算探测：整状态深克隆+全量重编码（O(状态大小)，当前上限
    // 16MiB 量级）后整体替换——S2.1 低频控制命令可接受，S3 HTTP 接线
    // 后若成为热点改增量编码。
    let mut probe = state.clone();
    let server_seq = probe.next_server_seq;
    let stream = probe
        .streams
        .get_mut(stream_id)
        .expect("流已在前置校验确认存在");
    stream.entries.insert(
        client_seq,
        CommandEntry {
            server_seq,
            kind: kind.to_string(),
            schema_version,
            payload: payload.clone(),
            command_hash: command_hash(kind, schema_version, payload),
            status: EntryStatus::Received,
            received_at_ms: now_ms,
        },
    );
    stream.next_client_seq = client_seq + 1;
    probe.next_server_seq += 1;
    let encoded = serde_json::to_vec(&probe).map_err(|error| format!("状态编码失败: {error}"))?;
    if encoded.len() > limits.max_state_bytes {
        return Err(format!(
            "收件箱状态将超字节预算 {}（当前编码 {} 字节）",
            limits.max_state_bytes,
            encoded.len()
        ));
    }
    *state = probe;
    Ok(AcceptOutcome::Received { server_seq })
}

/// 签发新命令流：同主体活跃流数受上限约束；streamId 全局唯一且退役后
/// 不复用（占用墓碑保留在 streams 中）。
pub fn open_stream(state: &mut InboxState, principal: &str, stream_id: &str) -> Result<(), String> {
    if principal.is_empty() || stream_id.is_empty() {
        return Err("主体与流身份不能为空".into());
    }
    if let Some(existing) = state.streams.get(stream_id) {
        return Err(match existing.retired {
            true => format!("流 {stream_id} 已退役，不能复用"),
            false => format!("流 {stream_id} 已存在"),
        });
    }
    if active_streams_of(state, principal) >= MAX_STREAMS_PER_PRINCIPAL {
        return Err(format!(
            "主体 {principal} 的活跃流数达上限 {MAX_STREAMS_PER_PRINCIPAL}"
        ));
    }
    state.streams.insert(
        stream_id.to_string(),
        StreamState {
            principal: principal.to_string(),
            closed: false,
            retired: false,
            next_client_seq: 1,
            applied_seq: 0,
            retained_floor: 0,
            entries: Default::default(),
        },
    );
    Ok(())
}

/// 关闭流：拒绝关闭仍有在途（未终态）命令的流——不能靠关闭绕过完成
/// 确认；关闭后旧身份仍可查询直到退役。
pub fn close_stream(
    state: &mut InboxState,
    principal: &str,
    stream_id: &str,
) -> Result<(), String> {
    let stream =
        stream_of_mut(state, principal, stream_id).ok_or("流不存在或已退役".to_string())?;
    let in_flight = stream
        .entries
        .values()
        .any(|entry| !entry.status.is_settled());
    if in_flight {
        return Err("流仍有在途命令；等待完成或显式拒绝后才能关闭".into());
    }
    stream.closed = true;
    Ok(())
}

/// 退役流：在关闭（无在途）前提下整体失效旧身份并释放明细预算；
/// 墓碑保留 streamId 防复用。
pub fn retire_stream(
    state: &mut InboxState,
    principal: &str,
    stream_id: &str,
) -> Result<(), String> {
    let stream =
        stream_of_mut(state, principal, stream_id).ok_or("流不存在或已退役".to_string())?;
    let in_flight = stream
        .entries
        .values()
        .any(|entry| !entry.status.is_settled());
    if in_flight {
        return Err("流仍有在途命令；先完成或拒绝并关闭后再退役".into());
    }
    stream.retired = true;
    stream.closed = true;
    stream.entries.clear();
    stream.retained_floor = stream.next_client_seq;
    Ok(())
}

/// 淘汰已完成明细：删除终态且 clientSeq 严格小于 floor 的条目，推进
/// 保留下界；在途（received）明细不得删除——完成屏障确认前保留载荷。
/// 前置契约：调用方 floor 不得超过该流 next_client_seq（超水位会把
/// 未接收区间误标为已淘汰，后续新接收被 ResultExpired 吞掉）。
pub fn evict_settled_below(state: &mut InboxState, stream_id: &str, floor: u64) -> usize {
    let Some(stream) = state.streams.get_mut(stream_id) else {
        return 0;
    };
    let doomed: Vec<u64> = stream
        .entries
        .range(..floor)
        .filter(|(_, entry)| entry.status.is_settled())
        .map(|(seq, _)| *seq)
        .collect();
    let count = doomed.len();
    for seq in doomed {
        stream.entries.remove(&seq);
    }
    if count > 0 {
        // 保留下界指向"最后一个已删除的终态序号"，只进不退。
        stream.retained_floor = stream.retained_floor.max(floor.saturating_sub(1));
    }
    count
}

fn str_field<'a>(envelope: &'a Value, key: &str) -> Result<&'a str, String> {
    envelope[key]
        .as_str()
        .ok_or_else(|| format!("信封缺 {key} 字符串字段"))
}

/// 命令规范形摘要：进程内以 (kind, schemaVersion, payload) 的 serde_json
/// 序列化字节为规范形（S3 网络面接入时统一 canonical 化规则）。
fn command_hash(kind: &str, schema_version: u32, payload: &Value) -> String {
    let canonical = serde_json::json!({
        "kind": kind,
        "schemaVersion": schema_version,
        "payload": payload,
    });
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&canonical)
                .unwrap_or_default()
                .as_slice()
        )
    )
}
