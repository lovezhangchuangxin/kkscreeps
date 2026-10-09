//! powerBank（能量银行）域：高速路房判定。
//! 房间调度状态（官方 `room.powerBankTime`）作为 `RoomState.power_bank_time`
//! 字段随完整检查点保存；生产者（官方 backend `genPowerBanks` cron）与
//! 消费者（反伤/衰变/废墟/掉落）见架构 game-loop.md 的 powerBank 节。

/// 是否高速路房（官方 db.rooms 的 `bus` 标志）：房间编号任一轴 ≡0 (mod 10)。
/// 负半轴（W/N）的房间编号 = |坐标|−1（与 RoomName 的负一约定一致），
/// 即 W0/W10… 与 E0/E10… 同为扇区边界线。官方 world-gen 不在本仓库，
/// 边界编号以 backend strongholds.js `getSectorEdges` 的 10*xx+i 生成式为证。
pub fn is_highway_room(name: &str) -> bool {
    let Ok(parsed) = kk_core::RoomName::parse(name) else {
        return false;
    };
    let on_boundary = |v: i32| {
        let n = if v < 0 {
            v.unsigned_abs() - 1
        } else {
            v.unsigned_abs()
        };
        n % 10 == 0
    };
    on_boundary(parsed.xx) || on_boundary(parsed.yy)
}

fn unsigned(data: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<(), String> {
    if let Some(value) = data.get(key)
        && value
            .as_u64()
            .filter(|n| *n <= crate::domain::deposit::MAX_SAFE_INTEGER)
            .is_none()
    {
        return Err(format!("powerBank {key} 必须是非负安全整数"));
    }
    Ok(())
}

/// powerBank 对象数据校验：store.power、decayTime、hits、hitsMax 在场时
/// 须为非负安全整数（缺失不补造——放置/生成层保证 canonical 形态，
/// 此处拦手改载荷的负数/小数/超界形态，否则运行时 fail-open：衰变
/// 永不触发、object_hits 读 0 早退成无敌）。插入、管理编辑和检查点
/// 恢复共用（与 deposit::validate 同一挂点）。
pub fn validate(data: &serde_json::Value) -> Result<(), String> {
    if data
        .get("structureType")
        .and_then(serde_json::Value::as_str)
        != Some("powerBank")
    {
        return Ok(());
    }
    let fields = data.as_object().ok_or("powerBank 记录必须为对象")?;
    for key in ["decayTime", "hits", "hitsMax"] {
        unsigned(fields, key)?;
    }
    if let Some(store) = fields.get("store") {
        let store = store.as_object().ok_or("powerBank store 必须是对象")?;
        unsigned(store, "power")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highway_rooms_match_official_sector_edges() {
        // 扇区边界线：E0/E10/W0/W10 两轴任一贴线即高速路（S 半轴同构）。
        for name in [
            "E0N5", "E10N5", "W0N5", "W10N5", "E5N0", "E5N10", "W5N0", "W10N10", "E5S0", "E5S10",
            "W10S10", "E10S10",
        ] {
            assert!(is_highway_room(name), "{name} 应为高速路房");
        }
        // 扇区内房：两轴编号尾数均非 0（E11N10 的 N10 贴 y 边界线，是高速路）。
        for name in [
            "E1N1", "W1N1", "E5N5", "W9N9", "E11N11", "W11N11", "E11S11", "W9S9",
        ] {
            assert!(!is_highway_room(name), "{name} 不应为高速路房");
        }
    }

    #[test]
    fn invalid_room_name_is_not_highway() {
        assert!(!is_highway_room("not a room"));
        assert!(!is_highway_room(""));
    }

    #[test]
    fn validate_rejects_illegal_values_and_passes_other_types() {
        let ok = serde_json::json!({
            "structureType": "powerBank",
            "store": { "power": 500 },
            "hits": 100, "hitsMax": 2_000_000, "decayTime": 1000
        });
        assert!(validate(&ok).is_ok());
        // 非 powerBank 记录静默通过（与其他域校验同一分派口径）。
        assert!(validate(&serde_json::json!({"structureType": "spawn"})).is_ok());
        // 权威键非法形态逐键拒绝（恢复/编辑通道 fail-closed）。
        for bad in [
            serde_json::json!({"structureType":"powerBank","hits":-1}),
            serde_json::json!({"structureType":"powerBank","hits":1.5}),
            serde_json::json!({"structureType":"powerBank","decayTime":9.1e15}),
            serde_json::json!({"structureType":"powerBank","hitsMax":"x"}),
            serde_json::json!({"structureType":"powerBank","store":{"power":-5}}),
            serde_json::json!({"structureType":"powerBank","store":"x"}),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
    }
}
