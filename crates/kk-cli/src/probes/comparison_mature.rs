//! 成熟房共同对照的公开状态；按位置/名字归一，排除实现自有对象 ID。
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

fn store(object: &Value) -> Value {
    let values = object["store"]
        .as_object()
        .map(|s| {
            s.iter()
                .filter(|(_, v)| v.as_i64().is_some_and(|n| n > 0))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<_, _>>()
        })
        .unwrap_or_default();
    Value::Object(values)
}

pub(super) fn observe(engine: &EmbedEngine, rooms: &[Value]) -> anyhow::Result<Vec<Value>> {
    rooms.iter().map(|r| {
        let name = r["name"].as_str().ok_or_else(|| anyhow::anyhow!("缺少房间名"))?;
        let objects = engine.find_in_room(name, kk_core::constants::FIND_STRUCTURES);
        let controller = objects.iter().find(|o| o["structureType"] == "controller")
            .ok_or_else(|| anyhow::anyhow!("房间 {name} 缺少控制器"))?;
        let mut structures: Vec<Value> = objects.iter().filter(|o| o["structureType"] != "controller")
            .map(|o| json!({"type":o["structureType"], "x":o["pos"]["x"], "y":o["pos"]["y"],
                "hits":o["hits"], "store":store(o), "cooldown":o["cooldown"].as_i64().unwrap_or(0),
                "spawning":o.get("spawning").is_some_and(|v| !v.is_null())})).collect();
        structures.sort_by_key(|o| (o["type"].as_str().unwrap_or("").to_owned(), o["y"].as_i64(), o["x"].as_i64()));
        let mut creeps: Vec<Value> = engine.find_in_room(name, kk_core::constants::FIND_CREEPS)
            .iter().map(|o| json!({"name":o["name"], "x":o["pos"]["x"], "y":o["pos"]["y"],
                "hits":o["hits"], "store":store(o), "fatigue":o["fatigue"],
                "ttl":o["ticksToLive"], "spawning":o["spawning"],
                "body":o["body"].as_array().map(|parts| parts.iter().map(|p|p["type"].clone()).collect::<Vec<_>>())})).collect();
        creeps.sort_by_key(|o| o["name"].as_str().unwrap_or("").to_owned());
        let mut sources: Vec<Value> = engine.find_in_room(name, kk_core::constants::FIND_SOURCES)
            .iter().map(|o| json!({"x":o["pos"]["x"], "y":o["pos"]["y"],
                "energy":o["energy"], "capacity":o["energyCapacity"]})).collect();
        sources.sort_by_key(|o| (o["y"].as_i64(), o["x"].as_i64()));
        let mut minerals: Vec<Value> = engine.find_in_room(name, kk_core::constants::FIND_MINERALS)
            .iter().map(|o| json!({"x":o["pos"]["x"], "y":o["pos"]["y"],
                "type":o["mineralType"], "amount":o["mineralAmount"]})).collect();
        minerals.sort_by_key(|o| (o["y"].as_i64(), o["x"].as_i64()));
        Ok(json!({"room":name,"level":controller["level"],"structures":structures,
            "creeps":creeps,"sources":sources,"minerals":minerals}))
    }).collect()
}
