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
        self.world_json_impl(None, None)
    }

    /// 为指定玩家生成沙箱数据，gcl_points 是玩家累计 GCL 点数，不是等级。
    /// 房内存在己方对象时授予视野，但旗和工地不授予；可见房中的敌方对象也会导出。
    /// 己方旗和工地可单独导出，即使其房间不可见；他人的旗被排除。
    /// 所有已注册房间的地形仍会提供，无主旗只在可见房间导出。
    /// observer 观察标记授予限时额外房间视野（observed_rooms_of 并入，
    /// 一拍生命周期）；控制器预订仍未实现、不授予视野。
    /// 视野归集参考：driver/lib/runtime/data.js 的 userObjects 房间计算。
    pub fn world_json_for_viewer(&self, user: &str, gcl_points: i64) -> Value {
        self.world_json_impl(Some((user, gcl_points)), None)
    }

    /// 带扩展字段目录与 Mod 玩家面的玩家投影：扩展字段按权限裁切后
    /// 进入对象的 `ext` 键；Mod 对象经玩家面目录（`PlayerSurface`）按对象/字段
    /// 可见性裁切后进入对象数组（surface 为 None 时不进——vanilla 口径）。
    /// Public 对看到该对象的玩家可见；Owner 仅对象主人可见；ServerOnly
    /// 不进入任何玩家投影。未声明扩展字段的类型与全部字段被裁掉的对象
    /// 都不输出 `ext` 键。
    pub fn world_json_for_viewer_with_extensions(
        &self,
        user: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
    ) -> Value {
        use crate::world::ObjId;
        let mut world = self.world_json_impl(Some((user, gcl_points)), surface);
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
            // owner 与 KKPV 通道同源（domain::object_owner 从权威枚举派生），
            // 不读投影 JSON 的 owner 键——两通道的字段可见性判定必须一致，
            // 避免记录键与枚举派生值出现分歧时 Owner 字段裁切分叉。
            let owner = state
                .objects
                .get(&oid)
                .and_then(crate::domain::object_owner);
            if let Some(ext) = ext_entries_json(
                catalog,
                type_key,
                &state.extensions,
                oid,
                owner.as_deref(),
                user,
            ) && let Some(map) = object.as_object_mut()
            {
                map.insert("ext".into(), Value::Object(ext));
            }
        }
        world
    }

    fn world_json_impl(
        &self,
        viewer: Option<(&str, i64)>,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
    ) -> Value {
        let user = viewer.map(|(u, _)| u);
        // observer 观察标记授予下一拍限时视野：并入可见集后，房间的能量
        // 统计、事件与对象导出与己方对象房同口径（官方 data.js 同源）。
        let observed = user.map(|user| self.observed_rooms_of(user));
        let mut visible: std::collections::BTreeSet<&String> = match user {
            None => self.rooms.keys().collect(),
            Some(user) => self
                .rooms
                .iter()
                .filter(|(_, state)| room_visible_to(state, user))
                .map(|(name, _)| name)
                .collect(),
        };
        if let Some(observed) = &observed {
            visible.extend(observed.iter());
        }
        let mut objects = Vec::new();
        let mut rooms = serde_json::Map::new();
        for (name, state) in &self.rooms {
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), json!(name));
            entry.insert("terrain".into(), self.terrain_json(name, state));

            let controller = state.controller();
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
                // Mod 对象只在携带玩家面目录时进入投影（按对象/字段可见
                // 性裁切）；内部全量形态（world_json，无目录）与查询快照
                // 走 object_snapshot 的专用读面。
                if let RoomObject::Mod(mod_object) = object {
                    if let Some(surface) = surface
                        && let Some(record) =
                            mod_record_json(name, *id, mod_object, surface, user, room_visible)
                    {
                        objects.push(record);
                    }
                    continue;
                }
                if !object_exported_for(user, room_visible, object) {
                    continue;
                }
                objects.push(
                    object_json(
                        name,
                        *id,
                        object,
                        self.time,
                        &state.terrain,
                        controller,
                        state.effects.get(id),
                    )
                    .expect("Mod 对象已在 world_json_impl 上文分流"),
                );
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
    pub(crate) fn terrain_json(&self, name: &str, state: &crate::world::RoomState) -> Value {
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
    /// Mod 对象不产出原始记录（object_json 对其返回 None），本面天然不含；
    /// Mod 的管理查询走 object_snapshot_by_id 的单对象读面，玩家面走带
    /// 裁切的投影通道。需要查询快照格式时使用 find_snapshots_in_room。
    pub fn find_in_room(
        &self,
        room: &str,
        find_const: i64,
        viewer: Option<&str>,
    ) -> Vec<(crate::world::ObjId, Value)> {
        let Some(state) = self.rooms.get(room) else {
            return Vec::new();
        };
        let controller = state.controller();
        state
            .objects
            .iter()
            .filter_map(|(id, o)| {
                let record = object_json(
                    room,
                    *id,
                    o,
                    self.time,
                    &state.terrain,
                    controller,
                    state.effects.get(id),
                )?;
                matches_find(&record, find_const, viewer).then_some((*id, record))
            })
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

    /// 按 ID 返回原始沙箱记录，不过滤玩家视野。Mod 对象无原始记录
    /// （object_json 返回 None）；其查询走 object_snapshot_by_id 的
    /// 专用读面。运行时的 getObject 查询接口使用快照格式。
    pub fn object_json_by_id(&self, id: crate::world::ObjId) -> Option<Value> {
        let (room, object) = self.get(id)?;
        let state = self.rooms.get(room)?;
        let terrain = &state.terrain;
        object_json(
            room,
            id,
            object,
            self.time,
            terrain,
            state.controller(),
            state.effects.get(&id),
        )
    }
}

mod find;
mod object;
mod visibility;
pub(crate) use crate::domain::room::{road_hits_max, room_energy};
pub use find::matches_find;
pub(crate) use object::{energy_resource_name, object_json, part_json};
pub(crate) use visibility::{
    ext_entries_json, mod_record_json, object_exported_for, room_visible_to,
};

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

/// 在沙箱 store 中合并额外正量资源，仍保留 energy=0 的键。
pub fn store_with_extra_json(
    store: &kk_schema::room::Store,
    extra: &[kk_schema::room::ResourceStack],
) -> Value {
    let mut v = crate::domain::store_positive_entries(store);
    v.insert("energy".into(), json!(store.energy));
    crate::domain::apply_store_extra(&mut v, extra);
    Value::Object(v)
}

#[cfg(test)]
mod tests;
