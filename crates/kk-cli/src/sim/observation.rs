//! 将已提交世界投影为模拟报告的观察快照。
use super::scene::Scene;
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn observe(
    engine: &EmbedEngine,
    scene: &Scene,
    aliases: &BTreeMap<String, String>,
    mod_names: &[String],
) -> Value {
    let players: BTreeMap<_, _> = scene.bots.iter().map(|bot| {
        let state = engine.player_state(&bot.player).unwrap();
        (bot.player.clone(), json!({"gcl": state.gcl, "rooms": state.rooms,
            "memory": serde_json::from_str::<Value>(&state.raw_memory).ok(), "raw_memory": state.raw_memory,
            "cpu_bucket": state.cpu.bucket()}))
    }).collect();
    let objects: BTreeMap<_, _> = aliases
        .iter()
        .map(|(name, id)| (name, engine.get_object(id)))
        .collect();
    let rooms: BTreeMap<_, _> = scene
        .shard
        .rooms
        .iter()
        .map(|room| {
            let name = room["name"].as_str().unwrap();
            let controller = engine
                .find_in_room(name, kk_core::constants::FIND_STRUCTURES)
                .into_iter()
                .find(|o| o["structureType"] == "controller");
            (name, json!({"controller": controller}))
        })
        .collect();
    let states: BTreeMap<_, _> = mod_names
        .iter()
        .map(|name| (name, engine.engine_mod_state(name)))
        .collect();
    json!({"time": engine.get_game_time(), "seed": engine.world_seed(), "players": players, "objects": objects, "rooms": rooms, "world": engine.world_snapshot(), "mod_objects": engine.mod_object_snapshots(), "mod_states": states})
}
