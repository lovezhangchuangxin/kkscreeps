//! deposit 的数值表示与放置时钟归一；不包含采集或冷却公式。
use serde_json::{Map, Value, json};

pub(crate) const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

fn unsigned(data: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    data.get(key)
        .map(|value| {
            value
                .as_u64()
                .filter(|n| *n <= MAX_SAFE_INTEGER)
                .ok_or_else(|| format!("deposit {key} 必须是非负安全整数"))
        })
        .transpose()
}

fn validate_fields(data: &Map<String, Value>) -> Result<(), String> {
    for key in ["cooldownTime", "decayTime", "harvested"] {
        unsigned(data, key)?;
    }
    Ok(())
}

/// 保存状态中的绝对时钟和累计量必须可由玩家 JS 精确表示。
/// 插入、管理编辑、候选发布和检查点恢复共用此校验。
pub(crate) fn validate(data: &Value) -> Result<(), String> {
    if data.get("type").and_then(Value::as_str) == Some("deposit") {
        validate_fields(data.as_object().ok_or("deposit 记录必须为对象")?)?;
    }
    Ok(())
}

/// 相对倒计时优先于合法的绝对键；缺失不补造，非法值不静默忽略。
/// 加法和结果范围分别检查，避免 debug panic 或 release 回绕。
pub(crate) fn normalize_times(data: &mut Map<String, Value>, time: u64) -> Result<(), String> {
    validate_fields(data)?;
    for (relative, absolute) in [("cooldown", "cooldownTime"), ("ticksToDecay", "decayTime")] {
        if let Some(delta) = unsigned(data, relative)? {
            let deadline = time
                .checked_add(delta)
                .filter(|n| *n <= MAX_SAFE_INTEGER)
                .ok_or_else(|| format!("deposit {relative} 换算后的截止时间超出安全整数范围"))?;
            data.insert(absolute.into(), json!(deadline));
        }
        data.remove(relative);
    }
    Ok(())
}
