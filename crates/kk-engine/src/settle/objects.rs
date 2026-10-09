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
    let mut fields = kk_mod_api::objects::prepare_fields(fields, ty)?;
    let (x, y) = kk_mod_api::objects::position(&fields, ty)?;
    // store 声明类型创建即携带库存信封（G09.a）：容量为冻结声明值，
    // 库存只能经资源动作变动。
    if let Some(capacity) = ty.store_capacity {
        kk_mod_api::objects::inject_store_envelope(&mut fields, capacity);
    }
    // hits 声明类型创建即满血（G09.b），耐久只能经伤害动作变动。
    if let Some(hits_max) = ty.hits_max {
        kk_mod_api::objects::inject_hits_envelope(&mut fields, hits_max);
    }
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
    if patch.keys().any(|key| {
        kk_mod_api::objects::STORE_ENVELOPE_KEYS.contains(&key.as_str())
            || kk_mod_api::objects::HITS_ENVELOPE_KEYS.contains(&key.as_str())
    }) {
        return Err("库存/耐久信封键分别由资源/伤害动作维护，不能经字段更新写入".into());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小 store 声明类型（无业务字段），驱动 update 的信封键拒绝。
    fn store_plane() -> Plane {
        let contribution = kk_mod_api::Contribution {
            source: kk_mod_api::Source {
                namespace: "upmod".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [9; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: std::collections::BTreeSet::from([kk_mod_api::Key::new(
                "upmod:box.field.label",
            )
            .unwrap()]),
            field_mounts: Vec::new(),
            object_types: vec![kk_mod_api::ObjectTypeDescriptor {
                key: kk_mod_api::Key::new("upmod:box").unwrap(),
                schema_version: 1,
                owner_field: None,
                position_fields: None,
                fields: vec![kk_mod_api::FieldDescriptor {
                    key: kk_mod_api::Key::new("upmod:label").unwrap(),
                    schema: kk_mod_api::ValueSchema {
                        kind: kk_mod_api::ValueKind::String { max_bytes: 16 },
                        nullable: false,
                    },
                    default: kk_mod_api::FieldDefault::Value(Value::String("none".into())),
                    visibility: kk_mod_api::Visibility::Public,
                    binding: kk_mod_api::Key::new("upmod:box.field.label").unwrap(),
                }],
                capabilities: Vec::new(),
                lifecycle_slots: std::collections::BTreeSet::new(),
                visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                player_class: "Box".into(),
                store_capacity: Some(100),
                hits_max: Some(100),
                retaliation_milli: None,
            }],
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: std::collections::BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        };
        let plan = std::sync::Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![contribution],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        Plane {
            world_span: Plane::UNWRAPPED_SPAN,
            buildings: Default::default(),
            links: kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap(),
            stores: kk_mod_api::builtin::StoreRules::from_plan(plan.rules()).unwrap(),
            combat: kk_mod_api::builtin::CombatRanges::from_plan(plan.rules()).unwrap(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            static_passability: Default::default(),
            resources: std::rc::Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan)),
            contrib: std::rc::Rc::new(super::super::contrib::ContributionDispatch::default()),
            extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan,
        }
    }

    fn working() -> Working {
        let room = kk_world::RoomState::new("W1N1", kk_core::Terrain::plains());
        Working::new(
            room,
            0,
            0,
            crate::settle::TickState::new(std::collections::BTreeMap::new(), 100),
            store_plane(),
        )
    }

    #[test]
    fn create_injects_envelope_and_update_rejects_envelope_keys() {
        let mut w = working();
        let created = create(&mut w, "upmod", "upmod:box", &serde_json::json!({})).unwrap();
        let payload = &created.fields;
        assert_eq!(payload["store"]["energy"], serde_json::json!(0));
        assert_eq!(payload["storeCapacity"], serde_json::json!(100));
        // 字段更新拒绝写信封键（库存只能经资源动作变动）。
        let err = update(
            &mut w,
            "upmod",
            ObjId::parse(&created.id).unwrap(),
            &serde_json::json!({"store": {"energy": 99}}),
        )
        .err()
        .expect("信封键应拒绝");
        assert!(err.contains("不能经字段更新写入"), "实得 {err}");
        // 无关字段更新在信封在场时仍可行（拆出→校验→并回，信封原值保留）。
        // 本类型无业务字段，改用非法键证明校验仍覆盖声明字段集。
        let err = update(
            &mut w,
            "upmod",
            ObjId::parse(&created.id).unwrap(),
            &serde_json::json!({"hack": 1}),
        )
        .err()
        .expect("未声明键应拒绝");
        assert!(err.contains("未在类型中声明"), "实得 {err}");
        let err = update(
            &mut w,
            "upmod",
            ObjId::parse(&created.id).unwrap(),
            &serde_json::json!({"label": "x", "hits": 1, "hitsMax": 1}),
        )
        .err()
        .expect("耐久信封键应拒绝");
        assert!(err.contains("耐久信封键"), "实得 {err}");
    }

    #[test]
    fn update_preserves_world_envelope_against_handler_writes() {
        let mut w = working();
        let created = create(&mut w, "upmod", "upmod:box", &serde_json::json!({})).unwrap();
        let id = ObjId::parse(&created.id).unwrap();
        // 正常字段更新后信封原值保留。
        w.update_mod_fields(
            id,
            &serde_json::json!({"label": "hi", "store": {"energy": 0},
                                "storeCapacity": 100, "hits": 100, "hitsMax": 100}),
        )
        .unwrap();
        let view = get(&w, id).unwrap().unwrap();
        assert_eq!(view.fields["label"], serde_json::json!("hi"));
        assert_eq!(view.fields["store"]["energy"], serde_json::json!(0));
        assert_eq!(view.fields["storeCapacity"], serde_json::json!(100));
        assert_eq!(view.fields["hits"], serde_json::json!(100));
        assert_eq!(view.fields["hitsMax"], serde_json::json!(100));
        // 资源动作写入库存后，携带陈旧/篡改信封的字段写回不覆盖库存。
        w.room.objects.get_mut(&id).and_then(|o| match o {
            RoomObject::Mod(m) => {
                let mut payload: Value = serde_json::from_str(&m.data).unwrap();
                payload["store"]["energy"] = serde_json::json!(40);
                m.data = payload.to_string();
                Some(())
            }
            _ => None,
        });
        w.update_mod_fields(
            id,
            &serde_json::json!({"label": "stale", "store": {"energy": 999},
                                "storeCapacity": 1, "hits": 999, "hitsMax": 1}),
        )
        .unwrap();
        let view = get(&w, id).unwrap().unwrap();
        assert_eq!(view.fields["label"], serde_json::json!("stale"));
        assert_eq!(
            view.fields["store"]["energy"],
            serde_json::json!(40),
            "写回信封一律丢弃，世界库存为准"
        );
        assert_eq!(view.fields["storeCapacity"], serde_json::json!(100));
        // 伤害动作扣血后同理：写回携带的耐久信封被丢弃，世界耐久为准。
        w.room.objects.get_mut(&id).and_then(|o| match o {
            RoomObject::Mod(m) => {
                let mut payload: Value = serde_json::from_str(&m.data).unwrap();
                payload["hits"] = serde_json::json!(60);
                m.data = payload.to_string();
                Some(())
            }
            _ => None,
        });
        w.update_mod_fields(
            id,
            &serde_json::json!({"label": "again", "hits": 1, "hitsMax": 2}),
        )
        .unwrap();
        let view = get(&w, id).unwrap().unwrap();
        assert_eq!(view.fields["hits"], serde_json::json!(60));
        assert_eq!(view.fields["hitsMax"], serde_json::json!(100));
    }
}
