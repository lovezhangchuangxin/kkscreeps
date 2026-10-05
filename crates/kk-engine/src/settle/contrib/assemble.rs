//! 按冻结槽分配处理器，校验并装配 native 或贡献链。
use super::*;

impl ContributionDispatch {
    /// 装配整槽宿主执行器。冻结计划中的可执行绑定必须由宿主逐项确认；
    /// 未扩展的内建槽保留 native 直调，Mod 自有槽交给宿主执行 JS 中心。
    pub fn assemble_hosted(
        plan: &ContributionPlan,
        executor: Rc<dyn SlotExecutor>,
        bindings: &BTreeSet<Key>,
    ) -> Result<Self, String> {
        for slot in plan.slots() {
            let (before, wraps, center, after) = match &slot.dispatch {
                Dispatch::Native(center) => (&[][..], &[][..], center, &[][..]),
                Dispatch::Chained {
                    before,
                    wraps,
                    center,
                    after,
                } => (
                    before.as_slice(),
                    wraps.as_slice(),
                    center,
                    after.as_slice(),
                ),
            };
            for binding in before
                .iter()
                .chain(wraps)
                .chain(std::iter::once(center))
                .chain(after)
            {
                if binding.namespace() != "builtin" && !bindings.contains(binding) {
                    return Err(missing(slot.key.as_str(), binding, "宿主"));
                }
                if binding.namespace() == "builtin"
                    && !(BUILTIN_SLOT_KEYS.contains(&slot.key.as_str())
                        && binding.as_str() == format!("{}.native", slot.key))
                {
                    return Err(format!("槽 {} 的内建绑定 {} 未接线", slot.key, binding));
                }
            }
        }
        let host = |slot: &str| -> Result<HostedSlot, String> {
            let handle = plan
                .resolve_slot(&key(slot))
                .ok_or_else(|| format!("计划缺少槽 {slot}"))?;
            Ok(HostedSlot {
                plan: Rc::new(plan.get_slot(handle).expect("句柄来自计划").clone()),
                executor: executor.clone(),
            })
        };
        let transfer = match dispatch_of(plan, LINK_SLOT_KEYS[0])? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:link.transfer.native" => {
                ActionSlot::Native(crate::settle::structures::link_transfer)
            }
            _ => ActionSlot::Hosted {
                slot: host(LINK_SLOT_KEYS[0])?,
                native: crate::settle::structures::link_transfer,
            },
        };
        let tick = match dispatch_of(plan, LINK_SLOT_KEYS[1])? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:link.tick.native" => {
                TickSlot::Native(crate::settle::economy::link_cooldown_tick)
            }
            _ => TickSlot::Hosted(host(LINK_SLOT_KEYS[1])?),
        };
        let incoming = match dispatch_of(plan, kk_mod_api::builtin::INCOMING_DAMAGE_SLOT)? {
            Dispatch::Native(binding)
                if binding.as_str() == "builtin:creep.receiveDamage.native" =>
            {
                IncomingSlot::Native
            }
            _ => IncomingSlot::Hosted(host(kk_mod_api::builtin::INCOMING_DAMAGE_SLOT)?),
        };
        let move_contest = match dispatch_of(plan, kk_mod_api::builtin::MOVE_CONTEST_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.moveContest.native" => {
                super::contest::ContestSlot::Native
            }
            _ => super::contest::ContestSlot::Hosted(host(kk_mod_api::builtin::MOVE_CONTEST_SLOT)?),
        };
        let move_admission = match dispatch_of(plan, kk_mod_api::builtin::MOVE_ADMISSION_SLOT)? {
            Dispatch::Native(binding)
                if binding.as_str() == "builtin:creep.moveAdmission.native" =>
            {
                super::admission::AdmissionSlot::Native
            }
            _ => super::admission::AdmissionSlot::Hosted(host(
                kk_mod_api::builtin::MOVE_ADMISSION_SLOT,
            )?),
        };
        let move_obstacle = match dispatch_of(plan, kk_mod_api::builtin::MOVE_OBSTACLE_SLOT)? {
            Dispatch::Native(binding) if binding.as_str() == "builtin:room.moveObstacle.native" => {
                super::obstacle::ObstacleSlot::Native
            }
            _ => super::obstacle::ObstacleSlot::Hosted(host(
                kk_mod_api::builtin::MOVE_OBSTACLE_SLOT,
            )?),
        };
        let mut ext = BTreeMap::new();
        let action = |slot: &str, native| -> Result<ActionSlot, String> {
            match dispatch_of(plan, slot)? {
                Dispatch::Native(binding) if binding.as_str() == format!("{slot}.native") => {
                    Ok(ActionSlot::Native(native))
                }
                _ => Ok(ActionSlot::Hosted {
                    slot: host(slot)?,
                    native,
                }),
            }
        };
        let creep_transfer = action(
            "builtin:creep.transfer",
            crate::settle::creep::intent_transfer,
        )?;
        let creep_withdraw = action(
            "builtin:creep.withdraw",
            crate::settle::creep::intent_withdraw,
        )?;
        let creep_move = action("builtin:creep.move", crate::settle::creep::intent_move)?;
        let creep_attack = action("builtin:creep.attack", crate::settle::creep::intent_attack)?;
        let creep_ranged_attack = action(
            "builtin:creep.rangedAttack",
            crate::settle::creep::intent_ranged_attack,
        )?;
        let creep_ranged_mass_attack = action(
            "builtin:creep.rangedMassAttack",
            crate::settle::creep::ranged_mass_native,
        )?;
        let creep_heal = action(
            "builtin:creep.heal",
            crate::settle::creep::heal_melee_native,
        )?;
        let creep_ranged_heal = action(
            "builtin:creep.rangedHeal",
            crate::settle::creep::heal_ranged_native,
        )?;
        let creep_build = action("builtin:creep.build", crate::settle::creep::intent_build)?;
        let creep_repair = action("builtin:creep.repair", crate::settle::creep::intent_repair)?;
        let creep_dismantle = action(
            "builtin:creep.dismantle",
            crate::settle::creep::intent_dismantle,
        )?;
        for slot in plan.slots() {
            if !BUILTIN_SLOT_KEYS.contains(&slot.key.as_str()) {
                ext.insert(slot.key.clone(), ExtSlot::Hosted(host(slot.key.as_str())?));
            }
        }
        Ok(Self {
            transfer,
            tick,
            creep_transfer,
            creep_withdraw,
            creep_move,
            creep_attack,
            creep_ranged_attack,
            creep_ranged_mass_attack,
            creep_heal,
            creep_ranged_heal,
            creep_build,
            creep_repair,
            creep_dismantle,
            incoming,
            move_contest,
            move_admission,
            move_obstacle,
            ext,
        })
    }

    /// 按冻结计划装配执行链。native 键固定指向内建实现；
    /// 处理器键必须在 `action_handlers` / `tick_handlers` 中提供，
    /// kind 与描述不符、或键缺失时装配失败。
    pub fn assemble(
        plan: &ContributionPlan,
        action_handlers: BTreeMap<Key, ActionHandler>,
        tick_handlers: BTreeMap<Key, TickHandler>,
    ) -> Result<Self, String> {
        Self::assemble_with_ext(
            plan,
            action_handlers,
            tick_handlers,
            BTreeMap::new(),
            BTreeMap::new(),
        )
    }

    /// [`Self::assemble`] 的完整形态：`ext_handlers` 是 Mod 自有槽上的
    /// before/wrap/replace/after 贡献处理器，`ext_processes` 是各槽中心
    /// 的动作处理实现（键为槽的 native 绑定，通常是 `{intent}.process`）。
    /// 扩展槽的 wrap 处理器同样装配成链，语义与 link 槽一致（`next()`
    /// 至多一次，续行到剩余 wrap 与中心，成功后才跑 after）。装配门不在
    /// 此处：JS 宿主经 assemble_hosted 安装整槽执行器。
    pub fn assemble_with_ext(
        plan: &ContributionPlan,
        mut action_handlers: BTreeMap<Key, ActionHandler>,
        tick_handlers: BTreeMap<Key, TickHandler>,
        ext_handlers: BTreeMap<Key, ExtHandler>,
        ext_processes: BTreeMap<Key, ExtSimple>,
    ) -> Result<Self, String> {
        let transfer = assemble_action(
            plan,
            "builtin:link.transfer",
            "builtin:link.transfer.native",
            take_action_handlers(plan, "builtin:link.transfer", &mut action_handlers)?,
            crate::settle::structures::link_transfer,
        )?;
        let creep_transfer = assemble_action(
            plan,
            "builtin:creep.transfer",
            "builtin:creep.transfer.native",
            take_action_handlers(plan, "builtin:creep.transfer", &mut action_handlers)?,
            crate::settle::creep::intent_transfer,
        )?;
        let creep_withdraw = assemble_action(
            plan,
            "builtin:creep.withdraw",
            "builtin:creep.withdraw.native",
            take_action_handlers(plan, "builtin:creep.withdraw", &mut action_handlers)?,
            crate::settle::creep::intent_withdraw,
        )?;
        let creep_move = assemble_action(
            plan,
            "builtin:creep.move",
            "builtin:creep.move.native",
            take_action_handlers(plan, "builtin:creep.move", &mut action_handlers)?,
            crate::settle::creep::intent_move,
        )?;
        let mut combat = |slot: &str,
                          native: fn(
            &mut crate::settle::Working,
            kk_world::ObjId,
            &crate::intent::IntentData,
        )|
         -> Result<ActionSlot, String> {
            assemble_action(
                plan,
                slot,
                &format!("{slot}.native"),
                take_action_handlers(plan, slot, &mut action_handlers)?,
                native,
            )
        };
        let creep_attack = combat("builtin:creep.attack", crate::settle::creep::intent_attack)?;
        let creep_ranged_attack = combat(
            "builtin:creep.rangedAttack",
            crate::settle::creep::intent_ranged_attack,
        )?;
        let creep_ranged_mass_attack = combat(
            "builtin:creep.rangedMassAttack",
            crate::settle::creep::ranged_mass_native,
        )?;
        let creep_heal = combat(
            "builtin:creep.heal",
            crate::settle::creep::heal_melee_native,
        )?;
        let creep_ranged_heal = combat(
            "builtin:creep.rangedHeal",
            crate::settle::creep::heal_ranged_native,
        )?;
        let creep_build = combat("builtin:creep.build", crate::settle::creep::intent_build)?;
        let creep_repair = combat("builtin:creep.repair", crate::settle::creep::intent_repair)?;
        let creep_dismantle = combat(
            "builtin:creep.dismantle",
            crate::settle::creep::intent_dismantle,
        )?;
        reject_leftover("内建动作", &action_handlers)?;
        let tick = assemble_tick(
            plan,
            "builtin:link.tick",
            "builtin:link.tick.native",
            tick_handlers,
        )?;
        let ext = assemble_ext_slots(plan, ext_handlers, ext_processes)?;
        let incoming = IncomingSlot::native_from_plan(plan)?;
        let move_contest = super::contest::ContestSlot::native_from_plan(plan)?;
        let move_admission = super::admission::AdmissionSlot::native_from_plan(plan)?;
        let move_obstacle = super::obstacle::ObstacleSlot::native_from_plan(plan)?;
        Ok(Self {
            transfer,
            tick,
            creep_transfer,
            creep_withdraw,
            creep_move,
            creep_attack,
            creep_ranged_attack,
            creep_ranged_mass_attack,
            creep_heal,
            creep_ranged_heal,
            creep_build,
            creep_repair,
            creep_dismantle,
            incoming,
            move_contest,
            move_admission,
            move_obstacle,
            ext,
        })
    }
}

