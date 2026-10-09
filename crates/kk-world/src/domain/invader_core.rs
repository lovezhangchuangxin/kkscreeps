//! invaderCore 的保存数值域与 NPC 归属；部署/坍塌/孵化与预定规则由 engine 执行。
use serde_json::{Map, Value, json};

use super::npc::INVADER_OWNER;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
/// 官方 invader core 等级 0..=5（stronghold 模板与常量表按级取值）。
const MAX_LEVEL: i64 = 5;

fn clock(value: &Value) -> Result<u64, String> {
    value
        .as_u64()
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| "invaderCore 时钟必须是非负安全整数".into())
}

/// 将倒计时换算为 JS 可精确表示的保存时钟，放置与结算共用。
///
/// # Errors
/// 加法溢出或结果超出安全整数范围时返回错误。
pub fn deadline(time: u64, delta: u64) -> Result<u64, String> {
    time.checked_add(delta)
        .filter(|n| *n <= MAX_SAFE_INTEGER)
        .ok_or_else(|| "invaderCore 截止时间超出安全整数范围".into())
}

/// null/缺键为无值；deployTime 与 spawning 仅接受安全整数时钟。
pub(crate) fn validate(data: &Value) -> Result<(), String> {
    if data.get("structureType").and_then(Value::as_str) != Some("invaderCore") {
        return Ok(());
    }
    if data.get("owner").and_then(Value::as_str) != Some(INVADER_OWNER) {
        return Err("invaderCore 归属固定为 Invader，不接受其他归属".into());
    }
    let level = data
        .get("level")
        .and_then(Value::as_i64)
        .ok_or_else(|| "invaderCore level 必须是 0..=5 整数".to_string())?;
    if !(0..=MAX_LEVEL).contains(&level) {
        return Err("invaderCore level 必须是 0..=5 整数".into());
    }
    if let Some(value) = data.get("deployTime").filter(|v| !v.is_null()) {
        clock(value)?;
    }
    if let Some(spawning) = data.get("spawning").filter(|v| !v.is_null()) {
        let spawning = spawning
            .as_object()
            .ok_or_else(|| "invaderCore spawning 必须是对象或 null".to_string())?;
        if spawning
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err("invaderCore spawning.name 必须是非空字符串".into());
        }
        for key in ["needTime", "spawnTime"] {
            clock(
                spawning
                    .get(key)
                    .ok_or_else(|| format!("invaderCore spawning.{key} 必须是非负安全整数"))?,
            )?;
        }
    }
    Ok(())
}

