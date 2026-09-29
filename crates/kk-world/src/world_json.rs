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
    /// `gcl_points`：官方 runtimeData.user.gcl（GCL 点数）——PlayerState 真值，
    /// prelude 按官方公式换算 Game.gcl（game.js:130-163）。
    /// observer/预订等更宽的可见性来源随对应能力批扩展。
    pub fn world_json_for_viewer(&self, user: &str, gcl_points: i64) -> Value {
        self.world_json_impl(Some((user, gcl_points)))
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
                // per-viewer 房间能量面（官方 game.js:324-328 按查看者过滤自家
                // spawn/extension；None 面=M1 全量投影，测试/内部消费）
                let (available, capacity) = room_energy(state, user);
                entry.insert("energyAvailable".into(), json!(available));
                entry.insert("energyCapacityAvailable".into(), json!(capacity));
                // 房间事件日志（官方 runtimeData.roomEventLog 按可见房注入——
                // data.js:141/:219；B3 的 RoomState.events 落账面，prelude
                // room.getEventLog() 消费）。仅可见房注入（房内观测面）
                if !state.events.is_empty() {
                    let events: Vec<Value> = state
                        .events
                        .iter()
                        .filter_map(|e| serde_json::from_str(e).ok())
                        .collect();
                    entry.insert("events".into(), json!(events));
                }
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
            // cpu 为占位基准值（默认配置下与预算真值相等）：真值由沙箱注入点
            // 传递（sandbox.rs Sandbox::begin_tick 的 beginTick 第二参数，JS
            // 侧合并覆盖——官方 make.js 先算窗口再装 runtimeData 的同构）
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": user,
            "gcl": viewer.map(|(_, g)| g).unwrap_or(0),
            "rooms": Value::Object(rooms),
            "objects": objects,
        })
    }

    /// 按 FIND 常量过滤房间内对象（embed findInRoom 的后端）。
    /// `viewer`：玩家面归属语义（MY_*/HOSTILE_*）；None = 视角中立面。
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
/// B3：tombstone/ruin 的 user 同样授房视野（data.js:105-116 只跳过 flag/
/// constructionSite——死爬虫的墓碑为其主人保住房间视野是官方机制）。
fn object_owner(object: &RoomObject) -> Option<std::borrow::Cow<'_, str>> {
    match object {
        RoomObject::Creep(o) => Some(o.owner.as_str().into()),
        RoomObject::Spawn(o) => Some(o.owner.as_str().into()),
        RoomObject::Extension(o) => Some(o.owner.as_str().into()),
        RoomObject::Storage(o) => Some(o.owner.as_str().into()),
        RoomObject::Controller(o) => o.owner.as_deref().map(Into::into),
        RoomObject::ConstructionSite(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Tombstone(o) => o.owner.as_deref().map(Into::into),
        RoomObject::Ruin(o) => o.owner.as_deref().map(Into::into),
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

/// 房间能量面（官方 game.js:324-328：非 off 的 spawn/extension 且
/// `object.user == runtimeData.user._id` 才计入——**per-viewer 过滤**，容量侧
/// spawn 固定 300、extension 按 RCL 表；结算侧重算不滤 owner（processor.js:326
/// 全房聚合）——两口径并存是官方事实，勿互抄）。`viewer=None` = 全房聚合
///（M1 全量投影面，测试/内部消费）。
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
        if viewer.is_none_or(|v| object_owner(o).as_deref() == Some(v)) {
            available += energy;
            capacity += cap;
        }
    }
    (available, capacity)
}

