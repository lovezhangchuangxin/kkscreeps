//! 将 Mod 整槽链接到房间候选，暂存 state 并校验字段写面。
use crate::mod_trace::{ModTraceEntry, ModTraceEvent, fault_kind_label, stage_label, truncate};
use crate::mod_vm::ModVm;
use kk_engine::settle::RoomCtx;
use kk_engine::settle::contrib::{SlotCall, SlotExecutor};
use kk_mod_api::{Key, SlotOutcome, SlotPlan};
use kk_mod_host::{ChainContext, ChainHost, ChainTarget, ChainWrite};
use kk_world::ObjId;
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;

mod call;
mod capability;
mod combat;
mod completion;
mod fields;
mod movement;
mod objects;
mod resources;
use call::{NativeCall, PreparedCall};
use capability::ChainCapability;
use completion::StagedStates;

/// JS 组合的整槽执行器。进入 VM 一次，native 中心只经普通 op 执行。
pub(super) struct ModSlotExecutor {
    pub(super) realm: Rc<RefCell<ModVm>>,
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
    staged_states: Option<StagedStates>,
}

impl SlotExecutor for ModSlotExecutor {
    fn run(
        &self,
        slot: &SlotPlan,
        ctx: &RoomCtx,
        call: SlotCall<'_>,
    ) -> Result<SlotOutcome, String> {
        let call = PreparedCall::new(slot.key.as_str(), call)?;
        let id = call.id;
        let room = ctx.borrow().room.name.clone();
        let (context, completion) = call.into_context(ctx);
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
        let result = vm
            .realm_mut()?
            .invoke_chain(slot.key.as_str(), Box::new(context));
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
        completion.finish(ctx, &outcome)?;
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
            NativeCall::Preserve => {}
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
            journal.remember_redirects(&w, &touched);
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
