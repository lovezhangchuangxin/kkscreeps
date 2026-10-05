//! 冻结计划安装和可执行绑定的接线校验。
use super::*;

impl Engine {
    /// 核对计划的全部声明绑定已有实现：native 键由本引擎内置，
    /// 声明性绑定须出现在已接线消费者清单（见下方常量），处理器绑定
    /// 已在装配时逐键消费。声明未安装的绑定在此拒绝。
    pub(super) fn check_installed_bindings(
        plan: &kk_mod_api::ContributionPlan,
        handler_keys: &BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        /// 已接线的声明性绑定：每项对应一个真实消费者。
        /// 新增声明绑定时须先接线，或在此登记例外依据。
        const WIRED: &[&str] = &[
            "builtin:store.init",
            "builtin:store.build",
            "builtin:store.constants",
            "builtin:store.labSlots",
            "builtin:store.terminalTick",
            "builtin:store.containerDeposit",
            // 放置与建造默认值经 PlaceCtx.links / Working.links。
            "builtin:link.init",
            "builtin:link.build",
            // Store 容量在放置/建造时落入权威字段。
            "builtin:link.capacity",
            // isActive 与配额检查读取 links.rcl。
            "builtin:link.active",
            // 玩家预检读取权威字段与常量覆盖通道。
            "builtin:link.precheck",
            // 玩家常量经 SandboxConfig 覆盖注入。
            "builtin:link.constants",
            // 投影直接输出权威字段。
            "builtin:link.project",
            // vanilla 参数转换沿用现有意图清洗。
            "builtin:link.sanitize",
            // 字段适配绑定：权威 Generic JSON 的对应键。
            "builtin:link.field.id",
            "builtin:link.field.room",
            "builtin:link.field.x",
            "builtin:link.field.y",
            "builtin:link.field.owner",
            "builtin:link.field.structureType",
            "builtin:link.field.hits",
            "builtin:link.field.hitsMax",
            "builtin:link.field.cooldown",
            "builtin:link.field.store",
            "builtin:link.field.storeCapacity",
            "builtin:link.field.storeCapacityResource",
        ];
        const NATIVE: &[&str] = &[
            "builtin:link.transfer.native",
            "builtin:link.tick.native",
            "builtin:creep.transfer.native",
            "builtin:creep.withdraw.native",
            "builtin:creep.move.native",
            "builtin:creep.attack.native",
            "builtin:creep.rangedAttack.native",
            "builtin:creep.rangedMassAttack.native",
            "builtin:creep.heal.native",
            "builtin:creep.rangedHeal.native",
            "builtin:creep.build.native",
            "builtin:creep.repair.native",
            "builtin:creep.dismantle.native",
            "builtin:creep.receiveDamage.native",
            "builtin:room.moveContest.native",
        ];
        for binding in plan.required_bindings() {
            let name = binding.as_str();
            // 只认显式清单与调用方提供的处理器键：新增声明绑定必须先
            // 接线再进清单，前缀放行会让未接线的 field 键静默通过。
            let wired =
                WIRED.contains(&name) || NATIVE.contains(&name) || handler_keys.contains(binding);
            if !wired {
                return Err(format!("计划声明了未安装的绑定 {binding}"));
            }
        }
        Ok(())
    }

