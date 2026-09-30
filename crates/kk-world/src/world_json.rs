//! typed 世界 → prelude"世界 JSON"投影（M0 契约形状，kk-runner beginTick 约定）。
//!
//! 字段名与 prelude wrapObject 的消费面对齐；这也是 embed findInRoom 的
//! Rust 侧查询面（matches_find 与 prelude room.find 是同一 FIND 语义的两层，
//! 行为必须一致——M0 第七轮"层间一致"教训）。出口常量（1/3/5/7/10）是
//! JS 层独有面：出口是 terrain 派生位置非对象，embed 契约快照（带 id 形状）
//! 装不下，matches_find 对其返空是既定口径（A5 片 3b 评审 P3 登记）。

use std::sync::Arc;

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
                        // 后变成玩家可自主触发的持续视野）。M2B P2 清偿：旗有
                        // owner 后必须显式跳过（原「无 owner 天然不授」失效）——
                        // 旗同样不授房视野
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
                // M2B 扩面 P2：旗按玩家私有（官方 flags 是 per-user 集合，
                // runtimeData 只注入查看者自己的旗——data.js flags 面；他人/
                // 无主旗不进该查看者的投影。None 面=内部全量消费不受滤）
                if let Some(viewer) = user
                    && let RoomObject::Flag(f) = object
                    && f.owner.as_deref().is_some_and(|o| o != viewer)
                {
                    continue;
                }
                if !room_visible {
                    // 自家私有集合破例导出（三路评审 P2 清偿，工地先例）：
                    // 官方按 user 全量拉自家对象（data.js:103）——工地全集进
                    // Game.constructionSites 含不可见房（game.js:366-374）；
                    // 旗全集进 Game.flags（data.js:140 `rooms.flags` 按 user
                    // 全局查询，不按房圈定）。两语义与「不授视野」（data.js:
                    // 113-116 跳过表）正交——破例导出不等于授视野
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
        // 可见房显式传递（官方 data.js:103-116 服务端归集 runtimeData.rooms
        // ——JS 不从对象字段重算；GPT 复审 P1-1 清偿：原 prelude 用 o.owner
        // 推断，tombstone/ruin 投影键是 user → 仅剩己方墓碑/废墟时房从
        // Game.rooms 消失）。BTreeSet 迭代=确定性序
        let visible_rooms: Vec<&String> = visible.iter().copied().collect();
        json!({
            "time": self.time,
            // cpu 为占位基准值（默认配置下与预算真值相等）：真值由沙箱注入点
            // 传递（sandbox.rs Sandbox::begin_tick 的 beginTick 第二参数，JS
            // 侧合并覆盖——官方 make.js 先算窗口再装 runtimeData 的同构）
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": user,
            "gcl": viewer.map(|(_, g)| g).unwrap_or(0),
            "visibleRooms": visible_rooms,
            "rooms": Value::Object(rooms),
            "objects": objects,
        })
    }

    /// 地形注入（M2B 前置批 P3：按纪元缓存的紧凑载荷）。
    ///
    /// `rooms[name].terrain` 值 = `{"rev": <terrain_epoch>, "payload":
    /// "<base64(2500B)>"}`——prelude 按 rev 命中跨 tick 复用解码产物，未命中
    /// 解码 b64；旧 2500 元素数字数组形态不再由真路径产出（宿主手搓注入的
    /// 夹具路径仍被 prelude 接受，game.js 双形态收）。旧形态每 tick 每玩家
    /// 每房重建 2500 元素 Value 数组（~80KB Vec 缓冲）+ 序列化 ~5.0KB +
    /// JS 解析 2500 个数字 token；新形态 b64 ~3.3KB 一个字符串 token、
    /// Rust 侧纪元内零重编码、JS 侧纪元内零重解码（P3 实测 project 相
    /// −42% / run 相 −40%，方案 §7）。
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

/// 标准字母表 base64（含 '=' 填充）——地形紧凑载荷的编码面（P3）。无新
/// 依赖的自包含实现；解码侧在 prelude（game.js 的 b64ToBytes）与测试孪生。
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
        if viewer.is_none_or(|v| crate::domain::object_owner(o).as_deref() == Some(v)) {
            available += energy;
            capacity += cap;
        }
    }
    (available, capacity)
}

