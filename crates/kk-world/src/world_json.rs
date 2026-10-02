//! 将世界状态转换为玩家沙箱需要的 JSON，并提供同格式的内部查询。
//!
//! 字段供 kk-runner 的 prelude 装配玩家对象使用：部分计时保留绝对 tick，
//! 部分提前转为剩余值。查询快照由 snapshot 模块另行生成。
//! FIND 过滤只处理对象；出口是由地形派生的位置，须由沙箱的 room.find 处理。

use std::sync::Arc;

use kk_core::constants as c;
use kk_schema::room::RoomObject;
use serde_json::{Value, json};

use crate::world::World;

impl World {
    /// 导出所有房间和对象，不限制玩家视野，供内部调用及测试使用。
    pub fn world_json(&self) -> Value {
        self.world_json_impl(None)
    }

    /// 为指定玩家生成沙箱数据，gcl_points 是玩家累计 GCL 点数，不是等级。
    /// 房内存在己方对象时授予视野，但旗和工地不授予；可见房中的敌方对象也会导出。
    /// 己方旗和工地可单独导出，即使其房间不可见；他人的旗被排除。
    /// 所有已注册房间的地形仍会提供，无主旗只在可见房间导出。
    /// 当前不根据 observer 观察或控制器预订授予额外房间视野。
    /// 视野归集参考：driver/lib/runtime/data.js 的 userObjects 房间计算。
    pub fn world_json_for_viewer(&self, user: &str, gcl_points: i64) -> Value {
        self.world_json_impl(Some((user, gcl_points)))
    }

    /// 带扩展字段目录的玩家投影：目录内声明的扩展字段按权限裁切后
    /// 进入对象的 `ext` 键。Public 对看到该对象的玩家可见；Owner 仅
    /// 对象主人可见；ServerOnly 不进入任何玩家投影，只保存在权威
    /// 状态 `RoomState.extensions` 中。未声明扩展字段的类型与全部
    /// 字段被裁掉的对象都不输出 `ext` 键。
    pub fn world_json_for_viewer_with_extensions(
        &self,
        user: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
    ) -> Value {
        use crate::world::ObjId;
        let mut world = self.world_json_impl(Some((user, gcl_points)));
        if catalog.is_empty() {
            return world;
        }
        let Some(objects) = world.get_mut("objects").and_then(|v| v.as_array_mut()) else {
            return world;
        };
        for object in objects.iter_mut() {
            let Some(id) = object.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(oid) = ObjId::parse(id) else {
                continue;
            };
            let structure_type = object.get("structureType").and_then(|v| v.as_str());
            let Some(type_key) = structure_type.and_then(|st| catalog.type_for_structure(st))
            else {
                continue;
            };
            // 扩展值从权威房间状态读取；玩家 JSON 不携带扩展源数据。
            let Some(state) = self.rooms.values().find(|r| r.objects.contains_key(&oid)) else {
                continue;
            };
            let Some(values) = state.extensions.get(&oid) else {
                continue;
            };
            let owner = object.get("owner").and_then(|v| v.as_str());
            let mut ext = serde_json::Map::new();
            for (field_key, value) in values {
                let Some(field_key) = kk_mod_api::Key::new(field_key.clone()).ok() else {
                    // 写入口径已保证键形合法；此处防御坏键。
                    continue;
                };
                let Some(rule) = catalog.field(type_key, &field_key) else {
                    continue;
                };
                let visible = match rule.visibility {
                    kk_mod_api::Visibility::Public => true,
                    kk_mod_api::Visibility::Owner => owner == Some(user),
                    _ => false,
                };
                if visible {
                    ext.insert(field_key.to_string(), value.clone());
                }
            }
            if !ext.is_empty()
                && let Some(map) = object.as_object_mut()
            {
                map.insert("ext".into(), Value::Object(ext));
            }
        }
        world
    }

