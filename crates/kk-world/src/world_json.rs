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
    /// 产出注入沙箱的世界 JSON（全量——bot 验收/内部消费）。
    pub fn world_json(&self) -> Value {
        self.world_json_impl(None)
    }

    /// 按查看者过滤的世界 JSON（runPlayer 注入面，M2A-A2.1 视野裁切）：
    /// 只注入查看者有对象的房间（官方 runtimeData.get 的可见房间近似——
    /// driver/lib/runtime/data.js:103 按 userObjects 归集房间后拉全房对象），
    /// 房间内对象全量保留（可见房内的敌方对象官方同样可见）；附带 `user`
    /// 字段供 prelude 装配 per-viewer 集合（官方 runtimeData.user 等价物）。
    /// observer/预订等更宽的可见性来源随对应能力批扩展。
    pub fn world_json_for_viewer(&self, user: &str) -> Value {
        self.world_json_impl(Some(user))
    }

    fn world_json_impl(&self, user: Option<&str>) -> Value {
        let visible: std::collections::BTreeSet<&String> = match user {
            None => self.rooms.keys().collect(),
            Some(user) => self
                .rooms
                .iter()
                .filter(|(_, state)| {
                    state.objects.values().any(|o| {
                        // 官方视野归集（driver/lib/runtime/data.js:113-116）显式
                        // 跳过 flag 与 constructionSite——工地不授房视野（自审
                        // P2：工地授视野与官方相反，且 A4 接线 createConstructionSite
                        // 后变成玩家可自主触发的持续视野）。flag 无 owner 天然不授
                        !matches!(o, RoomObject::ConstructionSite(_))
                            && object_owner(o).as_deref() == Some(user)
                    })
                })
                .map(|(name, _)| name)
                .collect(),
        };
        let mut objects = Vec::new();
        let mut rooms = serde_json::Map::new();
        for (name, state) in &self.rooms {
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), json!(name));
            entry.insert("terrain".into(), json!(&state.terrain.as_slice()[..]));
            // RCL（extension 的 storeCapacityResource 归一与房间聚合共用）
            let rcl = room_rcl(state);
            if visible.contains(name) {
                let (available, capacity) = room_energy(state);
                entry.insert("energyAvailable".into(), json!(available));
                entry.insert("energyCapacityAvailable".into(), json!(capacity));
            }
            // 不可见房条目只保地形（Game.map 面全量地形，灰区登记口径）——
            // 房间聚合（energy 等）不注入，防未来任何读 rooms[n].energy* 的
            // 新代码泄漏敌房聚合（自审 P3：载荷面收窄，当前无消费者）
            rooms.insert(name.clone(), Value::Object(entry));
            let room_visible = visible.contains(name);
            for (id, object) in &state.objects {
                if !room_visible {
                    // 自家工地破例导出：官方按 user 全量拉自家对象
                    //（data.js:103），Game.constructionSites 收全部自家工地含
                    // 不可见房（game.js:366-374）——工地不授视野（可见性判定）
                    // 与工地全集进 Game.constructionSites（装配面）是两个正交
                    // 官方语义，裁掉工地视野后此破例使全集仍成立
                    let own_site = matches!(object, RoomObject::ConstructionSite(_))
                        && object_owner(object).as_deref() == Some(user.unwrap_or(""));
                    if !own_site {
                        continue;
                    }
                }
                objects.push(object_json(
                    name,
                    *id,
                    object,
                    self.time,
                    &state.terrain,
                    rcl,
                ));
            }
        }
        json!({
            "time": self.time,
            // M1：cpu 桩值与 M0 一致（真实预算桶接入是 M2 沙箱正式接入范围）
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": user,
            "rooms": Value::Object(rooms),
            "objects": objects,
        })
    }

    /// 按 FIND 常量过滤房间内对象（embed findInRoom 的后端）。
    pub fn find_in_room(&self, room: &str, find_const: i64) -> Vec<(crate::world::ObjId, Value)> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        let rcl = room_rcl(state);
        state
            .objects
            .iter()
            .map(|(id, o)| {
                (
                    *id,
                    object_json(room, *id, o, self.time, &state.terrain, rcl),
                )
            })
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
        let state = self.rooms.get(room)?;
        let terrain = &state.terrain;
        Some(object_json(
            room,
            id,
            object,
            self.time,
            terrain,
            room_rcl(state),
        ))
    }
}

