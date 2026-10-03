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

use crate::world::{ObjId, World};
use crate::world_json::{
    ext_entries_json, object_exported_for, object_json, room_energy, room_rcl, room_visible_to,
};

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
            let rcl = room_rcl(state);
            let mut objects = Vec::new();
            for (id, object) in &state.objects {
                // Mod 分支先于内建导出门（与 world_json_impl 同序）：
                // mod_record_json 自带完整的对象级/字段级门——OwnerCollection
                // 的归属者集合语义不受房间视野门，若先过 object_exported_for
                // 会在不可见房被丢弃，造成与 JSON 通道的可见性分叉。
                if let RoomObject::Mod(mod_object) = object {
                    if let Some(surface) = surface
                        && let Some(record) = crate::world_json::mod_record_json(
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
                let record = object_json(name, *id, object, self.time, &state.terrain, rcl);
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
mod tests {
    use super::*;
    use kk_core::Terrain;
    use kk_schema::room::{
        BodyPart, ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj, ExtensionObj, FlagObj,
        GenericObj, Reservation, RuinObj, Sign, SourceObj, SpawnObj, SpawningJob, StorageObj,
        Store, TombstoneObj,
    };
    use serde_json::json;

    fn key(name: &str) -> kk_mod_api::Key {
        kk_mod_api::Key::new(name).expect("测试键合法")
    }

    /// 挂载在 builtin:link 上的三权限扩展字段贡献。
    fn ext_contribution() -> kk_mod_api::Contribution {
        use kk_mod_api::*;
        use std::collections::BTreeSet;
        let mount =
            |name: &str, schema: ValueSchema, visibility: Visibility| FieldMountDescriptor {
                target: TypeRef {
                    key: key("builtin:link"),
                    schema_version: 1,
                },
                field: FieldDescriptor {
                    key: key(name),
                    schema,
                    default: FieldDefault::Missing,
                    visibility,
                    binding: key(&format!("{name}.impl")),
                },
            };
        let signed = || ValueSchema {
            kind: ValueKind::Number(NumberDomain::Signed { min: 0, max: 1000 }),
            nullable: false,
        };
        let text = || ValueSchema {
            kind: ValueKind::String { max_bytes: 64 },
            nullable: false,
        };
        Contribution {
            source: Source {
                namespace: "test".into(),
                api_version: 1,
                revision: "ext-fields".into(),
                code_digest: [0; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: BTreeSet::from([
                key("test:charge.impl"),
                key("test:ownerNote.impl"),
                key("test:secret.impl"),
                key("test:ratio.impl"),
            ]),
            object_types: Vec::new(),
            field_mounts: vec![
                mount("test:charge", signed(), Visibility::Public),
                mount(
                    "test:ratio",
                    ValueSchema {
                        kind: ValueKind::Number(NumberDomain::Float {
                            min: 0.0,
                            max: 10.0,
                        }),
                        nullable: false,
                    },
                    Visibility::Public,
                ),
                mount("test:ownerNote", text(), Visibility::Owner),
                mount(
                    "test:secret",
                    ValueSchema {
                        kind: ValueKind::Bool,
                        nullable: false,
                    },
                    Visibility::ServerOnly,
                ),
            ],
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
        }
    }

    fn catalog_fixture() -> kk_mod_api::extension::ExtensionCatalog {
        let plan = kk_mod_api::freeze(
            vec![kk_mod_api::builtin::link(), ext_contribution()],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("夹具计划可冻结");
        kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录提取成功")
    }

    fn store(energy: u32) -> Store {
        Store { energy, ghodium: 0 }
    }

    /// 覆盖全部 16 个内建对象族变体与两种查看者视野的对照夹具。
    fn rich_world() -> World {
        let mut world = World::new(11);
        world.set_time_for_test(500);
        let mut w1 = Terrain::plains();
        w1.normalize();
        world.register_room("W1N1", w1);
        world.register_room("E2S2", Terrain::plains());
        let insert = |world: &mut World, room: &str, object| {
            world.insert(room, object).expect("夹具对象可插入")
        };
        insert(
            &mut world,
            "W1N1",
            RoomObject::Controller(ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level: 3,
                safe_mode: 0,
                safe_mode_available: 1,
                progress: 12_345,
                downgrade_time: Some(900),
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: true,
                reservation: Some(Reservation {
                    user: "p2".into(),
                    end_time: 700,
                }),
                sign: Some(Sign {
                    user: "p1".into(),
                    text: "签名".into(),
                    time: 400,
                    datetime: Some(1_700_000_000_000),
                }),
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: store(220),
                spawning: Some(SpawningJob {
                    name: "b1".into(),
                    need_time: 3,
                    spawn_time: 505,
                    directions: vec![1, 5],
                }),
                notify_when_attacked: Some(true),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Extension(ExtensionObj {
                id: 0,
                x: 11,
                y: 10,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: store(200),
                notify_when_attacked: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Storage(StorageObj {
                id: 0,
                x: 12,
                y: 10,
                hits: 10_000,
                owner: "p1".into(),
                store: store(900),
                notify_when_attacked: Some(false),
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 40,
                }],
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Source(SourceObj {
                id: 0,
                x: 20,
                y: 20,
                energy: 2500,
                energy_capacity: 3000,
                next_regeneration_time: Some(600),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 15,
                y: 15,
                fatigue: 2,
                age_time: 800,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    BodyPart {
                        kind: 0,
                        boost: None,
                        hits: 100,
                    },
                    BodyPart {
                        kind: 4,
                        boost: Some("XUHO2".into()),
                        hits: 90,
                    },
                ],
                store: store(50),
                store_extra: Vec::new(),
                say_message: Some("hi".into()),
                say_public: Some(true),
                notify_when_attacked: Some(false),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 25,
                y: 25,
                fatigue: 0,
                age_time: 900,
                hits: 250,
                spawning: false,
                action_log: 0,
                name: "e1".into(),
                owner: "p2".into(),
                body: Vec::new(),
                store: store(0),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        );
        let link = insert(
            &mut world,
            "W1N1",
            RoomObject::Generic(GenericObj {
                id: 0,
                x: 30,
                y: 30,
                data: r#"{"type":"structure","structureType":"link","owner":"p1","store":{"energy":120},"storeCapacityResource":{"energy":800},"cooldown":3,"hits":500,"hitsMax":500}"#.into(),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Tombstone(TombstoneObj {
                id: 0,
                x: 35,
                y: 35,
                death_time: 495,
                decay_time: 600,
                creep_id: Some(7),
                creep_name: Some("w0".into()),
                creep_ticks_to_live: 12,
                creep_body: vec![0, 4],
                creep_saying: Some("bye".into()),
                owner: Some("p1".into()),
                store: store(30),
                store_extra: Vec::new(),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Ruin(RuinObj {
                id: 0,
                x: 36,
                y: 35,
                structure_id: 9,
                structure_type: "spawn".into(),
                structure_hits_max: Some(5000),
                destroy_time: 498,
                decay_time: 598,
                owner: Some("p1".into()),
                store: store(300),
                store_extra: Vec::new(),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Energy(EnergyObj {
                id: 0,
                x: 40,
                y: 40,
                resource_type: 0,
                amount: 120,
                resource: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Energy(EnergyObj {
                id: 0,
                x: 41,
                y: 40,
                resource_type: 1,
                amount: 60,
                resource: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Flag(FlagObj {
                id: 0,
                x: 1,
                y: 1,
                name: "mine".into(),
                color: 1,
                secondary_color: 1,
                owner: Some("p1".into()),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Flag(FlagObj {
                id: 0,
                x: 2,
                y: 1,
                name: "theirs".into(),
                color: 2,
                secondary_color: 2,
                owner: Some("p2".into()),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 6,
                y: 6,
                hits: 25_000,
                notify_when_attacked: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 7,
                y: 6,
                hits: 1_000,
                next_decay_time: 0,
                notify_when_attacked: Some(true),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Mineral(kk_schema::room::MineralObj {
                id: 0,
                x: 8,
                y: 6,
                mineral_type: "X".into(),
                mineral_amount: 45_000,
                density: 3,
                next_regeneration_time: Some(1_000),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Extractor(kk_schema::room::ExtractorObj {
                id: 0,
                x: 9,
                y: 6,
                hits: 500,
                owner: Some("p1".into()),
                cooldown: 7,
                notify_when_attacked: None,
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::Tombstone(TombstoneObj {
                id: 0,
                x: 34,
                y: 35,
                death_time: 493,
                decay_time: 599,
                creep_id: None,
                creep_name: None,
                creep_ticks_to_live: 0,
                creep_body: Vec::new(),
                creep_saying: None,
                owner: None,
                store: store(0),
                store_extra: Vec::new(),
            }),
        );
        insert(
            &mut world,
            "W1N1",
            RoomObject::ConstructionSite(ConstructionSiteObj {
                id: 0,
                x: 3,
                y: 1,
                structure_type: "extension".into(),
                progress: 100,
                progress_total: 300,
                owner: Some("p1".into()),
                name: None,
            }),
        );
        insert(
            &mut world,
            "E2S2",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 5000,
                off: false,
                name: "SpawnE".into(),
                owner: "p2".into(),
                store: store(300),
                spawning: None,
                notify_when_attacked: None,
            }),
        );
        insert(
            &mut world,
            "E2S2",
            RoomObject::ConstructionSite(ConstructionSiteObj {
                id: 0,
                x: 2,
                y: 2,
                structure_type: "road".into(),
                progress: 0,
                progress_total: 300,
                owner: Some("p1".into()),
                name: None,
            }),
        );
        insert(
            &mut world,
            "E2S2",
            RoomObject::Flag(FlagObj {
                id: 0,
                x: 1,
                y: 2,
                name: "far".into(),
                color: 3,
                secondary_color: 3,
                owner: Some("p1".into()),
            }),
        );
        // 扩展值只写入权威状态；投影经目录裁切。
        let w1_state = world.rooms.get_mut("W1N1").expect("房间存在");
        w1_state.extensions.insert(
            link,
            std::collections::BTreeMap::from([
                ("test:charge".to_string(), json!(55)),
                ("test:ownerNote".to_string(), json!("仅主人")),
                ("test:secret".to_string(), json!(true)),
                ("test:ratio".to_string(), json!(1.5)),
            ]),
        );
        world
    }

    fn number_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
        (a.as_i64(), a.as_u64(), a.as_f64()) == (b.as_i64(), b.as_u64(), b.as_f64())
    }

    fn values_equal(a: &Value, b: &Value, context: &str) {
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                assert!(number_equal(a, b), "{context}: 数值 {a} != {b}");
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len(), "{context}: 数组长度");
                for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                    values_equal(x, y, &format!("{context}[{i}]"));
                }
            }
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len(), "{context}: 键数");
                for (k, v) in a {
                    let other = b.get(k).unwrap_or_else(|| panic!("{context}: 缺键 {k}"));
                    values_equal(v, other, &format!("{context}.{k}"));
                }
            }
            _ => assert_eq!(a, b, "{context}"),
        }
    }

    fn compare_worlds(
        world: &World,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        viewer: &str,
    ) -> usize {
        let plan_hash = [7u8; 32];
        let audience = [9u8; 32];
        let reference = world.world_json_for_viewer_with_extensions(viewer, 1000, catalog, None);
        let blocks = world
            .kkpv_room_blocks_for_viewer(viewer, catalog, None, plan_hash, audience)
            .expect("块输入构造成功");
        let mut total_block_bytes = 0usize;
        let reference_rooms: Vec<&String> = reference["rooms"]
            .as_object()
            .expect("rooms 是对象")
            .keys()
            .collect();
        let block_rooms: Vec<&String> = blocks.iter().map(|b| &b.room_name).collect();
        assert_eq!(reference_rooms, block_rooms, "{viewer}: 每注册房间一块");
        let mut objects_by_room: std::collections::BTreeMap<&str, Vec<&Value>> =
            std::collections::BTreeMap::new();
        for object in reference["objects"].as_array().expect("objects 是数组") {
            objects_by_room
                .entry(object["room"].as_str().expect("room 键"))
                .or_default()
                .push(object);
        }
        for block in &blocks {
            let bytes = kk_schema::kkpv::encode(block, &kk_schema::kkpv::KkpvLimits::default())
                .expect("块编码成功");
            total_block_bytes += bytes.len();
            let view = KkpvView::decode(&bytes, &kk_schema::kkpv::KkpvLimits::default())
                .expect("块解码成功");
            assert!(view.is_private(), "{viewer}: 桥产出私有块");
            assert_eq!(view.tick(), world.time);
            assert_eq!(view.generation(), world.generation());
            assert_eq!(view.plan_hash(), plan_hash);
            assert_eq!(view.audience_hash(), audience);
            let room = view.room_name();
            let reference_objects = objects_by_room.remove(room).unwrap_or_default();
            assert_eq!(
                view.object_count(),
                reference_objects.len(),
                "{viewer}/{room}: 对象数"
            );
            for object in reference_objects {
                let id = object["id"].as_str().expect("id 键");
                let oid = ObjId::parse(id).expect("id 形态");
                let index = view
                    .object_index_by_id(oid.0)
                    .unwrap_or_else(|| panic!("{viewer}/{room}: 块缺对象 {id}"));
                let materialized = record_json_from_view(&view, index).expect("物化成功");
                values_equal(object, &materialized, &format!("{viewer}/{room}/{id}"));
                assert_eq!(
                    materialized["id"],
                    json!(id),
                    "{viewer}/{room}: id 由目录还原"
                );
                assert!(
                    materialized.get("test:secret").is_none(),
                    "{viewer}/{room}: ServerOnly 不进玩家数据"
                );
            }
            // 地形由公开束承载（见 public_terrain_blocks 对照测试），私有
            // 块不再携带地形段。
            assert!(view.terrain().is_none(), "{viewer}/{room}: 私有块不带地形");
        }
        assert!(
            objects_by_room.values().all(|v| v.is_empty()),
            "{viewer}: JSON 参考的对象房都有对应块"
        );
        total_block_bytes
    }

    /// 逐字段、缺失、null 与全部对象族的 KKPV/JSON 参考对照。
    #[test]
    fn kkpv_blocks_match_json_reference_per_field() {
        let world = rich_world();
        let plan = kk_mod_api::freeze(
            vec![kk_mod_api::builtin::link(), ext_contribution()],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("夹具计划可冻结");
        let catalog =
            kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录提取成功");
        let block_bytes_p1 = compare_worlds(&world, &catalog, "p1");
        let block_bytes_p2 = compare_worlds(&world, &catalog, "p2");
        // Owner 权限对两名查看者分别裁切：p1 见 ownerNote，p2 不见。
        let for_p1 = world.world_json_for_viewer_with_extensions("p1", 0, &catalog, None);
        let link_p1 = for_p1["objects"]
            .as_array()
            .expect("数组")
            .iter()
            .find(|o| o["structureType"] == json!("link"))
            .expect("link 存在");
        assert_eq!(link_p1["ext"]["test:ownerNote"], json!("仅主人"));
        assert_eq!(link_p1["ext"]["test:charge"], json!(55));
        assert_eq!(link_p1["ext"]["test:ratio"], json!(1.5));
        assert!(link_p1["ext"].get("test:secret").is_none());
        let for_p2 = world.world_json_for_viewer_with_extensions("p2", 0, &catalog, None);
        let link_p2 = for_p2["objects"]
            .as_array()
            .expect("数组")
            .iter()
            .find(|o| o["structureType"] == json!("link"))
            .expect("p2 视野内可见敌方 link");
        assert!(link_p2["ext"].get("test:ownerNote").is_none());
        assert_eq!(link_p2["ext"]["test:charge"], json!(55));
        assert_eq!(link_p2["ext"]["test:ratio"], json!(1.5));
        // 记录对照测试里 p2 路径同样校验了 ext 裁切后的逐字段一致。
        let json_bytes = serde_json::to_string(&for_p1).expect("序列化").len();
        eprintln!(
            "KKPV 对照尺寸：p1 块合计 {block_bytes_p1} 字节，p2 块合计 {block_bytes_p2} 字节，JSON 参考 {json_bytes} 字节"
        );
    }

    /// KKPV 信封与 JSON 参考的房间级内容同形：去掉 objects 与逐房
    /// terrain 后逐键相等（对象记录与地形分别由私有块和公开块承载），
    /// 两名查看者的可见房集合与逐房裁切一致（能量/事件只在可见房出现）。
    #[test]
    fn kkpv_envelope_matches_json_reference_minus_objects() {
        let world = rich_world();
        for viewer in ["p1", "p2"] {
            let mut reference = world.world_json_for_viewer(viewer, 1000);
            let reference_map = reference.as_object_mut().expect("参考是对象");
            reference_map.remove("objects");
            if let Some(rooms) = reference_map
                .get_mut("rooms")
                .and_then(Value::as_object_mut)
            {
                for entry in rooms.values_mut() {
                    if let Some(room) = entry.as_object_mut() {
                        room.remove("terrain");
                    }
                }
            }
            let envelope = world.kkpv_envelope_for_viewer(viewer, 1000);
            assert_eq!(
                reference, envelope,
                "{viewer}: 信封与参考（去 objects 与地形）同形"
            );
        }
    }

    /// 公开地形块覆盖全部注册房间（含无对象房间），字节与 JSON 参考的
    /// base64 地形载荷同源，且不随查看者或受众参数中查看者相关的部分
    /// 变化——公开通道对全部玩家是同一份字节。
    #[test]
    fn kkpv_public_terrain_blocks_match_json_reference() {
        let world = rich_world();
        let reference = world.world_json_for_viewer("p1", 1000);
        let blocks = world.kkpv_public_terrain_blocks([1; 32], [2; 32]);
        assert_eq!(
            blocks.len(),
            world.rooms.len(),
            "公开块数 = 全部注册房间数（不按查看者裁切）"
        );
        for block in &blocks {
            assert!(!block.private, "公开块 flags 无私有位");
            assert!(block.objects.is_empty(), "公开地形块不带对象");
            assert_eq!(block.tick, world.time);
            assert_eq!(block.generation, world.generation());
            let payload = reference["rooms"][block.room_name.as_str()]["terrain"]["payload"]
                .as_str()
                .expect("JSON 参考的每注册房间都有地形载荷");
            let encoded = crate::world_json::base64_encode(&block.terrain.clone().expect("地形"));
            assert_eq!(
                encoded,
                payload,
                "{room}: 公开块地形与 JSON 参考同源",
                room = block.room_name
            );
        }
        // 两次调用产出逐字节相同的公开束（确定编码，可跨查看者共享）。
        let terrain_of = |blocks: &[KkpvBlockInput]| -> Vec<(String, Vec<u8>)> {
            blocks
                .iter()
                .map(|b| {
                    (
                        b.room_name.clone(),
                        b.terrain.clone().expect("公开块带地形"),
                    )
                })
                .collect()
        };
        assert_eq!(
            terrain_of(&blocks),
            terrain_of(&world.kkpv_public_terrain_blocks([1; 32], [2; 32]))
        );
    }

    /// Generic 原生数据自带 ext 键时，目录扩展按 JSON 参考的覆盖语义
    /// 合并，不因键冲突失败。
    #[test]
    fn generic_native_ext_key_is_overwritten_by_catalog() {
        let mut world = World::new(3);
        let mut terrain = Terrain::plains();
        terrain.normalize();
        world.register_room("W1N1", terrain);
        world
            .insert(
                "W1N1",
                RoomObject::Spawn(SpawnObj {
                    id: 0,
                    x: 10,
                    y: 10,
                    hits: 5000,
                    off: false,
                    name: "S".into(),
                    owner: "p1".into(),
                    store: Store {
                        energy: 0,
                        ghodium: 0,
                    },
                    spawning: None,
                    notify_when_attacked: None,
                }),
            )
            .expect("插入");
        world
            .insert(
                "W1N1",
                RoomObject::Generic(GenericObj {
                    id: 0,
                    x: 1,
                    y: 1,
                    data: r#"{"type":"structure","structureType":"link","ext":{"原生":1}}"#.into(),
                }),
            )
            .expect("插入");
        let link_id = world
            .rooms
            .get_mut("W1N1")
            .expect("房间")
            .objects
            .values()
            .find_map(|o| match o {
                RoomObject::Generic(g) => Some(g.id),
                _ => None,
            })
            .expect("link id");
        world
            .rooms
            .get_mut("W1N1")
            .expect("房间")
            .extensions
            .insert(
                crate::world::ObjId(link_id),
                std::collections::BTreeMap::from([
                    ("test:charge".to_string(), json!(55)),
                    ("test:ratio".to_string(), json!(1.5)),
                ]),
            );
        let _ = link_id;
        let reference =
            world.world_json_for_viewer_with_extensions("p1", 0, &catalog_fixture(), None);
        let link_json = reference["objects"]
            .as_array()
            .expect("数组")
            .iter()
            .find(|o| o["structureType"] == json!("link"))
            .expect("link");
        assert_eq!(
            link_json["ext"],
            json!({"test:charge": 55, "test:ratio": 1.5}),
            "JSON 参考的原生 ext 被目录 ext 覆盖"
        );
        let blocks = world
            .kkpv_room_blocks_for_viewer("p1", &catalog_fixture(), None, [0; 32], [0; 32])
            .expect("ext 键冲突按覆盖语义合并，不是错误");
        let bytes = kk_schema::kkpv::encode(&blocks[0], &kk_schema::kkpv::KkpvLimits::default())
            .expect("编码");
        let view = KkpvView::decode(&bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("解码");
        let index = view
            .object_index_by_id(2)
            .expect("link 在块内（第二个插入对象 id=2）");
        assert_eq!(
            view.field(index, "ext"),
            Ok(Some(KkpvValue::Object(vec![
                ("test:charge".to_string(), KkpvValue::Int(55)),
                ("test:ratio".to_string(), KkpvValue::Float(1.5)),
            ])))
        );
    }

    /// 真实形状的私有块可与同版本公开块合并，overlay 对象可区分来源。
    #[test]
    fn world_block_merges_with_public_overlay() {
        let world = rich_world();
        let plan = kk_mod_api::freeze(
            vec![kk_mod_api::builtin::link(), ext_contribution()],
            kk_mod_api::FreezeOptions::default(),
        )
        .expect("冻结");
        let catalog = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan).expect("目录");
        let mut blocks = world
            .kkpv_room_blocks_for_viewer("p1", &catalog, None, [3u8; 32], [4u8; 32])
            .expect("块");
        let private_input = blocks.remove(0);
        let private_bytes =
            kk_schema::kkpv::encode(&private_input, &kk_schema::kkpv::KkpvLimits::default())
                .expect("编码");
        let public_input = world
            .kkpv_public_terrain_blocks([3u8; 32], [4u8; 32])
            .into_iter()
            .find(|b| b.room_name == private_input.room_name)
            .expect("公开地形块覆盖同一房间");
        let public_bytes =
            kk_schema::kkpv::encode(&public_input, &kk_schema::kkpv::KkpvLimits::default())
                .expect("编码");
        let private_view =
            KkpvView::decode(&private_bytes, &kk_schema::kkpv::KkpvLimits::default())
                .expect("解码");
        let public_view =
            KkpvView::decode(&public_bytes, &kk_schema::kkpv::KkpvLimits::default()).expect("解码");
        let merged = kk_schema::kkpv::merge(&public_view, &private_view).expect("合并");
        assert_eq!(merged.object_count(), private_view.object_count());
        assert!(merged.terrain().is_some(), "地形取公开块");
        for i in 0..merged.object_count() {
            assert!(merged.from_overlay(i), "对象全部来自 overlay");
        }
    }
}
