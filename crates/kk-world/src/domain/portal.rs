//! portal（传送门）域：目的地形态校验与放置归一。
//!
//! 官方 portal 对象无 owner、无 hits 面（免一切伤害——`_damage` 的
//! `!target.hits` 早退）、不在 OBSTACLE_OBJECT_TYPES（可踩上，寻路默认
//! 矩阵单独避让）。目的地两种形态：同 shard `{room,x,y}`（玩家面折算
//! RoomPosition 实例）、跨 shard `{shard,room}`（玩家面 POJO）。目的房
//! 只校验房名格式，不要求已注册（官方世界全房存在，kk 世界仅含放置房；
//! 传送结算对未注册目的房保持原地）。

use serde_json::Value;

/// 放置归一：相对 `ticksToDecay` 换算为绝对 `decayTime`（与 deposit/
/// nuke/powerBank 同一适配器契约：放置后一次 tick 读恰好减一）；显式
/// 绝对 `decayTime` 透传；两者皆缺为永久门（无 decayTime 键，玩家面
/// ticksToDecay 为 undefined）。目的地必须已在 spec 中（`destination`）。
pub fn normalize(
    data: &mut serde_json::Map<String, serde_json::Value>,
    time: u64,
) -> Result<(), String> {
    for key in ["store", "owner"] {
        if data.contains_key(key) {
            return Err(format!(
                "portal 放置不接受 spec 键 {key}（官方无库存无主，由归一层独占拒绝）"
            ));
        }
    }
    if !data.contains_key("destination") {
        return Err("portal 放置必须提供 destination".into());
    }
    match data.remove("ticksToDecay") {
        Some(Value::Null) | None => {}
        Some(v) => {
            let delta = v
                .as_u64()
                .filter(|n| *n <= crate::domain::deposit::MAX_SAFE_INTEGER)
                .ok_or("portal ticksToDecay 必须是非负安全整数")?;
            let deadline = time
                .checked_add(delta)
                .filter(|n| *n <= crate::domain::deposit::MAX_SAFE_INTEGER)
                .ok_or("portal ticksToDecay 换算后的截止时间超出安全整数范围")?;
            data.insert("decayTime".into(), serde_json::json!(deadline));
        }
    }
    if let Some(decay) = data.get("decayTime")
        && decay
            .as_u64()
            .filter(|n| *n <= crate::domain::deposit::MAX_SAFE_INTEGER)
            .is_none()
    {
        return Err("portal decayTime 必须是非负安全整数".into());
    }
    Ok(())
}