    /// Mod 自有意图的可执行性门：只放行已接线的执行形态，其余在装配期
    /// 显式拒绝而不是静默不执行。豁免只按**意图键**的命名空间（builtin
    /// 意图由 Rust native 执行器消费）——按槽豁免会让 Mod 意图挂 builtin
    /// 槽逃过全部门检查，运行期找不到扩展执行器而成为可重复触发的
    /// 整轮失败。中心检查在调用方（须结合 Replace 处理器实现判断）。
    pub(super) fn check_executable_intents(
        plan: &kk_mod_api::ContributionPlan,
    ) -> Result<(), String> {
        use kk_mod_api::{ArgumentConversion, IntentAuthority, IntentScope, Merge};
        Self::check_object_lifecycles(plan)?;
        for intent in plan.intents() {
            if intent.key.namespace() == "builtin" {
                continue; // 内建意图由 Rust native 执行器消费
            }
            let Some(slot_handle) = plan.resolve_slot(&intent.slot) else {
                return Err(format!(
                    "计划缺少动作 {} 的入口 {}",
                    intent.key, intent.slot
                ));
            };
            let slot = plan.get_slot(slot_handle).expect("句柄来自计划");
            if slot.stage != kk_mod_api::Stage::ObjectAction {
                return Err(format!(
                    "动作 {} 的入口 {} 阶段为 {:?}，扩展意图只在对象动作阶段派发",
                    intent.key, intent.slot, slot.stage
                ));
            }
            if intent.scope != IntentScope::Object {
                return Err(format!(
                    "动作 {} 的 {} 域执行通道未接线（当前只接线对象域）",
                    intent.key,
                    match intent.scope {
                        IntentScope::Room => "房间",
                        IntentScope::Player => "玩家",
                        _ => "世界",
                    }
                ));
            }
            if !matches!(intent.merge, Merge::Last) {
                return Err(format!(
                    "动作 {} 声明队列合并模式，该模式的执行装配未接线",
                    intent.key
                ));
            }
            if !matches!(intent.conversion, ArgumentConversion::Strict) {
                return Err(format!(
                    "动作 {} 声明非严格参数转换，该形态的执行装配未接线",
                    intent.key
                ));
            }
            if intent.authority != IntentAuthority::Player {
                return Err(format!(
                    "动作 {} 声明非玩家授权，服务端发射通道未接线",
                    intent.key
                ));
            }
            if !intent.suppressed_by.is_empty() {
                return Err(format!(
                    "动作 {} 声明压制关系，压制判定的执行装配未接线",
                    intent.key
                ));
            }
        }
        // 非内建槽必须被对象意图或已接线的自有类型生命周期挂载，
        // 否则装配了执行器也没有实际派发点。
        for slot in plan.slots() {
            if contrib::BUILTIN_SLOT_KEYS.contains(&slot.key.as_str()) {
                continue;
            }
            let referenced = plan.intents().iter().any(|intent| intent.slot == slot.key);
            let lifecycle = plan.projection().types().iter().any(|ty| {
                ty.descriptor.key.namespace() != "builtin"
                    && ty.descriptor.lifecycle_slots.contains(&slot.key)
            });
            if !referenced && !lifecycle {
                return Err(format!(
                    "入口 {} 没有挂载任何动作或自有对象 tick，该槽位的执行接线未开放",
                    slot.key
                ));
            }
        }
        Ok(())
    }

    /// 首版只接自有类型的具名 tick；不允许借此安装未派发阶段、内建
    /// 生命周期或多个类型共享的槽，防止声明通过但读取错误对象表示。
    fn check_object_lifecycles(plan: &kk_mod_api::ContributionPlan) -> Result<(), String> {
        for ty in plan.projection().types() {
            let ty = &ty.descriptor;
            if ty.key.namespace() == "builtin" {
                continue;
            }
            for key in &ty.lifecycle_slots {
                if key.as_str() != format!("{}.tick", ty.key) {
                    return Err(format!("类型 {} 只开放自身的 object.tick 生命周期", ty.key));
                }
                let slot = plan
                    .resolve_slot(key)
                    .and_then(|handle| plan.get_slot(handle))
                    .ok_or_else(|| format!("类型 {} 缺少生命周期槽 {key}", ty.key))?;
                if slot.stage != kk_mod_api::Stage::ObjectTick {
                    return Err(format!("生命周期槽 {key} 必须在 ObjectTick 阶段"));
                }
            }
        }
        Ok(())
    }

