//! typed 世界 → prelude"世界 JSON"投影（M0 契约形状，kk-runner beginTick 约定）。
//!
//! 字段名与 prelude wrapObject 的消费面对齐；这也是 embed findInRoom 的
//! Rust 侧查询面（matches_find 与 prelude room.find 是同一 FIND 语义的两层，
//! 行为必须一致——M0 第七轮"层间一致"教训）。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use crate::world::World;

impl World {
    /// 产出注入沙箱的世界 JSON。
    pub fn world_json(&self) -> Value {
        let mut objects = Vec::new();
        let mut rooms = serde_json::Map::new();
        for (name, state) in &self.rooms {
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), json!(name));
            entry.insert("terrain".into(), json!(&state.terrain.as_slice()[..]));
            let (available, capacity) = room_energy(state);
            entry.insert("energyAvailable".into(), json!(available));
            entry.insert("energyCapacityAvailable".into(), json!(capacity));
            rooms.insert(name.clone(), Value::Object(entry));
            for (id, object) in &state.objects {
                objects.push(object_json(name, *id, object, self.time));
            }
        }
        json!({
            "time": self.time,
            // M1：cpu 桩值与 M0 一致（真实预算桶接入是 M2 沙箱正式接入范围）
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "rooms": Value::Object(rooms),
            "objects": objects,
        })
    }

    /// 按 FIND 常量过滤房间内对象（embed findInRoom 的后端）。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<(crate::world::ObjId, Value)> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        state
            .objects
            .iter()
            .map(|(id, o)| (*id, object_json(room, *id, o, self.time)))
            .filter(|(_, o)| matches_find(o, find_const))
            .collect()
    }

    /// 房间内 controller 的坐标（embed getControllerPos 后端）。
    pub fn controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        let state = self.rooms.get(room)?;
        state.objects.values().find_map(|o| match o {
            RoomObject::Controller(ctl) => Some((ctl.x as u64, ctl.y as u64)),
            _ => None,
        })
    }

    /// 单对象投影（embed getObject 后端）。
    pub fn object_json_by_id(&self, id: crate::world::ObjId) -> Option<Value> {
        let (room, object) = self.get(id)?;
        Some(object_json(room, id, object, self.time))
    }
}

/// 房间能量面（官方 game.js:325-328：非 off 的 spawn/extension 计入；容量侧
/// spawn 固定 300、extension 按 RCL 表）。官方按查看者过滤自家结构，M1 单投影
/// 先按房间合计（灰区登记：多主人房间的 per-owner 视图 M2 玩家 API 层实现）。
fn room_energy(state: &crate::world::RoomState) -> (i64, i64) {
    let rcl = state
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(ctl) => Some(ctl.level),
            _ => None,
        })
        .unwrap_or(0);
    let mut available = 0i64;
    let mut capacity = 0i64;
    for o in state.objects.values() {
        match o {
            RoomObject::Spawn(s) if !s.off => {
                available += s.store.energy as i64;
                capacity += c::SPAWN_ENERGY_CAPACITY;
            }
            RoomObject::Extension(e) if !e.off => {
                available += e.store.energy as i64;
                capacity += crate::types::extension_energy_capacity(rcl);
            }
            _ => {}
        }
    }
    (available, capacity)
}

