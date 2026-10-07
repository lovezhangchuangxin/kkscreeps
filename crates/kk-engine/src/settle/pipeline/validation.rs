//! 世界发布前的不变式、Mod 对象和扩展字段校验。
use super::txn;
use crate::settle::{Engine, SettleError};
use kk_schema::room::RoomObject;
use kk_world::{ObjId, RoomState};
use serde_json::Value;

impl Engine {
    /// 只读检查候选房间覆盖、ID 游标和玩家键，失败进入 NeedsRecovery。
    pub(in crate::settle) fn validate_stage(
        &self,
        cand: &mut txn::OwnedTickTxn,
    ) -> Result<(), SettleError> {
        cand.state.check_fatal()?;
        // 房间数和顺序均须与准备阶段一致，集合相等不能验证顺序。
        if cand.rooms.len() != cand.room_names.len()
            || cand
                .rooms
                .iter()
                .zip(&cand.room_names)
                .any(|((settled_name, _), want)| settled_name != want)
        {
            return Err(SettleError(format!(
                "候选房间覆盖不完整：期望 {} 间，实得 {} 间",
                cand.room_names.len(),
                cand.rooms.len()
            )));
        }
        // 游标不得低于入口的 ID 起点，避免后续创建对象复用旧 ID。
        let base = self
            .next_id
            .max(self.world.next_available_obj_id().map_err(SettleError)?);
        let state = cand.state.inner.borrow();
        if state.next_id < base {
            return Err(SettleError(format!(
                "id 候选回退：{} < 基线 {base}",
                state.next_id
            )));
        }
        let mut ids = std::collections::HashSet::new();
        for (name, room) in &cand.rooms {
            for (id, object) in &room.objects {
                let (x, y) = object.pos();
                if id.0 == 0
                    || id.0 >= state.next_id
                    || object.id() != id.0
                    || x >= 50
                    || y >= 50
                    || !ids.insert(*id)
                {
                    return Err(SettleError(format!(
                        "房间 {name} 对象 {} 的 ID/坐标不变式无效",
                        id.as_str()
                    )));
                }
            }
        }
        // 候选可以增加玩家，不能删除已提交的玩家。
        if let Some(missing) = self
            .players
            .keys()
            .find(|k| !state.players.contains_key(*k))
        {
            return Err(SettleError(format!("玩家候选丢键：{missing}")));
        }
        drop(state);
        self.validate_extensions(cand)?;
        self.validate_mod_objects(cand)?;
        self.validate_building_rooms(cand.rooms.iter().map(|(name, room)| (name, room)))?;
        Ok(())
    }

    /// 发布前校验候选中的 Mod 对象：类型在计划中、字段 payload 整体
    /// 满足描述符、声明的位置字段与信封坐标同步。写入路径（创建与
    /// 处理器回写）已各自校验；此处封住绕过写入口的直改形态。
    pub(super) fn validate_mod_objects(
        &self,
        cand: &mut txn::OwnedTickTxn,
    ) -> Result<(), SettleError> {
        self.validate_mod_object_rooms(&cand.rooms)
    }

    pub(super) fn validate_mod_object_rooms(
        &self,
        rooms: &[(String, RoomState)],
    ) -> Result<(), SettleError> {
        self.validate_mod_object_iter(rooms.iter().map(|(name, room)| (name, room)))
    }

