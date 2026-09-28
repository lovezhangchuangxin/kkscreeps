//! 查询面快照投影：embed `getObject`/`findInRoom` 的 screeps-ok 契约形状。
//!
//! 与 world_json（prelude 世界投影，M0 契约形状）是**两个消费面**：本投影产出
//! "玩家 getter 值"形态——契约 `kind` 字段、剩余 tick 数（绝对时间 − time）、
//! 正量 store（0 值资源不出现）、spawning 的 remainingTime 形态。逐组对照
//! vanilla 参照实现 screeps-ok/adapters/vanilla/snapshots.ts（其每组字段注明
//! 官方 game/*.js 出处）；`null` 语义同契约头注（getter 的 undefined 读 null）。
//! 两投影的字段一致性由 embed_tests 对表断言锁定（M0 第七轮"层间一致"教训：
//! 同一语义两层实现必须有显式对表）。

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use crate::world::{ObjId, World};

impl World {
    /// 单对象快照（embed getObject 后端；flag 不在契约 ObjectSnapshot 联合内
    /// ——vanilla snapshotObject 无 flag 分支，返回 null，同构）。
    pub fn object_snapshot_by_id(&self, id: ObjId) -> Option<Value> {
        let (room, object) = self.get(id)?;
        if matches!(object, RoomObject::Flag(_)) {
            return None;
        }
        let terrain = &self.rooms.get(room)?.terrain;
        Some(snapshot_json(room, id, object, self.time, terrain))
    }

    /// 按 FIND 常量过滤房间内对象并出快照（embed findInRoom 后端）。
    /// 过滤复用 world_json::matches_find（FIND 语义单源）；flag 不进快照结果
    /// （契约查询面的中性选择器不含 flags——vanilla findTypeMap 同构）。
    pub fn find_snapshots_in_room(&self, room: &str, find_const: i64) -> Vec<Value> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        let terrain = &state.terrain;
        state
            .objects
            .iter()
            .filter(|(id, o)| {
                if matches!(*o, RoomObject::Flag(_)) {
                    return false;
                }
                let internal = crate::world_json::object_json(room, **id, o, self.time, terrain);
                crate::world_json::matches_find(&internal, find_const)
            })
            .map(|(id, o)| snapshot_json(room, *id, o, self.time, terrain))
            .collect()
    }
}

/// 官方 getter 的 `t ? t - time : undefined` 形态（vanilla snapshots.ts:54-56；
/// 契约把 undefined 读作 null）。t=0 视同未设置（JS falsy）。
/// 注意：与 vanilla 的差异——过期挂钟官方发负数（契约头注 "unclamped"），
/// 我方 saturating_sub 钳 0；正常路径不可达（钟到点即被结算消费清掉），
/// 仅缺结算/构造性放置可观察，登记灰区表。
fn remaining(t: u64, time: u64) -> Value {
    if t != 0 {
        json!(t.saturating_sub(time))
    } else {
        Value::Null
    }
}

fn remaining_opt(t: Option<u64>, time: u64) -> Value {
    match t {
        Some(t) if t != 0 => json!(t.saturating_sub(time)),
        _ => Value::Null,
    }
}

/// 官方 safeMode/cooldown 的 `t && t > time ? t - time : undefined` 形态
/// （vanilla snapshots.ts:62-64）：过期挂钟也读 null。
fn active_remaining(t: u64, time: u64) -> Value {
    if t > time {
        json!(t - time)
    } else {
        Value::Null
    }
}

fn snap_pos(room: &str, x: u8, y: u8) -> Value {
    json!({ "x": x, "y": y, "roomName": room })
}

/// 官方 store getter 只暴露正量资源（vanilla snapStore：amount > 0 才入表）。
fn snap_store(energy: u32) -> Value {
    if energy > 0 {
        json!({ "energy": energy })
    } else {
        json!({})
    }
}

