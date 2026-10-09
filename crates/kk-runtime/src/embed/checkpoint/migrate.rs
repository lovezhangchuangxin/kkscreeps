//! 检查点 JSON 载荷的格式版本与显式迁移。
//!
//! 职权分界：信封 `version`（[`crate::checkpoint::CHECKPOINT_VERSION`]）是
//! 线格式（结构、sha256 机制）；本模块的 `checkpoint_format_version` 是
//! JSON 载荷域布局的版本。`engine_version`/`room_schema_version` 双硬门
//! 语义不变（二进制身份），不承担载荷形状。
//!
//! JSON 载荷迁移按相邻版本注册：第 n 项
//!（0 起算）把版本 n+1 升到 n+2，`skip(raw - 1)` 起依次执行到当前版本；
//! 版本号由 [`migrate_to_current`] 在链执行完毕后统一改写，单步不触碰。
//! 版本 0、未来版本、缺失与非法版本值一律拒绝；任一步失败整体拒绝——
//! 调用方在替代分片切换前失败即保留旧分片。迁移必须发生在
//! `deny_unknown_fields` 解码**之前**（对原始 Value 操作），否则旧载荷
//! 在版本路由前就被拒。
use serde_json::Value;

/// 当前载荷格式版本。v5：房间 NPC 巡守记忆独立表（必填，与 v4 的 effects
/// 同一拒绝策略）；v4：房间对象效果独立表（必填）；v3：宿主玩法协议身份；
/// v2：新增 `checkpoint_format_version` 键本身，并把
/// `last_auto_disable` 的 trace 批次前旧形态（缺 `mod_id`，来源只在
/// trigger.error.fault.binding）显式归一为结构化形态。
pub(super) const CURRENT_FORMAT_VERSION: u32 = 5;

type MigrationFn = fn(&mut Value) -> Result<(), String>;

/// 升级链：第 n 项（0 起算）执行 n+1 → n+2。
const MIGRATIONS: [MigrationFn; (CURRENT_FORMAT_VERSION - 1) as usize] =
    [v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5];

/// 把载荷迁移到当前格式；返回**迁移前的原始版本**供语义门决策
///（如执行策略身份硬门只约束 v2 起保存的检查点，v1 存量按 legacy 宽容）。
///
/// 版本字段必填：项目当前仅本地开发、无存量用户（2026-10-06 拍板），
/// 缺失或非法即按畸形载荷拒绝——不为“同版本开发期产物”留缺键猜测。
/// v1 只以显式 `checkpoint_format_version: 1` 的载荷存在，供迁移链
/// 与测试演练（真实历史形状的证据来源在测试注释登记）。
pub(super) fn migrate_to_current(value: &mut Value) -> Result<u32, String> {
    let raw = match value.get("checkpoint_format_version") {
        None => {
            return Err(
                "检查点缺少 checkpoint_format_version（缺失即拒，不做缺键猜测）".to_string(),
            );
        }
        Some(version) => version
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| {
                format!("checkpoint_format_version 必须是非负 u32 整数（当前 {version}）")
            })?,
    };
    if raw == 0 {
        return Err(format!(
            "检查点格式版本 0 无效（有效范围 1..={CURRENT_FORMAT_VERSION}）"
        ));
    }
    if raw > CURRENT_FORMAT_VERSION {
        return Err(format!(
            "检查点格式版本 {raw} 高于当前支持 {CURRENT_FORMAT_VERSION}；尚未提供降级迁移"
        ));
    }
    for step in MIGRATIONS.iter().skip((raw - 1) as usize) {
        step(value)?;
    }
    value["checkpoint_format_version"] = Value::from(CURRENT_FORMAT_VERSION);
    Ok(raw)
}

/// v1 → v2：归一 `last_auto_disable` 旧形态（无 `mod_id`）——来源从
/// `trigger.error.fault.binding` 的命名空间派生。已有 `mod_id` 且与来源
/// 不一致、或无法派生来源的载荷是**不一致载荷**，按台账口径拒绝而非
/// 修补。原生检查点（无该键）不动。
fn v1_to_v2(value: &mut Value) -> Result<(), String> {
    let Some(record) = value.get_mut("last_auto_disable") else {
        return Ok(());
    };
    if record.is_null() {
        return Ok(());
    }
    let binding = record
        .pointer("/trigger/error/fault/binding")
        .and_then(Value::as_str)
        .ok_or_else(|| "迁移 v1→v2：last_auto_disable 缺少故障绑定来源".to_string())?;
    let namespace = binding
        .split_once(':')
        .map(|(ns, _)| ns)
        .ok_or_else(|| format!("迁移 v1→v2：故障绑定 {binding:?} 缺命名空间前缀"))?;
    let existing = record.get("mod_id").and_then(Value::as_str);
    if existing.is_some_and(|id| id != namespace) {
        return Err(format!(
            "迁移 v1→v2：last_auto_disable 的 mod_id {existing:?} 与故障绑定来源 {namespace:?} 不一致"
        ));
    }
    record["mod_id"] = Value::String(namespace.to_string());
    Ok(())
}

// 旧载荷显式标为旧语义，恢复门拒绝跨 SDK 语义自动升级；已有协议身份
// 的降版本测试载荷保留其身份，不能伪造当前身份来掩盖旧时区历史。
fn v2_to_v3(value: &mut Value) -> Result<(), String> {
    let object = value.as_object_mut().ok_or("迁移 v2→v3：载荷须为对象")?;
    object
        .entry("host_protocol")
        .or_insert_with(|| Value::String("kk-gameplay-v1-local-date".into()));
    Ok(())
}

// 不补造缺失效果域；旧布局必须拒绝，避免恢复时静默丢失生效状态。
fn v3_to_v4(value: &mut Value) -> Result<(), String> {
    if let Some(rooms) = value
        .pointer("/engine/world/rooms")
        .and_then(Value::as_object)
        && rooms.values().any(|room| room.get("effects").is_none())
    {
        return Err("检查点房间缺少 effects，无法恢复效果状态".into());
    }
    Ok(())
}

// 与 v3→v4 同一拒绝策略：Keeper 巡守记忆不补造，缺域的 v4 房间显式拒绝，
// 避免恢复时撞裸 serde 错误或静默丢失巡守目标与路径缓存。
fn v4_to_v5(value: &mut Value) -> Result<(), String> {
    if let Some(rooms) = value
        .pointer("/engine/world/rooms")
        .and_then(Value::as_object)
        && rooms
            .values()
            .any(|room| room.get("keeper_memory").is_none())
    {
        return Err("检查点房间缺少 keeper_memory，无法恢复 NPC 巡守记忆".into());
    }
    Ok(())
}