    fn world_json_impl(&self, viewer: Option<(&str, i64)>) -> Value {
        let user = viewer.map(|(u, _)| u);
        let visible: std::collections::BTreeSet<&String> = match user {
            None => self.rooms.keys().collect(),
            Some(user) => self
                .rooms
                .iter()
                .filter(|(_, state)| {
                    state.objects.values().any(|o| {
                        // 归属只用于判断房间是否可见；旗和工地虽有主人，但不授予视野。
                        // 上游依据：driver/lib/runtime/data.js 的 userObjects 过滤。
                        !matches!(o, RoomObject::ConstructionSite(_) | RoomObject::Flag(_))
                            && crate::domain::object_owner(o).as_deref() == Some(user)
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
            entry.insert("terrain".into(), self.terrain_json(name, state));

            let rcl = room_rcl(state);
            if visible.contains(name) {
                // 玩家能量统计只计己方可用的 spawn、extension；内部全量投影统计全房。
                let (available, capacity) = room_energy(state, user);
                entry.insert("energyAvailable".into(), json!(available));
                entry.insert("energyCapacityAvailable".into(), json!(capacity));
                // 仅可见房间导出事件，解析失败的条目忽略。
                if !state.events.is_empty() {
                    let events: Vec<Value> = state
                        .events
                        .iter()
                        .filter_map(|e| serde_json::from_str(e).ok())
                        .collect();
                    entry.insert("events".into(), json!(events));
                }
            }
            // 不可见房间仅导出名称和地形，不导出能量统计及事件。
            rooms.insert(name.clone(), Value::Object(entry));
            let room_visible = visible.contains(name);
            for (id, object) in &state.objects {
                // 他人的旗不进入玩家数据；无主旗保留给可见房间的兼容性场景。
                if let Some(viewer) = user
                    && let RoomObject::Flag(f) = object
                    && f.owner.as_deref().is_some_and(|o| o != viewer)
                {
                    continue;
                }
                if !room_visible {
                    // 己方旗和工地可出现在全局集合中，即使所在房间不可见。
                    // 导出这些对象不应使该房间进入 Game.rooms。
                    // 参考：driver/lib/runtime/data.js 的 userObjects、flags 查询。
                    let own_private = matches!(
                        object,
                        RoomObject::ConstructionSite(_) | RoomObject::Flag(_)
                    ) && crate::domain::object_owner(object).as_deref()
                        == Some(user.unwrap_or(""));
                    if !own_private {
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
        // 显式传递可见房名，prelude 不必再从 owner/user 等不同字段推断视野。
        // BTreeSet 保证输出顺序确定。
        let visible_rooms: Vec<&String> = visible.iter().copied().collect();
        json!({
            "time": self.time,
            // CPU 是占位值；Sandbox::begin_tick 传入实际预算，由 prelude 覆盖。
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": user,
            "gcl": viewer.map(|(_, g)| g).unwrap_or(0),
            "visibleRooms": visible_rooms,
            "rooms": Value::Object(rooms),
            "objects": objects,
        })
    }

    /// 按地形版本缓存 2500 字节的 base64 载荷，输出 {rev, payload}。
    /// prelude 的 game.js 按 rev 复用解码后的地形；版本变化时重新解码。
    /// 任一房间设置地形或新房注册都会改变全局 rev，使全部房间缓存失效。
    fn terrain_json(&self, name: &str, state: &crate::world::RoomState) -> Value {
        let epoch = self.terrain_epoch();
        let mut cache = self.terrain_cache.borrow_mut();
        let payload = match cache.get(name) {
            Some((cached_epoch, b64)) if *cached_epoch == epoch => b64.clone(),
            _ => {
                let b64: Arc<str> = Arc::from(base64_encode(state.terrain.as_slice()).as_str());
                cache.insert(name.to_string(), (epoch, b64.clone()));
                b64
            }
        };
        json!({ "rev": epoch, "payload": payload.as_ref() })
    }

    /// 按对象 ID 顺序返回匹配 FIND 的原始沙箱记录，不执行房间视野裁切。
    /// viewer 只用于归属和孵化过滤；None 使用中立查询规则，详见 matches_find。
    /// 需要查询快照格式时使用 find_snapshots_in_room。
    pub fn find_in_room(
        &self,
        room: &str,
        find_const: i64,
        viewer: Option<&str>,
    ) -> Vec<(crate::world::ObjId, Value)> {
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
            .filter(|(_, o)| matches_find(o, find_const, viewer))
            .collect()
    }

    /// 按对象 ID 顺序返回房内第一个控制器的坐标。
    pub fn controller_pos(&self, room: &str) -> Option<(u64, u64)> {
        let state = self.rooms.get(room)?;
        state.objects.values().find_map(|o| match o {
            RoomObject::Controller(ctl) => Some((ctl.x as u64, ctl.y as u64)),
            _ => None,
        })
    }

    /// 按 ID 返回原始沙箱记录，不过滤玩家视野。
    /// 运行时的 getObject 查询接口使用 object_snapshot_by_id 的快照格式。
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

/// 用标准 base64 字母表编码地形字节，尾组使用 = 填充。
pub(crate) fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(n >> 12) as usize & 0x3f] as char);
        // 尾组不足 3 字节：剩余字节仍全量编码，缺位以 '=' 填充
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

/// 根据地形计算道路最大 hits，沼泽和墙系数分别相乘。
/// 两种投影共用此计算；不处理上游另有的同格 swamp、wall 对象。
/// 参考：engine/src/processor/intents/creeps/build.js 的 road 分支。
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

/// 按对象 ID 顺序读取首个控制器等级，无控制器时返回 0。
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

/// 汇总非 off 的 spawn、extension 能量与容量，有 viewer 时只计己方对象。
/// spawn 容量固定，extension 容量按房间 RCL 查表。结算所用的全房聚合不在此处。
/// 玩家统计参考：engine/src/game/game.js 的房间 energyAvailable 计算。
fn room_energy(state: &crate::world::RoomState, viewer: Option<&str>) -> (i64, i64) {
    let rcl = room_rcl(state);
    let mut available = 0i64;
    let mut capacity = 0i64;
    for o in state.objects.values() {
        let (energy, cap) = match o {
            RoomObject::Spawn(s) if !s.off => (s.store.energy as i64, c::SPAWN_ENERGY_CAPACITY),
            RoomObject::Extension(e) if !e.off => (
                e.store.energy as i64,
                crate::types::extension_energy_capacity(rcl),
            ),
            _ => continue,
        };
        if viewer.is_none_or(|v| crate::domain::object_owner(o).as_deref() == Some(v)) {
            available += energy;
            capacity += cap;
        }
    }
    (available, capacity)
}

/// 原始沙箱 store 保留 energy 键（包括 0），G 只在正量时出现。
/// prelude 的 Store 构造器再过滤 0 值资源，因此原始 JSON 不等于玩家枚举结果。
pub(crate) fn store_json(store: &kk_schema::room::Store) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    Value::Object(v)
}

/// 在沙箱 store 中合并额外正量资源，仍保留 energy=0 的键。
pub(crate) fn store_with_extra_json(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    crate::domain::apply_store_extra(&mut v, extra);
    Value::Object(v)
}

/// 显式资源名优先；否则码值 1、2 对应 power、G，其余值对应 energy。
pub(crate) fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> String {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => name.clone(),
        (None, 1) => "power".into(),
        (None, 2) => kk_core::constants::RESOURCE_GHODIUM.into(),
        (None, _) => "energy".into(),
    }
}

/// 将对象转换为 prelude 消费的原始记录，并附加 pos。
/// rcl 决定 extension 的能量容量；不会读取或改变对象所属世界的状态。
/// 可选通知开关只在 Some 时输出，None 保持缺键。
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
            // age_time=0 表示尚未设置寿命，不输出 ticksToLive。
            // 其余值转为非负剩余 tick；额外资源也须传给玩家 Store。
            let store = store_with_extra_json(&o.store, &o.store_extra);
            let mut v = json!({
                "type": "creep", "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "body": o.body.iter().map(part_json).collect::<Vec<_>>(),
                "fatigue": o.fatigue,
                "ageTime": o.age_time,
                "hits": o.hits,
                "hitsMax": o.body.len() * 100,
                "store": store,
                // 活跃 CARRY（包含强化）决定容量，供 carryCapacity 和 Store 共用。
                "storeCapacity": crate::snapshot::creep_store_capacity_raw(&o.body),
                "spawning": o.spawning,
            });
            if o.age_time != 0 {
                v["ticksToLive"] = json!(o.age_time.saturating_sub(time));
            }
            // prelude 根据 owner 和 sayPublic 决定能否读到说话内容。
            // notifyWhenAttacked 原值用于避免重复产生相同开关指令。
            if let Some(m) = &o.say_message {
                v["say"] = json!(m);
                v["sayPublic"] = json!(o.say_public.unwrap_or(false));
            }
            if let Some(f) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(f);
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
                // 保留绝对 tick 和 reservation.endTime，prelude getter 再计算剩余时间。
                // sign.time 是世界 tick，sign.datetime 是签名时的 Unix 墙钟毫秒。
                "upgradeBlocked": o.upgrade_blocked,
                "safeModeCooldown": o.safe_mode_cooldown,
                "reservation": match &o.reservation {
                    Some(r) => json!({ "user": r.user, "endTime": r.end_time }),
                    None => Value::Null,
                },
                "sign": match &o.sign {
                    Some(s) => {
                        let mut sv = serde_json::Map::new();
                        sv.insert("user".into(), json!(s.user));
                        sv.insert("text".into(), json!(s.text));
                        sv.insert("time".into(), json!(s.time));
                        // 未设置 datetime 时省略该键；这与查询快照始终省略 datetime 不同。
                        if let Some(dt) = s.datetime {
                            sv.insert("datetime".into(), json!(dt));
                        }
                        Value::Object(sv)
                    }
                    None => Value::Null,
                },
            });

            if let Some(val) = crate::domain::controller_owner_value(o) {
                v["owner"] = val;
            }
            v
        }
        RoomObject::Spawn(o) => {
            // spawn 只有按资源的容量字段，没有通用 storeCapacity。
            // 这使 Store.getCapacity() 返回 null、getCapacity('energy') 返回数值。
            let mut v = json!({
                "type": "spawn", "structureType": "spawn",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "store": store_json(&o.store),
                "energy": o.store.energy,
                "energyCapacity": c::SPAWN_ENERGY_CAPACITY,
                "storeCapacityResource": { "energy": c::SPAWN_ENERGY_CAPACITY },
                // off 表示当前 RCL 配额下不可用，spawn API 会回查此字段。
                "off": o.off,
                "hits": o.hits, "hitsMax": c::SPAWN_HITS,
                "spawning": spawn_spawning_json(o),
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Extension(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "extension",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": store_json(&o.store),
            "energy": o.store.energy,
            // extension 按资源限制容量，能量容量随房间 RCL 改变。
            "storeCapacityResource": { "energy": crate::types::extension_energy_capacity(rcl) },
            "off": o.off,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::ConstructedWall(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "constructedWall",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": c::WALL_HITS_MAX,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Road(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "road",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "hits": o.hits, "hitsMax": road_hits_max(terrain, o.x, o.y),
            "nextDecayTime": o.next_decay_time,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Storage(o) => {
            let mut v = json!({
            "type": "structure", "structureType": "storage",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "owner": o.owner,
            "store": store_with_extra_json(&o.store, &o.store_extra),
            // storage 使用通用容量，无 storeCapacityResource。
            "storeCapacity": c::STORAGE_CAPACITY,
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
            });
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
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
        RoomObject::Energy(o) => {
            let mut v = json!({
                "type": "resource",
                "resourceType": energy_resource_name(o),
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "amount": o.amount,
            });
            // energy 资源额外提供 energy 别名，其他资源没有该键。
            if v["resourceType"] == json!("energy") {
                v["energy"] = json!(o.amount);
            }
            v
        }

        RoomObject::Flag(o) => {
            let mut v = json!({
                "type": "flag",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "color": o.color, "secondaryColor": o.secondary_color,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            v
        }
        // 墓碑保留绝对 decayTime，并提供非负 ticksToDecay。
        // creepBody 用部件名，creepId 用玩家可见 ID；主人字段为 user，供 prelude 读取。
        RoomObject::Tombstone(o) => {
            let mut v = json!({
                "type": "tombstone",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "deathTime": o.death_time,
                "decayTime": o.decay_time,
                "ticksToDecay": o.decay_time.saturating_sub(time),
                "creepId": match o.creep_id {
                    Some(cid) => json!(crate::world::ObjId(cid).as_str()),
                    None => Value::Null,
                },
                "creepName": o.creep_name,
                "creepTicksToLive": o.creep_ticks_to_live,
                "creepBody": o.creep_body.iter().filter_map(|code| {
                    crate::types::BodyKind::from_code(*code).map(crate::types::BodyKind::name)
                }).collect::<Vec<_>>(),
                "store": store_with_extra_json(&o.store, &o.store_extra),
            });
            if let Some(saying) = &o.creep_saying {
                v["creepSaying"] = json!(saying);
            }
            if let Some(owner) = &o.owner {
                v["user"] = json!(owner);
            }
            v
        }
        // 废墟保留被毁结构的 ID、类型及最大 hits，当前 hits 固定为 0。
        // 嵌套类型字段必须叫 type，prelude 的 Ruin.structure 从这里读取。
        // 主人同时写入顶层和 structure.user。
        // 来源：engine/src/processor/intents/structures/_destroy.js。
        RoomObject::Ruin(o) => {
            let mut structure = json!({
                "id": crate::world::ObjId(o.structure_id).as_str(),
                "type": o.structure_type,
                "hits": 0,
            });
            if let Some(hits_max) = o.structure_hits_max {
                structure["hitsMax"] = json!(hits_max);
            }
            if let Some(owner) = &o.owner {
                structure["user"] = json!(owner);
            }
            let mut v = json!({
                "type": "ruin",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "structure": structure,
                "destroyTime": o.destroy_time,
                "decayTime": o.decay_time,
                "ticksToDecay": o.decay_time.saturating_sub(time),
                "store": store_with_extra_json(&o.store, &o.store_extra),
            });
            if let Some(owner) = &o.owner {
                v["user"] = json!(owner);
            }
            v
        }
        // 无再生时间时提供 0；有值时提供非负剩余 tick。
        // 虽使用原始 type='structure'，矿物仍须在 FIND_STRUCTURES 中排除。
        RoomObject::Mineral(o) => json!({
            "type": "structure", "structureType": "mineral",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "mineralType": o.mineral_type,
            "mineralAmount": o.mineral_amount,
            "density": o.density,
            "ticksToRegeneration": match o.next_regeneration_time {
                Some(t) => t.saturating_sub(time),
                None => 0,
            },
        }),
        // extractor 的 cooldown 是逐 tick 递减的计数，不能当成截止时间换算。
        RoomObject::Extractor(o) => {
            let mut v = json!({
                "type": "structure", "structureType": "extractor",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "hits": o.hits, "hitsMax": c::EXTRACTOR_HITS,
                "cooldown": o.cooldown,
            });
            if let Some(owner) = &o.owner {
                v["owner"] = json!(owner);
            }
            if let Some(flag) = o.notify_when_attacked {
                v["notifyWhenAttacked"] = json!(flag);
            }
            v
        }
        RoomObject::Generic(o) => {
            // 恢复 Generic 的 JSON，ID 和位置以对象表中的字段为准。
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
    // pos 供使用原始 JSON 的调用方读取；快照模块另行构造查询格式。
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

/// 孵化任务保留绝对 spawnTime，供 prelude 计算 remainingTime。
/// 空 directions 表示未指定，省略键以使玩家读到 undefined。
/// 来源：engine/src/processor/intents/spawns/create-creep.js。
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

/// 原始沙箱值使用 0 表示无升级进度条，查询快照则使用 null。
fn controller_progress_total(level: u8) -> i64 {
    crate::domain::controller_progress_total_opt(level).unwrap_or(0)
}

fn obj_str<'a>(o: &'a Value, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

fn obj_energy(o: &Value) -> i64 {
    o.get("energy").and_then(Value::as_i64).unwrap_or(3000)
}

/// 按 FIND 常量匹配原始沙箱记录，不执行房间视野或私有旗过滤。
/// 有 viewer 时，MY_* 和 HOSTILE_* 按字符串 owner 判定，creep 查询排除 spawning。
/// 无 viewer 时，MY_*、HOSTILE_* 均不匹配，但 FIND_CREEPS 保留孵化中的 creep。
/// 这种中立查询供测试接口使用，不等同于玩家 room.find。
/// 参考：engine/src/game/game.js 的 findCacheFn；screeps-ok 的 vanilla findTypeMap。
pub fn matches_find(o: &Value, find_const: i64, viewer: Option<&str>) -> bool {
    let kind = obj_str(o, "type");
    let st = obj_str(o, "structureType");
    let is_struct = kind == "structure" || kind == "spawn";
    // 只有字符串 owner 参与归属；null 或缺键都不算敌方。
    // 这保留无主控制器的查询行为。creep、spawn 的结构化 owner 必为字符串；
    // 无主工地或手写 Generic 数据未必与上游玩家 getter 的敌对判定相同。
    let owner_str = o.get("owner").and_then(Value::as_str);
    let owned_by = |v: &str| owner_str == Some(v);
    let hostile_of = |v: &str| owner_str.is_some() && owner_str != Some(v);
    let spawning = o.get("spawning").and_then(Value::as_bool).unwrap_or(false);
    match find_const {
        // 玩家查询排除孵化中的 creep，中立查询保留。
        f if f == c::FIND_CREEPS => match viewer {
            Some(_) => kind == "creep" && !spawning,
            None => kind == "creep",
        },
        f if f == c::FIND_MY_CREEPS => kind == "creep" && !spawning && viewer.is_some_and(owned_by),
        f if f == c::FIND_HOSTILE_CREEPS => {
            kind == "creep" && !spawning && viewer.is_some_and(hostile_of)
        }

        f if f == c::FIND_SOURCES_ACTIVE => {
            kind == "structure" && st == "source" && obj_energy(o) > 0
        }
        f if f == c::FIND_SOURCES => kind == "structure" && st == "source",
        f if f == c::FIND_DROPPED_RESOURCES => kind == "resource",
        f if f == c::FIND_STRUCTURES => {
            // source、mineral 使用此原始 type，但玩家侧属于独立类别。
            // 来源：engine/src/game/game.js 的 structureTypes 与对象装配分支。
            is_struct && st != "source" && st != "mineral"
        }
        f if f == c::FIND_MY_STRUCTURES => {
            is_struct && st != "source" && st != "mineral" && viewer.is_some_and(owned_by)
        }
        f if f == c::FIND_HOSTILE_STRUCTURES => {
            is_struct && st != "source" && st != "mineral" && viewer.is_some_and(hostile_of)
        }
        f if f == c::FIND_FLAGS => kind == "flag",
        f if f == c::FIND_CONSTRUCTION_SITES => kind == "constructionSite",
        f if f == c::FIND_MY_SPAWNS => kind == "spawn" && viewer.is_some_and(owned_by),
        f if f == c::FIND_HOSTILE_SPAWNS => kind == "spawn" && viewer.is_some_and(hostile_of),
        f if f == c::FIND_MY_CONSTRUCTION_SITES => {
            kind == "constructionSite" && viewer.is_some_and(owned_by)
        }
        f if f == c::FIND_HOSTILE_CONSTRUCTION_SITES => {
            kind == "constructionSite" && viewer.is_some_and(hostile_of)
        }
        f if f == c::FIND_MINERALS => kind == "structure" && st == "mineral",
        f if f == c::FIND_TOMBSTONES => kind == "tombstone",
        f if f == c::FIND_POWER_CREEPS => kind == "powerCreep",
        f if f == c::FIND_DEPOSITS => kind == "deposit",
        f if f == c::FIND_RUINS => kind == "ruin",
        _ => false,
    }
}

#[cfg(test)]
mod tests;
