//! 同房 Mod 候选对象读写。调用方负责能力寿命与绑定身份；写面仅接受自有类型。
use super::*;
use kk_mod_api::objects::{ObjectPage, ObjectView};

pub fn get(w: &Working, id: ObjId) -> Result<Option<ObjectView>, String> {
    let Some(object) = w.room.objects.get(&id) else {
        return Ok(None);
    };
    let RoomObject::Mod(object) = object else {
        return Err("对象读取能力当前只支持 Mod 对象".into());
    };
    Ok(Some(ObjectView {
        fields: w.mod_fields(id).ok_or("Mod 对象 payload 非法")?,
        id: id.as_str(),
        room: w.room.name.clone(),
        type_key: object.type_key.clone(),
    }))
}

/// 每页最多 256 项，游标按 ID 排序；列表和字段都是当次候选的独立快照。
pub fn list(w: &Working, type_key: &str, after: &str, limit: u32) -> Result<ObjectPage, String> {
    if !(1..=256).contains(&limit) {
        return Err("对象列表 limit 须为 1..256".into());
    }
    if !type_key.is_empty() {
        let key = kk_mod_api::Key::new(type_key)?;
        if w.plane.plan.projection().resolve(&key).is_none() {
            return Err(format!("类型 {key} 不在计划中"));
        }
    }
    let after = if after.is_empty() {
        None
    } else {
        Some(ObjId::parse(after).ok_or("对象游标 ID 非法")?)
    };
    let mut matching = w.room.objects.iter().filter_map(|(id, object)| {
        if after.is_some_and(|after| *id <= after) {
            return None;
        }
        match object {
            RoomObject::Mod(object) if type_key.is_empty() || object.type_key == type_key => {
                Some(*id)
            }
            _ => None,
        }
    });
    let mut objects = Vec::new();
    for id in matching.by_ref().take(limit as usize) {
        objects.push(get(w, id)?.expect("遍历中已确认 Mod 对象"));
    }
    let next = matching
        .next()
        .and_then(|_| objects.last().map(|object| object.id.clone()));
    Ok(ObjectPage { objects, next })
}

fn own_type<'a>(
    w: &'a Working,
    namespace: &str,
    key: &str,
) -> Result<&'a kk_mod_api::ObjectTypeDescriptor, String> {
    let key = kk_mod_api::Key::new(key)?;
    if key.namespace() != namespace || namespace == "builtin" {
        return Err("对象写能力只允许当前绑定命名空间的自有类型".into());
    }
    w.plane
        .plan
        .projection()
        .resolve(&key)
        .and_then(|h| w.plane.plan.projection().get(h))
        .map(|ty| &ty.descriptor)
        .ok_or_else(|| format!("类型 {key} 不在计划中"))
}

pub fn create(
    w: &mut Working,
    namespace: &str,
    type_key: &str,
    fields: &Value,
) -> Result<ObjectView, String> {
    let ty = own_type(w, namespace, type_key)?;
    let fields = kk_mod_api::objects::prepare_fields(fields, ty)?;
    let (x, y) = kk_mod_api::objects::position(&fields, ty)?;
    w.check_mod_mutation_budget()?;
    let id = w
        .insert_new(RoomObject::Mod(kk_schema::room::ModObj {
            id: 0,
            x,
            y,
            type_key: type_key.into(),
            data: fields.to_string(),
        }))
        .map_err(|error| error.to_string())?;
    Ok(get(w, id)?.expect("刚创建的 Mod 对象"))
}

pub fn update(
    w: &mut Working,
    namespace: &str,
    id: ObjId,
    patch: &Value,
) -> Result<ObjectView, String> {
    let current = get(w, id)?.ok_or("更新目标不在本次房间候选中")?;
    own_type(w, namespace, &current.type_key)?;
    let patch = patch.as_object().ok_or("对象更新须为字段对象")?;
    let mut fields = current.fields;
    fields
        .as_object_mut()
        .ok_or("Mod payload 非法")?
        .extend(patch.clone());
    w.update_mod_fields(id, &fields)?;
    Ok(get(w, id)?.expect("更新后仍为 Mod 对象"))
}

pub fn remove(w: &mut Working, namespace: &str, id: ObjId) -> Result<(), String> {
    let current = get(w, id)?.ok_or("删除目标不在本次房间候选中")?;
    own_type(w, namespace, &current.type_key)?;
    w.check_mod_mutation_budget()?;
    w.remove_obj(id).expect("已确认同房存活对象");
    Ok(())
}
