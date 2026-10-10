//! 房间公共状态域：novice/respawn/closed 种子与到期墙钟。
//!
//! 官方 db.rooms 没有四态枚举——novice 区/respawn 区/定时关闭由三个
//! 墙钟毫秒时间戳（`novice`/`respawnArea`/`openTime`）表达，driver
//! `updateRoomStatusData` 折叠成玩家面 `roomStatusData` 三张表。kk 把
//! 种子态聚合为本域（`RoomState.room_status`，normal 为 None 随完整
//! 检查点保存），投影层还原官方玩家面形态；时间戳语义与官方一致为
//! Unix 毫秒墙钟（官方到期判定 `> Date.now()`）。保存原始种子；消费者
//! 按输入墙钟求有效状态，过期即 normal，避免恢复后重新激活旧保护。

/// 非 normal 的房间公共状态种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomStatusKind {
    Novice,
    Respawn,
    Closed,
}

impl RoomStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoomStatusKind::Novice => "novice",
            RoomStatusKind::Respawn => "respawn",
            RoomStatusKind::Closed => "closed",
        }
    }

    /// 从字符串解析（createShard 种子与检查点恢复共用）。
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "novice" => Ok(RoomStatusKind::Novice),
            "respawn" => Ok(RoomStatusKind::Respawn),
            "closed" => Ok(RoomStatusKind::Closed),
            other => Err(format!("未知房间状态: {other}")),
        }
    }
}

/// 一条房间公共状态种子：种类与到期墙钟（Unix 毫秒）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomStatus {
    pub status: RoomStatusKind,
    pub timestamp: u64,
}

impl RoomStatus {
    pub fn active_kind(&self, now_ms: u64) -> Option<RoomStatusKind> {
        (self.timestamp > now_ms).then_some(self.status)
    }
}

/// 管理命令的墙钟输入；结算和回放使用各自已记录的 TickInput.now_ms。
pub fn wall_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 值域校验：时间戳须为正安全整数（玩家面 `getRoomStatus` 断言
/// `timestamp > 0`；检查点恢复与 createShard 种子共用）。
pub fn validate(status: &RoomStatus) -> Result<(), String> {
    if status.timestamp == 0 {
        return Err("房间状态 timestamp 必须为正数（墙钟毫秒）".into());
    }
    if status.timestamp > crate::domain::deposit::MAX_SAFE_INTEGER {
        return Err("房间状态 timestamp 超出安全整数范围".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_roundtrip_and_rejects_unknown() {
        for kind in [
            RoomStatusKind::Novice,
            RoomStatusKind::Respawn,
            RoomStatusKind::Closed,
        ] {
            assert_eq!(RoomStatusKind::parse(kind.as_str()), Ok(kind));
            assert_eq!(
                serde_json::to_value(kind).unwrap().as_str().unwrap(),
                kind.as_str()
            );
        }
        assert!(RoomStatusKind::parse("normal").is_err());
        assert!(RoomStatusKind::parse("").is_err());
    }

    #[test]
    fn expiry_is_strict_at_wall_clock_boundary() {
        let status = RoomStatus {
            status: RoomStatusKind::Closed,
            timestamp: 100,
        };
        assert_eq!(status.active_kind(99), Some(RoomStatusKind::Closed));
        assert_eq!(status.active_kind(100), None);
        assert_eq!(status.active_kind(101), None);
    }

    #[test]
    fn validate_bounds() {
        assert!(
            validate(&RoomStatus {
                status: RoomStatusKind::Novice,
                timestamp: 1
            })
            .is_ok()
        );
        assert!(
            validate(&RoomStatus {
                status: RoomStatusKind::Closed,
                timestamp: crate::domain::deposit::MAX_SAFE_INTEGER
            })
            .is_ok()
        );
        assert!(
            validate(&RoomStatus {
                status: RoomStatusKind::Respawn,
                timestamp: 0
            })
            .is_err()
        );
        assert!(
            validate(&RoomStatus {
                status: RoomStatusKind::Respawn,
                timestamp: crate::domain::deposit::MAX_SAFE_INTEGER + 1
            })
            .is_err()
        );
    }

    #[test]
    fn serde_shape_is_flat_two_key_object() {
        let value = serde_json::to_value(RoomStatus {
            status: RoomStatusKind::Respawn,
            timestamp: 1234,
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"status": "respawn", "timestamp": 1234})
        );
        // 未知键拒绝（deny_unknown_fields）。
        let bad = serde_json::from_value::<RoomStatus>(serde_json::json!({
            "status": "novice", "timestamp": 5, "extra": 1
        }));
        assert!(bad.is_err());
    }
}
