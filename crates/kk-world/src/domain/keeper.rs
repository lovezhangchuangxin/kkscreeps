//! keeper lair 的保存数值域与 NPC 归属；刷怪规则由 engine 执行。
use serde_json::{Map, Value, json};

/// 本项目以用户名保存归属，此名称保留给 Source Keeper，不能注册为玩家。
pub const OWNER: &str = "Source Keeper";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn clock(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| "keeperLair 时钟必须是非负安全整数".into())
}

/// null/缺键为未计时，0 保留上游的 falsy 语义。
pub(crate) fn validate(data: &Value) -> Result<(), String> {
    if data.get("structureType").and_then(Value::as_str) == Some("keeperLair") {
        validate_clock(data.get("nextSpawnTime"))?;
    }
    Ok(())
}

fn validate_clock(value: Option<&Value>) -> Result<(), String> {
    if let Some(value) = value.filter(|v| !v.is_null()) {
        clock(value)?;
    }
    Ok(())
}

/// 将倒计时换算为 JS 可精确表示的保存时钟，放置与结算共用。
///
/// # Errors
/// 加法溢出或结果超出安全整数范围时返回错误。
pub fn deadline(time: u64, delta: u64) -> Result<u64, String> {
    time.checked_add(delta)
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| "keeperLair 截止时间超出安全整数范围".into())
}

/// 放置倒计时换算为唯一保存键 nextSpawnTime；超限不生成候选对象。
pub(crate) fn normalize(data: &mut Map<String, Value>, time: u64) -> Result<(), String> {
    validate_clock(data.get("nextSpawnTime"))?;
    if let Some(delta) = data.remove("ticksToSpawn") {
        let deadline = deadline(time, clock(&delta)?)?;
        data.insert("nextSpawnTime".into(), json!(deadline));
    } else {
        data.entry("nextSpawnTime").or_insert(Value::Null);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeper_clock_preserves_falsy_values_and_rejects_invalid_or_overflowing_times() {
        for value in [Value::Null, json!(0), json!(MAX_SAFE_INTEGER)] {
            assert!(validate(&json!({"structureType":"keeperLair","nextSpawnTime":value})).is_ok());
        }
        for value in [json!(-1), json!(0.5), json!("3"), json!(u64::MAX)] {
            assert!(
                validate(&json!({"structureType":"keeperLair","nextSpawnTime":value})).is_err()
            );
            let mut data = json!({"ticksToSpawn":value}).as_object().unwrap().clone();
            assert!(normalize(&mut data, 1).is_err());
        }
        assert_eq!(deadline(1, MAX_SAFE_INTEGER - 1).unwrap(), MAX_SAFE_INTEGER);
        assert!(deadline(1, MAX_SAFE_INTEGER).is_err());
        assert!(deadline(u64::MAX, 1).is_err());
        let mut data = json!({"ticksToSpawn":0}).as_object().unwrap().clone();
        normalize(&mut data, 10).unwrap();
        assert_eq!(data.get("nextSpawnTime"), Some(&json!(10)));
        assert!(!data.contains_key("ticksToSpawn"));
    }
}
