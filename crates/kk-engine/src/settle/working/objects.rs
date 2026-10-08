//! 对象增删、字段和位置修改，维护伴随索引与日志。
use super::*;

impl Working {
    /// 使用共享游标创建对象，并登记位置索引。
    /// 非法坐标、耗尽或冲突留存为整轮故障；捕获返回错误不能解除故障。
    pub fn insert_new(&mut self, object: RoomObject) -> Result<ObjId, SettleError> {
        let (x, y) = object.pos();
        if x >= 50 || y >= 50 {
            return Err(self
                .state
                .record_fatal(SettleError(format!("对象坐标越界：({x},{y})"))));
        }
        let id = self.state.allocate_id()?;
        self.room
            .insert_with_id(id, object)
            .map_err(|error| self.state.record_fatal(SettleError(error)))?;
        self.remember_index_cell((x, y));
        self.index.insert(ObjId(id), (x, y));
        let id = ObjId(id);
        if let Some(log) = &mut self.chain_mutations {
            log.push(ChainMutation::Inserted(id));
        }
        Ok(id)
    }

    /// 删除对象及位置索引，同时清理它的疲劳、待伤害、待治疗和扩展字段。
    /// 链日志武装期间记录 Removed；同链先插后删的对象经逆序回放净效果
    /// 仍为删除，无须特判。
    pub fn remove_obj(&mut self, id: ObjId) -> Option<RoomObject> {
        if let Some(object) = self.room.objects.get(&id) {
            self.remember_index_cell(object.pos());
        }
        let journaled = self
            .chain_mutations
            .is_some()
            .then(|| {
                self.room.objects.get(&id).map(|object| RemovedObject {
                    id,
                    object: object.clone(),
                    extensions: self.room.extensions.get(&id).cloned(),
                    effects: self.room.effects.get(&id).cloned(),
                    old_fatigue: self.old_fatigue.get(&id).copied(),
                    tick_fatigue: self.tick_fatigue.get(&id).copied(),
                    damage: self.damage_to_apply.get(&id).copied(),
                    heal: self.heal_to_apply.get(&id).copied(),
                })
            })
            .flatten();
        let obj = self.room.objects.remove(&id)?;
        let (x, y) = obj.pos();
        self.index.remove(id, (x, y));
        self.room.extensions.remove(&id);
        self.room.effects.remove(&id);
        self.old_fatigue.remove(&id);
        self.tick_fatigue.remove(&id);
        self.damage_to_apply.remove(&id);
        self.heal_to_apply.remove(&id);
        if let (Some(log), Some(bundle)) = (&mut self.chain_mutations, journaled) {
            log.push(ChainMutation::Removed(Box::new(bundle)));
        }
        Some(obj)
    }

