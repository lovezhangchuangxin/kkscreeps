//! 将 Mod 整槽链接到房间候选，暂存 state 并校验字段写面。
use super::*;
use crate::mod_trace::{ModTraceEntry, ModTraceEvent, fault_kind_label, stage_label, truncate};

mod capability;
mod combat;
mod fields;
mod movement;
mod objects;
mod resources;
use capability::ChainCapability;

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
    /// 默认 TOUGH 已在入链前准备；中心保留数值，避免擦掉 before 的调整。
    Incoming,
    /// 移动竞争的默认中心：保留已设置的优先级，无默认计算可执行。
    Contest,
    /// 移动准入的默认中心：保留候选中的原生准入终值，无计算可执行。
    Admission,
    /// 移动障碍的默认中心：无追加阻挡，无计算可执行。
    Obstacle,
    /// 移动预检的默认中心：无否决，无计算可执行。
    Precheck,
    /// 拉拽登记的默认中心：无否决，无计算可执行。
    PullRegistration,
    /// 移动代价的默认中心：无覆盖，执行原生代价。
    MoveCost,
    None,
}

struct ModChainContext {
    ctx: RoomCtx,
    id: ObjId,
    args: Value,
    native: NativeCall,
    /// 仅在本次能力内缓存已解析字段；改写与 native 前失效，不跨槽/对象/tick。
    cached_fields: Option<Value>,
    /// 能力和其领域状态不可分离；同一槽只装配一个领域。
    capability: ChainCapability,
    /// 可拒绝动作的 namespace 写入暂存到调用成功；拒绝时丢弃。
    staged_states: Option<Rc<RefCell<BTreeMap<String, Value>>>>,
}

/// 战斗链的运行时状态与首触日志。日志记录能力改向触达、且不在引擎
/// 预计算受影响集内的对象原始记录与延迟队列条目；拒绝时恢复。
#[derive(Clone)]
struct CombatChainState {
    action: kk_engine::settle::creep::CombatAction,
    consumed: bool,
    retaliation: Option<kk_engine::settle::creep::RetaliationPolicy>,
    journal: Rc<RefCell<BTreeMap<ObjId, RedirectJournalEntry>>>,
}

/// 建造族链的运行时状态与首触日志，与战斗链同型；触达对象不含延迟
/// 队列写入面（建造族目标均为工地/结构），条目仍按同构保存。
#[derive(Clone)]
struct BuildChainState {
    action: kk_engine::settle::creep::BuildAction,
    consumed: bool,
    journal: Rc<RefCell<BTreeMap<ObjId, RedirectJournalEntry>>>,
}

struct RedirectJournalEntry {
    object: kk_schema::room::RoomObject,
    damage: Option<i64>,
    heal: Option<i64>,
}

