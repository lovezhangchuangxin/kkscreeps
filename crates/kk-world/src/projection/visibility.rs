//! 玩家视野、对象与字段可见性裁切。
use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// 同一代际的全服可见性索引：一次对象扫描的派生物，供逐玩家投影共用，
/// 取代"每玩家每房间重复全服扫描"。语义与逐玩家扫描版完全一致：
/// `room_visible_to` 等价于 room_owners 查表；`observed_rooms_of` 等价于
/// observed 查表（observer 的归属读取与目标判定在构建时共用同一次
/// Generic 身份缓存）。绑定世界 generation 失效——所有内容变更路径
/// 都推进 generation（含 observer 标记清零所在的预意图相）。
#[derive(Debug)]
pub(crate) struct VisibilityIndex {
    /// 房间 → 授予视野的玩家集合（房内存在该玩家的非旗/非工地对象）。
    room_owners: BTreeMap<String, BTreeSet<String>>,
    /// observer 主人 → 观察目标房集合（目标未注册的标记不进集合，
    /// 与 observed_rooms_of 的 contains_key 过滤一致）。
    observed: BTreeMap<String, BTreeSet<String>>,
    /// 玩家 → 房间 → 己方旗/工地对象 ID（升序）。不可见房的私有块
    /// 只需枚举这些对象：object_exported_for 在不可见房只放行己方旗与工地。
    collections: BTreeMap<String, BTreeMap<String, Vec<crate::world::ObjId>>>,
    /// 房间 → Mod 对象 ID（升序）。OwnerCollection 语义不受房间视野门，
    /// 不可见房仍须逐对象走 mod_record_json 裁切；可见房在常规路径
    /// 顺带处理，不查本表。
    mod_objects: BTreeMap<String, Vec<crate::world::ObjId>>,
    /// 玩家 → 授予视野的自有房集合（room_owners 的反转，房名升序）。
    /// 与 observed_rooms_of 合并即玩家完整可见集；可见集相同的查看者
    /// 私有束字节一致（块内容与查看者无关，仅差无人消费的 audience
    /// 改写位），宿主按可见集分组共享装配。集合必须与逐房
    /// room_visible_to/observed 判定同源，否则共享束会把不可见房内容
    /// 泄露给玩家。
    owned_rooms: BTreeMap<String, BTreeSet<String>>,
}

impl VisibilityIndex {
    /// 单次扫描构建。Generic 的 owner 与 observer 判定共用载荷身份缓存，
    /// 未修改载荷跨代际不再解析；替换载荷后新缓存自然失效。
    pub(crate) fn build(world: &World) -> Self {
        let mut index = Self {
            room_owners: BTreeMap::new(),
            observed: BTreeMap::new(),
            collections: BTreeMap::new(),
            mod_objects: BTreeMap::new(),
            owned_rooms: BTreeMap::new(),
        };
        for (name, state) in &world.rooms {
            for (id, object) in &state.objects {
                match object {
                    // 旗与工地有归属但不授予视野；只作为归属者集合候选。
                    RoomObject::ConstructionSite(_) | RoomObject::Flag(_) => {
                        if let Some(owner) = crate::domain::object_owner(object) {
                            index
                                .collections
                                .entry(owner.into_owned())
                                .or_default()
                                .entry(name.clone())
                                .or_default()
                                .push(*id);
                        }
                    }
                    RoomObject::Mod(_) => {
                        index.mod_objects.entry(name.clone()).or_default().push(*id);
                    }
                    RoomObject::Generic(g) => {
                        let Some(data) = g.data.identity() else {
                            continue;
                        };
                        let owner = data.owner.as_deref();
                        if let Some(owner) = owner {
                            index
                                .room_owners
                                .entry(name.clone())
                                .or_default()
                                .insert(owner.to_string());
                        }
                        if data.structure_type.as_deref() == Some("observer")
                            && let Some(target) = data.observe_room.as_deref()
                            && let Some(owner) = owner
                            && world.rooms.contains_key(target)
                        {
                            index
                                .observed
                                .entry(owner.to_string())
                                .or_default()
                                .insert(target.to_string());
                        }
                    }
                    _ => {
                        if let Some(owner) = crate::domain::object_owner(object) {
                            index
                                .room_owners
                                .entry(name.clone())
                                .or_default()
                                .insert(owner.into_owned());
                        }
                    }
                }
            }
        }
        // room_owners 反转出玩家→自有房集合：与 observed 合并即完整可见
        // 集（两者都只含已注册房，与逐房判定同源）。
        for (room, owners) in &index.room_owners {
            for user in owners {
                index
                    .owned_rooms
                    .entry(user.clone())
                    .or_default()
                    .insert(room.clone());
            }
        }
        index
    }

