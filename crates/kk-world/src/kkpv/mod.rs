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

use kk_schema::kkpv::{
    KkpvBlockInput, KkpvBlockRef, KkpvLimits, KkpvObject, KkpvRow, KkpvVal, KkpvValue, KkpvView,
    encode_bundle, encode_ref, patch_block_audience,
};
use kk_schema::room::RoomObject;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::domain::room::room_energy;
use crate::projection::{ext_entries_json, object_exported_for, object_json};
use crate::world::{ObjId, World};

mod generic_scan;
mod objects;
use objects::{RoomBlockMeta, encode_room_block, type_key_cow};

/// 同代际同计划下可跨查看者共享的房间私有块字节。块头 audience 置零，
/// 装配时按查看者改写（[`kk_schema::kkpv::patch_block_audience`]），
/// 产物与按查看者直接编码逐字节一致。
///
/// 共享条件（构建时判定）：房内无归属旗（他人旗逐查看者裁切）、无
/// Mod 对象（对象/字段级权限）、无扩展字段（Owner 字段裁切）。归属
/// 工地不阻碍 full 块共享（可见房工地对全员导出），但不可见查看者的
/// 空块含归属者自己的工地，故 `empty` 仅在无归属工地时共享。
#[derive(Debug)]
pub(crate) struct SharedRoomBlock {
    pub(crate) full: Arc<Vec<u8>>,
    pub(crate) empty: Option<Arc<Vec<u8>>>,
}

impl World {
    /// 取当前 (generation, plan_hash) 的可共享房间块缓存；过期则单次
    /// 重建。只含可共享房，缺失即回退逐查看者编码。
    fn shared_room_blocks(&self, plan_hash: [u8; 32]) -> Arc<BTreeMap<String, SharedRoomBlock>> {
        {
            let cache = self.room_block_cache.borrow();
            if let Some((generation, cached_plan, blocks)) = cache.as_ref()
                && *generation == self.generation()
                && *cached_plan == plan_hash
            {
                return blocks.clone();
            }
        }
        let blocks = Arc::new(self.build_shared_room_blocks(plan_hash));
        *self.room_block_cache.borrow_mut() = Some((self.generation(), plan_hash, blocks.clone()));
        blocks
    }

    /// 扫描全服构建可共享块。共享条件见 [`SharedRoomBlock`]；构建用
    /// 空 user 与空目录——可共享房内 user 只作用于旗/Mod/ext 裁切，
    /// 这些对象类型已被共享条件排除，输出与任意真实查看者一致。
    fn build_shared_room_blocks(&self, plan_hash: [u8; 32]) -> BTreeMap<String, SharedRoomBlock> {
        let empty_catalog = kk_mod_api::extension::ExtensionCatalog::default();
        let meta = RoomBlockMeta {
            tick: self.time,
            generation: self.generation(),
            plan_hash,
            audience_hash: [0u8; 32],
        };
        let mut map = BTreeMap::new();
        for (name, state) in &self.rooms {
            let (mut has_owned_flag, mut has_mod, mut has_owned_site) = (false, false, false);
            for object in state.objects.values() {
                match object {
                    RoomObject::Flag(f) if f.owner.is_some() => has_owned_flag = true,
                    RoomObject::ConstructionSite(cs) if cs.owner.is_some() => has_owned_site = true,
                    RoomObject::Mod(_) => has_mod = true,
                    _ => {}
                }
            }
            if has_owned_flag || has_mod || !state.extensions.is_empty() {
                continue;
            }
            let controller = state.controller();
            // 共享构建失败不缓存半成品：该房回退逐查看者路径，由真实
            // 调用方报出原始错误。
            let Ok(full) = encode_room_block(
                name,
                state,
                controller,
                self.time,
                meta,
                "",
                true,
                None,
                &empty_catalog,
                None,
            ) else {
                continue;
            };
            // 不可见查看者的空块：无归属工地即可共享（空 user 下导出门
            // 排除全部对象，产物是零行块）；失败同样回退逐查看者路径。
            let empty = if has_owned_site {
                None
            } else {
                encode_room_block(
                    name,
                    state,
                    controller,
                    self.time,
                    meta,
                    "",
                    false,
                    None,
                    &empty_catalog,
                    None,
                )
                .ok()
                .map(Arc::new)
            };
            map.insert(
                name.clone(),
                SharedRoomBlock {
                    full: Arc::new(full),
                    empty,
                },
            );
        }
        map
    }

