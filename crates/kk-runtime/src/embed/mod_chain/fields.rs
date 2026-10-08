//! 字段投影缓存与领域写入校验；每次 native 执行前失效。
use super::*;
use serde_json::json;

impl ModChainContext {
    pub(super) fn fields(&mut self) -> Result<&Value, String> {
        if self.cached_fields.is_none() {
            self.cached_fields = Some(self.load_fields()?);
        }
        Ok(self.cached_fields.as_ref().expect("已加载字段"))
    }

    fn load_fields(&self) -> Result<Value, String> {
        let w = self.ctx.borrow();
        match w.room.objects.get(&self.id) {
            Some(kk_schema::room::RoomObject::Creep(object)) => {
                let view = kk_engine::settle::creep::slot_combat_get(&w, self.id)
                    .ok_or("creep 战斗读面非法")?;
                Ok(json!({
                    "name": object.name, "owner": object.owner, "x":object.x,"y":object.y,
                    "spawning":object.spawning,"hits":object.hits,"fatigue":object.fatigue,
                    "hitsMax": view.hits_max, "body": view.body,
                    "store":kk_world::world_json::store_with_extra_json(&object.store,&object.store_extra),
                    "storeCapacity":kk_engine::settle::creep::store_capacity(object),
                }))
            }
            Some(kk_schema::room::RoomObject::Mod(_)) => w
                .mod_fields(self.id)
                .ok_or_else(|| "Mod payload 非法".into()),
            Some(kk_schema::room::RoomObject::Generic(object)) => {
                let fields: Value =
                    serde_json::from_str(&object.data).map_err(|error| error.to_string())?;
                if fields["structureType"] == "link" {
                    return Ok(fields);
                }
                // Mod 建筑记录（原生键 + 冒号命名空间自定义键）整读；
                // 目录命中即建筑目标，写面按 update_building_fields 校验。
                if fields
                    .get("structureType")
                    .and_then(Value::as_str)
                    .is_some_and(|st| self.ctx.borrow().plane.buildings.get(st).is_some())
                {
                    return Ok(fields);
                }
                Err("内建目标不是 link 或 Mod 建筑".into())
            }
            None if matches!(self.capability, ChainCapability::Objects) => Ok(Value::Null),
            // 房间级竞争/准入槽没有对象目标；占位 id 不对应任何真实对象。
            None if matches!(self.capability, ChainCapability::Contest(_)) => Ok(Value::Null),
            None if matches!(self.capability, ChainCapability::Admission(_)) => Ok(Value::Null),
            None if matches!(self.capability, ChainCapability::Obstacle(_)) => Ok(Value::Null),
            None if matches!(self.capability, ChainCapability::Precheck(_)) => Ok(Value::Null),
            None if matches!(self.capability, ChainCapability::PullRegistration(_)) => {
                Ok(Value::Null)
            }
            None if matches!(self.capability, ChainCapability::MoveCost(_)) => Ok(Value::Null),
            _ => Err(format!("槽目标 {} 已不存在或未接线", self.id.as_str())),
        }
    }

    pub(super) fn write_fields(&mut self, fields: &Value) -> Result<(), String> {
        if fields.is_null() {
            return Ok(());
        }
        // 房间级竞争/准入槽没有对象目标；提前拒绝，避免落入 link 写面报误导文案。
        if matches!(self.capability, ChainCapability::Contest(_)) {
            return Err("移动竞争槽没有对象目标，target.fields 不可写".into());
        }
        if matches!(self.capability, ChainCapability::Admission(_)) {
            return Err("移动准入槽没有对象目标，target.fields 不可写".into());
        }
        if matches!(self.capability, ChainCapability::Obstacle(_)) {
            return Err("移动障碍槽没有对象目标，target.fields 不可写".into());
        }
        if matches!(self.capability, ChainCapability::Precheck(_)) {
            return Err("移动预检槽没有对象目标，target.fields 不可写".into());
        }
        if matches!(self.capability, ChainCapability::PullRegistration(_)) {
            return Err("拉拽登记槽没有对象目标，target.fields 不可写".into());
        }
        if matches!(self.capability, ChainCapability::MoveCost(_)) {
            return Err("移动代价槽没有对象目标，target.fields 不可写".into());
        }
        if !fields.is_object() {
            return Err("target.fields 终值须为对象或 null".into());
        }
        if self.fields()? == fields {
            return Ok(());
        }
        // 受击槽只提交数值候选；没有对象写权限，也没有字段回滚快照。
        if matches!(self.capability, ChainCapability::ModIncoming(_)) {
            return Err("结构受击槽的 target.fields 当前只读".into());
        }
        if matches!(
            self.ctx.borrow().room.objects.get(&self.id),
            Some(kk_schema::room::RoomObject::Creep(_))
        ) {
            return Err("creep 槽的 target.fields 当前只读".into());
        }
        let is_mod = matches!(
            self.ctx.borrow().room.objects.get(&self.id),
            Some(kk_schema::room::RoomObject::Mod(_))
        );
        if is_mod {
            self.cached_fields = None;
            return self.ctx.borrow_mut().update_mod_fields(self.id, fields);
        }
        // Mod 建筑写面：自定义键按字段描述校验整写，原生键只读。
        let is_building = 'building: {
            let w = self.ctx.borrow();
            let Some(kk_schema::room::RoomObject::Generic(g)) = w.room.objects.get(&self.id) else {
                break 'building false;
            };
            let Ok(data) = serde_json::from_str::<Value>(&g.data) else {
                break 'building false;
            };
            data.get("structureType")
                .and_then(Value::as_str)
                .is_some_and(|st| w.plane.buildings.get(st).is_some())
        };
        if is_building {
            self.cached_fields = None;
            return self
                .ctx
                .borrow_mut()
                .update_building_fields(self.id, fields);
        }
        // 当前内建写面仅接 cooldown；身份/坐标和其余字段只读。后续规则
        // 字段按领域校验逐项开放，不能把 Generic JSON 整写当通用事务 API。
        let mut current = self.fields()?.clone();
        let mut proposed = fields.clone();
        let cooldown = proposed
            .as_object_mut()
            .expect("上面已确认对象")
            .remove("cooldown");
        current
            .as_object_mut()
            .ok_or("link 记录不是对象")?
            .remove("cooldown");
        if proposed != current {
            return Err("link 当前仅允许改写 cooldown，其余字段只读".into());
        }
        if let Some(value) = &cooldown {
            if value.as_u64().is_none_or(|n| n > u64::from(u32::MAX)) {
                return Err("link.cooldown 须为 u32 整数".into());
            }
            current["cooldown"] = value.clone();
        }
        self.cached_fields = None;
        if let Some(kk_schema::room::RoomObject::Generic(object)) =
            self.ctx.borrow_mut().room.objects.get_mut(&self.id)
        {
            object.data = current.to_string();
            Ok(())
        } else {
            Err("link 目标已失效".into())
        }
    }
}
