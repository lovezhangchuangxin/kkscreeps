//! 玩家视野、对象与字段可见性裁切。
use super::*;

/// 房间对查看者可见的条件：房内存在己方非旗/非工地对象。
/// 归属只用于判断房间是否可见；旗和工地虽有主人，但不授予视野。
/// 上游依据：driver/lib/runtime/data.js 的 userObjects 房间计算。
pub(crate) fn room_visible_to(state: &crate::world::RoomState, user: &str) -> bool {
    state.objects.values().any(|o| {
        !matches!(o, RoomObject::ConstructionSite(_) | RoomObject::Flag(_))
            && crate::domain::object_owner(o).as_deref() == Some(user)
    })
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
