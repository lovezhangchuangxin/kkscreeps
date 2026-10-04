//! 将 Mod 整槽链接到房间候选，暂存 state 并校验字段写面。
use super::*;

mod resources;

/// JS 组合的整槽执行器。进入 VM 一次，native 中心只经普通 op 执行。
pub(super) struct ModSlotExecutor {
    pub(super) realm: Rc<RefCell<ModVm>>,
}

enum NativeCall {
    Action {
        data: Box<kk_engine::intent::IntentData>,
        step: fn(&mut kk_engine::settle::Working, ObjId, &kk_engine::intent::IntentData),
    },
    Tick(fn(&mut kk_engine::settle::Working, ObjId)),
    None,
}

struct ModChainContext {
    ctx: RoomCtx,
    id: ObjId,
    args: Value,
    native: NativeCall,
    /// 仅在本次能力内缓存已解析字段；改写与 native 前失效，不跨槽/对象/tick。
    cached_fields: Option<Value>,
    /// 可拒绝动作的 namespace 写入暂存到调用成功；拒绝时丢弃。
    staged_states: Option<Rc<RefCell<BTreeMap<String, Value>>>>,
    /// 资源写能力首次触达的对象快照；共享给整槽外层恢复。
    resource_journal: Option<Rc<RefCell<BTreeMap<ObjId, kk_schema::room::RoomObject>>>>,
}

impl SlotExecutor for ModSlotExecutor {
    fn run(
        &self,
        slot: &SlotPlan,
        ctx: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<SlotOutcome, String> {
        // 自有动作字段与额外资源对象由此适配器恢复；内建资源动作的
        // 原始两端/事件由引擎在整链外恢复。namespace 仅成功后移交。
        let extension = matches!(&call, SlotCall::Extension { .. });
        let resource_action = matches!(&call, SlotCall::Action { .. })
            && kk_mod_api::builtin::RESOURCE_ACTION_SLOTS.contains(&slot.key.as_str());
        let (id, args, native) = match call {
            SlotCall::Action { id, data, native } => (
                id,
                json!({
                    "id": data.id.map(|id| id.as_str()), "amount": data.amount, "resourceType": data.resource_type,
                }),
                NativeCall::Action {
                    data: Box::new(data.clone()),
                    step: native,
                },
            ),
            SlotCall::Tick { id, native } => (id, Value::Null, NativeCall::Tick(native)),
            SlotCall::Extension { id, args } => (id, args.clone(), NativeCall::None),
        };
        let room = ctx.borrow().room.name.clone();
        let original_fields = if extension {
            Some(
                ctx.borrow()
                    .mod_fields(id)
                    .ok_or("扩展动作目标不是有效 Mod 对象")?,
            )
        } else {
            None
        };
        let staged_states =
            (extension || resource_action).then(|| Rc::new(RefCell::new(BTreeMap::new())));
        let resource_journal = resource_action.then(|| Rc::new(RefCell::new(BTreeMap::new())));
        let mut vm = self
            .realm
            .try_borrow_mut()
            .map_err(|_| format!("槽 {} @ {room}: realm 已在执行", slot.key))?;
        let outcome = vm
            .realm_mut()?
            .invoke_chain(
                slot.key.as_str(),
                Box::new(ModChainContext {
                    ctx: ctx.clone(),
                    id,
                    args,
                    native,
                    cached_fields: None,
                    staged_states: staged_states.clone(),
                    resource_journal: resource_journal.clone(),
                }),
            )
            .map_err(|error| {
                // 引擎仍接收原有字符串错误，另在 VM 生命周期容器中保留类型化证据。
                vm.last_fault
                    .get_or_insert_with(|| crate::ModSettlementFault {
                        tick: ctx.borrow().time,
                        room: room.clone(),
                        target: id.as_str(),
                        error: error.clone(),
                    });
                format!("槽 {} @ {room}: {error}", slot.key)
            })?;
        drop(vm);
        match &outcome {
            SlotOutcome::Completed => {
                if let Some(states) = staged_states {
                    let states = std::mem::take(&mut *states.borrow_mut());
                    for (namespace, value) in states {
                        ctx.borrow().state.set_mod_state(&namespace, value);
                    }
                }
            }
            SlotOutcome::Declined(_) => {
                if let Some(journal) = resource_journal {
                    let mut w = ctx.borrow_mut();
                    for (id, object) in std::mem::take(&mut *journal.borrow_mut()) {
                        w.room.objects.insert(id, object);
                    }
                }
                if let Some(fields) = original_fields {
                    ctx.borrow_mut()
                        .update_mod_fields(id, &fields)
                        .map_err(|error| format!("动作拒绝恢复失败：{error}"))?;
                } else if !resource_action {
                    return Err("该槽没有正常拒绝的局部恢复边界".into());
                }
            }
        }
        Ok(outcome)
    }
}

impl ModChainContext {
    fn fields(&mut self) -> Result<&Value, String> {
        if self.cached_fields.is_none() {
            self.cached_fields = Some(self.load_fields()?);
        }
        Ok(self.cached_fields.as_ref().expect("已加载字段"))
    }

