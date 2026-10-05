//! 战斗能力的候选门控、首触日志与恰一次消耗；不在候选借用内进入 JS。
use super::*;

impl ModChainContext {
    pub(super) fn request_combat(&mut self, target: &str) -> Result<(), String> {
        let (action, journal) = {
            let Some(combat) = &self.combat else {
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
        kk_engine::settle::creep::slot_combat_request(
            &mut self.ctx.borrow_mut(),
            self.id,
            target_id,
            action,
        )?;
        self.cached_fields = None;
        if let Some(combat) = &mut self.combat {
            combat.consumed = true;
        }
        Ok(())
    }

    pub(super) fn cancel_combat(&mut self) -> Result<(), String> {
        let Some(combat) = &mut self.combat else {
            return Err("本次槽没有战斗能力".into());
        };
        if combat.consumed {
            return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
        }
        combat.consumed = true;
        Ok(())
    }

    pub(super) fn apply_combat_damage(&mut self, target: &str, amount: i64) -> Result<(), String> {
        let Some(combat) = &self.combat else {
            return Err("本次槽没有策略伤害能力".into());
        };
        if combat.consumed {
            return Err("本动作的战斗能力已消耗（改向/取消/执行恰一次）".into());
        }
        let action = combat.action;
        let journal = combat.journal.clone();
        let target_id = ObjId::parse(target).ok_or("战斗目标 ID 非法")?;
        // 策略伤害不做 rampart 改向，只有指定目标为额外写入面。
        self.remember_combat_targets(&journal, &[target_id]);
        kk_engine::settle::creep::slot_combat_apply_damage(
            &mut self.ctx.borrow_mut(),
            self.id,
            target_id,
            amount,
            action,
        )?;
        self.cached_fields = None;
        self.combat.as_mut().expect("已检查战斗能力").consumed = true;
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