    /// 中心规则：动作或生命周期中心须是自身 process，或由调用方以
    /// Replace 处理器占位（跨 Mod 替换处理）。多意图共享一槽时
    /// 非自有中心的意图在此拒绝。
    pub(super) fn check_intent_centers(
        plan: &kk_mod_api::ContributionPlan,
        replacements: &BTreeSet<kk_mod_api::Key>,
    ) -> Result<(), String> {
        for intent in plan.intents() {
            if intent.key.namespace() == "builtin" {
                continue;
            }
            let own_process = match kk_mod_api::Key::new(format!("{}.process", intent.key)) {
                Ok(key) => key,
                Err(_) => {
                    return Err(format!(
                        "动作 {} 的键过长，无法派生 process 绑定（键上限 128 字节）",
                        intent.key
                    ));
                }
            };
            let slot_handle = plan
                .resolve_slot(&intent.slot)
                .expect("check_executable_intents 已保证入口存在");
            let slot = plan.get_slot(slot_handle).expect("句柄来自计划");
            // 内建槽由类型化对象动作派发，自有意图由 Mod 对象派发。
            // 即使 Replace 已安装也不能混接，否则创建成功后找不到 ext 执行器。
            if contrib::BUILTIN_SLOT_KEYS.contains(&slot.key.as_str()) {
                return Err(format!(
                    "动作 {} 的入口 {} 中心不是自身 process；扩展意图不能挂载内建派发槽（Replace 也不能改变派发域）",
                    intent.key, slot.key
                ));
            }
            // 冻结后的入口计划只保留 dispatch；Native 绑定或未被占位的
            // Chained 中心即 native 绑定。
            let center = match &slot.dispatch {
                kk_mod_api::Dispatch::Native(binding) => binding,
                kk_mod_api::Dispatch::Chained { center, .. } => center,
            };
            let replaced = replacements.contains(center);
            if center != &own_process && !replaced {
                return Err(format!(
                    "动作 {} 的入口 {} 中心不是自身 process，也不是已提供的 Replace 处理器（多意图共享入口或显式入口的执行装配未接线）",
                    intent.key, intent.slot
                ));
            }
        }
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() == "builtin" {
                continue;
            }
            for key in &ty.descriptor.lifecycle_slots {
                let slot = plan
                    .get_slot(plan.resolve_slot(key).expect("生命周期已校验"))
                    .expect("句柄来自计划");
                let center = match &slot.dispatch {
                    kk_mod_api::Dispatch::Native(binding) => binding,
                    kk_mod_api::Dispatch::Chained { center, .. } => center,
                };
                if center.as_str() != format!("{key}.process") && !replacements.contains(center) {
                    return Err(format!(
                        "生命周期槽 {key} 的中心不是自身 process 或 Replace"
                    ));
                }
            }
        }
        Ok(())
    }

    /// 用冻结计划装配引擎；处理器实现随计划提供，缺失或 kind
    /// 不符时拒绝。计划须包含 builtin:link 贡献。
    pub fn with_plan(
        world: kk_world::World,
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
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
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
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
        plan: std::sync::Arc<kk_mod_api::ContributionPlan>,
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
        let links = kk_mod_api::builtin::LinkRules::from_plan(plan.rules())?;
        let stores = kk_mod_api::builtin::StoreRules::from_plan(plan.rules())?;
        let resources = Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan));
        let buildings = Rc::new(
            kk_mod_api::buildings::BuildingCatalog::from_plan(&plan)
                .map_err(|error| format!("建筑目录装配失败：{error}"))?,
        );
        let contrib = contrib::ContributionDispatch::assemble_with_ext(
            &plan,
            action_handlers,
            tick_handlers,
            ext_handlers,
            ext_processes,
        )?;
        let extensions = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan)?;
        let mut installed = handler_keys;
        installed.extend(extensions.field_bindings().cloned());
        // Mod 类型字段绑定的实现是 payload 存储与写入门（创建/处理器回写/
        // 发布前校验），随计划装配即安装——与扩展目录的安装口径同原则。
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() == "builtin" {
                continue;
            }
            installed.extend(ty.fields.iter().map(|f| f.descriptor.binding.clone()));
        }
        Self::check_installed_bindings(&plan, &installed)?;
        self.links = links;
        self.stores = stores;
        self.resources = resources;
        self.buildings = buildings;
        self.contrib = Rc::new(contrib);
        self.extensions = Rc::new(extensions);
        self.plan = plan;
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
        let links = kk_mod_api::builtin::LinkRules::from_plan(plan.rules())?;
        let stores = kk_mod_api::builtin::StoreRules::from_plan(plan.rules())?;
        let resources = Rc::new(kk_mod_api::resources::ResourceCatalog::from_plan(&plan));
        let buildings = Rc::new(
            kk_mod_api::buildings::BuildingCatalog::from_plan(&plan)
                .map_err(|error| format!("建筑目录装配失败：{error}"))?,
        );
        let contrib = contrib::ContributionDispatch::assemble_hosted(&plan, executor, &bindings)?;
        let extensions = kk_mod_api::extension::ExtensionCatalog::from_plan(&plan)?;
        let mut installed = bindings;
        installed.extend(extensions.field_bindings().cloned());
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() != "builtin" {
                installed.extend(
                    ty.fields
                        .iter()
                        .map(|field| field.descriptor.binding.clone()),
                );
            }
        }
        Self::check_installed_bindings(&plan, &installed)?;
        self.links = links;
        self.stores = stores;
        self.resources = resources;
        self.buildings = buildings;
        self.contrib = Rc::new(contrib);
        self.extensions = Rc::new(extensions);
        self.plan = plan;
        Ok(())
    }
}
