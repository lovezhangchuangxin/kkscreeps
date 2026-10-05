//! 原生受击公式；只计算扣血量，不改动身体、队列或事件。

use kk_schema::room::BodyPart;
use kk_world::types::BodyKind;

/// 依身体顺序消耗减伤前的承伤额度，最后对总减伤量四舍五入一次。
/// 未强化部件也消耗额度，已毁部件额度为零；超出身体的伤害不再减免。
/// 保留原始整数再扣减伤，避免大额 JS 伤害经过浮点转换丢失低位。
/// 来源：engine/src/processor/intents/creeps/tick.js 的 _applyDamage。
pub(super) fn effective_damage(body: &[BodyPart], damage: i64) -> i64 {
    if damage <= 0 {
        return damage;
    }
    let mut remaining = damage as f64;
    let mut reduction = 0.0;
    for part in body {
        if remaining <= 0.0 {
            break;
        }
        let ratio = damage_ratio(part);
        let absorbed = (part.hits as f64 / ratio).min(remaining);
        reduction += absorbed * (1.0 - ratio);
        remaining -= absorbed;
    }
    damage - (reduction.round() as i64).min(damage)
}

fn damage_ratio(part: &BodyPart) -> f64 {
    if part.hits == 0 || BodyKind::from_code(part.kind) != Some(BodyKind::Tough) {
        return 1.0;
    }
    let Some(boost) = &part.boost else {
        return 1.0;
    };
    static BOOSTS: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let table = BOOSTS.get_or_init(|| {
        serde_json::from_str(kk_core::constants::BOOSTS).expect("内建 BOOSTS 常量须为有效 JSON")
    });
    table["tough"][boost]["damage"].as_f64().unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(kind: BodyKind, hits: u32, boost: Option<&str>) -> BodyPart {
        BodyPart {
            kind: kind.code(),
            hits,
            boost: boost.map(str::to_string),
        }
    }

    #[test]
    fn rounds_total_reduction_once_and_preserves_large_integer_damage() {
        let body = [part(BodyKind::Tough, 100, Some("GHO2"))];
        assert_eq!(effective_damage(&body, 1), 0, "round(0.5) 减免 1");
        assert_eq!(effective_damage(&body, 2), 1, "同轮两点伤害只取整一次");
        assert_eq!(effective_damage(&body, 3), 1);
        assert_eq!(effective_damage(&body, i64::MAX), i64::MAX - 100);
        assert_eq!(effective_damage(&[], i64::MAX), i64::MAX);
        assert_eq!(effective_damage(&body, 0), 0);
    }

    #[test]
    fn spends_remaining_hits_in_body_order_and_ignores_invalid_boosts() {
        let body = [
            part(BodyKind::Move, 100, Some("XGHO2")),
            part(BodyKind::Tough, 100, Some("unknown")),
            part(BodyKind::Tough, 0, Some("XGHO2")),
            part(BodyKind::Tough, 10, Some("GHO2")),
        ];
        assert_eq!(effective_damage(&body, 200), 200);
        assert_eq!(effective_damage(&body, 210), 205);
        assert_eq!(effective_damage(&body, 230), 220);
    }

    #[test]
    fn sums_different_tiers_before_rounding() {
        let body = [
            part(BodyKind::Tough, 1, Some("GO")),
            part(BodyKind::Tough, 1, Some("XGHO2")),
        ];
        // 两件完全消耗时总减免约 2.762；分别取整会错误地只减免 2。
        assert_eq!(effective_damage(&body, 5), 2);
    }
}