    /// 房间对查看者可见的条件：房内存在己方非旗/非工地对象。
    /// 归属只用于判断房间是否可见；旗和工地虽有主人，但不授予视野。
    /// 上游依据：driver/lib/runtime/data.js 的 userObjects 房间计算。
    /// （原逐房扫描版语义，构建时物化为 room_owners 表。）
    pub(crate) fn room_visible_to(&self, room: &str, user: &str) -> bool {
        self.room_owners
            .get(room)
            .is_some_and(|owners| owners.contains(user))
    }

    /// 观察限时视野：查看者名下 observer 的 observeRoom 标记（结算相
    /// 写入、预意图相清零）指向的目标房集合；目标未注册时跳过（官方
    /// data.js 的房间查询自然落空）。来源：driver/lib/runtime/data.js
    /// 的 userObjects 房间计算。（原世界级扫描版语义，构建时物化。）
    pub(crate) fn observed_rooms_of(&self, user: &str) -> Option<&BTreeSet<String>> {
        self.observed.get(user)
    }

    /// 查看者的完整可见房集合（自有 ∪ 观察，房名升序新集合）。
    /// 与逐房"归属 ∨ 观察"判定同源；空集表示无任何可见来源。
    pub(crate) fn visible_rooms_of(&self, user: &str) -> BTreeSet<String> {
        let mut rooms = self.owned_rooms.get(user).cloned().unwrap_or_default();
        if let Some(observed) = self.observed.get(user) {
            rooms.extend(observed.iter().cloned());
        }
        rooms
    }

    /// 不可见房的私有块候选对象 ID（升序）：己方旗/工地与全部 Mod 对象
    /// 归并。两者类型互斥故无重复；None 表示该房无可导出对象，调用方
    /// 可整房跳过对象枚举。
    pub(crate) fn invisible_room_candidates(
        &self,
        room: &str,
        user: &str,
    ) -> Option<Vec<crate::world::ObjId>> {
        let own = self.collections.get(user).and_then(|rooms| rooms.get(room));
        let mods = self.mod_objects.get(room);
        if own.is_none() && mods.is_none() {
            return None;
        }
        let mut ids: Vec<crate::world::ObjId> = own.map(|ids| ids.to_vec()).unwrap_or_default();
        if let Some(mods) = mods {
            ids.extend(mods.iter().copied());
        }
        ids.sort_unstable();
        Some(ids)
    }
}

/// 对象进入玩家数据的条件：无查看者时全部导出；他人旗排除在先；
/// 不可见房只放行己方旗与工地——它们可出现在全局集合，即使所在
/// 房间不可见，且不使该房间进入 Game.rooms。
/// 参考：driver/lib/runtime/data.js 的 userObjects、flags 查询。
pub(crate) fn object_exported_for(
    user: Option<&str>,
    room_visible: bool,
    object: &RoomObject,
) -> bool {
    let Some(viewer) = user else {
        return true;
    };
    // 他人的旗不进入玩家数据；无主旗保留给可见房间的兼容性场景。
    if let RoomObject::Flag(f) = object
        && f.owner.as_deref().is_some_and(|o| o != viewer)
    {
        return false;
    }
    if !room_visible {
        return matches!(
            object,
            RoomObject::ConstructionSite(_) | RoomObject::Flag(_)
        ) && crate::domain::object_owner(object).as_deref() == Some(viewer);
    }
    true
}

