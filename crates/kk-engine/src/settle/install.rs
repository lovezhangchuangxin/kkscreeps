//! 计划安装入口：先准备并校验完整候选，再统一发布。
use crate::settle::{Engine, HostedRegistryGuard, contrib};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

mod candidate;
mod validation;

impl Engine {
    /// 用冻结计划装配引擎；处理器实现随计划提供，缺失或 kind
    /// 不符时拒绝。计划须包含 builtin:link 贡献。
    pub fn with_plan(
        world: kk_world::World,
        plan: Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
    ) -> Result<Self, String> {
        let mut engine = Self::new(world);
        engine.install_plan(plan, action_handlers, tick_handlers)?;
        Ok(engine)
    }

    /// 就地装配冻结计划：换入有效规则、处理链、扩展目录与计划本体，
    /// 保留世界与玩家状态。与 [`Self::with_plan`] 走同一校验；装配
    /// 失败时引擎保持原状。规则与已装配的处理链须一起换，不允许只动其一。
    pub fn install_plan(
        &mut self,
        plan: Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
    ) -> Result<(), String> {
        self.install_plan_with_ext(
            plan,
            action_handlers,
            tick_handlers,
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    /// [`Self::install_plan`] 的完整形态：`ext_handlers` 是 Mod 自有槽上的
    /// 贡献处理器，`ext_processes` 是各槽中心的动作处理实现。
    pub fn install_plan_with_ext(
        &mut self,
        plan: Arc<kk_mod_api::ContributionPlan>,
        action_handlers: BTreeMap<kk_mod_api::Key, contrib::ActionHandler>,
        tick_handlers: BTreeMap<kk_mod_api::Key, contrib::TickHandler>,
        ext_handlers: BTreeMap<kk_mod_api::Key, contrib::ExtHandler>,
        ext_processes: BTreeMap<kk_mod_api::Key, contrib::ExtSimple>,
    ) -> Result<(), String> {
        Self::check_executable_intents(&plan)?;
        Self::check_intent_centers(
            &plan,
            &ext_handlers
                .iter()
                .filter_map(|(key, handler)| {
                    matches!(handler, contrib::ExtHandler::Replace(_)).then_some(key.clone())
                })
                .collect(),
        )?;
        // 先取调用方提供的处理器键，再交给装配消耗。
        let handler_keys: BTreeSet<kk_mod_api::Key> = action_handlers
            .keys()
            .chain(tick_handlers.keys())
            .chain(ext_handlers.keys())
            .chain(ext_processes.keys())
            .cloned()
            .collect();
        let candidate = candidate::Installation::prepare(plan, handler_keys, |plan, _| {
            contrib::ContributionDispatch::assemble_with_ext(
                plan,
                action_handlers,
                tick_handlers,
                ext_handlers,
                ext_processes,
            )
        })?;
        candidate.publish(self);
        Ok(())
    }

    /// 安装整槽宿主执行器，维持同一冻结计划与装配校验。
    /// bindings 是已注册函数，replacements 是其中确认的 Replace 绑定。
    /// 失败时所有装配域保持原状。
    pub fn install_hosted_plan(
        &mut self,
        plan: Arc<kk_mod_api::ContributionPlan>,
        executor: Rc<dyn contrib::SlotExecutor>,
        bindings: BTreeSet<kk_mod_api::Key>,
        replacements: BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        Self::check_executable_intents(&plan)?;
        if !replacements.is_subset(&bindings) {
            return Err("Replace 绑定必须是已安装函数".into());
        }
        Self::check_intent_centers(&plan, &replacements)?;
        let watermark = contrib::hosted_id_watermark();
        let candidate = candidate::Installation::prepare(plan, bindings, |plan, bindings| {
            contrib::ContributionDispatch::assemble_hosted(plan, executor, bindings)
        })?;
        // 本次装配登记的句柄归本引擎；换装先退役旧句柄（旧 realm 随
        // 引用释放销毁）。
        let ids: Vec<u64> = (watermark..contrib::hosted_id_watermark()).collect();
        self.hosted_registry_ids = HostedRegistryGuard(ids);
        candidate.publish(self);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