/// 装配计划中全部非内建槽的扩展执行链。每个槽的中心取其
/// native 绑定的动作处理实现；贡献处理器按冻结链位接入。
/// 冻结后的 SlotPlan 不保留 native 绑定：Native 分派的绑定即中心，
/// Chained 的中心在未被 replace 占位时等于 native 绑定。
fn assemble_ext_slots(
    plan: &ContributionPlan,
    mut handlers: BTreeMap<Key, ExtHandler>,
    mut processes: BTreeMap<Key, ExtSimple>,
) -> Result<BTreeMap<Key, ExtSlot>, String> {
    let mut result = BTreeMap::new();
    for slot in plan.slots() {
        if BUILTIN_SLOT_KEYS.contains(&slot.key.as_str()) {
            continue;
        }
        match &slot.dispatch {
            Dispatch::Native(binding) => {
                let center = processes
                    .remove(binding)
                    .ok_or_else(|| missing(&slot.key.to_string(), binding, "动作处理"))?;
                result.insert(slot.key.clone(), ExtSlot::Process(Rc::new(center)));
            }
            Dispatch::Chained {
                before,
                wraps,
                center,
                after,
            } => {
                let slot_label = slot.key.to_string();
                let mut before_fns = Vec::new();
                for binding in before {
                    match handlers.remove(binding) {
                        Some(ExtHandler::Before(f)) => before_fns.push(Rc::new(f)),
                        Some(other) => {
                            return Err(kind_mismatch(binding, "before", other.label()));
                        }
                        None => return Err(missing(&slot_label, binding, "before")),
                    }
                }
                // wrap 处理器在扩展槽同样装配：它们由宿主桥经
                // Rust 的局部续行执行；JS 组合经整槽宿主执行器。
                let mut wrap_fns = Vec::new();
                for binding in wraps.iter() {
                    match handlers.remove(binding) {
                        Some(ExtHandler::Wrap(f)) => wrap_fns.push(Rc::new(f)),
                        Some(other) => {
                            return Err(kind_mismatch(binding, "wrap", other.label()));
                        }
                        None => return Err(missing(&slot_label, binding, "wrap")),
                    }
                }
                let after_fns = take_after(&slot_label, after, &mut handlers)?;
                let center_fn = match handlers.remove(center) {
                    Some(ExtHandler::Replace(f)) => Rc::new(f),
                    Some(other) => {
                        return Err(kind_mismatch(center, "replace", other.label()));
                    }
                    None => Rc::new(
                        processes
                            .remove(center)
                            .ok_or_else(|| missing(&slot_label, center, "replace/动作处理"))?,
                    ),
                };
                result.insert(
                    slot.key.clone(),
                    ExtSlot::Chained(Rc::new(ExtChained {
                        before: before_fns,
                        wraps: wrap_fns,
                        center: center_fn,
                        after: after_fns,
                    })),
                );
            }
        }
    }
    // 计划未引用的键是装配调用方的键名错误；静默丢弃会掩盖它。
    if let Some(binding) = handlers.keys().next() {
        return Err(format!("装配提供了计划未引用的扩展处理器 {binding}"));
    }
    // 处理实现的残余只有一种合法形态：动作或生命周期的 process 中心被
    // Replace 处理器占位（声明绑定仍要求安装、执行不再经它）。按此推导
    // 允许集，其余残余（含挂在字段/声明性绑定键上的杂项实现）一律拒绝。
    let mut replaced_processes = BTreeSet::new();
    for intent in plan.intents() {
        if intent.key.namespace() == "builtin" {
            continue; // builtin 意图的派生键不构成合法残余
        }
        let Ok(own) = Key::new(format!("{}.process", intent.key)) else {
            continue;
        };
        let Some(handle) = plan.resolve_slot(&intent.slot) else {
            continue;
        };
        let Some(slot) = plan.get_slot(handle) else {
            continue;
        };
        let center = match &slot.dispatch {
            Dispatch::Native(binding) => binding,
            Dispatch::Chained { center, .. } => center,
        };
        if center != &own {
            replaced_processes.insert(own);
        }
    }
    for ty in plan.projection().types() {
        if ty.descriptor.key.namespace() == "builtin" {
            continue;
        }
        for key in &ty.descriptor.lifecycle_slots {
            let Ok(own) = Key::new(format!("{key}.process")) else {
                continue;
            };
            let Some(slot) = plan.resolve_slot(key).and_then(|h| plan.get_slot(h)) else {
                continue;
            };
            let center = match &slot.dispatch {
                Dispatch::Native(binding) => binding,
                Dispatch::Chained { center, .. } => center,
            };
            if center != &own {
                replaced_processes.insert(own);
            }
        }
    }
    for binding in processes.keys() {
        if !replaced_processes.contains(binding) {
            return Err(format!(
                "装配提供了未被任何入口消费的动作处理实现 {binding}（键名错误或挂错通道）"
            ));
        }
    }
    Ok(result)
}