/// 道路 hitsMax 的地形系数（官方 build.js:172-184：落成时把放大值同时写入
/// hits 与 hitsMax；同格 swamp/wall **对象**分支官方另有——typed 世界无该对象
/// 类型，与 damage.rs 修复上限同一近似）。snapshot.rs 复用本函数保持两投影一致。
pub(crate) fn road_hits_max(terrain: &kk_core::Terrain, x: u8, y: u8) -> i64 {
    let mut hits = c::ROAD_HITS;
    if terrain.check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_SWAMP) {
        hits *= c::CONSTRUCTION_COST_ROAD_SWAMP_RATIO;
    }
    if terrain.check_mask(x as usize, y as usize, kk_core::TERRAIN_MASK_WALL) {
        hits *= c::CONSTRUCTION_COST_ROAD_WALL_RATIO;
    }
    hits
}

/// 对象归属（视野过滤判定用）：typed 变体直读；Generic 透传解析 data.owner。
/// 官方 userObjects 按 DB 的 user 字段归集（data.js:103）——与投影面同源。
fn object_owner(object: &RoomObject) -> Option<std::borrow::Cow<'_, str>> {
    match object {
        RoomObject::Creep(o) => Some(o.owner.as_str().into()),
        RoomObject::Spawn(o) => Some(o.owner.as_str().into()),
        RoomObject::Extension(o) => Some(o.owner.as_str().into()),
        RoomObject::Storage(o) => Some(o.owner.as_str().into()),
        RoomObject::Controller(o) => o.owner.as_deref().map(Into::into),
        RoomObject::ConstructionSite(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Generic(o) => serde_json::from_str::<Value>(&o.data).ok().and_then(|v| {
            v.get("owner")
                .and_then(Value::as_str)
                .map(str::to_string)
                .map(Into::into)
        }),
        _ => None,
    }
}

/// 房间 RCL（无 controller 为 0）：extension 的 storeCapacityResource 归一
/// （官方 extensions/tick.js:11-17 每 tick 按 EXTENSION_ENERGY_CAPACITY[rcl]
/// 重写）与房间能量聚合共用。pub(crate)：snapshot.rs 查询面过滤复用。
pub(crate) fn room_rcl(state: &crate::world::RoomState) -> u8 {
    state
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(ctl) => Some(ctl.level),
            _ => None,
        })
        .unwrap_or(0)
}