/// portal 对象数据校验：destination 必须是两种合法形态之一、decayTime
/// 在场须为非负安全整数。插入、管理编辑和检查点恢复共用（与
/// deposit/power_bank::validate 同一挂点）。
pub fn validate(data: &serde_json::Value) -> Result<(), String> {
    if data
        .get("structureType")
        .and_then(serde_json::Value::as_str)
        != Some("portal")
    {
        return Ok(());
    }
    let fields = data.as_object().ok_or("portal 记录必须为对象")?;
    // 与放置归一同口径：官方 portal 无库存无主无 hits 面（免一切伤害的
    // 前提），恢复/编辑通道走私这些键会使 portal 可被攻击/带主。
    for key in ["store", "owner", "hits", "hitsMax"] {
        if fields.contains_key(key) {
            return Err(format!("portal 记录不接受键 {key}（无库存无主无 hits 面）"));
        }
    }
    if let Some(decay) = fields.get("decayTime")
        && decay
            .as_u64()
            .filter(|n| *n <= crate::domain::deposit::MAX_SAFE_INTEGER)
            .is_none()
    {
        return Err("portal decayTime 必须是非负安全整数".into());
    }
    let destination = fields
        .get("destination")
        .ok_or("portal 记录缺少 destination")?;
    let dest = destination
        .as_object()
        .ok_or("portal destination 必须是对象")?;
    let valid_room = |v: &serde_json::Value| {
        v.as_str()
            .map(|name| kk_core::RoomName::parse(name).is_ok())
            .unwrap_or(false)
    };
    let cross_shard = dest.contains_key("shard");
    let same_shard = dest.contains_key("x") || dest.contains_key("y");
    if cross_shard && same_shard {
        return Err("portal destination 不能同时携带 shard 与 x/y（两种形态互斥）".into());
    }
    if cross_shard {
        let shard = dest
            .get("shard")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or("portal destination.shard 必须是非空字符串")?;
        let _ = shard;
        if !valid_room(dest.get("room").unwrap_or(&serde_json::Value::Null)) {
            return Err("portal destination.room 必须是合法房名格式".into());
        }
        return Ok(());
    }
    for key in ["x", "y"] {
        if !dest
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|v| v <= 49)
        {
            return Err("portal destination.x/y 必须是 0..=49 整数".into());
        }
    }
    if !valid_room(dest.get("room").unwrap_or(&serde_json::Value::Null)) {
        return Err("portal destination.room 必须是合法房名格式".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_converts_relative_decay_and_rejects_reserved_keys() {
        let mut data =
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(json!({
                "destination": { "room": "W2N1", "x": 10, "y": 10 },
                "ticksToDecay": 3
            }))
            .unwrap();
        normalize(&mut data, 5).unwrap();
        assert_eq!(data.get("decayTime"), Some(&json!(8)));
        assert!(!data.contains_key("ticksToDecay"));

        let mut permanent = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({ "destination": { "shard": "shard1", "room": "W5N5" } }),
        )
        .unwrap();
        normalize(&mut permanent, 5).unwrap();
        assert!(!permanent.contains_key("decayTime"));

        let mut with_owner = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({ "destination": { "room": "W2N1", "x": 1, "y": 1 }, "owner": "p1" }),
        )
        .unwrap();
        assert!(normalize(&mut with_owner, 5).is_err());

        let mut missing = serde_json::Map::new();
        assert!(normalize(&mut missing, 5).is_err());

        let mut overflow = serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
            json!({ "destination": { "room": "W2N1", "x": 1, "y": 1 }, "ticksToDecay": u64::MAX }),
        )
        .unwrap();
        assert!(normalize(&mut overflow, 5).is_err());
    }

    #[test]
    fn validate_accepts_both_forms_and_rejects_mixed_or_illegal() {
        for ok in [
            json!({"structureType":"portal","destination":{"room":"W2N1","x":0,"y":49}}),
            json!({"structureType":"portal","destination":{"shard":"shard1","room":"E5S5"},"decayTime":100}),
            // 非 portal 记录静默通过（与其他域校验同一分派口径）。
            json!({"structureType":"spawn"}),
        ] {
            assert!(validate(&ok).is_ok(), "{ok}");
        }
        for bad in [
            json!({"structureType":"portal"}),
            json!({"structureType":"portal","destination":"W2N1"}),
            json!({"structureType":"portal","destination":{"room":"not a room","x":1,"y":1}}),
            json!({"structureType":"portal","destination":{"room":"W2N1","x":50,"y":1}}),
            json!({"structureType":"portal","destination":{"shard":"","room":"W2N1"}}),
            json!({"structureType":"portal","destination":{"shard":"shard1","room":"W2N1","x":1,"y":1}}),
            json!({"structureType":"portal","destination":{"shard":"shard1"}}),
            json!({"structureType":"portal","destination":{"room":"W2N1","x":1,"y":1},"decayTime":-1}),
            json!({"structureType":"portal","destination":{"room":"W2N1","x":1,"y":1},"hits":100}),
            json!({"structureType":"portal","destination":{"room":"W2N1","x":1,"y":1},"store":{"power":1}}),
            json!({"structureType":"portal","destination":{"room":"W2N1","x":1,"y":1},"owner":"p1"}),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
    }
}
