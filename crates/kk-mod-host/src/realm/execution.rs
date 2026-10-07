//! 单次处理器与整槽调用；候选能力在返回前清理。
use super::*;

pub(super) struct InstalledChain {
    pub(super) native_center: bool,
    pub(super) decline_allowed: bool,
    pub(super) bindings: Rc<BTreeSet<String>>,
}

impl ModRealm {
    /// 结算期调用一个已注册的处理器函数：`ctx`（tick/room/state 等）以
    /// JSON 传入，处理器同步执行并返回其终值。处理器异常、返回 Promise
    /// 或结果不可序列化都以错误返回——由调用方据此使本轮候选失败。
    /// 调用方负责用返回的 state 写回候选域。
    pub fn invoke(&mut self, binding: &str, ctx: Value) -> ModHostResult<Value> {
        self.with_budget(BudgetPhase::Settlement, |realm| {
            realm.invoke_inner(binding, ctx)
        })
    }

    fn invoke_inner(&mut self, binding: &str, ctx: Value) -> ModHostResult<Value> {
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let dispatch = v8::Local::new(tc, self.sdk_dispatch.clone());
        let key = v8::String::new_from_utf8(tc, binding.as_bytes(), v8::NewStringType::Normal)
            .ok_or_else(|| ModHostError::new("invoke", "键名构造失败"))?;
        let ctx_local = deno_core::serde_v8::to_v8(tc, &ctx).map_err(|error| {
            ModHostError::new("invoke", format!("处理器上下文序列化失败：{error}"))
        })?;
        let receiver = v8::undefined(tc).into();
        let result = dispatch
            .call(tc, receiver, &[key.into(), ctx_local])
            .ok_or_else(|| scope_error!(tc, "invoke", "处理器调用失败"))?;
        crate::v8_convert::from_v8(tc, result)
            .map_err(|error| ModHostError::new("invoke", format!("处理器结果序列化失败：{error}")))
    }

    /// 一次进入 JS 完成整个槽。候选能力仅在本次调用期间放入 OpState，
    /// 无论返回或抛错都取出并销毁；已捕获的 next/候选访问故障仍使调用失败。
    /// Declined 表示正常拒绝；调用方须恢复本次动作候选，保留此前动作结果。
    pub fn invoke_chain(
        &mut self,
        slot: &str,
        host: Box<dyn ChainHost>,
    ) -> ModHostResult<SlotOutcome> {
        let _timer = self.profiler.as_ref().map(|p| p.measure("slot"));
        self.with_budget(BudgetPhase::Settlement, |realm| {
            realm.invoke_chain_inner(slot, host)
        })
        .map_err(|mut error| {
            // 无法定位到处理器的 V8/宿主错误只归槽；预算优先覆盖普通 JS 错误。
            let fault = error.fault.get_or_insert(ModFault {
                slot: None,
                binding: None,
                kind: ModFaultKind::Chain,
            });
            fault.slot = Some(slot.to_string());
            error
        })
    }

    fn invoke_chain_inner(
        &mut self,
        slot: &str,
        host: Box<dyn ChainHost>,
    ) -> ModHostResult<SlotOutcome> {
        // 停用所属 Mod 的自有槽直接丢弃，连健康 Mod 挂在该槽的钩子也不运行。
        if self.inactive_slots.contains(slot) {
            return Ok(SlotOutcome::Completed);
        }
        let program = self
            .chains
            .get(slot)
            .ok_or_else(|| ModHostError::new("chain", format!("槽 {slot} 未安装")))?;
        self.chain_sequence = self
            .chain_sequence
            .checked_add(1)
            .ok_or_else(|| ModHostError::new("chain", "槽调用序号耗尽"))?;
        let token = self.chain_sequence.to_string();
        let active = ActiveChain {
            token: token.clone(),
            slot: slot.to_string(),
            bindings: program.bindings.clone(),
            host,
            fault: None,
            fault_detail_pending: false,
            declined: None,
            decline_allowed: program.decline_allowed,
            native_called: false,
            native_allowed: program.native_center,
            profiler: self.profiler.clone(),
        };
        let op_state = self.runtime.op_state();
        if op_state.borrow().borrow::<ChainState>().active.is_some() {
            return Err(ModHostError::new("chain", "不可重入整槽宿主"));
        }
        op_state.borrow_mut().borrow_mut::<ChainState>().active = Some(active);
        let result = self.call_chain(slot, &token);
        // 尾部清理必须先于向外传播错误，RoomCtx 不得残留到结算的取回阶段。
        let active = op_state
            .borrow_mut()
            .borrow_mut::<ChainState>()
            .active
            .take()
            .expect("本次调用已安装候选能力");
        if let Some(fault) = active.fault {
            return Err(fault);
        }
        result?;
        Ok(active
            .declined
            .map_or(SlotOutcome::Completed, SlotOutcome::Declined))
    }

    fn call_chain(&mut self, slot: &str, token: &str) -> ModHostResult<()> {
        let _timer = self.profiler.as_ref().map(|p| p.measure("js_call"));
        let runtime_ref = &mut self.runtime;
        deno_core::scope!(scope, runtime_ref);
        v8::tc_scope!(let tc, scope);
        let run = v8::Local::new(tc, self.sdk_run_chain.clone());
        let slot =
            v8::String::new(tc, slot).ok_or_else(|| ModHostError::new("chain", "槽键构造失败"))?;
        let token = v8::String::new(tc, token)
            .ok_or_else(|| ModHostError::new("chain", "能力标识构造失败"))?;
        let receiver = v8::undefined(tc).into();
        run.call(tc, receiver, &[slot.into(), token.into()])
            .ok_or_else(|| scope_error!(tc, "chain", "槽调用失败"))?;
        Ok(())
    }
}