    /// 读取 Mod 对象的字段 payload；非 Mod 对象或 data 非法 JSON 返回 None。
    pub fn mod_fields(&self, id: ObjId) -> Option<Value> {
        match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => serde_json::from_str(&o.data).ok(),
            _ => None,
        }
    }

    /// 校验并整值写入 Mod 对象的字段 payload：类型必须在计划中声明，
    /// 键集合与字段描述完全一致；声明的位置字段同步到对象信封坐标与
    /// 位置索引。只修改本房候选，失败随房间候选一并丢弃。
    pub fn update_mod_fields(&mut self, id: ObjId, fields: &Value) -> Result<(), String> {
        let fields = fields.clone();
        let type_key = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => o.type_key.clone(),
            _ => return Err(format!("对象 {} 不是 Mod 对象", id.as_str())),
        };
        let key = kk_mod_api::Key::new(type_key.clone())
            .map_err(|e| format!("Mod 对象类型键非法 {type_key:?}: {e}"))?;
        let Some(handle) = self.plane.plan.projection().resolve(&key) else {
            return Err(format!("Mod 对象类型 {type_key} 不在计划中"));
        };
        let ty = self
            .plane
            .plan
            .projection()
            .get(handle)
            .expect("句柄来自计划");
        // 库存/耐久信封键（G09.a/G09.b）不参与字段校验：拆出→校验声明
        // 字段→并回 **世界当前** 信封。调用方（ctx.objects.update 与槽
        // 写回）传入的信封值一律丢弃——库存只能经资源动作、耐久只能经
        // 伤害动作变动，槽内先读后写也不会用陈旧快照回滚结算结果。
        let (current_store, current_hits) = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => {
                let payload: Value = serde_json::from_str(&o.data)
                    .map_err(|e| format!("Mod 对象 payload 非法 JSON：{e}"))?;
                let (bare, store) = kk_mod_api::objects::split_store_envelope(&payload);
                let (_, hits) = kk_mod_api::objects::split_hits_envelope(&bare);
                (store, hits)
            }
            _ => unreachable!("上面已判 Mod"),
        };
        let (bare, _) = kk_mod_api::objects::split_store_envelope(&fields);
        let (mut fields, _) = kk_mod_api::objects::split_hits_envelope(&bare);
        kk_mod_api::validate::check_object_payload(&fields, &ty.descriptor.fields, &type_key)?;
        // 数值 backing 规范化（Float 域统一浮点 backing）：KKPV 列式编码
        // 按值分派列型，整数/浮点 backing 混桶会使整轮投影失败。
        kk_mod_api::validate::normalize_object_payload(&mut fields, &ty.descriptor.fields);
        kk_mod_api::objects::merge_store_envelope(&mut fields, current_store);
        kk_mod_api::objects::merge_hits_envelope(&mut fields, current_hits);
        if self.chain_mutations.as_ref().is_some_and(|log| {
            !log.iter()
                .any(|entry| matches!(entry, ChainMutation::ModUpdated(old, _) if *old == id))
        }) {
            self.check_mod_mutation_budget()?;
            let original = self.room.objects.get(&id).expect("已确认 Mod 对象").clone();
            self.chain_mutations
                .as_mut()
                .expect("日志已武装")
                .push(ChainMutation::ModUpdated(id, Box::new(original)));
        }
        let (mut x, mut y) = match self.room.objects.get(&id) {
            Some(RoomObject::Mod(o)) => (o.x, o.y),
            _ => unreachable!("上面已判 Mod"),
        };
        if let Some([fx, fy]) = &ty.descriptor.position_fields {
            let px = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fx))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            let py = u8::try_from(
                fields
                    .get(kk_mod_api::validate::field_name(fy))
                    .and_then(Value::as_u64)
                    .expect("payload 已通过 schema 校验，位置字段必为 0..49 整数"),
            )
            .expect("0..49 在 u8 内");
            if px != x || py != y {
                self.remember_index_cell((x, y));
                self.remember_index_cell((px, py));
                self.index.apply_move(id, (x, y), (px, py));
                x = px;
                y = py;
            }
        }
        if let Some(RoomObject::Mod(o)) = self.room.objects.get_mut(&id) {
            o.data = fields.to_string();
            o.x = x;
            o.y = y;
        }
        Ok(())
    }

    /// 对象移动（索引增量更新）。Mod 对象拒绝：其位置同时存在于
    /// payload 字段与信封坐标，发布前校验要求两者一致，原始索引移动
    /// 不维护该不变量——Mod 的位置变更走 update_mod_fields。
    pub fn move_obj(&mut self, id: ObjId, to: (u8, u8)) -> Result<(), String> {
        if let Some(obj) = self.room.objects.get_mut(&id) {
            if matches!(obj, RoomObject::Mod(_)) {
                return Err(format!(
                    "对象 {} 是 Mod 对象：位置变更须走 update_mod_fields（payload 字段与信封坐标同步）",
                    id.as_str()
                ));
            }
            let from = obj.pos();
            obj.set_pos(to.0, to.1);
            self.index.apply_move(id, from, to);
        }
        Ok(())
    }
}
