//! M0 哑世界：只有对象存取与时间计数，**没有任何游戏规则**。
//!
//! 服务于两个 M0 场景：screeps-ok 适配器（kk-engine::embed）与 bot 桩环境。
//! 没有移动、战斗、fatigue、intent 结算——那些从 M1 起在 kk-engine 里构建。
//! 对象记录是"世界 JSON"形状（kk-runner prelude 的约定）：
//! `{ id, type, room, x, y, structureType?, name?, owner?, store?, body?, ... }`。

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// 官方语义常量（来自 common/lib/constants.js，M1 由 kk-core 正式生成取代硬编码）。
pub mod find {
    pub const CREEPS: i64 = 101;
    pub const MY_CREEPS: i64 = 102;
    pub const SOURCES_ACTIVE: i64 = 104;
    pub const SOURCES: i64 = 105;
    pub const DROPPED_RESOURCES: i64 = 106;
    pub const STRUCTURES: i64 = 107;
    pub const MY_STRUCTURES: i64 = 108;
    pub const FLAGS: i64 = 110;
    pub const CONSTRUCTION_SITES: i64 = 111;
    pub const MY_SPAWNS: i64 = 112;
    pub const MINERALS: i64 = 116;
    pub const TOMBSTONES: i64 = 118;
    pub const POWER_CREEPS: i64 = 119;
    pub const DEPOSITS: i64 = 122;
    pub const RUINS: i64 = 123;
}

#[derive(Debug, Default)]
pub struct RoomRecord {
    pub name: String,
    /// 地形（50×50 平面数组，语义同官方 Terrain spec；M0 只存不用）。
    pub terrain: Option<Vec<u8>>,
}

#[derive(Debug, Default)]
pub struct DumbWorld {
    pub time: u64,
    /// 已经过玩家 tick（setTerrain 只允许在此为 0 时调用——screeps-ok 契约）。
    pub player_ticks: u64,
    pub rooms: BTreeMap<String, RoomRecord>,
    pub objects: BTreeMap<String, Value>,
    next_id: u64,
}

impl DumbWorld {
    pub fn new() -> Self {
        Self::default()
    }

    /// 完全重置（createShard 语义）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 生成一个不透明对象 id（screeps-ok 契约：id 不透明、不得假设格式）。
    pub fn next_object_id(&mut self) -> String {
        self.next_id += 1;
        format!("kk{:016x}", self.next_id)
    }

    pub fn register_room(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.rooms.entry(name.clone()).or_insert(RoomRecord {
            name,
            terrain: None,
        });
    }

    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> Result<(), String> {
        if self.player_ticks > 0 {
            // screeps-ok 契约：首个玩家 tick 之后必须抛错且不改变状态
            return Err("setTerrain 只允许在第一个玩家 tick 之前调用".into());
        }
        let record = self
            .rooms
            .get_mut(room)
            .ok_or_else(|| format!("房间 {room} 不存在"))?;
        record.terrain = Some(terrain);
        Ok(())
    }