fn take_after(
    slot: &str,
    after: &[Key],
    handlers: &mut BTreeMap<Key, ExtHandler>,
) -> Result<Vec<Rc<ExtSimple>>, String> {
    let mut after_fns = Vec::new();
    for binding in after {
        match handlers.remove(binding) {
            Some(ExtHandler::After(f)) => after_fns.push(Rc::new(f)),
            Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
            None => return Err(missing(slot, binding, "after")),
        }
    }
    Ok(after_fns)
}

fn key(value: &str) -> Key {
    Key::new(value).expect("内建槽键合法")
}

pub(super) fn dispatch_of(plan: &ContributionPlan, slot: &str) -> Result<Dispatch, String> {
    let handle = plan
        .resolve_slot(&key(slot))
        .ok_or_else(|| format!("计划缺少槽 {slot}"))?;
    Ok(plan
        .get_slot(handle)
        .expect("句柄来自计划")
        .dispatch
        .clone())
}

fn missing(slot: &str, binding: &Key, kind: &str) -> String {
    format!(
        "槽 {slot} 的处理器 {binding} 声明为 {kind}，但装配未提供该实现（或同一 binding 被多个处理器共用导致二次消耗）"
    )
}

/// 计划未引用的处理器实现是装配调用方的键名错误，静默丢弃会掩盖它。
fn reject_leftover<T>(slot: &str, handlers: &BTreeMap<Key, T>) -> Result<(), String> {
    if let Some(binding) = handlers.keys().next() {
        return Err(format!(
            "装配提供了计划未引用的处理器 {binding}（槽 {slot}）"
        ));
    }
    Ok(())
}