/// 对象 → 世界 JSON 记录（字段名 = prelude wrapObject 消费面约定）。
/// pub(crate)：snapshot.rs 的查询面过滤复用同源投影（FIND 语义单源）。
/// `rcl`：extension 的 storeCapacityResource.energy 归一（官方 extensions/
/// tick.js:11-17 每 tick 按房 RCL 查表重写——投影期直存等价）。
#[allow(clippy::too_many_arguments)]
/// store 两槽的全资源正量投影（官方 store.js:19-25 按资源名键构造、正量
/// 才暴露——GPT 复审 P2-3：spawn/extension/storage 与 creep 共用同口径，
/// 两投影面单源）。energy 恒投影是既有形状（M0 契约）。
pub(crate) fn store_json(store: &kk_schema::room::Store) -> Value {
    let mut v = serde_json::Map::new();
    v.insert("energy".into(), json!(store.energy));
    if store.ghodium > 0 {
        v.insert(
            kk_core::constants::RESOURCE_GHODIUM.into(),
            json!(store.ghodium),
        );
    }
    Value::Object(v)
}

/// 双槽 + 额外资源列表的全量投影（B3：tombstone/ruin 的 store——energy 恒投影
/// 的既有口径 + G/额外资源正量键，官方 store 按资源名键的正量暴露面）。
pub(crate) fn store_with_extra_json(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = match store_json(store) {
        Value::Object(m) => m,
        _ => unreachable!("store_json 恒产出对象"),
    };
    for stack in extra {
        if stack.amount > 0 {
            v.insert(stack.resource.clone(), json!(stack.amount));
        }
    }
    Value::Object(v)
}