/// creep 载荷容量：活跃（hits>0）CARRY 数 × CARRY_CAPACITY，boosted 部件乘
/// BOOSTS.carry 的 capacity 系数（KH×2/KH2O×3/XKH2O×4——constants.js BOOSTS 表；
/// hits>0 门对照官方 _recalc-body → calcBodyEffectiveness，utils.js:623-636——
/// 受损部件不计容量）。形态对齐 vanilla `storeCapacity || null`：0（无活跃
/// CARRY）读 null；带 CARRY 的 creep 恒 number（inspection 用例锁 number）。
fn creep_store_capacity(body: &[kk_schema::room::BodyPart]) -> Value {
    let mut capacity = 0i64;
    let mut boost_table: Option<serde_json::Value> = None;
    for part in body {
        if part.hits == 0
            || crate::types::BodyKind::from_code(part.kind) != Some(crate::types::BodyKind::Carry)
        {
            continue;
        }
        let multiplier = match &part.boost {
            None => 1,
            Some(boost) => {
                if boost_table.is_none() {
                    boost_table = serde_json::from_str(c::BOOSTS).ok();
                }
                boost_table
                    .as_ref()
                    .and_then(|t| t.get("carry"))
                    .and_then(|t| t.get(boost.as_str()))
                    .and_then(|t| t.get("capacity"))
                    .and_then(Value::as_i64)
                    .unwrap_or(1)
            }
        };
        capacity += c::CARRY_CAPACITY * multiplier;
    }
    if capacity > 0 {
        json!(capacity)
    } else {
        Value::Null
    }
}

/// 官方 progressTotal：1..7 级查 CONTROLLER_LEVELS；0 级与 8 级为 null
/// （vanilla：`level > 0 && level < 8 ? LEVELS[level] : null`——level 8 无进度条）。
/// 与 world_json 的 prelude 投影（0 兜底）不同形，属两消费面各自契约。
fn controller_progress_total(level: u8) -> Value {
    if level == 0 || level >= 8 {
        return Value::Null;
    }
    let key = level.to_string();
    c::CONTROLLER_LEVELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| json!(v))
        .unwrap_or(Value::Null)
}