    pub(in crate::settle) fn validate_mod_object_iter<'a>(
        &self,
        rooms: impl Iterator<Item = (&'a String, &'a RoomState)>,
    ) -> Result<(), SettleError> {
        // 短路按候选内容而非计划布尔量：本函数封的是"绕过写入口的直改"，
        // 计划里没有 Mod 类型不能成为跳过校验的理由——快照恢复等新来源
        // 仍可能带入计划外对象。扫描是一次枚举判别匹配，与结算既有的
        // 逐对象遍历同阶。
        for (room_name, room) in rooms {
            for (id, obj) in &room.objects {
                let kk_schema::room::RoomObject::Mod(o) = obj else {
                    continue;
                };
                let key = kk_mod_api::Key::new(o.type_key.clone()).map_err(|e| {
                    SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 类型键非法：{e}",
                        id.as_str()
                    ))
                })?;
                let Some(handle) = self.plan.projection().resolve(&key) else {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 类型 {} 不在计划中",
                        id.as_str(),
                        o.type_key
                    )));
                };
                let ty = self.plan.projection().get(handle).expect("句柄来自计划");
                let payload: serde_json::Value = serde_json::from_str(&o.data).map_err(|e| {
                    SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} payload 非法 JSON：{e}",
                        id.as_str()
                    ))
                })?;
                kk_mod_api::validate::check_object_payload(
                    &payload,
                    &ty.descriptor.fields,
                    &o.type_key,
                )
                .map_err(SettleError)?;
                if let Some([fx, fy]) = &ty.descriptor.position_fields {
                    let px = payload
                        .get(kk_mod_api::validate::field_name(fx))
                        .and_then(serde_json::Value::as_u64);
                    let py = payload
                        .get(kk_mod_api::validate::field_name(fy))
                        .and_then(serde_json::Value::as_u64);
                    if px != Some(o.x as u64) || py != Some(o.y as u64) {
                        return Err(SettleError(format!(
                            "房间 {room_name} 的 Mod 对象 {} 位置字段与信封坐标不同步",
                            id.as_str()
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub(in crate::settle) fn validate_building_rooms<'a>(
        &self,
        rooms: impl Iterator<Item = (&'a String, &'a RoomState)>,
    ) -> Result<(), SettleError> {
        if self.buildings.is_empty() {
            return Ok(());
        }
        for (name, room) in rooms {
            for (id, object) in &room.objects {
                let RoomObject::Generic(g) = object else {
                    continue;
                };
                let record: Value = serde_json::from_str(&g.data).map_err(|error| {
                    SettleError(format!(
                        "房间 {name} 对象 {} 建筑记录非法：{error}",
                        id.as_str()
                    ))
                })?;
                if let Some(building) = record["structureType"]
                    .as_str()
                    .and_then(|key| self.buildings.get(key))
                {
                    building.validate_record(&record).map_err(|error| {
                        SettleError(format!("房间 {name} 对象 {}：{error}", id.as_str()))
                    })?;
                }
            }
        }
        Ok(())
    }

    /// 规整并校验候选房间的扩展字段：键必须在目录中声明（封住绕过
    /// 写入 API 的直改），未声明或键非法仍然失败；ObjectRef 字段的
    /// 悬空引用在发布前收敛——nullable 字段写 null、非 nullable 字段
    /// 整项删除（与玩家侧 getObjectById 对失效引用返回 null 同形）。
    /// 收敛使被引用对象销毁的 tick 仍可提交。
    pub(super) fn validate_extensions(
        &self,
        cand: &mut txn::OwnedTickTxn,
    ) -> Result<(), SettleError> {
        self.validate_extension_rooms(&mut cand.rooms)
    }

    pub(in crate::settle) fn validate_extension_rooms(
        &self,
        rooms: &mut [(String, RoomState)],
    ) -> Result<(), SettleError> {
        // 短路按候选房间的扩展条目而非扩展目录：目录为空不是跳过的理由
        // （候选里的未声明条目正是要抓的直改形态），这里只省掉空候选
        // 的扫描成本。
        if rooms.iter().all(|(_, room)| room.extensions.is_empty()) {
            return Ok(());
        }
        // 收敛动作先收集后应用，避免边遍历边改。
        let mut reconcile: Vec<(String, ObjId, String, bool)> = Vec::new();
        for (room_name, room) in rooms.iter() {
            for (id, fields) in &room.extensions {
                for (field, value) in fields {
                    let structure_type: Option<String> =
                        room.objects.get(id).and_then(|o| match o {
                            RoomObject::Generic(g) => {
                                let v: serde_json::Value =
                                    serde_json::from_str(&g.data).unwrap_or_default();
                                v.get("structureType")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string)
                            }
                            _ => None,
                        });
                    let type_key = structure_type
                        .as_deref()
                        .and_then(|st| kk_mod_api::Key::new(format!("builtin:{st}")).ok());
                    let Some(type_key) = type_key else {
                        return Err(SettleError(format!(
                            "扩展候选引用了未接入扩展平面的对象：房间 {room_name} {}",
                            id.as_str()
                        )));
                    };
                    let Some(rule) =
                        self.extensions.field(
                            &type_key,
                            &kk_mod_api::Key::new(field.clone()).ok().ok_or(SettleError(
                                format!("扩展候选键非法：房间 {room_name} {field}"),
                            ))?,
                        )
                    else {
                        return Err(SettleError(format!(
                            "扩展候选含未声明字段：房间 {room_name} {field}"
                        )));
                    };
                    if matches!(rule.schema.kind, kk_mod_api::ValueKind::ObjectRef(_)) {
                        // nullable 字段的 null 是合法清除态，跳过引用检查。
                        if rule.schema.nullable && value.is_null() {
                            continue;
                        }
                        let Some(referred) = value.as_str().and_then(kk_world::ObjId::parse) else {
                            return Err(SettleError(format!(
                                "扩展引用值不是合法对象 ID：房间 {room_name} {field}"
                            )));
                        };
                        let exists = rooms.iter().any(|(_, r)| r.objects.contains_key(&referred));
                        if !exists {
                            reconcile.push((
                                room_name.clone(),
                                *id,
                                field.clone(),
                                rule.schema.nullable,
                            ));
                        }
                    }
                }
            }
        }
        for (room_name, id, field, nullable) in reconcile {
            let (_, room) = rooms
                .iter_mut()
                .find(|(name, _)| name == &room_name)
                .expect("房间来自候选");
            let Some(fields) = room.extensions.get_mut(&id) else {
                continue;
            };
            if nullable {
                fields.insert(field, serde_json::Value::Null);
            } else {
                fields.remove(&field);
                if fields.is_empty() {
                    room.extensions.remove(&id);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod invariant_tests {
    use super::*;
    use crate::intent::IntentBatch;
    use crate::settle::TickInput;

    #[test]
    fn publication_rejects_duplicate_cursor_record_id_and_coordinate_drift() {
        let mut engine = Engine::default();
        for name in ["W1N1", "W2N2"] {
            engine
                .world
                .register_room(name, kk_core::Terrain::plains())
                .unwrap();
            engine
                .world
                .insert(
                    name,
                    RoomObject::Energy(kk_schema::room::EnergyObj {
                        id: 0,
                        x: 20,
                        y: 20,
                        resource_type: 0,
                        amount: 10,
                        resource: None,
                    }),
                )
                .unwrap();
        }
        for mutation in 0..4 {
            let mut txn = engine
                .prepare_tick(IntentBatch::default(), TickInput { now_ms: 0 })
                .unwrap();
            txn.rooms = engine
                .world
                .rooms()
                .iter()
                .map(|(name, room)| (name.clone(), room.clone()))
                .collect();
            let (id, object) = txn.rooms[0]
                .1
                .objects
                .iter()
                .next()
                .map(|(id, o)| (*id, o.clone()))
                .unwrap();
            match mutation {
                0 => {
                    txn.rooms[1].1.objects.insert(id, object);
                }
                1 => {
                    txn.state.inner.borrow_mut().next_id = id.0;
                }
                2 => {
                    txn.rooms[0].1.objects.get_mut(&id).unwrap().set_id(999);
                }
                _ => {
                    if let RoomObject::Energy(object) = txn.rooms[0].1.objects.get_mut(&id).unwrap()
                    {
                        object.x = 50;
                    }
                }
            }
            assert!(
                engine.validate_stage(&mut txn).is_err(),
                "变异 {mutation} 必须失败"
            );
        }
    }
}
