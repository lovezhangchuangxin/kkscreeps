//! 战斗能力的候选门控、首触日志与恰一次消耗；不在候选借用内进入 JS。
use super::*;

type CombatEffect = fn(
    &mut kk_engine::settle::Working,
    ObjId,
    ObjId,
    i64,
    kk_engine::settle::creep::CombatAction,
    Option<&kk_engine::settle::creep::RetaliationPolicy>,
) -> Result<(), String>;

impl ModChainContext {
    pub(super) fn request_combat(&mut self, target: &str) -> Result<(), String> {
        let (action, journal) = {
            let Some(combat) = self.capability.combat() else {
                return Err("本次槽没有战斗目标改向能力".into());
            };
            if !combat.action.targeted() {
                return Err("rangedMassAttack 没有目标改向能力".into());
            }
            if combat.consumed {
                return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
            }
            (combat.action, combat.journal.clone())
        };
        let target_id = ObjId::parse(target).ok_or("战斗目标 ID 非法")?;
        let touched = kk_engine::settle::creep::combat_touch_targets(&self.ctx.borrow(), target_id);
        self.remember_combat_targets(&journal, &touched);
        if let Some(policy) = &self.capability.combat().unwrap().retaliation {
            kk_engine::settle::creep::slot_combat_execute_with_retaliation(
                &mut self.ctx.borrow_mut(),
                self.id,
                &kk_engine::intent::IntentData::target(target_id),
                action,
                policy,
            )?;
        } else {
            kk_engine::settle::creep::slot_combat_request(
                &mut self.ctx.borrow_mut(),
                self.id,
                target_id,
                action,
            )?;
        }
        self.cached_fields = None;
        if let Some(combat) = self.capability.combat_mut() {
            combat.consumed = true;
        }
        Ok(())
    }

    pub(super) fn cancel_combat(&mut self) -> Result<(), String> {
        let Some(combat) = self.capability.combat_mut() else {
            return Err("本次槽没有战斗能力".into());
        };
        if combat.consumed {
            return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
        }
        combat.consumed = true;
        Ok(())
    }

    pub(super) fn apply_combat_damage(&mut self, target: &str, amount: i64) -> Result<(), String> {
        self.apply_combat_effect(
            target,
            amount,
            kk_engine::settle::creep::slot_combat_apply_damage_with_retaliation,
        )
    }

    pub(super) fn apply_combat_heal(&mut self, target: &str, amount: i64) -> Result<(), String> {
        self.apply_combat_effect(target, amount, |w, actor, target, amount, action, _| {
            kk_engine::settle::creep::slot_combat_apply_heal(w, actor, target, amount, action)
        })
    }

    /// 声明一次反击表，不消耗攻击；重复声明和执行后的声明均为故障。
    pub(super) fn set_combat_retaliation(
        &mut self,
        requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        let combat = self.capability.combat().ok_or("本次槽没有反击策略能力")?;
        if matches!(
            combat.action,
            kk_engine::settle::creep::CombatAction::Heal
                | kk_engine::settle::creep::CombatAction::RangedHeal
        ) {
            return Err("治疗槽没有反击策略能力".into());
        }
        if combat.consumed || combat.retaliation.is_some() {
            return Err("反击策略须在攻击执行前声明一次".into());
        }
        if requests.len() > kk_mod_api::combat::MAX_COMBAT_BATCH as usize {
            return Err("反击策略最多 256 项".into());
        }
        let requests = requests
            .iter()
            .map(|r| Ok((ObjId::parse(&r.id).ok_or("反击目标 ID 非法")?, r.amount)))
            .collect::<Result<Vec<_>, String>>()?;
        let policy =
            kk_engine::settle::creep::RetaliationPolicy::new(&self.ctx.borrow(), &requests)?;
        self.capability.combat_mut().unwrap().retaliation = Some(policy);
        Ok(())
    }

    pub(super) fn apply_combat_damage_batch(
        &mut self,
        requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        let Some(combat) = self.capability.combat() else {
            return Err("本次槽没有批量策略伤害能力".into());
        };
        if combat.consumed {
            return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
        }
        // Rust 公共宿主接口也受批量限额约束，不能仅依赖 SDK/op 的数组预检。
        if requests.len() > kk_mod_api::combat::MAX_COMBAT_BATCH as usize {
            return Err("批量策略伤害最多 256 项".into());
        }
        let action = combat.action;
        let journal = combat.journal.clone();
        let requests: Vec<_> = requests
            .iter()
            .map(|request| {
                Ok((
                    ObjId::parse(&request.id).ok_or("战斗目标 ID 非法")?,
                    request.amount,
                ))
            })
            .collect::<Result<_, String>>()?;
        self.remember_combat_targets(
            &journal,
            &requests.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        );
        kk_engine::settle::creep::slot_combat_apply_damage_batch_with_retaliation(
            &mut self.ctx.borrow_mut(),
            self.id,
            &requests,
            action,
            self.capability.combat().unwrap().retaliation.as_ref(),
        )?;
        self.cached_fields = None;
        self.capability
            .combat_mut()
            .expect("已检查战斗能力")
            .consumed = true;
        Ok(())
    }

    fn apply_combat_effect(
        &mut self,
        target: &str,
        amount: i64,
        apply: CombatEffect,
    ) -> Result<(), String> {
        let Some(combat) = self.capability.combat() else {
            return Err("本次槽没有战斗策略能力".into());
        };
        if combat.consumed {
            return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
        }
        let action = combat.action;
        let journal = combat.journal.clone();
        let target_id = ObjId::parse(target).ok_or("战斗目标 ID 非法")?;
        // 策略伤害/治疗均不做 rampart 改向，只有指定目标为额外写入面。
        self.remember_combat_targets(&journal, &[target_id]);
        apply(
            &mut self.ctx.borrow_mut(),
            self.id,
            target_id,
            amount,
            action,
            self.capability.combat().unwrap().retaliation.as_ref(),
        )?;
        self.cached_fields = None;
        self.capability
            .combat_mut()
            .expect("已检查战斗能力")
            .consumed = true;
        Ok(())
    }

    fn remember_combat_targets(
        &self,
        journal: &Rc<RefCell<BTreeMap<ObjId, RedirectJournalEntry>>>,
        targets: &[ObjId],
    ) {
        let w = self.ctx.borrow();
        let mut journal = journal.borrow_mut();
        for id in targets {
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
    }
}
