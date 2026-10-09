//! 装配准入：已接线绑定、意图派发域与生命周期中心。
use crate::settle::{Engine, contrib};
use std::collections::BTreeSet;

impl Engine {
    /// 核对计划的全部声明绑定已有实现：native 键由本引擎内置，
    /// 声明性绑定须出现在已接线消费者清单（见下方常量），处理器绑定
    /// 已在装配时逐键消费。声明未安装的绑定在此拒绝。
    pub(in crate::settle) fn check_installed_bindings(
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
            // isActive 与配额检查读取配额快照（G11.a 起并入配额族）。
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
            "builtin:mod.receiveDamage.native",
            "builtin:room.moveContest.native",
            "builtin:creep.moveAdmission.native",
            "builtin:room.moveObstacle.native",
            "builtin:room.movePrecheck.native",
            "builtin:room.movePullRegistration.native",
            "builtin:room.moveCost.native",
            // 地形通行声明驱动仲裁静态障碍的墙项（Plane 快照）与玩家面
            // 原生寻路地形（SandboxConfig），两端同源消费。
            "builtin:terrain.passability.native",
            "builtin:movement.staticPassability.native",
            "builtin:structure.quota.native",
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
    pub(in crate::settle) fn check_executable_intents(
        plan: &kk_mod_api::ContributionPlan,
    ) -> Result<(), String> {
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
            // 意图描述符级的未接线形态与 typegen 共用同一道门（kk-mod-api
            // intent_gate），声明生成与执行装配不再各自漂移。
            kk_mod_api::intent_gate::executable_gate(intent)?;
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
            let building_lifecycle = plan
                .buildings()
                .iter()
                .any(|b| b.lifecycle_slots.contains(&slot.key));
            if !referenced && !lifecycle && !building_lifecycle {
                return Err(format!(
                    "入口 {} 没有挂载任何动作或自有对象 tick，该槽位的执行接线未开放",
                    slot.key
                ));
            }
        }
        Ok(())
    }

    /// 首版只接自有类型与建筑的具名 tick；不允许借此安装未派发阶段、内建
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
        for building in plan.buildings() {
            for key in &building.lifecycle_slots {
                if key.as_str() != format!("{}.tick", building.key) {
                    return Err(format!(
                        "建筑 {} 只开放自身的 building.tick 生命周期",
                        building.key
                    ));
                }
                let slot = plan
                    .resolve_slot(key)
                    .and_then(|handle| plan.get_slot(handle))
                    .ok_or_else(|| format!("建筑 {} 缺少生命周期槽 {key}", building.key))?;
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
    pub(in crate::settle) fn check_intent_centers(
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
        for building in plan.buildings() {
            for key in &building.lifecycle_slots {
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
}