/// 对象 → 世界 JSON 记录（字段名 = prelude wrapObject 消费面约定）。
/// pub(crate)：snapshot.rs 的查询面过滤复用同源投影（FIND 语义单源）。
pub(crate) fn object_json(
    room: &str,
    id: crate::world::ObjId,
    object: &RoomObject,
    time: u64,
) -> Value {
    let id_str = id.as_str();
    let mut v = match object {
        RoomObject::Creep(o) => json!({
            "type": "creep", "id": id_str, "room": room, "x": o.x, "y": o.y,
            "name": o.name, "owner": o.owner,
            "body": o.body.iter().map(part_json).collect::<Vec<_>>(),
            "fatigue": o.fatigue,
            "ageTime": o.age_time,
            "ticksToLive": o.age_time.saturating_sub(time),
            "hits": o.hits,
            "hitsMax": o.body.len() * 100,
            "store": { "energy": o.store.energy },
            "spawning": o.spawning,
        }),
        RoomObject::Source(o) => json!({
            "type": "structure", "structureType": "source",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "energy": o.energy, "energyCapacity": o.energy_capacity,
            "ticksToRegeneration": match o.next_regeneration_time {
                Some(t) => t.saturating_sub(time),
                None => 0,
            },
        }),
        RoomObject::Controller(o) => {
            let mut v = json!({
                "type": "structure", "structureType": "controller",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "level": o.level, "progress": o.progress,
                "progressTotal": controller_progress_total(o.level),
                "safeMode": o.safe_mode,
                "safeModeAvailable": o.safe_mode_available,
                "downgradeTime": o.downgrade_time,
                "isPowerEnabled": o.is_power_enabled,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        RoomObject::Spawn(o) => json!({
            "type": "spawn", "structureType": "spawn",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "name": o.name, "owner": o.owner,
            "store": { "energy": o.store.energy },
            "energy": o.store.energy,
            "energyCapacity": c::SPAWN_ENERGY_CAPACITY,
            "hits": o.hits, "hitsMax": c::SPAWN_HITS,
            "spawning": match &o.spawning {
                None => Value::Null,
                Some(job) => json!({
                    "name": job.name, "needTime": job.need_time,
                    "spawnTime": job.spawn_time,
                    "directions": job.directions,
                }),
            },
        }),
        RoomObject::Extension(o) => json!({
            "type": "structure", "structureType": "extension",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": { "energy": o.store.energy },
            "energy": o.store.energy,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
        }),
        RoomObject::ConstructedWall(o) => json!({
            "type": "structure", "structureType": "constructedWall",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": c::WALL_HITS_MAX,
        }),
        RoomObject::Road(o) => json!({
            "type": "structure", "structureType": "road",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": c::ROAD_HITS,
            "nextDecayTime": o.next_decay_time,
        }),
        RoomObject::Storage(o) => json!({
            "type": "structure", "structureType": "storage",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": { "energy": o.store.energy },
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
        }),
        RoomObject::ConstructionSite(o) => {
            let mut v = json!({
                "type": "constructionSite",
                "structureType": o.structure_type,
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "progress": o.progress,
                "progressTotal": o.progress_total,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            if let Some(name) = &o.name {
                v["name"] = json!(name);
            }
            v
        }
        RoomObject::Energy(o) => json!({
            "type": "resource",
            "resourceType": if o.resource_type == 0 { "energy" } else { "power" },
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "amount": o.amount,
        }),
        RoomObject::Flag(o) => json!({
            "type": "flag",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "name": o.name, "color": o.color, "secondaryColor": o.secondary_color,
        }),
        RoomObject::Generic(o) => {
            // 透传桶：spec 折算 JSON 原样还原 + 结构化字段覆盖
            let mut v: Value = serde_json::from_str(&o.data).unwrap_or(json!({}));
            if let Some(map) = v.as_object_mut() {
                map.insert("id".into(), json!(id_str));
                map.insert("room".into(), json!(room));
                map.insert("x".into(), json!(o.x));
                map.insert("y".into(), json!(o.y));
            }
            v
        }
    };
    // M0 契约：快照查询（getObject/findInRoom）附带 pos 组装
    if let Some(obj) = v.as_object_mut() {
        let x = obj.get("x").cloned().unwrap_or(json!(0));
        let y = obj.get("y").cloned().unwrap_or(json!(0));
        let room_name = obj.get("room").cloned().unwrap_or(json!(""));
        obj.insert(
            "pos".into(),
            json!({ "x": x, "y": y, "roomName": room_name }),
        );
    }
    v
}

pub(crate) fn part_json(p: &kk_schema::room::BodyPart) -> Value {
    let kind = crate::types::BodyKind::from_code(p.kind).map(crate::types::BodyKind::name);
    match (&p.boost, kind) {
        (Some(b), Some(k)) => json!({ "type": k, "hits": p.hits, "boost": b }),
        (None, Some(k)) => json!({ "type": k, "hits": p.hits }),
        (_, None) => json!({ "hits": p.hits }),
    }
}

/// 官方 progressTotal：1..7 级查 CONTROLLER_LEVELS，8 级为 0。
fn controller_progress_total(level: u8) -> i64 {
    if level >= 8 {
        return 0;
    }
    let key = level.to_string();
    c::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

fn obj_str<'a>(o: &'a Value, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

fn obj_energy(o: &Value) -> i64 {
    o.get("energy").and_then(Value::as_i64).unwrap_or(3000)
}

/// FIND 常量过滤（dumb.rs 语义原样迁移；与 prelude room.find 两层一致）。
pub fn matches_find(o: &Value, find_const: i64) -> bool {
    let kind = obj_str(o, "type");
    let st = obj_str(o, "structureType");
    let is_struct = kind == "structure" || kind == "spawn";
    match find_const {
        f if f == c::FIND_CREEPS || f == c::FIND_MY_CREEPS => kind == "creep",
        // 104 官方条件 energy > 0（game.js:23）
        f if f == c::FIND_SOURCES_ACTIVE => {
            kind == "structure" && st == "source" && obj_energy(o) > 0
        }
        f if f == c::FIND_SOURCES => kind == "structure" && st == "source",
        f if f == c::FIND_DROPPED_RESOURCES => kind == "resource",
        f if f == c::FIND_STRUCTURES || f == c::FIND_MY_STRUCTURES => {
            // 官方 FIND_STRUCTURES 不含 source/mineral：game.js:229-251 的
            // structureTypes 表无此三类（source/mineral/deposit 是独立装配分支
            // game.js:329-345）；vanilla 查询面 findTypeMap 同构
            // （snapshots.ts:478-491）
            is_struct && st != "source" && st != "mineral"
        }
        f if f == c::FIND_FLAGS => kind == "flag",
        f if f == c::FIND_CONSTRUCTION_SITES => kind == "constructionSite",
        f if f == c::FIND_MY_SPAWNS => kind == "spawn",
        f if f == c::FIND_MINERALS => kind == "structure" && st == "mineral",
        f if f == c::FIND_TOMBSTONES => kind == "tombstone",
        f if f == c::FIND_POWER_CREEPS => kind == "powerCreep",
        f if f == c::FIND_DEPOSITS => kind == "deposit",
        f if f == c::FIND_RUINS => kind == "ruin",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::{PlaceCtx, normalize};
    use kk_core::Terrain;
    use kk_schema::room::ControllerObj;

    /// 投影形状与 M0 契约对表：rooms 带 terrain 数组、对象字段名对齐
    /// prelude wrapObject、FIND 过滤两层一致、pos 组装。
    #[test]
    fn projection_shape_and_find() {
        let mut world = World::new(42);
        world.time = 55;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);

        let ctl = ControllerObj {
            id: 0,
            x: 1,
            y: 1,
            level: 2,
            safe_mode: 0,
            safe_mode_available: 1,
            progress: 10,
            downgrade_time: None,
            owner: Some("p1".into()),
            is_power_enabled: false,
        };
        let ctx = PlaceCtx {
            time: world.time,
            controller: Some(&ctl),
            name_seq: 0,
        };
        let controller = normalize(
            "structure",
            &serde_json::json!({
                "pos": [1, 1], "structureType": "controller", "rcl": 2, "owner": "p1",
            }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", controller).unwrap();
        let source = normalize(
            "structure",
            &serde_json::json!({
                "pos": [10, 10], "structureType": "source",
            }),
            &ctx,
        )
        .unwrap();
        let source_id = world.insert("W1N1", source).unwrap();
        let creep = normalize(
            "creep",
            &serde_json::json!({
                "pos": [20, 20], "owner": "p1", "body": ["move", "work"], "name": "w1",
            }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", creep).unwrap();
        let j = world.world_json();
        assert_eq!(j["time"], 55);
        assert_eq!(j["rooms"]["W1N1"]["name"], "W1N1");
        assert_eq!(
            j["rooms"]["W1N1"]["terrain"].as_array().unwrap().len(),
            2500
        );

        // FIND 两层：104 按 energy>0（源 3000 满）
        let sources = world.find_in_room("W1N1", c::FIND_SOURCES_ACTIVE);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, source_id);
        assert_eq!(world.find_in_room("W1N1", c::FIND_CREEPS).len(), 1);
        assert_eq!(world.find_in_room("W1N1", c::FIND_MY_SPAWNS).len(), 0);

        // 对象字段名对齐 prelude
        let (_, creep_json) = world.find_in_room("W1N1", c::FIND_CREEPS)[0].clone();
        assert_eq!(creep_json["name"], "w1");
        // age_time(=time+1500) − time
        assert_eq!(creep_json["ticksToLive"], 1500);
        assert_eq!(creep_json["pos"]["roomName"], "W1N1");
        assert_eq!(creep_json["body"][0]["type"], "move");

        // controller pos 查询
        assert_eq!(world.controller_pos("W1N1"), Some((1, 1)));
    }

    /// tower 等结算范围外对象进 Generic 桶后仍对 FIND_* 可见（M0 接受面）。
    #[test]
    fn generic_objects_stay_visible_to_find() {
        let mut world = World::new(1);
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let ctx = PlaceCtx {
            time: 0,
            controller: None,
            name_seq: 0,
        };
        let tower = normalize(
            "structure",
            &serde_json::json!({
                "pos": [7, 7], "structureType": "tower", "owner": "p1",
            }),
            &ctx,
        )
        .unwrap();
        let tower_id = world.insert("W1N1", tower).unwrap();
        let tombstone = normalize(
            "tombstone",
            &serde_json::json!({ "pos": [8, 8], "store": { "energy": 30 } }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", tombstone).unwrap();

        let structs = world.find_in_room("W1N1", c::FIND_STRUCTURES);
        assert_eq!(structs.len(), 1, "tower 必须对 FIND_STRUCTURES 可见");
        assert_eq!(structs[0].0, tower_id);
        assert_eq!(structs[0].1["type"], "structure");
        assert_eq!(structs[0].1["structureType"], "tower");
        assert_eq!(world.find_in_room("W1N1", c::FIND_TOMBSTONES).len(), 1);
        // 投影单对象可见
        let j = world.object_json_by_id(tower_id).unwrap();
        assert_eq!(j["structureType"], "tower");
        assert_eq!(j["pos"]["x"], 7);
    }
}