    fn load_fields(&self) -> Result<Value, String> {
        let w = self.ctx.borrow();
        match w.room.objects.get(&self.id) {
            Some(kk_schema::room::RoomObject::Creep(object)) => Ok(json!({
                "name": object.name, "owner": object.owner, "x":object.x,"y":object.y,
                "spawning":object.spawning,"hits":object.hits,"fatigue":object.fatigue,
                "store":kk_world::world_json::store_with_extra_json(&object.store,&object.store_extra),
                "storeCapacity":kk_engine::settle::creep::store_capacity(object),
            })),
            Some(kk_schema::room::RoomObject::Mod(_)) => w
                .mod_fields(self.id)
                .ok_or_else(|| "Mod payload 非法".into()),
            Some(kk_schema::room::RoomObject::Generic(object)) => {
                let fields: Value =
                    serde_json::from_str(&object.data).map_err(|error| error.to_string())?;
                if fields["structureType"] != "link" {
                    return Err("内建目标不是 link".into());
                }
                Ok(fields)
            }
            _ => Err(format!("槽目标 {} 已不存在或未接线", self.id.as_str())),
        }
    }

    fn write_fields(&mut self, fields: &Value) -> Result<(), String> {
        if fields.is_null() {
            return Ok(());
        }
        if !fields.is_object() {
            return Err("target.fields 终值须为对象或 null".into());
        }
        if self.fields()? == fields {
            return Ok(());
        }
        if matches!(
            self.ctx.borrow().room.objects.get(&self.id),
            Some(kk_schema::room::RoomObject::Creep(_))
        ) {
            return Err("creep 资源动作的 target.fields 当前只读".into());
        }
        let is_mod = matches!(
            self.ctx.borrow().room.objects.get(&self.id),
            Some(kk_schema::room::RoomObject::Mod(_))
        );
        if is_mod {
            self.cached_fields = None;
            return self.ctx.borrow_mut().update_mod_fields(self.id, fields);
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

impl ChainHost for ModChainContext {
    fn resource_read(
        &mut self,
        id: &str,
        resource: &str,
    ) -> Result<kk_mod_api::resources::ResourceView, String> {
        self.read_resource(id, resource)
    }

    fn resource_transfer(
        &mut self,
        from: &str,
        to: &str,
        resource: &str,
        amount: i64,
        loss: i64,
    ) -> Result<kk_mod_api::resources::ResourceTransfer, String> {
        self.transfer_resource(from, to, resource, amount, loss)
    }

    fn read(&mut self, binding: &str) -> Result<ChainContext, String> {
        let key = Key::new(binding)?;
        let fields = self.fields()?.clone();
        let w = self.ctx.borrow();
        let state = self
            .staged_states
            .as_ref()
            .and_then(|states| states.borrow().get(key.namespace()).cloned())
            .unwrap_or_else(|| w.state.mod_state(key.namespace()));
        let tick = w.time;
        let room = w.room.name.clone();
        drop(w);
        // 固定封套不再建动态 Map；直接交给 serde_v8 的 struct 路径。
        // 缓存字段与固定 args 仍各自克隆，保持候选隔离。
        Ok(ChainContext {
            tick,
            room,
            state,
            target: ChainTarget {
                id: self.id.as_str(),
                fields,
            },
            args: self.args.clone(),
        })
    }

    fn write(&mut self, binding: &str, context: ChainWrite) -> Result<(), String> {
        let key = Key::new(binding)?;
        if let Some(fields) = context.target_fields.as_ref() {
            self.write_fields(fields)?;
        }
        // 字段校验完成后移交 state；固定封套不再构造或拆除 JSON Map。
        if let Some(states) = &self.staged_states {
            states
                .borrow_mut()
                .insert(key.namespace().to_string(), context.state);
        } else {
            self.ctx
                .borrow()
                .state
                .set_mod_state(key.namespace(), context.state);
        }
        Ok(())
    }

    fn native(&mut self) -> Result<(), String> {
        // native 可改资源、字段或移除目标，返回后的 read 必须重新取候选。
        self.cached_fields = None;
        match &self.native {
            NativeCall::Action { data, step } => step(&mut self.ctx.borrow_mut(), self.id, data),
            NativeCall::Tick(step) => step(&mut self.ctx.borrow_mut(), self.id),
            NativeCall::None => return Err("Mod 自有槽没有 Rust native 中心".into()),
        }
        Ok(())
    }
}