    /// KKPV 通道的完整私有束装配：信封加逐房私有块字节，束框架与
    /// [`encode_bundle`] 一致（u32 块数 + 逐块 u32 长度 + 顺序拼接）。
    /// 可共享房直接复用共享字节并改写 audience；不可共享房逐查看者
    /// 编码，行为与 [`World::kkpv_room_blocks_for_viewer`] +
    /// `encode_bundle` 完全同源。
    ///
    /// # Errors
    /// 逐查看者回退房的对象记录无法映射为 KKPV 值形态时返回错误。
    pub fn kkpv_bundle_for_viewer(
        &self,
        user: &str,
        gcl_points: i64,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Result<(Value, Vec<u8>), String> {
        let envelope = self.kkpv_envelope_for_viewer(user, gcl_points);
        let index = self.visibility_index();
        let observed = index.observed_rooms_of(user);
        let shared = self.shared_room_blocks(plan_hash);
        let limits = KkpvLimits::default();
        // 先逐房定字节来源与长度，再一次性装配：共享块拷贝后原位改写
        // audience，回退块以真实查看者编码（audience 已正确）。
        enum Source {
            Shared { bytes: Arc<Vec<u8>> },
            Fresh(Vec<u8>),
        }
        let mut sources = Vec::with_capacity(self.rooms.len());
        for (name, state) in &self.rooms {
            let room_visible = index.room_visible_to(name, user)
                || observed.is_some_and(|observed| observed.contains(name));
            let use_shared = match shared.get(name) {
                Some(entry) => {
                    if room_visible {
                        Some(Source::Shared {
                            bytes: entry.full.clone(),
                        })
                    } else {
                        entry.empty.as_ref().map(|bytes| Source::Shared {
                            bytes: bytes.clone(),
                        })
                    }
                }
                None => None,
            };
            let source = match use_shared {
                Some(source) => source,
                None => {
                    // 不可共享（或空块不可共享）：逐查看者直写编码，输出与
                    // 逐查看者全量构造同源。
                    let candidates = if room_visible {
                        None
                    } else {
                        index.invisible_room_candidates(name, user)
                    };
                    let candidates = candidates.as_deref();
                    Source::Fresh(encode_room_block(
                        name,
                        state,
                        state.controller(),
                        self.time,
                        RoomBlockMeta {
                            tick: self.time,
                            generation: self.generation(),
                            plan_hash,
                            audience_hash,
                        },
                        user,
                        room_visible,
                        candidates,
                        catalog,
                        surface,
                    )?)
                }
            };
            sources.push(source);
        }
        let framing = 4 + sources.len() * 4;
        let total = framing
            + sources
                .iter()
                .map(|source| match source {
                    Source::Shared { bytes } => bytes.len(),
                    Source::Fresh(bytes) => bytes.len(),
                })
                .sum::<usize>();
        // 与 encode_bundle 同口径的束总量兜底，见其注释。
        if total > limits.max_bytes.saturating_mul(16) {
            return Err("束总量超出 16×单块限额".to_string());
        }
        let fits = |v: usize| -> Result<u32, String> {
            u32::try_from(v).map_err(|_| "束长度超出 u32 框架".to_string())
        };
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&fits(sources.len())?.to_le_bytes());
        for source in &sources {
            let len = match source {
                Source::Shared { bytes } => bytes.len(),
                Source::Fresh(bytes) => bytes.len(),
            };
            out.extend_from_slice(&fits(len)?.to_le_bytes());
        }
        for source in sources {
            match source {
                Source::Shared { bytes } => {
                    let start = out.len();
                    out.extend_from_slice(&bytes);
                    patch_block_audience(&mut out[start..], &audience_hash)
                        .map_err(|e| e.to_string())?;
                }
                Source::Fresh(bytes) => out.extend_from_slice(&bytes),
            }
        }
        Ok((envelope, out))
    }

