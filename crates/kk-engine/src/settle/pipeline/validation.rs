//! 世界发布前的不变式、Mod 对象和扩展字段校验。
use super::txn;
use crate::settle::{Engine, SettleError};
use kk_schema::room::RoomObject;
use kk_world::{ObjId, RoomState};
use serde_json::Value;

/// ObjId（u64）唯一性扫描的乘法散列：validate 每拍 9 万+插入，默认
/// SipHash 的逐键哈希曾是该段约一半成本（2026-10-12 sample 取证）。
#[derive(Default)]
struct IdHasher(u64);
impl std::hash::Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(32)
    }
    fn write(&mut self, bytes: &[u8]) {
        // ObjId 的派生 Hash 走 write_u64；此兜底仅保证散列器完备。
        for &byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ byte as u64).wrapping_mul(0x517C_C1B7_2722_0A95);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517C_C1B7_2722_0A95);
    }
}
type IdHashBuild = std::hash::BuildHasherDefault<IdHasher>;

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
        let state = cand.state.inner.lock().expect("共享候选锁");
        if state.next_id < base {
            return Err(SettleError(format!(
                "id 候选回退：{} < 基线 {base}",
                state.next_id
            )));
        }
        // 唯一性集合：u64 键乘法散列 + 按对象总量预容量——默认 SipHash +
        // 空集增量扩容曾占 validate 段约一半（2026-10-12 sample 取证，
        // 9 万+插入逐键 SipHash 与 rehash 双重开销）。
        let total_objects: usize = cand.rooms.iter().map(|(_, room)| room.objects.len()).sum();
        let mut ids = std::collections::HashSet::with_capacity_and_hasher(
            total_objects,
            IdHashBuild::default(),
        );
        // Mod 对象检查融进同一趟全对象遍历：单独的 Mod 扫曾是第二趟
        // 全量枚举（B0 无 Mod 也要逐对象判别），管理路径仍走独立入口。
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
                if let kk_schema::room::RoomObject::Mod(o) = object {
                    self.check_mod_object(name, id, o)?;
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
        self.validate_building_rooms(cand.rooms.iter().map(|(name, room)| (name, room)))?;
        Ok(())
    }

    /// 发布前校验候选中的 Mod 对象：类型在计划中、字段 payload 整体
    /// 满足描述符、声明的位置字段与信封坐标同步。写入路径（创建与
    /// 处理器回写）已各自校验；此处封住绕过写入口的直改形态。结算
    /// validate 已把单对象检查融进不变式遍历（免第二趟全扫），本入口
    /// 供检查点/管理路径整房扫描。
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
                self.check_mod_object(room_name, id, o)?;
            }
        }
        Ok(())
    }

    /// 单个 Mod 对象的发布前校验（类型键/计划内/payload 字段/库存与
    /// 耐久信封/位置同步）。validate 融合路径与管理扫描入口共用。
    fn check_mod_object(
        &self,
        room_name: &str,
        id: &kk_world::ObjId,
        o: &kk_schema::room::ModObj,
    ) -> Result<(), SettleError> {
        {
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
            // 库存/耐久信封键（G09.a/G09.b）不属于声明字段：拆出后
            // 校验，再按类型声明核对信封在场的合法性（防篡改与防缺失）。
            let (bare, envelope) = kk_mod_api::objects::split_store_envelope(&payload);
            let (fields, hits_env) = kk_mod_api::objects::split_hits_envelope(&bare);
            kk_mod_api::validate::check_object_payload(&fields, &ty.descriptor.fields, &o.type_key)
                .map_err(SettleError)?;
            let envelope_complete = envelope.as_ref().is_some_and(|env| {
                kk_mod_api::objects::STORE_ENVELOPE_KEYS
                    .iter()
                    .filter(|key| **key != "storeCapacityResource")
                    .all(|key| env.contains_key(*key))
            });
            if let Some(declared) = ty.descriptor.store_capacity {
                if !envelope_complete {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 声明 store 却缺少完整库存信封",
                        id.as_str()
                    )));
                }
                let env = envelope.as_ref().expect("上面已判 complete");
                // Mod 当前只有冻结的总容量；单资源槽未开放。库存数值与
                // 总和须能在玩家 JS 读面中精确表示，恢复时同样校验。
                let total = env
                    .get("store")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|store| {
                        store.values().try_fold(0_u64, |total, value| {
                            total
                                .checked_add(value.as_u64()?)
                                .filter(|sum| *sum <= 9_007_199_254_740_991)
                        })
                    });
                if env.get("storeCapacity").and_then(serde_json::Value::as_u64) != Some(declared)
                    || env.contains_key("storeCapacityResource")
                    || total.is_none()
                    || kk_world::domain::generic::decode_store(&o.data).is_none()
                {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 库存信封非法",
                        id.as_str()
                    )));
                }
            } else if envelope.is_some() {
                return Err(SettleError(format!(
                    "房间 {room_name} 的 Mod 对象 {} 未声明 store 却携带库存信封",
                    id.as_str()
                )));
            }
            // 耐久信封（G09.b）三臂：声明缺信封、信封与冻结声明不符、
            // 未声明携带。hitsMax 恒等于冻结声明（写一次语义），hits
            // 在 1..=hitsMax 内（伤害扣至 0 即移除，存活对象恒 ≥1；
            // 0 只能由直改造出，发布前拒绝）。
            let hits_complete = hits_env
                .as_ref()
                .is_some_and(|env| env.contains_key("hits") && env.contains_key("hitsMax"));
            if let Some(declared) = ty.descriptor.hits_max {
                if !hits_complete {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 声明耐久却缺少完整耐久信封",
                        id.as_str()
                    )));
                }
                let env = hits_env.as_ref().expect("上面已判 complete");
                let hits = env.get("hits").and_then(serde_json::Value::as_i64);
                let hits_max = env.get("hitsMax").and_then(serde_json::Value::as_i64);
                if hits_max != Some(declared as i64)
                    || !hits.is_some_and(|h| (1..=declared as i64).contains(&h))
                {
                    return Err(SettleError(format!(
                        "房间 {room_name} 的 Mod 对象 {} 耐久信封非法",
                        id.as_str()
                    )));
                }
            } else if hits_env.is_some() {
                return Err(SettleError(format!(
                    "房间 {room_name} 的 Mod 对象 {} 未声明耐久却携带耐久信封",
                    id.as_str()
                )));
            }
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

    /// 库存/耐久信封（G09.a/G09.b）三臂守卫：声明缺信封、信封键齐全但
    /// 内容非法、未声明却携带信封；完整合法信封（含双信封并存）照常
    /// 通过。绕过写入口直改世界的形态由发布前校验封住。
    #[test]
    fn mod_store_envelope_guards_discriminate() {
        use std::collections::{BTreeMap, BTreeSet};
        use std::sync::Arc;
        let field = |name: String| kk_mod_api::FieldDescriptor {
            binding: kk_mod_api::Key::new(format!("gval:crate.field.{name}")).unwrap(),
            key: kk_mod_api::Key::new(format!("gval:{name}")).unwrap(),
            schema: kk_mod_api::ValueSchema {
                kind: kk_mod_api::ValueKind::Number(kk_mod_api::NumberDomain::Unsigned {
                    min: 0,
                    max: 49,
                }),
                nullable: false,
            },
            default: kk_mod_api::FieldDefault::Missing,
            visibility: kk_mod_api::Visibility::Public,
        };
        let descriptor = |type_key: &str, capacity: Option<u64>, hits_max: Option<u64>| {
            kk_mod_api::ObjectTypeDescriptor {
                key: kk_mod_api::Key::new(type_key).unwrap(),
                schema_version: 1,
                owner_field: None,
                position_fields: Some([
                    kk_mod_api::Key::new("gval:x").unwrap(),
                    kk_mod_api::Key::new("gval:y").unwrap(),
                ]),
                fields: vec![field("x".into()), field("y".into())],
                capabilities: Vec::new(),
                lifecycle_slots: BTreeSet::new(),
                visibility: kk_mod_api::ObjectVisibility::RoomVisible,
                player_class: "Obj".into(),
                store_capacity: capacity,
                hits_max,
                retaliation_milli: None,
            }
        };
        let contribution = kk_mod_api::Contribution {
            source: kk_mod_api::Source {
                namespace: "gval".into(),
                api_version: 1,
                revision: "1".into(),
                code_digest: [7; 32],
                config_digest: [0; 32],
                player_api_digest: [0; 32],
            },
            bindings: BTreeSet::from([
                kk_mod_api::Key::new("gval:crate.field.x").unwrap(),
                kk_mod_api::Key::new("gval:crate.field.y").unwrap(),
            ]),
            field_mounts: Vec::new(),
            object_types: vec![
                descriptor("gval:crate", Some(500), None),
                descriptor("gval:marker", None, None),
                descriptor("gval:bunker", None, Some(300)),
                descriptor("gval:depot", Some(200), Some(300)),
            ],
            intents: Vec::new(),
            rules: Vec::new(),
            overrides: Vec::new(),
            constraints: BTreeSet::new(),
            slots: Vec::new(),
            handlers: Vec::new(),
            resources: Vec::new(),
            buildings: Vec::new(),
        };
        let plan = Arc::new(
            kk_mod_api::freeze_with_builtin(
                kk_mod_api::builtin::link(),
                vec![contribution],
                kk_mod_api::FreezeOptions::default(),
            )
            .unwrap(),
        );
        let validate = |type_key: &str, payload: Value| {
            let mut world = kk_world::World::new(1);
            world
                .register_room("W1N1", kk_core::Terrain::plains())
                .unwrap();
            world
                .insert(
                    "W1N1",
                    RoomObject::Mod(kk_schema::room::ModObj {
                        id: 0,
                        x: 10,
                        y: 11,
                        type_key: type_key.into(),
                        data: payload.to_string(),
                    }),
                )
                .unwrap();
            let engine =
                Engine::with_plan(world, plan.clone(), BTreeMap::new(), BTreeMap::new()).unwrap();
            let mut txn = engine
                .prepare_tick(
                    IntentBatch::default(),
                    TickInput {
                        now_ms: 0,
                        warmup: false,
                    },
                )
                .unwrap();
            txn.rooms = txn
                .room_names
                .iter()
                .map(|name| (name.clone(), engine.world.room(name).unwrap().clone()))
                .collect();
            let validation = engine
                .validate_stage(&mut txn)
                .map_err(|err| err.to_string());
            let saved = engine.checkpoint().unwrap();
            let mut restored = Engine::with_plan(
                kk_world::World::new(1),
                plan.clone(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
            let recovery = restored.restore_checkpoint(saved);
            assert_eq!(
                validation.is_ok(),
                recovery.is_ok(),
                "发布与恢复同一信封校验：{payload} {recovery:?}"
            );
            validation
        };
        validate(
            "gval:crate",
            serde_json::json!({"x": 10, "y": 11, "store": {"energy": 0}, "storeCapacity": 500}),
        )
        .expect("完整合法信封通过");
        validate(
            "gval:bunker",
            serde_json::json!({"x": 10, "y": 11, "hits": 120, "hitsMax": 300}),
        )
        .expect("耐久信封合法通过（hits 可低于上限）");
        validate(
            "gval:depot",
            serde_json::json!({"x": 10, "y": 11, "store": {"energy": 0}, "storeCapacity": 200,
                                "hits": 300, "hitsMax": 300}),
        )
        .expect("双信封并存合法通过");
        // 恢复与发布共用此门：共享容量须等于冻结声明，库存合计须可表示，
        // 不允许直改引入首片未开放的逐资源槽。
        for payload in [
            serde_json::json!({"x":10,"y":11,"store":{"energy":0},"storeCapacity":999}),
            serde_json::json!({"x":10,"y":11,"store":{"energy":0},"storeCapacity":"infinite"}),
            serde_json::json!({"x":10,"y":11,"store":{"energy":0},"storeCapacity":null}),
            serde_json::json!({"x":10,"y":11,"store":{"energy":9007199254740992u64},"storeCapacity":500}),
            serde_json::json!({"x":10,"y":11,"store":{"energy":9007199254740991u64,"H":1},"storeCapacity":500}),
            serde_json::json!({"x":10,"y":11,"store":{"energy":0},"storeCapacity":500,"storeCapacityResource":{"energy":1000}}),
        ] {
            let err = validate("gval:crate", payload.clone()).expect_err(&payload.to_string());
            assert!(err.contains("库存信封非法"), "{err}");
        }
        for (type_key, payload, needle) in [
            (
                "gval:crate",
                serde_json::json!({"x": 10, "y": 11}),
                "缺少完整库存信封",
            ),
            (
                "gval:crate",
                serde_json::json!({"x": 10, "y": 11, "store": {"energy": 0}}),
                "缺少完整库存信封",
            ),
            (
                "gval:crate",
                serde_json::json!({"x": 10, "y": 11, "store": {"energy": -5}, "storeCapacity": 500}),
                "库存信封非法",
            ),
            (
                "gval:marker",
                serde_json::json!({"x": 10, "y": 11, "store": {"energy": 0}, "storeCapacity": 100}),
                "未声明 store 却携带库存信封",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11}),
                "声明耐久却缺少完整耐久信封",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11, "hits": 300}),
                "声明耐久却缺少完整耐久信封",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11, "hitsMax": 300}),
                "声明耐久却缺少完整耐久信封",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11, "hits": 300, "hitsMax": 200}),
                "耐久信封非法",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11, "hits": 400, "hitsMax": 300}),
                "耐久信封非法",
            ),
            (
                "gval:bunker",
                serde_json::json!({"x": 10, "y": 11, "hits": 0, "hitsMax": 300}),
                "耐久信封非法",
            ),
            (
                "gval:marker",
                serde_json::json!({"x": 10, "y": 11, "hits": 100, "hitsMax": 100}),
                "未声明耐久却携带耐久信封",
            ),
        ] {
            let err = validate(type_key, payload).unwrap_err();
            assert!(err.contains(needle), "类型 {type_key} 实得 {err}");
        }
    }

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
                .prepare_tick(
                    IntentBatch::default(),
                    TickInput {
                        now_ms: 0,
                        warmup: false,
                    },
                )
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
                    txn.state.inner.lock().expect("共享候选锁").next_id = id.0;
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