/// 掉落资源的资源名（code 0/1/2 之外经命名槽——EnergyObj 文档）。
pub(crate) fn energy_resource_name(o: &kk_schema::room::EnergyObj) -> String {
    match (&o.resource, o.resource_type) {
        (Some(name), _) => name.clone(),
        (None, 1) => "power".into(),
        (None, 2) => kk_core::constants::RESOURCE_GHODIUM.into(),
        (None, _) => "energy".into(),
    }
}

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
            let store = store_json(&o.store);
            let mut v = json!({
                "type": "creep", "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "body": o.body.iter().map(part_json).collect::<Vec<_>>(),
                "fatigue": o.fatigue,
                "ageTime": o.age_time,
                "hits": o.hits,
                "hitsMax": o.body.len() * 100,
                "store": store,
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
                    Some(s) => {
                        let mut sv = serde_json::Map::new();
                        sv.insert("user".into(), json!(s.user));
                        sv.insert("text".into(), json!(s.text));
                        sv.insert("time".into(), json!(s.time));
                        // datetime 结算时落的墙钟毫秒（signController.js:23）；
                        // None（手写世界缺省）缺键——官方 DB 恒有，放置面不可达
                        if let Some(dt) = s.datetime {
                            sv.insert("datetime".into(), json!(dt));
                        }
                        Value::Object(sv)
                    }
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
            let mut v = json!({
                "type": "spawn", "structureType": "spawn",
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "name": o.name, "owner": o.owner,
                "store": store_json(&o.store),
                "energy": o.store.energy,
                "energyCapacity": c::SPAWN_ENERGY_CAPACITY,
                "storeCapacityResource": { "energy": c::SPAWN_ENERGY_CAPACITY },
                // off（_calc_spawns 维护的 RCL 超限标记）：spawnCreep/
                // renewCreep/recycleCreep 的 data(this.id).off 显式回查面
                "off": o.off,
                "hits": o.hits, "hitsMax": c::SPAWN_HITS,
                "spawning": spawn_spawning_json(o),
            });
            // A4-iv：通知开关落库面（官方 build.js:111-117 出生即 true；
            // place 放置面 opt 透传）——None≙undefined 缺键
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
            // 同 spawn 的官方落库形态（build.js:130-137 + extensions/tick.js
            // :11-17 的 RCL 归一——投影期直存等价）
            "storeCapacityResource": { "energy": crate::types::extension_energy_capacity(rcl) },
            "off": o.off,
            "hits": o.hits, "hitsMax": c::EXTENSION_HITS,
            });
            // A4-iv：通知开关落库面（官方 build.js:111-117 出生即 true；
            // place 放置面 opt 透传）——None≙undefined 缺键
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
            // A4-iv：通知开关落库面（官方 build.js:111-117 出生即 true；
            // place 放置面 opt 透传）——None≙undefined 缺键
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
            // A4-iv：通知开关落库面（官方 build.js:111-117 出生即 true；
            // place 放置面 opt 透传）——None≙undefined 缺键
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
            "store": store_json(&o.store),
            // 官方 storage 落库带数值 storeCapacity（build.js:150-156）、无
            // storeCapacityResource——Store.getCapacity() 读数值（A4-i Store 族）
            "storeCapacity": c::STORAGE_CAPACITY,
            "hits": o.hits, "hitsMax": c::STORAGE_HITS,
            });
            // A4-iv：通知开关落库面（官方 build.js:111-117 出生即 true；
            // place 放置面 opt 透传）——None≙undefined 缺键
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
        RoomObject::Energy(o) => json!({
            "type": "resource",
            "resourceType": energy_resource_name(o),
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "amount": o.amount,
        }),
        RoomObject::Flag(o) => json!({
            "type": "flag",
            "id": id_str, "room": room, "x": o.x, "y": o.y,
            "name": o.name, "color": o.color, "secondaryColor": o.secondary_color,
        }),
        // B3：官方 DB 形态（_die.js:22-34）——ticksToDecay 为 getter
        // （tombstones.js:36 `decayTime - runtimeData.time`），与 creep 的
        // ticksToLive 同款折叠投影；creepBody 投类型名数组（官方存形态）；
        // creepId 投字符串 id
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
        // B3：官方 DB 形态（structures/_destroy.js:22-40）——structure 存照
        // {id(字符串), type, hits: 0, hitsMax, user}；user 双落（顶层与
        // structure 内同值——官方两处同写）。键名 `type`：官方四源一致
        //（_destroy.js:29 `type: object.type`、ruins.js 代理 getter 读
        // o.structure.type、vanilla placeRuin/snapshots.ts:394、xxscreeps
        // schema——B3 评审 1 F1/评审 3-10 清偿：原 structureType 轨 A 按
        // ruins.js 接线时读 .type 会落空）
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
///
/// `viewer`：玩家面语义（官方 findCacheFn 表 game.js:17-28——MY_*/HOSTILE_* 按
/// 归属过滤、FIND_CREEPS 族滤孵化雏形 `!i.spawning`）。`None` = 视角中立面
/// （screeps-ok oracle 契约 find.ts 只支持 9 个中立常量）：101 不滤雏形
/// （CreepSnapshot.spawning 字段存在本身即证据——中立快照含孵化中 creep），
/// MY_*/HOSTILE_* 无视角不可判定 → false（官方不支持该面查询，不假装有结果）。
pub fn matches_find(o: &Value, find_const: i64, viewer: Option<&str>) -> bool {
    let kind = obj_str(o, "type");
    let st = obj_str(o, "structureType");
    let is_struct = kind == "structure" || kind == "spawn";
    // 归属三态（raw 投影面）：Some(v) 字符串=有主、null=降级清零（controller
    // 哨兵）、缺键=从未拥有。official `my === false && i.owner`：owner getter
    // 对 null 归 undefined（structures.js:136-138）→ 敌对臂只认字符串 owner。
    // 注：官方 103/113/115（creep/spawn/site 敌对臂）无 `&& i.owner` 检查
    //（game.js:18/24/29）——null owner 假想态官方判敌对我方排除；typed 投影
    // 下 owner 恒字符串或缺键（creep/spawn 必填、site None 投影缺键），null
    // owner 唯一可达对象是 controller（其敌对臂 109 官方也排除）→ 现有
    // schema 下两层与官方三方一致；schema 演化（owner 可无主化）时需复核
    let owner_str = o.get("owner").and_then(Value::as_str);
    let owned_by = |v: &str| owner_str == Some(v);
    let hostile_of = |v: &str| owner_str.is_some() && owner_str != Some(v);
    let spawning = o.get("spawning").and_then(Value::as_bool).unwrap_or(false);
    match find_const {
        // 官方 FIND_CREEPS 族滤 `!i.spawning`（game.js:18-19，雏形不进玩家面
        // FIND；中立面保留雏形——见函数头注）
        f if f == c::FIND_CREEPS => match viewer {
            Some(_) => kind == "creep" && !spawning,
            None => kind == "creep",
        },
        f if f == c::FIND_MY_CREEPS => kind == "creep" && !spawning && viewer.is_some_and(owned_by),
        f if f == c::FIND_HOSTILE_CREEPS => {
            kind == "creep" && !spawning && viewer.is_some_and(hostile_of)
        }
        // 104 官方条件 energy > 0（game.js:23）
        f if f == c::FIND_SOURCES_ACTIVE => {
            kind == "structure" && st == "source" && obj_energy(o) > 0
        }
        f if f == c::FIND_SOURCES => kind == "structure" && st == "source",
        f if f == c::FIND_DROPPED_RESOURCES => kind == "resource",
        f if f == c::FIND_STRUCTURES => {
            // 官方 FIND_STRUCTURES 不含 source/mineral：game.js:229-251 的
            // structureTypes 表无此三类（source/mineral/deposit 是独立装配分支
            // game.js:329-345）；vanilla 查询面 findTypeMap 同构
            // （snapshots.ts:478-491）
            is_struct && st != "source" && st != "mineral"
        }
        f if f == c::FIND_MY_STRUCTURES => {
            is_struct && st != "source" && st != "mineral" && viewer.is_some_and(owned_by)
        }
        f if f == c::FIND_HOSTILE_STRUCTURES => {
            // 官方 `i.my === false && i.owner`（game.js:25）：owner getter 对
            // null/缺键归 undefined → 敌对臂只认字符串 owner
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
        let sources = world.find_in_room("W1N1", c::FIND_SOURCES_ACTIVE, None);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, source_id);
        assert_eq!(world.find_in_room("W1N1", c::FIND_CREEPS, None).len(), 1);
        // 中立面 MY_* 无视角不可判定（screeps-ok find.ts 不支持该面查询）
        assert_eq!(world.find_in_room("W1N1", c::FIND_MY_SPAWNS, None).len(), 0);
        assert_eq!(
            world
                .find_in_room("W1N1", c::FIND_MY_SPAWNS, Some("p1"))
                .len(),
            0
        );

        // 对象字段名对齐 prelude
        let (_, creep_json) = world.find_in_room("W1N1", c::FIND_CREEPS, None)[0].clone();
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

        let structs = world.find_in_room("W1N1", c::FIND_STRUCTURES, None);
        assert_eq!(structs.len(), 1, "tower 必须对 FIND_STRUCTURES 可见");
        assert_eq!(structs[0].0, tower_id);
        assert_eq!(structs[0].1["type"], "structure");
        assert_eq!(structs[0].1["structureType"], "tower");
        assert_eq!(
            world.find_in_room("W1N1", c::FIND_TOMBSTONES, None).len(),
            1
        );
        // 投影单对象可见
        let j = world.object_json_by_id(tower_id).unwrap();
        assert_eq!(j["structureType"], "tower");
        assert_eq!(j["pos"]["x"], 7);
    }
    /// owner 三态投影锁（官方 controllers/tick.js:56 降级归零 `user = null`
    /// 哨兵）：Some → 字符串；cleared → null（玩家侧 my === false）；
    /// 从未拥有 → 无键（my === undefined）。与 snapshot.rs 同名测试层间对表。
    /// B3：tombstone/ruin 的世界投影（官方 DB 形态 + getter 折叠的
    /// ticksToDecay）；墓碑主人保住房间视野（data.js:105-116 只跳过 flag/
    /// constructionSite——tombstone/ruin 的 user 同样归集）。
    #[test]
    fn tombstone_and_ruin_world_projection_and_vision() {
        let mut world = World::new(42);
        world.time = 100;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        world.register_room("W2N2", Terrain::plains());
        let tomb = world
            .insert(
                "W1N1",
                RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                    id: 0,
                    x: 10,
                    y: 11,
                    death_time: 95,
                    decay_time: 130,
                    creep_id: Some(7),
                    creep_name: Some("w1".into()),
                    creep_ticks_to_live: 5,
                    creep_body: vec![0, 1],
                    creep_saying: None,
                    owner: Some("p1".into()),
                    store: kk_schema::room::Store {
                        energy: 30,
                        ghodium: 0,
                    },
                    store_extra: vec![kk_schema::room::ResourceStack {
                        resource: "XUHO2".into(),
                        amount: 6,
                    }],
                }),
            )
            .unwrap();
        let ruin = world
            .insert(
                "W1N1",
                RoomObject::Ruin(kk_schema::room::RuinObj {
                    id: 0,
                    x: 12,
                    y: 11,
                    structure_id: 9,
                    structure_type: "spawn".into(),
                    structure_hits_max: Some(5000),
                    destroy_time: 98,
                    decay_time: 598,
                    owner: Some("p1".into()),
                    store: kk_schema::room::Store {
                        energy: 300,
                        ghodium: 4,
                    },
                    store_extra: Vec::new(),
                }),
            )
            .unwrap();
        let j = world.object_json_by_id(tomb).unwrap();
        assert_eq!(j["type"], "tombstone");
        assert_eq!(j["user"], "p1");
        assert_eq!(j["deathTime"], 95);
        assert_eq!(j["decayTime"], 130);
        assert_eq!(j["ticksToDecay"], 30, "getter 折叠（decayTime − time）");
        assert_eq!(j["creepId"], "kk0000000000000007", "u64 转官方字符串 id");
        assert_eq!(j["creepName"], "w1");
        assert_eq!(j["creepTicksToLive"], 5);
        assert_eq!(j["creepBody"], serde_json::json!(["move", "work"]));
        assert_eq!(j["store"]["energy"], 30);
        assert_eq!(j["store"]["XUHO2"], 6, "额外资源并入 store 投影");
        assert!(
            j.get("creepSaying").is_none(),
            "无遗言不落键（官方 undefined）"
        );

        let r = world.object_json_by_id(ruin).unwrap();
        assert_eq!(r["type"], "ruin");
        assert_eq!(r["destroyTime"], 98);
        assert_eq!(r["ticksToDecay"], 498);
        assert_eq!(r["structure"]["id"], "kk0000000000000009");
        assert_eq!(r["structure"]["type"], "spawn");
        assert_eq!(
            r["structure"]["hits"], 0,
            "存照 hits 恒 0（_destroy.js:29）"
        );
        assert_eq!(r["structure"]["hitsMax"], 5000);
        assert_eq!(r["structure"]["user"], "p1");
        assert_eq!(r["user"], "p1", "顶层 user 双落（官方两处同写）");
        assert_eq!(r["store"]["energy"], 300);
        assert_eq!(r["store"]["G"], 4);

        // FIND 两层（A3c 起 find_in_room 带 viewer——中立面 None）
        assert_eq!(
            world.find_in_room("W1N1", c::FIND_TOMBSTONES, None).len(),
            1
        );
        assert_eq!(world.find_in_room("W1N1", c::FIND_RUINS, None).len(), 1);
        assert_eq!(
            world.find_in_room("W1N1", c::FIND_STRUCTURES, None).len(),
            0
        );

        // 视野：仅有一枚墓碑的房间对 p1 可见（官方 userObjects 归集含墓碑）
        let viewer = world.world_json_for_viewer("p1", 0);
        let rooms = viewer["rooms"].as_object().unwrap();
        assert!(rooms["W1N1"].get("terrain").is_some(), "墓碑授视野");
        assert!(rooms["W2N2"].get("terrain").is_some(), "地形面恒在");
        let visible_objects = viewer["objects"].as_array().unwrap();
        assert_eq!(visible_objects.len(), 2, "可见房内对象全量（墓碑+废墟）");
    }

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
                        datetime: None,
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

    /// A3c：FIND 族 per-viewer 语义锁（官方 findCacheFn 表 game.js:17-28）。
    /// 双主人房：creep（含孵化雏形）/spawn/extension/tower/工地，验证
    /// MY_*/HOSTILE_* 归属分野 + FIND_CREEPS 玩家面滤雏形 + 中立面语义。
    #[test]
    fn find_per_viewer_semantics() {
        let mut world = World::new(3);
        world.time = 10;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let ctx = PlaceCtx {
            time: world.time,
            controller: None,
            name_seq: 0,
        };
        // p1：普通 creep + 孵化雏形 + spawn + extension；p2：creep + spawn；
        // 工地 p1 一个
        let creep1 = normalize(
            "creep",
            &serde_json::json!({ "pos": [10, 10], "owner": "p1", "body": ["move"], "name": "c1" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", creep1).unwrap();
        let ghost = normalize(
            "creep",
            &serde_json::json!({ "pos": [11, 10], "owner": "p1", "body": ["move"], "name": "g1" }),
            &ctx,
        )
        .unwrap();
        let ghost_id = world.insert("W1N1", ghost).unwrap();
        // place 对 spawning 硬编码 false——insert 后直改（孵化雏形形态）
        if let Some(RoomObject::Creep(g)) =
            world.room_mut("W1N1").unwrap().objects.get_mut(&ghost_id)
        {
            g.spawning = true;
        }
        let creep2 = normalize(
            "creep",
            &serde_json::json!({ "pos": [12, 10], "owner": "p2", "body": ["move"], "name": "c2" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", creep2).unwrap();
        let spawn1 = normalize(
            "spawn",
            &serde_json::json!({ "pos": [13, 10], "structureType": "spawn", "owner": "p1", "name": "s1" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", spawn1).unwrap();
        let spawn2 = normalize(
            "spawn",
            &serde_json::json!({ "pos": [14, 10], "structureType": "spawn", "owner": "p2", "name": "s2" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", spawn2).unwrap();
        let ext1 = normalize(
            "structure",
            &serde_json::json!({ "pos": [15, 10], "structureType": "extension", "owner": "p1" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", ext1).unwrap();
        let tower2 = normalize(
            "structure",
            &serde_json::json!({ "pos": [16, 10], "structureType": "tower", "owner": "p2" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", tower2).unwrap();
        let wall = normalize(
            "structure",
            &serde_json::json!({ "pos": [17, 10], "structureType": "constructedWall" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", wall).unwrap();
        let site1 = normalize(
            "constructionSite",
            &serde_json::json!({ "pos": [18, 10], "structureType": "road", "owner": "p1" }),
            &ctx,
        )
        .unwrap();
        world.insert("W1N1", site1).unwrap();

        // 降级清零 controller（owner:null 哨兵——官方 owner getter 对 null 归
        // undefined，structures.js:136-138）：MY/HOSTILE 双臂均不进（109 的
        // `my===false && i.owner` 把它挡在敌对外——评审 3 P3-3 边角锁）
        let cleared_ctl = RoomObject::Controller(ControllerObj {
            id: 0,
            x: 2,
            y: 1,
            level: 1,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 0,
            downgrade_time: None,
            owner: None,
            owner_cleared: true,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        });
        world.insert("W1N1", cleared_ctl).unwrap();
        let count = |f: i64, v: Option<&str>| world.find_in_room("W1N1", f, v).len();
        // FIND_CREEPS：玩家面滤雏形（game.js:18 !i.spawning）；中立面含雏形
        assert_eq!(count(c::FIND_CREEPS, Some("p1")), 2, "c1+c2，雏形被滤");
        assert_eq!(count(c::FIND_CREEPS, None), 3, "中立面含孵化雏形");
        // MY/HOSTILE creep（game.js:18-19）
        assert_eq!(count(c::FIND_MY_CREEPS, Some("p1")), 1);
        assert_eq!(count(c::FIND_MY_CREEPS, Some("p2")), 1);
        assert_eq!(count(c::FIND_HOSTILE_CREEPS, Some("p1")), 1, "p2 的 c2");
        assert_eq!(
            count(c::FIND_HOSTILE_CREEPS, Some("p2")),
            1,
            "p1 的 c1（雏形滤）"
        );
        // MY/HOSTILE spawns（game.js:22-23）
        assert_eq!(count(c::FIND_MY_SPAWNS, Some("p1")), 1);
        assert_eq!(count(c::FIND_HOSTILE_SPAWNS, Some("p1")), 1);
        // MY/HOSTILE structures（game.js:24-25）：spawn+extension+tower 归属
        // 分野；wall 无 owner（my undefined）与降级清零 controller（owner:
        // null 哨兵）两边都不进
        assert_eq!(count(c::FIND_MY_STRUCTURES, Some("p1")), 2, "s1+ext1");
        assert_eq!(count(c::FIND_MY_STRUCTURES, Some("p2")), 2, "s2+tower");
        assert_eq!(
            count(c::FIND_HOSTILE_STRUCTURES, Some("p1")),
            2,
            "s2+tower；wall 无 owner/cleared ctl 的 null owner 都不进"
        );
        assert_eq!(count(c::FIND_HOSTILE_STRUCTURES, Some("p2")), 2, "s1+ext1");
        // MY/HOSTILE construction sites（game.js:26-27）
        assert_eq!(count(c::FIND_MY_CONSTRUCTION_SITES, Some("p1")), 1);
        assert_eq!(count(c::FIND_HOSTILE_CONSTRUCTION_SITES, Some("p2")), 1);
        assert_eq!(count(c::FIND_MY_CONSTRUCTION_SITES, Some("p2")), 0);
        // 中立面 MY_*：不可判定 → 空
        assert_eq!(count(c::FIND_MY_CREEPS, None), 0);
        assert_eq!(count(c::FIND_HOSTILE_STRUCTURES, None), 0);
    }

    /// A3c：energyAvailable/CapacityAvailable per-viewer（官方 game.js:324-328
    /// 只累计查看者自家非 off 的 spawn/extension）。双主人房 + off spawn。
    #[test]
    fn energy_per_viewer_projection() {
        let mut world = World::new(3);
        world.time = 1;
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let ctx = PlaceCtx {
            time: world.time,
            controller: None,
            name_seq: 0,
        };
        let mk_spawn = |name: &str, owner: &str, energy: i64| {
            normalize(
                "spawn",
                &serde_json::json!({ "pos": [10, 10], "structureType": "spawn", "owner": owner,
                    "name": name, "store": { "energy": energy } }),
                &ctx,
            )
            .unwrap()
        };
        world.insert("W1N1", mk_spawn("s1", "p1", 100)).unwrap();
        let off_id = world.insert("W1N1", mk_spawn("s1off", "p1", 50)).unwrap();
        // place 对 off 硬编码 false——insert 后直改（_calc_spawns 的 off 标记）
        if let Some(RoomObject::Spawn(s)) = world.room_mut("W1N1").unwrap().objects.get_mut(&off_id)
        {
            s.off = true;
        }
        world.insert("W1N1", mk_spawn("s2", "p2", 200)).unwrap();
        let j1 = world.world_json_for_viewer("p1", 0);
        // p1 只见自家非 off spawn：100/300（off 的 50 不计、p2 的 200 不计）
        assert_eq!(j1["rooms"]["W1N1"]["energyAvailable"], 100);
        assert_eq!(j1["rooms"]["W1N1"]["energyCapacityAvailable"], 300);
        let j2 = world.world_json_for_viewer("p2", 0);
        assert_eq!(j2["rooms"]["W1N1"]["energyAvailable"], 200);
        assert_eq!(j2["rooms"]["W1N1"]["energyCapacityAvailable"], 300);
        // 全量面（M1 测试/内部消费）：全房聚合
        let jn = world.world_json();
        assert_eq!(jn["rooms"]["W1N1"]["energyAvailable"], 300);
        assert_eq!(jn["rooms"]["W1N1"]["energyCapacityAvailable"], 600);
    }

    /// A3c：world.gcl 投影（官方 runtimeData.user.gcl 点数直存——换算在
    /// prelude，game.js:130-163 正变换的锚点数据）。
    #[test]
    fn gcl_points_projection() {
        let mut world = World::new(1);
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        let j = world.world_json_for_viewer("p1", 1_000_000);
        assert_eq!(j["gcl"], 1_000_000);
        assert_eq!(j["user"], "p1");
        assert_eq!(world.world_json()["gcl"], 0);
    }
}