/// 把 Mod 对象裁切为玩家面记录：对象级可见性（OwnerCollection 仅归属
/// 者可见；RoomVisible 走调用方的房间视野门）与字段级可见性（Public /
/// Owner=查看者是归属者 / ServerOnly 不出）都在此定论，玩家 getter 只
/// 读当轮记录。信封键（type/modType/id/room/x/y/pos）最后写入——
/// 声明位置字段同名时被等值覆写（写入口径保证同步）。类型不在目录、
/// 归属不可解析或 payload 损坏（发布前校验兜底的防御路径）返回 None。
pub(crate) fn mod_record_json(
    room: &str,
    id: crate::world::ObjId,
    o: &kk_schema::room::ModObj,
    surface: &kk_mod_api::player_surface::PlayerSurface,
    viewer: Option<&str>,
    room_visible: bool,
) -> Option<Value> {
    let ty = surface
        .types
        .iter()
        .find(|ty| ty.key.as_str() == o.type_key)?;
    // payload 损坏（写入口校验兜底的防御路径）对两种对象可见性都
    // 整体不导出——骨架记录会让玩家读到"存在但无字段"的假对象。
    let payload: serde_json::Map<String, Value> = serde_json::from_str(&o.data).ok()?;
    let owner = ty
        .owner_field
        .as_ref()
        .and_then(|name| payload.get(name))
        .and_then(Value::as_str);
    match ty.visibility {
        // 房间可见即导出（视野门与内建对象同口径——Mod 对象自身不授予
        // 房间视野）。
        kk_mod_api::ObjectVisibility::RoomVisible => {
            if viewer.is_some() && !room_visible {
                return None;
            }
        }
        // 归属者集合语义（与己方旗同形）：归属者可见，不受房间视野门；
        // 他人不可见。
        kk_mod_api::ObjectVisibility::OwnerCollection => {
            let viewer = viewer?;
            if owner != Some(viewer) {
                return None;
            }
        }
        // 目录只收 RoomVisible/OwnerCollection（ServerOnly 不进玩家面，
        // Conditional 在导出时拒绝）；此处为穷尽性防御。
        _ => return None,
    }
    let mut record = serde_json::Map::new();
    for field in &ty.fields {
        let visible = match field.visibility {
            kk_mod_api::Visibility::Public => true,
            kk_mod_api::Visibility::Owner => viewer.is_some_and(|v| Some(v) == owner),
            _ => false,
        };
        if visible && let Some(value) = payload.get(&field.name) {
            record.insert(field.name.clone(), value.clone());
        }
    }
    // 库存信封键（G09.a）：store 声明类型的 payload 由引擎维护库存；
    // 投影直供两键（与结构 store 同公开口径），玩家面与 transfer/
    // withdraw 预检的容量门同源读取。未声明类型无信封，记录不含两键。
    for key in ["store", "storeCapacity"] {
        if let Some(value) = payload.get(key) {
            record.insert(key.into(), value.clone());
        }
    }
    // 耐久信封键（G09.b）：hits 声明类型同口径直供（与结构 hits 同
    // 公开面），玩家面 attack 预检与结算伤害读同一信封。未声明类型
    // 无信封，记录不含两键。
    for key in kk_mod_api::objects::HITS_ENVELOPE_KEYS {
        if let Some(value) = payload.get(key) {
            record.insert(key.into(), value.clone());
        }
    }
    record.insert("type".into(), json!("mod"));
    record.insert("modType".into(), json!(o.type_key));
    record.insert("id".into(), json!(id.as_str()));
    record.insert("room".into(), json!(room));
    record.insert("x".into(), json!(o.x));
    record.insert("y".into(), json!(o.y));
    record.insert(
        "pos".into(),
        json!({ "x": o.x, "y": o.y, "roomName": room }),
    );
    Some(Value::Object(record))
}

/// 按目录权限把对象的扩展字段裁切为 `ext` 键内容；无可见字段时返回
/// None。写入口径已保证键形合法；此处对坏键防御跳过。
pub(crate) fn ext_entries_json(
    catalog: &kk_mod_api::extension::ExtensionCatalog,
    type_key: &kk_mod_api::Key,
    extensions: &std::collections::BTreeMap<
        crate::world::ObjId,
        std::collections::BTreeMap<String, Value>,
    >,
    oid: crate::world::ObjId,
    owner: Option<&str>,
    viewer: &str,
) -> Option<serde_json::Map<String, Value>> {
    let values = extensions.get(&oid)?;
    let mut ext = serde_json::Map::new();
    for (field_key, value) in values {
        let Some(field_key) = kk_mod_api::Key::new(field_key.clone()).ok() else {
            continue;
        };
        let Some(rule) = catalog.field(type_key, &field_key) else {
            continue;
        };
        let visible = match rule.visibility {
            kk_mod_api::Visibility::Public => true,
            kk_mod_api::Visibility::Owner => owner == Some(viewer),
            _ => false,
        };
        if visible {
            ext.insert(field_key.to_string(), value.clone());
        }
    }
    (!ext.is_empty()).then_some(ext)
}