    /// 摆放一个对象（记录已是世界 JSON 形状，含 id）。返回该 id。
    pub fn place(&mut self, mut record: Value) -> String {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| self.next_object_id());
        if record.get("id").is_none() {
            record["id"] = json!(id);
        }
        if let Some(room) = record.get("room").and_then(Value::as_str) {
            self.register_room(room);
        }
        self.objects.insert(id.clone(), record);
        id
    }

    pub fn get(&self, id: &str) -> Option<&Value> {
        self.objects.get(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Value> {
        self.objects.remove(id)
    }

    /// 按 FIND 常量过滤房间内对象。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<&Value> {
        self.objects
            .values()
            .filter(|o| o.get("room").and_then(Value::as_str) == Some(room))
            .filter(|o| matches_find(o, find_const))
            .collect()
    }

    /// 房间内的 controller 位置（getControllerPos）。
    pub fn controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        self.objects
            .values()
            .find(|o| {
                o.get("room").and_then(Value::as_str) == Some(room)
                    && o.get("type").and_then(Value::as_str) == Some("structure")
                    && o.get("structureType").and_then(Value::as_str) == Some("controller")
            })
            .and_then(|o| {
                let x = o.get("x")?.as_u64()?;
                let y = o.get("y")?.as_u64()?;
                Some((x, y))
            })
    }

    /// 产出注入沙箱的"世界 JSON"（kk-runner prelude 的 beginTick 约定）。
    pub fn world_json(&self) -> Value {
        json!({
            "time": self.time,
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "rooms": self
                .rooms
                .iter()
                .map(|(name, r)| {
                    // 地形随快照下发（getTerrainAt 等官方语义消费；无地形的世界省略该字段）
                    let mut entry = serde_json::Map::new();
                    entry.insert("name".into(), json!(name));
                    if let Some(t) = &r.terrain {
                        entry.insert("terrain".into(), json!(t));
                    }
                    (name.clone(), Value::Object(entry))
                })
                .collect::<BTreeMap<_, _>>(),
            "objects": self.objects.values().cloned().collect::<Vec<_>>(),
        })
    }
}

fn obj_str<'a>(o: &'a Value, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

/// source 初态能量（energy 缺省视为 3000——与 prelude wrapObject 的投影缺省一致）。
fn obj_energy(o: &Value) -> i64 {
    o.get("energy").and_then(Value::as_i64).unwrap_or(3000)
}

fn matches_find(o: &Value, find_const: i64) -> bool {
    let kind = obj_str(o, "type").unwrap_or("");
    let st = obj_str(o, "structureType").unwrap_or("");
    let is_struct = kind == "structure" || kind == "spawn";
    match find_const {
        f if f == find::CREEPS || f == find::MY_CREEPS => kind == "creep",
        // 104 的官方条件是 energy > 0（game.js:23）——哑世界按初态能量过滤；
        // 与 prelude room.find 的 104/105 分支对齐（同一 FIND 语义两层一致）
        f if f == find::SOURCES_ACTIVE => {
            kind == "structure" && st == "source" && obj_energy(o) > 0
        }
        f if f == find::SOURCES => kind == "structure" && st == "source",
        f if f == find::DROPPED_RESOURCES => kind == "resource",
        f if f == find::STRUCTURES || f == find::MY_STRUCTURES => is_struct,
        f if f == find::FLAGS => kind == "flag",
        f if f == find::CONSTRUCTION_SITES => kind == "constructionSite",
        f if f == find::MY_SPAWNS => kind == "spawn",
        f if f == find::MINERALS => kind == "structure" && st == "mineral",
        f if f == find::TOMBSTONES => kind == "tombstone",
        f if f == find::POWER_CREEPS => kind == "powerCreep",
        f if f == find::DEPOSITS => kind == "deposit",
        f if f == find::RUINS => kind == "ruin",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_and_find() {
        let mut w = DumbWorld::new();
        w.register_room("W1N1");
        let id = w.place(json!({
            "type": "creep", "room": "W1N1", "x": 25, "y": 25,
            "name": "w1", "body": ["move"], "owner": "p1",
        }));
        assert!(id.starts_with("kk"));
        assert_eq!(w.find_in_room("W1N1", find::CREEPS).len(), 1);
        assert_eq!(w.find_in_room("W1N1", find::STRUCTURES).len(), 0);
        assert_eq!(w.get(&id).unwrap()["name"], "w1");
    }

    #[test]
    fn terrain_guard_after_player_tick() {
        let mut w = DumbWorld::new();
        w.register_room("W1N1");
        assert!(w.set_terrain("W1N1", vec![0; 2500]).is_ok());
        w.player_ticks = 1;
        assert!(w.set_terrain("W1N1", vec![0; 2500]).is_err());
    }
}