impl SlotExecutor for ModSlotExecutor {
    fn run(
        &self,
        slot: &SlotPlan,
        ctx: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<SlotOutcome, String> {
        // 自有对象变换与额外资源对象由此适配器恢复；内建资源动作的
        // 原始两端/事件由引擎在整链外恢复；移动槽的请求登记与意图
        // 疲劳同样由引擎按执行者粒度恢复。namespace 仅成功后移交。
        let extension = matches!(&call, SlotCall::Extension { .. });
        let incoming = match &call {
            SlotCall::Incoming { candidate, .. } => Some(candidate.clone()),
            _ => None,
        };
        let contest = match &call {
            SlotCall::Contest { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let admission = match &call {
            SlotCall::Admission { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let obstacle = match &call {
            SlotCall::Obstacle { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let precheck = match &call {
            SlotCall::Precheck { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let pull_registration = match &call {
            SlotCall::PullRegistration { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let move_cost = match &call {
            SlotCall::MoveCost { candidate } => Some(candidate.clone()),
            _ => None,
        };
        let kind = kk_mod_api::builtin_capabilities::SlotCapability::for_key(slot.key.as_str());
        let is_action = matches!(&call, SlotCall::Action { .. });
        let resource_action =
            is_action && kind == Some(kk_mod_api::builtin_capabilities::SlotCapability::Resource);
        let move_action =
            is_action && kind == Some(kk_mod_api::builtin_capabilities::SlotCapability::Move);
        let combat_action =
            is_action && kind == Some(kk_mod_api::builtin_capabilities::SlotCapability::Combat);
        let build_action =
            is_action && kind == Some(kk_mod_api::builtin_capabilities::SlotCapability::Build);
        let (id, args, native) = match call {
            SlotCall::Incoming { id, candidate } => (
                id,
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Incoming,
            ),
            SlotCall::Contest { candidate } => (
                // 房间级槽没有对象目标；u64::MAX 不与真实对象 ID 冲突，
                // 仅作为故障诊断中的占位 target。
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Contest,
            ),
            SlotCall::Admission { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Admission,
            ),
            SlotCall::Obstacle { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Obstacle,
            ),
            SlotCall::Precheck { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::Precheck,
            ),
            SlotCall::PullRegistration { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::PullRegistration,
            ),
            SlotCall::MoveCost { candidate } => (
                ObjId(u64::MAX),
                serde_json::to_value(candidate.input()).map_err(|e| e.to_string())?,
                NativeCall::MoveCost,
            ),
            SlotCall::Action { id, data, native } => (
                id,
                if move_action {
                    // move 槽固定参数面：方向或用于求方向的相邻 creep。
                    json!({"id": data.id.map(|id| id.as_str()), "direction": data.direction})
                } else {
                    json!({
                        "id": data.id.map(|id| id.as_str()), "amount": data.amount, "resourceType": data.resource_type,
                    })
                },
                NativeCall::Action {
                    data: Box::new(data.clone()),
                    step: native,
                },
            ),
            SlotCall::Tick { id, native } => (id, Value::Null, NativeCall::Tick(native)),
            SlotCall::Extension { id, args } => (id, args.clone(), NativeCall::None),
        };
        let room = ctx.borrow().room.name.clone();
        let object_snapshot = extension.then(|| ctx.borrow_mut().snapshot_mod_objects());
        let staged_states = (extension
            || resource_action
            || move_action
            || combat_action
            || build_action
            || incoming.is_some()
            || contest.is_some()
            || admission.is_some()
            || obstacle.is_some()
            || precheck.is_some()
            || pull_registration.is_some()
            || move_cost.is_some())
        .then(|| Rc::new(RefCell::new(BTreeMap::new())));
        let resource_journal = resource_action.then(|| Rc::new(RefCell::new(BTreeMap::new())));
        // 外层副本仅供 Declined 分支读共享 journal 与 is_some() 判定；
        // consumed/action 以链内上下文的副本为唯一权威（外层恒为初值）。
        let combat_state = combat_action.then(|| CombatChainState {
            action: kk_engine::settle::creep::CombatAction::from_slot_key(slot.key.as_str())
                .expect("战斗槽键已由 COMBAT_ACTION_SLOTS 判定"),
            consumed: false,
            retaliation: None,
            journal: Rc::new(RefCell::new(BTreeMap::new())),
        });
        let build_state = build_action.then(|| BuildChainState {
            action: kk_engine::settle::creep::BuildAction::from_slot_key(slot.key.as_str())
                .expect("建造族槽键已由 BUILD_ACTION_SLOTS 判定"),
            consumed: false,
            journal: Rc::new(RefCell::new(BTreeMap::new())),
        });
        let capability = if extension {
            ChainCapability::Objects
        } else if let Some(journal) = &resource_journal {
            ChainCapability::Resource(journal.clone())
        } else if move_action {
            ChainCapability::Move
        } else if let Some(state) = &combat_state {
            ChainCapability::Combat(state.clone())
        } else if let Some(state) = &build_state {
            ChainCapability::Build(state.clone())
        } else if let Some(state) = &incoming {
            ChainCapability::Incoming(state.clone())
        } else if let Some(state) = &contest {
            ChainCapability::Contest(state.clone())
        } else if let Some(state) = &admission {
            ChainCapability::Admission(state.clone())
        } else if let Some(state) = &obstacle {
            ChainCapability::Obstacle(state.clone())
        } else if let Some(state) = &precheck {
            ChainCapability::Precheck(state.clone())
        } else if let Some(state) = &pull_registration {
            ChainCapability::PullRegistration(state.clone())
        } else if let Some(state) = &move_cost {
            ChainCapability::MoveCost(state.clone())
        } else {
            ChainCapability::Plain
        };
        let mut vm = self
            .realm
            .try_borrow_mut()
            .map_err(|_| format!("槽 {} @ {room}: realm 已在执行", slot.key))?;
        let stage = stage_label(slot.stage);
        // trace 关闭时不构造任何条目（含字符串克隆/格式化）；tick 与
        // "realm 已在执行"/"待恢复"早退路径不记录（槽未进 VM，无结果
        // 可言）。
        let tracing = vm.trace_enabled();
        // 定位四元组只在开启时物化；记录点按需克隆产出条目。
        let trace_parts: Option<(u64, String, String)> = tracing.then(|| {
            (
                ctx.borrow().time,
                id.as_str().to_string(),
                slot.key.to_string(),
            )
        });
        let result = vm.realm_mut()?.invoke_chain(
            slot.key.as_str(),
            Box::new(ModChainContext {
                ctx: ctx.clone(),
                id,
                args,
                native,
                cached_fields: None,
                staged_states: staged_states.clone(),
                capability,
            }),
        );
        let outcome = match result {
            Ok(outcome) => {
                if let Some((tick, target, slot_key)) = &trace_parts {
                    let event = match &outcome {
                        // 停用命名空间的槽被 realm 丢弃而非执行（invoke_chain
                        // 对 inactive 槽恒返 Completed），与真实完成区分。
                        SlotOutcome::Completed => {
                            if vm.slot_inactive(slot.key.as_str()) {
                                ModTraceEvent::SlotInactive
                            } else {
                                ModTraceEvent::SlotCompleted
                            }
                        }
                        SlotOutcome::Declined(decline) => ModTraceEvent::SlotDeclined {
                            binding: decline.binding.to_string(),
                            code: truncate(&decline.code),
                        },
                    };
                    vm.record_trace(ModTraceEntry {
                        tick: *tick,
                        room: room.clone(),
                        target: target.clone(),
                        slot: slot_key.clone(),
                        stage,
                        event,
                    });
                }
                outcome
            }
            Err(error) => {
                if let Some((tick, target, slot_key)) = &trace_parts {
                    // 与 mod_fault 报告的 serde 名同源（fault_kind_label）；
                    // 无结构化来源的基础设施错误统一记 "chain"。
                    let event = ModTraceEvent::SlotFault {
                        binding: error
                            .fault
                            .as_ref()
                            .and_then(|fault| fault.binding.as_ref())
                            .map(|binding| binding.to_string()),
                        kind: error
                            .fault
                            .as_ref()
                            .map(|fault| fault_kind_label(fault.kind))
                            .unwrap_or("chain"),
                        error: truncate(&error.message),
                    };
                    vm.record_trace(ModTraceEntry {
                        tick: *tick,
                        room: room.clone(),
                        target: target.clone(),
                        slot: slot_key.clone(),
                        stage,
                        event,
                    });
                }
                // 引擎仍接收原有字符串错误，另在 VM 生命周期容器中保留类型化证据。
                vm.last_fault
                    .get_or_insert_with(|| crate::ModSettlementFault {
                        tick: ctx.borrow().time,
                        room: room.clone(),
                        target: id.as_str().to_string(),
                        error: error.clone(),
                    });
                return Err(format!("槽 {} @ {room}: {error}", slot.key));
            }
        };
        drop(vm);
        match &outcome {
            SlotOutcome::Completed => {
                if object_snapshot.is_some() {
                    ctx.borrow_mut().finish_chain_snapshot();
                }
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
                // 战斗与建造族互斥（每槽至多一个能力面），恢复顺序无关。
                for journal in [
                    combat_state.as_ref().map(|state| &state.journal),
                    build_state.as_ref().map(|state| &state.journal),
                ]
                .into_iter()
                .flatten()
                {
                    replay_redirect_journal(ctx, journal);
                }
                if let Some(snapshot) = object_snapshot {
                    ctx.borrow_mut().restore_chain_snapshot(snapshot);
                } else if !(resource_action
                    || move_action
                    || combat_state.is_some()
                    || build_state.is_some()
                    || incoming.is_some()
                    || contest.is_some()
                    || admission.is_some()
                    || obstacle.is_some()
                    || precheck.is_some()
                    || pull_registration.is_some()
                    || move_cost.is_some())
                {
                    return Err("该槽没有正常拒绝的局部恢复边界".into());
                }
            }
        }
        Ok(outcome)
    }
}

impl ChainHost for ModChainContext {
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
        // 战斗/建造族槽：能力改向/取消已消耗本动作时跳过 native，防止二次执行。
        if let Some(combat) = self.capability.combat_mut()
            && combat.consumed
        {
            return Ok(());
        }
        if let Some(build) = self.capability.build_mut()
            && build.consumed
        {
            return Ok(());
        }
        match &self.native {
            NativeCall::Action { data, step } => {
                if let Some(combat) = self.capability.combat()
                    && let Some(policy) = &combat.retaliation
                {
                    kk_engine::settle::creep::slot_combat_execute_with_retaliation(
                        &mut self.ctx.borrow_mut(),
                        self.id,
                        data,
                        combat.action,
                        policy,
                    )?;
                } else {
                    step(&mut self.ctx.borrow_mut(), self.id, data);
                }
            }
            NativeCall::Tick(step) => step(&mut self.ctx.borrow_mut(), self.id),
            NativeCall::Incoming => {}
            NativeCall::Contest => {}
            NativeCall::Admission => {}
            NativeCall::Obstacle => {}
            NativeCall::PullRegistration => {}
            NativeCall::MoveCost => {}
            NativeCall::Precheck => {}
            NativeCall::None => return Err("Mod 自有槽没有 Rust native 中心".into()),
        }
        if let Some(combat) = self.capability.combat_mut() {
            combat.consumed = true;
        }
        if let Some(build) = self.capability.build_mut() {
            build.consumed = true;
        }
        Ok(())
    }
}

impl kk_mod_host::ObjectsHost for ModChainContext {
    fn object_get(&mut self, id: &str) -> Result<Option<kk_mod_api::objects::ObjectView>, String> {
        self.get_object(id)
    }
    fn object_list(
        &mut self,
        type_key: &str,
        after: &str,
        limit: u32,
    ) -> Result<kk_mod_api::objects::ObjectPage, String> {
        self.list_objects(type_key, after, limit)
    }
    fn object_create(
        &mut self,
        binding: &str,
        type_key: &str,
        fields: &Value,
    ) -> Result<kk_mod_api::objects::ObjectView, String> {
        self.create_object(binding, type_key, fields)
    }
    fn object_update(
        &mut self,
        binding: &str,
        id: &str,
        fields: &Value,
    ) -> Result<kk_mod_api::objects::ObjectView, String> {
        self.update_object(binding, id, fields)
    }
    fn object_remove(&mut self, binding: &str, id: &str) -> Result<(), String> {
        self.remove_object(binding, id)
    }
}

impl kk_mod_host::ResourcesHost for ModChainContext {
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
}

impl kk_mod_host::CombatHost for ModChainContext {
    fn incoming_damage(&mut self) -> Result<i64, String> {
        self.capability
            .incoming()
            .map(|c| c.damage())
            .ok_or_else(|| "本次槽没有受击数值能力".into())
    }
    fn incoming_set_damage(&mut self, amount: i64) -> Result<(), String> {
        self.capability
            .incoming()
            .ok_or("本次槽没有受击数值能力")?
            .set_damage(amount)
    }
    fn combat_request(&mut self, target: &str) -> Result<(), String> {
        self.request_combat(target)
    }
    fn combat_cancel(&mut self) -> Result<(), String> {
        self.cancel_combat()
    }
    fn combat_apply_damage(&mut self, target: &str, amount: i64) -> Result<(), String> {
        self.apply_combat_damage(target, amount)
    }
    fn combat_apply_heal(&mut self, target: &str, amount: i64) -> Result<(), String> {
        self.apply_combat_heal(target, amount)
    }
    fn combat_set_retaliation(
        &mut self,
        requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        self.set_combat_retaliation(requests)
    }
    fn combat_apply_damage_batch(
        &mut self,
        requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        self.apply_combat_damage_batch(requests)
    }
    fn combat_list(
        &mut self,
        after: &str,
        limit: u32,
    ) -> Result<kk_mod_api::combat::CombatPage, String> {
        if self.capability.combat().is_none() && self.capability.incoming().is_none() {
            return Err("本次槽没有战斗读取能力".into());
        }
        kk_engine::settle::creep::slot_combat_list(&self.ctx.borrow(), after, limit)
    }
    fn combat_get(
        &mut self,
        target: &str,
    ) -> Result<Option<kk_mod_api::combat::CombatView>, String> {
        if self.capability.combat().is_none() && self.capability.incoming().is_none() {
            return Err("本次槽没有战斗读取能力".into());
        }
        let id = ObjId::parse(target).ok_or("战斗目标 ID 非法")?;
        Ok(kk_engine::settle::creep::slot_combat_get(
            &self.ctx.borrow(),
            id,
        ))
    }
}

impl kk_mod_host::BuildHost for ModChainContext {
    fn build_request(&mut self, target: &str) -> Result<(), String> {
        let (action, journal) = {
            let Some(build) = self.capability.build() else {
                return Err("本次槽没有建造目标改向能力".into());
            };
            if build.consumed {
                return Err("本动作的建造能力已消耗（改向/取消/执行恰一次）".into());
            }
            (build.action, build.journal.clone())
        };
        let target_id = ObjId::parse(target).ok_or("建造目标 ID 非法")?;
        let touched =
            kk_engine::settle::creep::build_touch_targets(&self.ctx.borrow(), target_id, action);
        {
            let mut w = self.ctx.borrow_mut();
            let mut journal = journal.borrow_mut();
            for id in &touched {
                if !journal.contains_key(id)
                    && let Some(object) = w.room.objects.get(id).cloned()
                {
                    journal.insert(
                        *id,
                        RedirectJournalEntry {
                            object,
                            damage: w.damage_to_apply.get(id).copied(),
                            heal: w.heal_to_apply.get(id).copied(),
                        },
                    );
                }
            }
            kk_engine::settle::creep::slot_build_request(&mut w, self.id, target_id, action)?;
        }
        self.cached_fields = None;
        if let Some(build) = self.capability.build_mut() {
            build.consumed = true;
        }
        Ok(())
    }
    fn build_cancel(&mut self) -> Result<(), String> {
        let Some(build) = self.capability.build_mut() else {
            return Err("本次槽没有建造能力".into());
        };
        if build.consumed {
            return Err("本动作的建造能力已消耗（改向/取消/执行恰一次）".into());
        }
        build.consumed = true;
        Ok(())
    }
}

/// 拒绝时按首触日志恢复能力改向触达的对象与延迟队列条目；在引擎
/// 边界之前执行，引擎快照随后落笔。journal 的 request 前值能当链前值
/// 用，依赖两个前提：consumed 使 native 与改向互斥（至多一次真实
/// 执行），且链内对这些对象没有其他候选写通道（creep 字段只读、
/// 资源/移动能力默认拒绝）；给这两族槽开放新写能力时须重审。
fn replay_redirect_journal(
    ctx: &RoomCtx,
    journal: &Rc<RefCell<BTreeMap<ObjId, RedirectJournalEntry>>>,
) {
    let mut w = ctx.borrow_mut();
    for (id, entry) in std::mem::take(&mut *journal.borrow_mut()) {
        w.room.objects.insert(id, entry.object);
        match entry.damage {
            Some(v) => {
                w.damage_to_apply.insert(id, v);
            }
            None => {
                w.damage_to_apply.remove(&id);
            }
        }
        match entry.heal {
            Some(v) => {
                w.heal_to_apply.insert(id, v);
            }
            None => {
                w.heal_to_apply.remove(&id);
            }
        }
    }
}
