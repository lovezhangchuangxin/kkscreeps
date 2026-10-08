//! 权威世界与 KKPV 块输入之间的转换。
//!
//! 块内容与 `world_json_for_viewer` 的玩家数据同源：对象集合走同一条
//! 可见性规则（`room_visible_to`/`object_exported_for`），对象记录复用
//! `object_json`，扩展字段复用同一权限裁切。房间级 energy、事件等授权
//! 通道不进入房间块。受众划分为两组：对象记录是每查看者独立的私有块；
//! 全部注册房间的地形对任意查看者相同，进入 [`World::kkpv_public_terrain_blocks`]
//! 产出的公开块（每代际编码一次、跨查看者共享），信封与私有块不再携带
//! 地形（设计见 domain-projection.md §5/§6）。
//!
//! 对象 ID 只进入块目录；墓碑 `creepId` 与废墟 `structure.id` 这两类由
//! `object_json` 从 u64 生成的引用字段转为 `Ref` 编码，其余字符串保持
//! 字符串——不猜测任意键的引用语义。

use kk_schema::kkpv::{KkpvBlockInput, KkpvLimits, KkpvObject, KkpvValue, KkpvView, encode_bundle};
use kk_schema::room::RoomObject;
use serde_json::Value;

use crate::domain::room::room_energy;
use crate::projection::{ext_entries_json, object_exported_for, object_json, room_visible_to};
use crate::world::{ObjId, World};

impl World {
    /// 为查看者生成每注册房间一个 KKPV 块输入。`plan_hash` 与
    /// `audience_hash` 由宿主提供：前者来自冻结计划指纹，后者是宿主
    /// 定义的查看者摘要，本函数不计算两者。
    ///
    /// # Errors
    /// 对象记录中出现无法映射为 KKPV 值形态的数据时返回错误；
    /// 正常 `object_json` 输出与已过写入门的扩展值不会触发。
    pub fn kkpv_room_blocks_for_viewer(
        &self,
        user: &str,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Result<Vec<KkpvBlockInput>, String> {
        let mut blocks = Vec::with_capacity(self.rooms.len());
        for (name, state) in &self.rooms {
            let room_visible = room_visible_to(state, user);
            let controller = state.controller();
            let mut objects = Vec::new();
            for (id, object) in &state.objects {
                // Mod 分支先于内建导出门（与 world_json_impl 同序）：
                // mod_record_json 自带完整的对象级/字段级门——OwnerCollection
                // 的归属者集合语义不受房间视野门，若先过 object_exported_for
                // 会在不可见房被丢弃，造成与 JSON 通道的可见性分叉。
                if let RoomObject::Mod(mod_object) = object {
                    if let Some(surface) = surface
                        && let Some(record) = crate::projection::mod_record_json(
                            name,
                            *id,
                            mod_object,
                            surface,
                            Some(user),
                            room_visible,
                        )
                    {
                        objects.push(record_to_kkpv(*id, object, &record)?);
                    }
                    continue;
                }
                if !object_exported_for(Some(user), room_visible, object) {
                    continue;
                }
                let record = object_json(
                    name,
                    *id,
                    object,
                    self.time,
                    &state.terrain,
                    controller,
                    state.effects.get(id),
                )
                .expect("Mod 对象已在 kkpv_room_blocks_for_viewer 上文分流");
                let mut kkpv_object = record_to_kkpv(*id, object, &record)?;
                // 扩展目录只按 structureType 登记；非结构类型无 ext 键，
                // 与 JSON 投影的裁切入口一致。
                if let Some(type_key) = record
                    .get("structureType")
                    .and_then(Value::as_str)
                    .and_then(|st| catalog.type_for_structure(st))
                    && let Some(ext) = ext_entries_json(
                        catalog,
                        type_key,
                        &state.extensions,
                        *id,
                        crate::domain::object_owner(object).as_deref(),
                        user,
                    )
                {
                    // ext 以嵌套记录进入块字段，键与值和 JSON 投影的
                    // ext 键一致；Generic 原生数据自带 ext 键时按参考的
                    // 覆盖语义替换。
                    let mut entries = Vec::with_capacity(ext.len());
                    for (key, value) in ext {
                        let converted =
                            value_to_kkpv(&value).map_err(|e| format!("ext {key}: {e}"))?;
                        entries.push((key, converted));
                    }
                    kkpv_object.fields.retain(|(key, _)| key != "ext");
                    kkpv_object
                        .fields
                        .push(("ext".to_string(), KkpvValue::Object(entries)));
                }
                objects.push(kkpv_object);
            }
            blocks.push(KkpvBlockInput {
                private: true,
                tick: self.time,
                generation: self.generation(),
                plan_hash,
                audience_hash,
                room_name: name.clone(),
                terrain: None,
                objects,
            });
        }
        Ok(blocks)
    }

    /// 为全部注册房间生成公开地形块：内容与查看者无关（同一代际下全部
    /// 玩家共享同一份字节），每房一块、只携带地形段。`audience_hash` 是
    /// 宿主定义的公开受众组摘要——同一公开通道对所有查看者使用同一值。
    pub fn kkpv_public_terrain_blocks(
        &self,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Vec<KkpvBlockInput> {
        self.rooms
            .iter()
            .map(|(name, state)| KkpvBlockInput {
                private: false,
                tick: self.time,
                generation: self.generation(),
                plan_hash,
                audience_hash,
                room_name: name.clone(),
                terrain: Some(state.terrain.as_slice().to_vec()),
                objects: Vec::new(),
            })
            .collect()
    }

    /// KKPV 通道的世界信封：与 [`World::world_json_for_viewer`] 的房间级
    /// 内容同源同形——同样的可见房集合、逐房名称/能量/事件与 user/gcl/cpu
    /// 占位——但不携带对象数组与地形：对象记录由每查看者私有块承载，
    /// 地形由公开块承载（`kkpv_public_terrain_blocks`）。房间级授权通道
    /// 不进入任何块，能量/事件按可见房裁切后留在信封侧。
    pub fn kkpv_envelope_for_viewer(&self, user: &str, gcl_points: i64) -> Value {
        let visible: std::collections::BTreeSet<&String> = self
            .rooms
            .iter()
            .filter(|(_, state)| room_visible_to(state, user))
            .map(|(name, _)| name)
            .collect();
        let mut rooms = serde_json::Map::new();
        for (name, state) in &self.rooms {
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), Value::String(name.clone()));
            if visible.contains(name) {
                let (available, capacity) = room_energy(state, Some(user));
                entry.insert("energyAvailable".into(), Value::from(available));
                entry.insert("energyCapacityAvailable".into(), Value::from(capacity));
                if !state.events.is_empty() {
                    let events: Vec<Value> = state
                        .events
                        .iter()
                        .filter_map(|e| serde_json::from_str(e).ok())
                        .collect();
                    entry.insert("events".into(), Value::Array(events));
                }
            }
            rooms.insert(name.clone(), Value::Object(entry));
        }
        let visible_rooms: Vec<&String> = visible.iter().copied().collect();
        serde_json::json!({
            "time": self.time,
            "cpu": { "limit": 20, "tickLimit": 500, "bucket": 10000 },
            "user": user,
            "gcl": gcl_points,
            "visibleRooms": visible_rooms,
            "rooms": Value::Object(rooms),
        })
    }
}