/// 房间能量面（官方 game.js:325-328：非 off 的 spawn/extension 计入；容量侧
/// spawn 固定 300、extension 按 RCL 表）。官方按查看者过滤自家结构，M1 单投影
/// 先按房间合计（灰区登记：多主人房间的 per-owner 视图 M2 玩家 API 层实现）。
fn room_energy(state: &crate::world::RoomState) -> (i64, i64) {
    let rcl = room_rcl(state);
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
/// `rcl`：extension 的 storeCapacityResource.energy 归一（官方 extensions/
/// tick.js:11-17 每 tick 按房 RCL 查表重写——投影期直存等价）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn object_json(
    room: &str,
    id: crate::world::ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
    rcl: u8,
) -> Value {
    let id_str = id.as_str();
    let mut v = match object {
        RoomObject::Creep(o) => {
            // 官方 creeps.js:78 `o.ageTime ? o.ageTime - time : undefined`：
            // 孵化中的雏形不带 ageTime（create-creep.js:80-98 无该字段，出生后
            // 首个 creep tick 赋值——CREEP-SPAWNING-002 锁值）→ 键缺省（prelude
            // 读 undefined；快照面 remaining(0) 同构 null）。
            // store：energy 恒投影（既有形状）；ghodium（A4-iii 的
            // generateSafeMode 消耗面）按正量投影——0 值不出现（官方 store
            // getter 正量暴露的既有口径，vanilla snapStore 同构）
            let mut store = serde_json::Map::new();
            store.insert("energy".into(), json!(o.store.energy));
            if o.store.ghodium > 0 {
                store.insert(
                    kk_core::constants::RESOURCE_GHODIUM.to_string(),
                    json!(o.store.ghodium),
                );
            }
            let mut v = json!({
                "type": "creep", "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "body": o.body.iter().map(part_json).collect::<Vec<_>>(),
                "fatigue": o.fatigue,
                "ageTime": o.age_time,
                "hits": o.hits,
                "hitsMax": o.body.len() * 100,
                "store": Value::Object(store),
                // 官方 creep 落库带 storeCapacity（_recalc-body：活跃 CARRY ×
                // CARRY_CAPACITY × boost 乘数）——creeps.js carryCapacity 直读
                // `o.storeCapacity`、Store.getCapacity() 同源（A4-i Store 族）
                "storeCapacity": crate::snapshot::creep_store_capacity_raw(&o.body),
                "spawning": o.spawning,
            });
            if o.age_time != 0 {
                v["ticksToLive"] = json!(o.age_time.saturating_sub(time));
            }
            v
        }
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
                // B2 schema 字段的投影清偿（A4-iii）：绝对 tick / {user,endTime}
                // 官方 DB 原始形态——prelude getter（structures.js 对照
                // structures.js:143-169）与快照面（snapshot.rs）各自换算，
                // None → null（downgradeTime 同形）。sign 无 datetime 键
                //（墙钟不落盘，灰区表既有登记）
                "upgradeBlocked": o.upgrade_blocked,
                "safeModeCooldown": o.safe_mode_cooldown,
                "reservation": match &o.reservation {
                    Some(r) => json!({ "user": r.user, "endTime": r.end_time }),
                    None => Value::Null,
                },
                "sign": match &o.sign {
                    Some(s) => json!({ "user": s.user, "text": s.text, "time": s.time }),
                    None => Value::Null,
                },
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            } else if o.owner_cleared {
                // 官方降级归零 `user = null`（controllers/tick.js:56）的落库
                // 哨兵——玩家侧 my === false；从未拥有则无键（my undefined）
                v["owner"] = Value::Null;
            }
            v
        }
        RoomObject::Spawn(o) => {
            // 官方落库形态（build.js:119-127）：store + storeCapacityResource
            // {energy: SPAWN_ENERGY_CAPACITY}、无 storeCapacity 键——Store 三函数
            // 族按此分岔（getCapacity() 读 null、getCapacity('energy') 读 300）
            json!({
                "type": "spawn", "structureType": "spawn",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "store": { "energy": o.store.energy },
                "energy": o.store.energy,
                "energyCapacity": c::SPAWN_ENERGY_CAPACITY,
                "storeCapacityResource": { "energy": c::SPAWN_ENERGY_CAPACITY },
                // off（_calc_spawns 维护的 RCL 超限标记）：spawnCreep/
                // renewCreep/recycleCreep 的 data(this.id).off 显式回查面
                "off": o.off,
                "hits": o.hits, "hitsMax": c::SPAWN_HITS,
                "spawning": spawn_spawning_json(o),
            })
        }
        RoomObject::Extension(o) => json!({
            "type": "structure", "structureType": "extension",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": { "energy": o.store.energy },
            "energy": o.store.energy,
            // 同 spawn 的官方落库形态（build.js:130-137 + extensions/tick.js
            // :11-17 的 RCL 归一——投影期直存等价）
            "storeCapacityResource": { "energy": crate::types::extension_energy_capacity(rcl) },
            "off": o.off,
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
            "hits": o.hits, "hitsMax": road_hits_max(terrain, o.x, o.y),
            "nextDecayTime": o.next_decay_time,
        }),
        RoomObject::Storage(o) => json!({
            "type": "structure", "structureType": "storage",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": { "energy": o.store.energy },
            // 官方 storage 落库带数值 storeCapacity（build.js:150-156）、无
            // storeCapacityResource——Store.getCapacity() 读数值（A4-i Store 族）
            "storeCapacity": c::STORAGE_CAPACITY,
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

/// spawn 的孵化任务投影（官方 DB 形态 `{name, needTime, spawnTime, directions}`，
/// create-creep.js:68-74）：directions 未指定时官方 undefined——键不落库，
/// runtimeData 对象上读 `spawning.directions` 得 undefined（Spawning 构造器
/// structures.js:1309 同读）。空列表在 typed 世界即"未指定"，投影按官方缺键。
fn spawn_spawning_json(o: &kk_schema::room::SpawnObj) -> Value {
    match &o.spawning {
        None => Value::Null,
        Some(job) => {
            let mut v = json!({
                "name": job.name, "needTime": job.need_time,
                "spawnTime": job.spawn_time,
            });
            if !job.directions.is_empty() {
                v["directions"] = json!(job.directions);
            }
            v
        }
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
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
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
    /// owner 三态投影锁（官方 controllers/tick.js:56 降级归零 `user = null`
    /// 哨兵）：Some → 字符串；cleared → null（玩家侧 my === false）；
    /// 从未拥有 → 无键（my === undefined）。与 snapshot.rs 同名测试层间对表。
    #[test]
    fn controller_owner_three_states() {
        let mk = |owner: Option<&str>, cleared: bool| {
            RoomObject::Controller(ControllerObj {
                id: 0,
                x: 1,
                y: 1,
                level: 1,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: owner.map(str::to_string),
                owner_cleared: cleared,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            })
        };
        let mut world = World::new(42);
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let owned = world.insert("W1N1", mk(Some("p1"), false)).unwrap();
        let cleared = world.insert("W1N1", mk(None, true)).unwrap();
        let virgin = world.insert("W1N1", mk(None, false)).unwrap();
        assert_eq!(world.object_json_by_id(owned).unwrap()["owner"], "p1");
        assert_eq!(
            world.object_json_by_id(cleared).unwrap()["owner"],
            serde_json::Value::Null,
            "降级归零哨兵投影 null"
        );
        assert!(
            world
                .object_json_by_id(virgin)
                .unwrap()
                .get("owner")
                .is_none(),
            "从未拥有无键"
        );
    }

    /// 显式回查面；spawning.directions 未指定时官方缺键（undefined）。
    #[test]
    fn store_capacity_projection_family() {
        let mut world = World::new(7);
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let ctl = ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level: 7,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 0,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        };
        let ctx = PlaceCtx {
            time: world.time,
            controller: Some(&ctl),
            name_seq: 0,
        };
        world
            .insert("W1N1", RoomObject::Controller(ctl.clone()))
            .unwrap();
        let spawn = normalize(
            "spawn",
            &serde_json::json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
            &ctx,
        )
        .unwrap();
        let spawn_id = world.insert("W1N1", spawn).unwrap();
        let ext = normalize(
            "structure",
            &serde_json::json!({ "pos": [21, 20], "structureType": "extension", "owner": "p1" }),
            &ctx,
        )
        .unwrap();
        let ext_id = world.insert("W1N1", ext).unwrap();
        let storage = normalize(
            "structure",
            &serde_json::json!({ "pos": [22, 20], "structureType": "storage", "owner": "p1" }),
            &ctx,
        )
        .unwrap();
        let storage_id = world.insert("W1N1", storage).unwrap();
        let creep = normalize(
            "creep",
            &serde_json::json!({
                "pos": [23, 20], "owner": "p1", "body": ["carry", "carry", "move"], "name": "c1"
            }),
            &ctx,
        )
        .unwrap();
        let creep_id = world.insert("W1N1", creep).unwrap();

        let spawn_j = world.object_json_by_id(spawn_id).unwrap();
        assert_eq!(spawn_j["storeCapacityResource"]["energy"], 300);
        assert!(
            spawn_j.get("storeCapacity").is_none(),
            "spawn 无数值容量键（官方落库无 storeCapacity）"
        );
        assert_eq!(spawn_j["off"], false, "off 标记投影（data(id).off 回查面）");
        // 孵化任务 directions 缺键（官方 undefined 语义）
        assert_eq!(spawn_j["spawning"], serde_json::Value::Null);

        let ext_j = world.object_json_by_id(ext_id).unwrap();
        assert_eq!(
            ext_j["storeCapacityResource"]["energy"], 100,
            "RCL 7 的 extension 容量（EXTENSION_ENERGY_CAPACITY[7]）"
        );
        assert_eq!(ext_j["off"], false);

        let storage_j = world.object_json_by_id(storage_id).unwrap();
        assert_eq!(storage_j["storeCapacity"], 1_000_000);
        assert!(
            storage_j.get("storeCapacityResource").is_none(),
            "storage 无 per-resource 容量（通用商店）"
        );

        let creep_j = world.object_json_by_id(creep_id).unwrap();
        assert_eq!(creep_j["storeCapacity"], 100, "CARRY×2 × 50");

        // 手写孵化任务（带 directions）经 world_json 全量投影
        if let Some(RoomObject::Spawn(s)) =
            world.room_mut("W1N1").unwrap().objects.get_mut(&spawn_id)
        {
            s.spawning = Some(kk_schema::room::SpawningJob {
                name: "child".into(),
                need_time: 6,
                spawn_time: 6,
                directions: vec![3, 5],
            });
        }
        let spawn_j = world.object_json_by_id(spawn_id).unwrap();
        assert_eq!(spawn_j["spawning"]["name"], "child");
        assert_eq!(spawn_j["spawning"]["needTime"], 6);
        assert_eq!(spawn_j["spawning"]["spawnTime"], 6);
        assert_eq!(spawn_j["spawning"]["directions"], serde_json::json!([3, 5]));
        // 空 directions = 未指定 → 缺键（官方 undefined）
        if let Some(RoomObject::Spawn(s)) =
            world.room_mut("W1N1").unwrap().objects.get_mut(&spawn_id)
        {
            s.spawning = Some(kk_schema::room::SpawningJob {
                name: "child".into(),
                need_time: 6,
                spawn_time: 6,
                directions: vec![],
            });
        }
        let spawn_j = world.object_json_by_id(spawn_id).unwrap();
        assert!(
            spawn_j["spawning"].get("directions").is_none(),
            "空 directions 缺键（官方 Spawning.directions === undefined）"
        );
    }

    /// B2 schema 四字段（A4-iii 投影清偿）的 prelude 世界投影锁：绝对 tick/
    /// {user,endTime} 官方 DB 原始形态（prelude getter 换算用），None → null；
    /// sign 无 datetime 键（墙钟不落盘——灰区表既有登记，getter 侧合成）。
    /// 与 snapshot.rs 同名测试层间对表（两消费面各自换算、字段单源）。
    #[test]
    fn controller_b2_projection_fields() {
        let mk = |reservation, sign, blocked, cooldown| {
            RoomObject::Controller(ControllerObj {
                id: 0,
                x: 1,
                y: 1,
                level: 1,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: false,
                reservation,
                sign,
                upgrade_blocked: blocked,
                safe_mode_cooldown: cooldown,
            })
        };
        let mut world = World::new(42);
        world.time = 100;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let full = world
            .insert(
                "W1N1",
                mk(
                    Some(kk_schema::room::Reservation {
                        user: "p2".into(),
                        end_time: 600,
                    }),
                    Some(kk_schema::room::Sign {
                        user: "p1".into(),
                        text: "hi".into(),
                        time: 80,
                    }),
                    Some(400),
                    Some(50_500),
                ),
            )
            .unwrap();
        let empty = world.insert("W1N1", mk(None, None, None, None)).unwrap();
        let j = world.object_json_by_id(full).unwrap();
        assert_eq!(
            j["reservation"],
            serde_json::json!({ "user": "p2", "endTime": 600 }),
            "原始 DB 形态（getter 侧换算 ticksToEnd）"
        );
        assert_eq!(
            j["sign"],
            serde_json::json!({ "user": "p1", "text": "hi", "time": 80 })
        );
        assert!(
            j["sign"].get("datetime").is_none(),
            "墙钟不落盘不投影（灰区表登记）"
        );
        assert_eq!(j["upgradeBlocked"], 400, "绝对 tick 原样");
        assert_eq!(j["safeModeCooldown"], 50_500, "绝对 tick 原样");
        let e = world.object_json_by_id(empty).unwrap();
        assert_eq!(e["reservation"], serde_json::Value::Null);
        assert_eq!(e["sign"], serde_json::Value::Null);
        assert_eq!(e["upgradeBlocked"], serde_json::Value::Null);
        assert_eq!(e["safeModeCooldown"], serde_json::Value::Null);
    }
}