fn kind_mismatch(binding: &Key, want: &str, got: &str) -> String {
    format!("处理器 {binding} 声明 kind 为 {want}，装配提供的实现是 {got}")
}

/// action_handlers 共用安装表，按各槽冻结后的实际绑定分配；剩余键仍报错。
fn take_action_handlers(
    plan: &ContributionPlan,
    slot: &str,
    handlers: &mut BTreeMap<Key, ActionHandler>,
) -> Result<BTreeMap<Key, ActionHandler>, String> {
    let mut result = BTreeMap::new();
    if let Dispatch::Chained {
        before,
        wraps,
        center,
        after,
    } = dispatch_of(plan, slot)?
    {
        for binding in before
            .into_iter()
            .chain(wraps)
            .chain(std::iter::once(center))
            .chain(after)
        {
            if let Some(handler) = handlers.remove(&binding) {
                result.insert(binding, handler);
            }
        }
    }
    Ok(result)
}

fn assemble_action(
    plan: &ContributionPlan,
    slot: &str,
    native_binding: &str,
    mut handlers: BTreeMap<Key, ActionHandler>,
    native_step: fn(&mut Working, ObjId, &IntentData),
) -> Result<ActionSlot, String> {
    let native = key(native_binding);
    match dispatch_of(plan, slot)? {
        Dispatch::Native(binding) => {
            if binding != native {
                return Err(format!(
                    "槽 {slot} 的 native 绑定须为 {native_binding}，实得 {binding}"
                ));
            }
            Ok(ActionSlot::Native(native_step))
        }
        Dispatch::Chained {
            before,
            wraps,
            center,
            after,
        } => {
            let mut before_fns = Vec::new();
            for binding in &before {
                match handlers.remove(binding) {
                    Some(ActionHandler::Before(f)) => before_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "before", other.label())),
                    None => return Err(missing(slot, binding, "before")),
                }
            }
            let mut wrap_fns = Vec::new();
            for binding in &wraps {
                match handlers.remove(binding) {
                    Some(ActionHandler::Wrap(f)) => wrap_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "wrap", other.label())),
                    None => return Err(missing(slot, binding, "wrap")),
                }
            }
            let center_fn = if center == native {
                ActionCenter::Native(native_step)
            } else {
                match handlers.remove(&center) {
                    Some(ActionHandler::Replace(f)) => ActionCenter::Custom(f),
                    Some(other) => {
                        return Err(kind_mismatch(&center, "replace", other.label()));
                    }
                    None => return Err(missing(slot, &center, "replace")),
                }
            };
            let mut after_fns = Vec::new();
            for binding in &after {
                match handlers.remove(binding) {
                    Some(ActionHandler::After(f)) => after_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
                    None => return Err(missing(slot, binding, "after")),
                }
            }
            let slot_result = ActionSlot::Chained(Rc::new(ActionChained {
                before: before_fns,
                wraps: wrap_fns.into_iter().map(Rc::new).collect(),
                center: Rc::new(center_fn),
                after: after_fns,
            }));
            reject_leftover(slot, &handlers)?;
            Ok(slot_result)
        }
    }
}