    /// 测试参考：不经共享块缓存，逐房以真实查看者直写编码后按束框架
    /// 拼接（与 [`encode_bundle`] 分帧一致）。生产装配走
    /// [`World::kkpv_bundle_for_viewer`]，共享房复用缓存字节——两者
    /// 的逐字节对照由 `shared_bundle_assembly_matches_per_viewer_encoding`
    /// 锁定。
    ///
    /// # Errors
    /// 对象记录无法映射为 KKPV 值形态或编码超限时返回错误。
    #[cfg(test)]
    pub(crate) fn kkpv_fresh_bundle_bytes_for_viewer(
        &self,
        user: &str,
        catalog: &kk_mod_api::extension::ExtensionCatalog,
        surface: Option<&kk_mod_api::player_surface::PlayerSurface>,
        plan_hash: [u8; 32],
        audience_hash: [u8; 32],
    ) -> Result<Vec<u8>, String> {
        let index = self.visibility_index();
        let observed = index.observed_rooms_of(user);
        let meta = RoomBlockMeta {
            tick: self.time,
            generation: self.generation(),
            plan_hash,
            audience_hash,
        };
        let mut blocks = Vec::with_capacity(self.rooms.len());
        for (name, state) in &self.rooms {
            let room_visible = index.room_visible_to(name, user)
                || observed.is_some_and(|observed| observed.contains(name));
            let candidates = if room_visible {
                None
            } else {
                index.invisible_room_candidates(name, user)
            };
            let candidates = candidates.as_deref();
            blocks.push(encode_room_block(
                name,
                state,
                state.controller(),
                self.time,
                meta,
                user,
                room_visible,
                candidates,
                catalog,
                surface,
            )?);
        }
        let total = 4 + blocks.len() * 4 + blocks.iter().map(Vec::len).sum::<usize>();
        let fits = |v: usize| u32::try_from(v).map_err(|_| "束长度超出 u32 框架".to_string());
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&fits(blocks.len())?.to_le_bytes());
        for block in &blocks {
            out.extend_from_slice(&fits(block.len())?.to_le_bytes());
        }
        for block in blocks {
            out.extend_from_slice(&block);
        }
        Ok(out)
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
        // 代际共享索引：视野与观察限时视野一次扫描派生，逐玩家查表。
        let index = self.visibility_index();
        let mut visible: std::collections::BTreeSet<&String> = self
            .rooms
            .iter()
            .filter(|(name, _)| index.room_visible_to(name, user))
            .map(|(name, _)| name)
            .collect();
        if let Some(observed) = index.observed_rooms_of(user) {
            visible.extend(observed.iter());
        }
        let mut rooms = serde_json::Map::new();
        for (name, state) in &self.rooms {
            // 全部注册房保持条目：玩家面把缺失条目当作界外房（closed），
            // 注册 normal 房与界外房必须可区分——公共地图数据不按视野
            // 裁切（getRoomStatus/isRoomAvailable 消费全房目）。
            let room_visible = visible.contains(name);
            let mut entry = serde_json::Map::new();
            entry.insert("name".into(), Value::String(name.clone()));
            // 与 JSON 通道 world_json_impl 同源：房间公共状态种子随房导出
            //（公共地图数据不按视野裁切），normal 缺省由玩家面兜底。
            if let Some(status) = &state.room_status {
                entry.insert(
                    "status".into(),
                    Value::String(status.status.as_str().into()),
                );
                entry.insert("statusTimestamp".into(), Value::from(status.timestamp));
            }
            if room_visible {
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
    type_key_cow(record).into_owned()
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