/// 放置归一：ticksToDeploy 换算为唯一保存键 deployTime；spawning 的
/// 相对 remainingTime 换算为绝对 spawnTime 并剥离 body（孵化 creep 的
/// 身体由放置层从原始 spec 另行构造，不进核心数据）；ticksToCollapse
/// 只做值域校验后移除——坍塌效果由放置层写入房间效果表，不在对象数据。
pub(crate) fn normalize(data: &mut Map<String, Value>, time: u64) -> Result<(), String> {
    if let Some(delta) = data.remove("ticksToDeploy") {
        let deadline = deadline(time, clock(&delta)?)?;
        data.insert("deployTime".into(), json!(deadline));
    } else {
        data.entry("deployTime").or_insert(Value::Null);
    }
    if let Some(delta) = data.remove("ticksToCollapse") {
        // 值域与 ticksToDeploy 同规；换算由放置层经 deadline 完成。
        clock(&delta)?;
    }
    if let Some(spawning) = data.remove("spawning") {
        let spawning = spawning
            .as_object()
            .ok_or_else(|| "invaderCore spawning 必须是对象".to_string())?;
        let name = spawning
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
            .ok_or_else(|| "invaderCore spawning.name 必须是非空字符串".to_string())?;
        let remaining = spawning
            .get("remainingTime")
            .ok_or_else(|| "invaderCore spawning 缺 remainingTime".to_string())?;
        let spawn_time = deadline(time, clock(remaining)?)?;
        let need_time = match spawning.get("needTime") {
            None => clock(remaining)?,
            Some(value) => clock(value)?,
        };
        data.insert(
            "spawning".into(),
            json!({"name": name, "needTime": need_time, "spawnTime": spawn_time}),
        );
    } else {
        data.entry("spawning").or_insert(Value::Null);
    }
    if !data.contains_key("level") {
        data.insert("level".into(), json!(0));
    }
    // 官方核心满血放置（vanilla 适配器与 deployStronghold 同值）；
    // 受击/耐久面经 Generic hits 键参与常规伤害链。
    let hits = serde_json::json!(kk_core::constants::INVADER_CORE_HITS);
    data.entry("hits").or_insert(hits.clone());
    data.entry("hitsMax").or_insert(hits);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invader_core_domain_preserves_value_ranges_and_rejects_invalid_shapes() {
        let valid = json!({
            "structureType":"invaderCore","owner":INVADER_OWNER,"level":3,
            "deployTime":10,"spawning":{"name":"d1","needTime":12,"spawnTime":9}
        });
        assert!(validate(&valid).is_ok());
        for level in [json!(-1), json!(6), json!(1.5), json!("2")] {
            assert!(
                validate(
                    &json!({"structureType":"invaderCore","owner":INVADER_OWNER,"level":level})
                )
                .is_err()
            );
        }
        assert!(validate(&json!({"structureType":"invaderCore","owner":"p1","level":1})).is_err());
        // null 为「未孵化」合法形态，与 deployTime null 同形。
        assert!(
            validate(
                &json!({"structureType":"invaderCore","owner":INVADER_OWNER,"level":0,
                    "spawning":Value::Null})
            )
            .is_ok()
        );
        for spawning in [
            json!("x"),
            json!(3),
            json!({"name":"","needTime":1,"spawnTime":1}),
            json!({"name":"d"}),
            json!({"name":"d","needTime":-1,"spawnTime":1}),
            json!({"name":"d","needTime":1,"spawnTime":u64::MAX}),
        ] {
            assert!(
                validate(
                    &json!({"structureType":"invaderCore","owner":INVADER_OWNER,"level":0,
                        "spawning":spawning})
                )
                .is_err()
            );
        }
        // 非 invaderCore 的 Generic 对象不受本域校验。
        assert!(validate(&json!({"structureType":"keeperLair"})).is_ok());
    }

    #[test]
    fn invader_core_normalize_converts_relative_keys_and_strips_body() {
        let mut data = json!({
            "ticksToDeploy":20,
            "ticksToCollapse":6,
            "spawning":{"name":"defender1","body":["attack","move"],"needTime":12,"remainingTime":6}
        })
        .as_object()
        .unwrap()
        .clone();
        normalize(&mut data, 1).unwrap();
        assert_eq!(data.get("deployTime"), Some(&json!(21)));
        assert_eq!(
            data.get("spawning"),
            Some(&json!({"name":"defender1","needTime":12,"spawnTime":7}))
        );
        assert!(!data.contains_key("ticksToDeploy"));
        assert!(!data.contains_key("ticksToCollapse"));
        // 相对键缺失时补空值形态，与 keeperLair 的 nextSpawnTime 同形。
        let mut data = Map::new();
        normalize(&mut data, 5).unwrap();
        assert_eq!(data.get("deployTime"), Some(&Value::Null));
        assert_eq!(data.get("spawning"), Some(&Value::Null));
        assert_eq!(data.get("level"), Some(&json!(0)));
        // needTime 缺省取 remainingTime（官方 create-creep 同拍生成的等价形态）。
        let mut data = json!({"spawning":{"name":"d","remainingTime":6}})
            .as_object()
            .unwrap()
            .clone();
        normalize(&mut data, 10).unwrap();
        assert_eq!(
            data.get("spawning"),
            Some(&json!({"name":"d","needTime":6,"spawnTime":16}))
        );
        // 非法相对值整单拒绝。
        for bad in [json!(-1), json!(0.5), json!(u64::MAX), json!("3")] {
            let mut data = json!({"ticksToDeploy": bad}).as_object().unwrap().clone();
            assert!(normalize(&mut data, 1).is_err());
            let mut data = json!({"spawning":{"name":"d","remainingTime":bad}})
                .as_object()
                .unwrap()
                .clone();
            assert!(normalize(&mut data, 1).is_err());
        }
        assert_eq!(deadline(1, MAX_SAFE_INTEGER - 1).unwrap(), MAX_SAFE_INTEGER);
        assert!(deadline(1, MAX_SAFE_INTEGER).is_err());
        assert!(deadline(u64::MAX, 1).is_err());
    }
}