/// 手搓世界 JSON（bot 夹具等不经权威世界构造的数据）的私有对象束转换。
/// 生产投影走 [`World::kkpv_room_blocks_for_viewer`]——权限裁切与
/// 墓碑/废墟引用推断同源；本函数面向夹具：全部字符串保持字符串
/// （不做引用语义推断），对象按记录内 room 字段分房，id 不成
/// kk 前缀十六进制时按字符串稳定合成小整数（跳过夹具内已用的真实
/// id 值，避免目录撞号）。块内对象按目录 ID 升序解出，与 JSON 通道
/// 的数组原序可能不同——依赖顺序的消费者按 id 定位。地形不进私有束
/// （公开束承载，见 [`kkpv_public_bundle_from_world_json`]）。
///
/// # Errors
/// 记录非对象、缺 id/room、字段值无法映射 KKPV 或编码超限时返回错误。
pub fn kkpv_bundle_from_world_json(
    world: &Value,
    plan_hash: [u8; 32],
    audience_hash: [u8; 32],
) -> Result<Vec<u8>, String> {
    let time = world.get("time").and_then(Value::as_u64).unwrap_or(0);
    let objects = world
        .get("objects")
        .and_then(Value::as_array)
        .ok_or("手搓世界缺 objects 数组")?;
    // 真实 id 值先入占用集，合成值从中跳过。
    let taken: std::collections::BTreeSet<u64> = objects
        .iter()
        .filter_map(|record| {
            record
                .get("id")
                .and_then(Value::as_str)
                .and_then(ObjId::parse)
                .map(|id| id.0)
        })
        .collect();
    let mut next_synthetic = 1u64;
    let mut synthetic_ids: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    let mut rooms: std::collections::BTreeMap<String, Vec<KkpvObject>> =
        std::collections::BTreeMap::new();
    for record in objects {
        let obj = record.as_object().ok_or("手搓世界对象须为 JSON 对象")?;
        let id_str = obj
            .get("id")
            .and_then(Value::as_str)
            .ok_or("手搓世界对象缺 id 字符串")?;
        let id = match ObjId::parse(id_str) {
            Some(parsed) => parsed.0,
            None => match synthetic_ids.get(id_str) {
                Some(assigned) => *assigned,
                None => {
                    let mut assigned = next_synthetic;
                    while taken.contains(&assigned) {
                        assigned += 1;
                    }
                    next_synthetic = assigned + 1;
                    synthetic_ids.insert(id_str.to_string(), assigned);
                    assigned
                }
            },
        };
        let room = obj
            .get("room")
            .and_then(Value::as_str)
            .ok_or("手搓世界对象缺 room 字符串")?
            .to_string();
        let mut fields = Vec::with_capacity(obj.len());
        for (key, value) in obj {
            if key == "id" {
                continue;
            }
            fields.push((key.clone(), value_to_kkpv(value)?));
        }
        rooms.entry(room).or_default().push(KkpvObject {
            id,
            type_key: type_key_of(record),
            fields,
        });
    }
    let blocks = rooms
        .into_iter()
        .map(|(room_name, objects)| {
            Ok::<_, String>(KkpvBlockInput {
                private: true,
                tick: time,
                generation: 0,
                plan_hash,
                audience_hash,
                room_name,
                terrain: None,
                objects,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    encode_bundle(&blocks, &KkpvLimits::default()).map_err(|e| e.to_string())
}

/// 手搓世界的公开地形束：`world.rooms` 下的全部房间各一块公开块，
/// 携带数组形态地形的原始字节，不依赖对象分布（无对象的房间同样
/// 有地形）。与 [`kkpv_bundle_from_world_json`] 的私有束配对使用。
///
/// # Errors
/// 地形数组缺失或存在大于 255 的字节值、编码超限时返回错误。
pub fn kkpv_public_bundle_from_world_json(
    world: &Value,
    plan_hash: [u8; 32],
    audience_hash: [u8; 32],
) -> Result<Vec<u8>, String> {
    let time = world.get("time").and_then(Value::as_u64).unwrap_or(0);
    let rooms = world
        .get("rooms")
        .and_then(Value::as_object)
        .ok_or("手搓世界缺 rooms 对象")?;
    let mut blocks = Vec::with_capacity(rooms.len());
    for (room_name, entry) in rooms {
        let terrain = terrain_bytes_of(entry)?;
        blocks.push(KkpvBlockInput {
            private: false,
            tick: time,
            generation: 0,
            plan_hash,
            audience_hash,
            room_name: room_name.clone(),
            terrain: Some(terrain),
            objects: Vec::new(),
        });
    }
    encode_bundle(&blocks, &KkpvLimits::default()).map_err(|e| e.to_string())
}

/// 房间条目里数组形态地形的原始字节。
fn terrain_bytes_of(entry: &Value) -> Result<Vec<u8>, String> {
    entry
        .get("terrain")
        .and_then(Value::as_array)
        .ok_or_else(|| "手搓世界房间缺数组形态地形".to_string())?
        .iter()
        .map(|b| {
            b.as_u64()
                .filter(|v| *v <= 255)
                .map(|v| v as u8)
                .ok_or_else(|| format!("地形字节越界：{b}"))
        })
        .collect()
}

/// 手搓世界的 KKPV 信封：去掉 objects 与逐房 terrain 键的浅拷贝
/// （对象记录与地形分别由私有束和公开束承载，与生产信封同形）。
pub fn kkpv_envelope_from_world_json(world: &Value) -> Value {
    let mut envelope = world.clone();
    let Some(map) = envelope.as_object_mut() else {
        return envelope;
    };
    map.remove("objects");
    if let Some(rooms) = map.get_mut("rooms").and_then(Value::as_object_mut) {
        for entry in rooms.values_mut() {
            if let Some(room) = entry.as_object_mut() {
                room.remove("terrain");
            }
        }
    }
    envelope
}

/// 记录的 KKPV 类型桶键：结构/出生点用 `builtin:{structureType}`，
/// 其余用 `builtin:{type}`。结构类型的桶键与扩展目录的类型键同约定；
/// 工地记录落 `builtin:constructionSite` 桶，而其扩展治理键仍是
/// `builtin:{目标结构类型}`——同一对象的两键不同，按桶键反查目录的
/// 消费者在接线时须以 structureType 为准。
fn type_key_of(record: &Value) -> String {
    let kind = record
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("generic");
    match kind {
        // Mod 记录的桶键是类型键本身（namespace:name 形，非 builtin 桶）。
        "mod" => record
            .get("modType")
            .and_then(Value::as_str)
            .unwrap_or("builtin:mod")
            .to_string(),
        _ => match (kind, record.get("structureType").and_then(Value::as_str)) {
            ("structure", Some(st)) | ("spawn", Some(st)) => format!("builtin:{st}"),
            _ => format!("builtin:{kind}"),
        },
    }
}

/// 把 `object_json` 记录转为块对象。`id` 由目录承担，字段中跳过；
/// 墓碑 `creepId` 与废墟 `structure.id` 转为 `Ref` 编码。
fn record_to_kkpv(id: ObjId, source: &RoomObject, record: &Value) -> Result<KkpvObject, String> {
    let type_key = type_key_of(record);
    let fields_src = record.as_object().ok_or("对象记录必须是 JSON 对象")?;
    let mut fields = Vec::with_capacity(fields_src.len());
    for (key, value) in fields_src {
        if key == "id" {
            continue;
        }
        let converted = match (source, key.as_str()) {
            (RoomObject::Tombstone(_), "creepId") => match value {
                Value::Null => KkpvValue::Null,
                Value::String(s) => match ObjId::parse(s) {
                    Some(id) => KkpvValue::Ref(id.0),
                    None => KkpvValue::Str(s.clone()),
                },
                other => value_to_kkpv(other)?,
            },
            (RoomObject::Ruin(_), "structure") => ruin_structure_value(value)?,
            _ => value_to_kkpv(value)?,
        };
        fields.push((key.clone(), converted));
    }
    Ok(KkpvObject {
        id: id.0,
        type_key,
        fields,
    })
}

/// 废墟内嵌 structure 记录：其 `id` 成员是 `object_json` 从 u64 生成的
/// 引用字符串，转回 `Ref`；其余成员按普通值转换。
fn ruin_structure_value(value: &Value) -> Result<KkpvValue, String> {
    let map = value.as_object().ok_or("废墟 structure 须为对象")?;
    let mut entries = Vec::with_capacity(map.len());
    for (key, member) in map {
        let converted = if key == "id"
            && let Value::String(s) = member
            && let Some(id) = ObjId::parse(s)
        {
            KkpvValue::Ref(id.0)
        } else {
            value_to_kkpv(member)?
        };
        entries.push((key.clone(), converted));
    }
    Ok(KkpvValue::Object(entries))
}

fn value_to_kkpv(value: &Value) -> Result<KkpvValue, String> {
    Ok(match value {
        Value::Null => KkpvValue::Null,
        Value::Bool(b) => KkpvValue::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                KkpvValue::Int(i)
            } else if let Some(u) = n.as_u64() {
                KkpvValue::Uint(u)
            } else if let Some(f) = n.as_f64() {
                // JSON 数值不会是非有限浮点；此处仅为穷尽性防御。
                KkpvValue::Float(f)
            } else {
                return Err(format!("无法编码的数值 {n}"));
            }
        }
        Value::String(s) => KkpvValue::Str(s.clone()),
        Value::Array(items) => items
            .iter()
            .map(value_to_kkpv)
            .collect::<Result<Vec<_>, _>>()
            .map(KkpvValue::Array)?,
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| value_to_kkpv(v).map(|v| (k.clone(), v)))
            .collect::<Result<Vec<_>, _>>()
            .map(KkpvValue::Object)?,
    })
}

/// 将块视图中的对象物化为 JSON 记录，键与值形态与 `object_json`
/// 的输出一致。`Ref` 还原为玩家可见的 ID 字符串。
///
/// # Errors
/// 惰性值解码失败（UTF-8、标签、限额）时返回错误的文字描述。
pub fn record_json_from_view(view: &KkpvView<'_>, index: usize) -> Result<Value, String> {
    let mut map = serde_json::Map::new();
    map.insert(
        "id".into(),
        Value::String(ObjId(view.object_id(index)).as_str()),
    );
    for key in view.field_keys(index) {
        let value = view
            .field(index, key)
            .map_err(|e| format!("字段 {key} 解码失败: {e}"))?
            .ok_or_else(|| format!("字段 {key} 声明于键集但读取为缺失"))?;
        map.insert(key.to_string(), value_to_json(&value));
    }
    Ok(Value::Object(map))
}

fn value_to_json(value: &KkpvValue) -> Value {
    match value {
        KkpvValue::Null => Value::Null,
        KkpvValue::Bool(b) => Value::Bool(*b),
        KkpvValue::Int(i) => Value::from(*i),
        KkpvValue::Uint(u) => Value::from(*u),
        KkpvValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        KkpvValue::Ref(id) => Value::String(ObjId(*id).as_str()),
        KkpvValue::Str(s) => Value::String(s.clone()),
        KkpvValue::Array(items) => Value::Array(items.iter().map(value_to_json).collect()),
        KkpvValue::Object(entries) => Value::Object(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), value_to_json(v)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests;