/// typed 对象 → 契约快照（逐组对照 vanilla snapshots.ts）。
fn snapshot_json(
    room: &str,
    id: ObjId,
    object: &RoomObject,
    time: u64,
    terrain: &kk_core::Terrain,
) -> Value {
    let id_str = id.as_str();
    match object {
        RoomObject::Creep(o) => json!({
            "kind": "creep", "id": id_str, "name": o.name,
            "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": o.body.len() * 100,
            "fatigue": o.fatigue,
            "body": o.body.iter().map(crate::world_json::part_json).collect::<Vec<_>>(),
            "owner": o.owner,
            "ticksToLive": remaining(o.age_time, time),
            "spawning": o.spawning,
            "store": snap_store(o.store.energy),
            "storeCapacity": creep_store_capacity(&o.body),
        }),
        RoomObject::Source(o) => json!({
            "kind": "source", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "energy": o.energy, "energyCapacity": o.energy_capacity,
            "ticksToRegeneration": remaining_opt(o.next_regeneration_time, time),
        }),
        RoomObject::Controller(o) => {
            let mut v = json!({
                "kind": "structure", "structureType": "controller",
                "id": id_str, "pos": snap_pos(room, o.x, o.y),
                "level": o.level,
                "progress": if o.level > 0 { json!(o.progress) } else { Value::Null },
                "progressTotal": controller_progress_total(o.level),
                "ticksToDowngrade": remaining_opt(o.downgrade_time, time),
                "safeMode": active_remaining(o.safe_mode, time),
                "safeModeAvailable": o.safe_mode_available,
                // schema 无 cooldown/reservation/sign 字段（M2 controller 结算族补），
                // 契约三字段按 null 形态占位
                "safeModeCooldown": Value::Null,
                "isPowerEnabled": o.is_power_enabled,
                "reservation": Value::Null,
                "sign": Value::Null,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        RoomObject::Spawn(o) => json!({
            "kind": "structure", "structureType": "spawn",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::SPAWN_HITS,
            "name": o.name,
            "store": snap_store(o.store.energy),
            // 官方 spawn 落库即带 storeCapacityResource（build.js:117-124），
            // 无资源参数的 getCapacity() 读 null（store.js:28-33；
            // adapter-spec.md:359-360 明文 "null for a store with per-resource
            // limits such as a spawn's"；vanilla 快照同构）——数值容量经
            // energyCapacityAvailable/结算扣费面消费
            "storeCapacity": Value::Null,
            // 官方 Spawning 暴露 remainingTime = spawnTime − time
            // （structures.js:1302-1310；vanilla snapSpawning 同构）
            "spawning": match &o.spawning {
                None => Value::Null,
                Some(job) => json!({
                    "name": job.name, "needTime": job.need_time,
                    "remainingTime": job.spawn_time.saturating_sub(time),
                }),
            },
        }),
        RoomObject::Extension(o) => json!({
            "kind": "structure", "structureType": "extension",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
            "store": snap_store(o.store.energy),
            // 同 spawn：官方 extension 带 storeCapacityResource（build.js:131-135、
            // extensions/tick.js:11-17 按 RCL 归一），getCapacity() 读 null
            "storeCapacity": Value::Null,
        }),
        RoomObject::ConstructedWall(o) => json!({
            "kind": "structure", "structureType": "constructedWall",
            "id": id_str, "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": c::WALL_HITS_MAX,
        }),
        RoomObject::Road(o) => json!({
            "kind": "structure", "structureType": "road",
            "id": id_str, "pos": snap_pos(room, o.x, o.y),
            "hits": o.hits, "hitsMax": crate::world_json::road_hits_max(terrain, o.x, o.y),
            "ticksToDecay": remaining(o.next_decay_time, time),
        }),
        RoomObject::Storage(o) => json!({
            "kind": "structure", "structureType": "storage",
            "id": id_str, "pos": snap_pos(room, o.x, o.y), "owner": o.owner,
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
            "store": snap_store(o.store.energy),
            "storeCapacity": c::STORAGE_CAPACITY,
        }),
        RoomObject::ConstructionSite(o) => {
            let mut v = json!({
                "kind": "site", "id": id_str,
                "pos": snap_pos(room, o.x, o.y),
                "structureType": o.structure_type,
                "progress": o.progress, "progressTotal": o.progress_total,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        RoomObject::Energy(o) => json!({
            "kind": "resource", "id": id_str,
            "pos": snap_pos(room, o.x, o.y),
            "resourceType": if o.resource_type == 0 { "energy" } else { "power" },
            "amount": o.amount,
        }),
        // 透传桶：kind 按透传 type 或 structureType 映射（tombstone/ruin/
        // mineral/deposit 有独立契约 kind——mineral 经适配器 structure 通道
        // 放置时 type 是 "structure"、structureType 才是 "mineral"，两键都查），
        // 其余按 structure 兜底（vanilla snapshotStructure 的 default 分支同构
        // ——只保底 base 字段）。计时字段（decayTime/cooldown 等）不折算
        // getter 剩余形态——透传桶无该面，随对应能力批（B3/B4）补
        RoomObject::Generic(o) => {
            let mut v: serde_json::Map<String, Value> = serde_json::from_str::<Value>(&o.data)
                .ok()
                .and_then(|d| d.as_object().cloned())
                .unwrap_or_default();
            let kind = match (
                v.get("type").and_then(Value::as_str),
                v.get("structureType").and_then(Value::as_str),
            ) {
                (Some("tombstone"), _) | (_, Some("tombstone")) => "tombstone",
                (Some("ruin"), _) | (_, Some("ruin")) => "ruin",
                (Some("mineral"), _) | (_, Some("mineral")) => "mineral",
                (Some("deposit"), _) | (_, Some("deposit")) => "deposit",
                _ => "structure",
            };
            v.insert("kind".into(), json!(kind));
            v.insert("id".into(), json!(id_str));
            v.insert("pos".into(), snap_pos(room, o.x, o.y));
            Value::Object(v)
        }
        // flag 不在查询面契约内（vanilla snapshotObject 无 flag 分支）——
        // 调用方（object_snapshot_by_id/find_snapshots_in_room）已前置排除
        RoomObject::Flag(_) => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::{PlaceCtx, normalize};
    use kk_core::Terrain;

    fn world_with_room() -> World {
        let mut world = World::new(42);
        world.time = 10;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        world
    }

    fn ctx(world: &World) -> PlaceCtx<'_> {
        PlaceCtx {
            time: world.time,
            controller: None,
            name_seq: 0,
        }
    }

    /// 契约形状逐类型锁定：kind 映射、null/剩余值形态、正量 store、
    /// storeCapacity number、spawning remainingTime。
    #[test]
    fn snapshot_contract_shapes() {
        let mut world = world_with_room();
        let creep = normalize(
            "creep",
            &serde_json::json!({
                "pos": [5, 5], "owner": "p1", "body": ["carry", "carry"],
                "store": { "energy": 0 },
            }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let creep_id = world.insert("W1N1", creep).unwrap();
        let snap = world.object_snapshot_by_id(creep_id).unwrap();
        assert_eq!(snap["kind"], "creep");
        assert_eq!(
            snap["store"].as_object().unwrap().len(),
            0,
            "0 值资源不出现"
        );
        assert_eq!(snap["storeCapacity"], 100, "CARRY×2 × 50");
        assert_eq!(
            snap["ticksToLive"], 1500,
            "默认寿命：age_time(=10+1500) − time=10"
        );
        assert!(snap["spawning"].is_boolean());

        let spawn = normalize(
            "structure",
            &serde_json::json!({ "pos": [6, 6], "structureType": "spawn", "owner": "p1" }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let spawn_id = world.insert("W1N1", spawn).unwrap();
        let snap = world.object_snapshot_by_id(spawn_id).unwrap();
        assert_eq!(snap["kind"], "structure");
        assert_eq!(snap["structureType"], "spawn");
        // 官方 spawn 带 storeCapacityResource——无参 getCapacity() 读 null
        assert_eq!(snap["storeCapacity"], serde_json::Value::Null);
        assert_eq!(
            snap["spawning"],
            serde_json::Value::Null,
            "无孵化任务读 null"
        );

        let source = normalize(
            "structure",
            &serde_json::json!({ "pos": [10, 10], "structureType": "source" }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let source_id = world.insert("W1N1", source).unwrap();
        let snap = world.object_snapshot_by_id(source_id).unwrap();
        assert_eq!(
            snap["kind"], "source",
            "source 有独立契约 kind（非 structure）"
        );
        assert_eq!(
            snap["ticksToRegeneration"],
            serde_json::Value::Null,
            "未挂钟读 null"
        );
    }

    /// getter 换算形态：controller 的剩余值/8 级 null、source 挂钟后的剩余数。
    #[test]
    fn snapshot_getter_forms() {
        let mut world = world_with_room();
        let ctl = kk_schema::room::ControllerObj {
            id: 0,
            x: 1,
            y: 1,
            level: 8,
            safe_mode: 0,
            safe_mode_available: 1,
            progress: 0,
            downgrade_time: Some(500),
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
            &serde_json::json!({ "pos": [1, 1], "structureType": "controller", "rcl": 8, "owner": "p1", "downgradeTime": 500 }),
            &ctx,
        )
        .unwrap();
        let ctl_id = world.insert("W1N1", controller).unwrap();
        let snap = world.object_snapshot_by_id(ctl_id).unwrap();
        assert_eq!(snap["kind"], "structure");
        assert_eq!(
            snap["progressTotal"],
            serde_json::Value::Null,
            "8 级无进度条"
        );
        assert_eq!(snap["ticksToDowngrade"], 490, "绝对 500 − time 10");
        assert_eq!(
            snap["safeMode"],
            serde_json::Value::Null,
            "safe_mode=0 读 null"
        );
        assert_eq!(snap["isPowerEnabled"], false);

        // FIND_STRUCTURES 不含 source（官方 game.js:229-251 structureTypes 表）
        let source = normalize(
            "structure",
            &serde_json::json!({ "pos": [10, 10], "structureType": "source" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", source).unwrap();
        let structures = world.find_snapshots_in_room("W1N1", c::FIND_STRUCTURES);
        assert_eq!(
            structures.len(),
            1,
            "只有 controller，source 不入 FIND_STRUCTURES"
        );
        assert_eq!(structures[0]["structureType"], "controller");
        let sources = world.find_snapshots_in_room("W1N1", c::FIND_SOURCES);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0]["kind"], "source");
    }

    /// boost 乘数与形态：KH boosted CARRY ×2；无活跃 CARRY 读 null
    /// （vanilla `storeCapacity || null` 同构）；受损部件不计容量。
    #[test]
    fn snapshot_creep_capacity_boosts_and_null() {
        let mut world = world_with_room();
        let boosted = normalize(
            "creep",
            &serde_json::json!({
                "pos": [7, 7], "owner": "p1",
                "body": ["carry", "carry"], "boosts": { "1": "KH" },
            }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let id = world.insert("W1N1", boosted).unwrap();
        let snap = world.object_snapshot_by_id(id).unwrap();
        assert_eq!(
            snap["storeCapacity"], 150,
            "裸 CARRY 50 + KH boosted ×2 = 150"
        );

        let no_carry = normalize(
            "creep",
            &serde_json::json!({ "pos": [8, 8], "owner": "p1", "body": ["move", "work"] }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let id = world.insert("W1N1", no_carry).unwrap();
        let snap = world.object_snapshot_by_id(id).unwrap();
        assert_eq!(
            snap["storeCapacity"],
            serde_json::Value::Null,
            "无 CARRY 读 null"
        );
    }

    /// Generic 透传的 kind 映射：mineral 经 structure 通道放置（适配器真实
    /// 路径——type=structure、structureType=mineral）与 tombstone 直通道。
    #[test]
    fn snapshot_generic_kind_mapping() {
        let mut world = world_with_room();
        let mineral = normalize(
            "structure",
            &serde_json::json!({
                "pos": [12, 12], "structureType": "mineral",
                "mineralType": "X", "mineralAmount": 2000, "density": 3,
            }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let id = world.insert("W1N1", mineral).unwrap();
        let snap = world.object_snapshot_by_id(id).unwrap();
        assert_eq!(
            snap["kind"], "mineral",
            "structure 通道的 mineral 按 structureType 判 kind"
        );
        assert_eq!(snap["structureType"], "mineral");
        assert_eq!(
            world.find_snapshots_in_room("W1N1", c::FIND_MINERALS).len(),
            1,
            "FIND_MINERALS 命中且 kind 正确"
        );

        let tombstone = normalize(
            "tombstone",
            &serde_json::json!({ "pos": [13, 13], "store": { "energy": 30 } }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let id = world.insert("W1N1", tombstone).unwrap();
        let snap = world.object_snapshot_by_id(id).unwrap();
        assert_eq!(
            snap["kind"], "tombstone",
            "直通道 tombstone 按透传 type 判 kind"
        );
    }

    /// 道路 hitsMax 的地形系数（外部复审 P2-4：官方 build.js:172-184 把放大值
    /// 写进 hits 与 hitsMax；两投影共用 road_hits_max）。
    #[test]
    fn road_hits_max_terrain_scaling() {
        let mut world = world_with_room();
        let mut terrain = kk_core::Terrain::plains();
        terrain.as_mut_slice()[25 * 50 + 10] |= kk_core::TERRAIN_MASK_SWAMP;
        terrain.as_mut_slice()[25 * 50 + 11] |= kk_core::TERRAIN_MASK_WALL;
        terrain.normalize();
        world.rooms.get_mut("W1N1").unwrap().terrain = terrain;
        let plain_road = normalize(
            "structure",
            &serde_json::json!({ "pos": [10, 24], "structureType": "road" }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let plain_id = world.insert("W1N1", plain_road).unwrap();
        let swamp_road = normalize(
            "structure",
            &serde_json::json!({ "pos": [10, 25], "structureType": "road" }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let swamp_id = world.insert("W1N1", swamp_road).unwrap();
        let wall_road = normalize(
            "structure",
            &serde_json::json!({ "pos": [11, 25], "structureType": "road" }),
            &PlaceCtx {
                time: world.time,
                controller: None,
                name_seq: 0,
            },
        )
        .unwrap();
        let wall_id = world.insert("W1N1", wall_road).unwrap();
        assert_eq!(
            world.object_snapshot_by_id(plain_id).unwrap()["hitsMax"],
            5000
        );
        assert_eq!(
            world.object_snapshot_by_id(swamp_id).unwrap()["hitsMax"],
            25000,
            "沼泽 ×5"
        );
        assert_eq!(
            world.object_snapshot_by_id(wall_id).unwrap()["hitsMax"],
            750000,
            "墙 ×150"
        );
        // prelude 世界投影同源
        let wj = world.world_json();
        let roads: Vec<&serde_json::Value> = wj["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["structureType"] == "road")
            .collect();
        assert_eq!(roads.len(), 3);
        assert!(roads.iter().all(|o| o["hitsMax"].as_i64().unwrap() > 0));
    }

    /// flag 不进查询面（vanilla snapshotObject 无 flag 分支）。
    #[test]
    fn flags_not_in_query_plane() {
        let mut world = world_with_room();
        let c = ctx(&world);
        let flag = normalize(
            "flag",
            &serde_json::json!({ "pos": [3, 3], "name": "f1" }),
            &c,
        )
        .unwrap();
        let flag_id = world.insert("W1N1", flag).unwrap();
        assert!(world.object_snapshot_by_id(flag_id).is_none());
        assert!(
            world
                .find_snapshots_in_room("W1N1", c::FIND_FLAGS)
                .is_empty()
        );
    }
}