fn assemble_tick(
    plan: &ContributionPlan,
    slot: &str,
    native_binding: &str,
    mut handlers: BTreeMap<Key, TickHandler>,
) -> Result<TickSlot, String> {
    let native = key(native_binding);
    match dispatch_of(plan, slot)? {
        Dispatch::Native(binding) => {
            if binding != native {
                return Err(format!(
                    "槽 {slot} 的 native 绑定须为 {native_binding}，实得 {binding}"
                ));
            }
            Ok(TickSlot::Native(crate::settle::economy::link_cooldown_tick))
        }
        Dispatch::Chained {
            before,
            wraps,
            center,
            after,
        } => {
            let mut before_fns = Vec::new();
            for binding in &before {
                match handlers.remove(binding) {
                    Some(TickHandler::Before(f)) => before_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "before", other.label())),
                    None => return Err(missing(slot, binding, "before")),
                }
            }
            let mut wrap_fns = Vec::new();
            for binding in &wraps {
                match handlers.remove(binding) {
                    Some(TickHandler::Wrap(f)) => wrap_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "wrap", other.label())),
                    None => return Err(missing(slot, binding, "wrap")),
                }
            }
            let center_fn = if center == native {
                TickCenter::Native(crate::settle::economy::link_cooldown_tick)
            } else {
                match handlers.remove(&center) {
                    Some(TickHandler::Replace(f)) => TickCenter::Custom(f),
                    Some(other) => {
                        return Err(kind_mismatch(&center, "replace", other.label()));
                    }
                    None => return Err(missing(slot, &center, "replace")),
                }
            };
            let mut after_fns = Vec::new();
            for binding in &after {
                match handlers.remove(binding) {
                    Some(TickHandler::After(f)) => after_fns.push(f),
                    Some(other) => return Err(kind_mismatch(binding, "after", other.label())),
                    None => return Err(missing(slot, binding, "after")),
                }
            }
            let slot_result = TickSlot::Chained(Rc::new(TickChained {
                before: before_fns,
                wraps: wrap_fns.into_iter().map(Rc::new).collect(),
                center: Rc::new(center_fn),
                after: after_fns,
            }));
            reject_leftover(slot, &handlers)?;
            Ok(slot_result)
        }
    }
}