/// store 两槽的全资源正量投影（官方 store.js:19-25 按资源名键构造、正量
/// 才暴露——GPT 复审 P2-3：spawn/extension/storage 与 creep 共用同口径，
/// 两投影面单源）。energy 恒投影是既有形状（M0 契约）。
pub(crate) fn store_json(store: &kk_schema::room::Store) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    Value::Object(v)
}

/// 双槽 + 额外资源列表的全量投影（B3：tombstone/ruin 的 store——energy 恒投影
/// 的既有口径 + G/额外资源正量键，官方 store 按资源名键的正量暴露面）。
pub(crate) fn store_with_extra_json(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    crate::domain::apply_store_extra(&mut v, extra);
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
            // getter 正量暴露的既有口径，vanilla snapStore 同构）；B4 起命名
            // 矿物槽并入（harvest mineral 产出——官方 store 按资源名键开放对象）
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
                // 官方 creep 落库带 storeCapacity（_recalc-body：活跃 CARRY ×
                // CARRY_CAPACITY × boost 乘数）——creeps.js carryCapacity 直读
                // `o.storeCapacity`、Store.getCapacity() 同源（A4-i Store 族）
                "storeCapacity": crate::snapshot::creep_store_capacity_raw(&o.body),
                "spawning": o.spawning,
            });
            if o.age_time != 0 {
                v["ticksToLive"] = json!(o.age_time.saturating_sub(time));
            }
            // say 载荷（M2B 扩面 P1）：官方 actionLog.say 的等价读取面——
            // prelude saying getter 按 owner/isPublic 折叠（creeps.js:84-92）；
            // notifyWhenAttacked：API 判重门读（creeps.js:1008）
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
                // B2 schema 字段的投影清偿（A4-iii）：绝对 tick / {user,endTime}
                // 官方 DB 原始形态——prelude getter（structures.js 对照
                // structures.js:143-169）与快照面（snapshot.rs）各自换算，
                // None → null（downgradeTime 同形）。sign 的 datetime 键
                // Some 时带（结算落的墙钟毫秒，经 TickInput 注入——ADR-013
                // 决策 5）、None 缺键（见下方 sign 分支）
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
            // 归属三态（与 snapshot 同形——controller_owner_value 单源）
            if let Some(val) = crate::domain::controller_owner_value(o) {
                v["owner"] = val;
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
        RoomObject::Energy(o) => {
            let mut v = json!({
                "type": "resource",
                "resourceType": energy_resource_name(o),
                "id": id_str, "room": room, "x": o.x, "y": o.y,
                "amount": o.amount,
            });
            // M2B 扩面 P4：energy 堆补 amount 别名键（官方 DB 形态 energy:N
            // ——resources.js:36 的 o.energy 读取面；非 energy 资源无此键
            //=undefined 官方同形）
            if v["resourceType"] == json!("energy") {
                v["energy"] = json!(o.amount);
            }
            v
        }
        // M2B 扩面 P2：owner 键（Some 才落——per-viewer 过滤在
        // world_json_impl 的对象循环，None=无主旗全局可见兜底）
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
        // B4：官方 DB 形态（minerals/tick.js 读写的字段面）——与 source 同款
        // 折叠（ticksToRegeneration = nextRegenerationTime ? n − time : 0；
        // 官方 getter 无钟为 undefined，prelude 端 ?? 0 兜底——None→0 既有
        // 口径）。type:"structure" + structureType:"mineral" 保持 FIND 装配面
        //（matches_find 的 116 臂与官方 game.js:329-345 独立装配分支同构）
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
        // B4：官方落库形态（build.js:214-220 的 extractor 分支：user/hits/
        // hitsMax=EXTRACTOR_HITS + 通用 notifyWhenAttacked）——cooldown 为
        // 计数器（extractors/tick.js 逐 tick 递减；getter `o.cooldown || 0`
        // structures.js:269）。owner 仅 Some 落键（无主提取器——harvest 的
        // 无主恒 active 分支 utils.js:457-460）
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
    crate::domain::controller_progress_total_opt(level).unwrap_or(0)
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
mod tests;
