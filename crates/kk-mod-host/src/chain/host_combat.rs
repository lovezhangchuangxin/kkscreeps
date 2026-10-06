//! 战斗与受击宿主能力；默认拒绝，具体槽显式开放。

pub trait CombatHost {
    fn incoming_damage(&mut self) -> Result<i64, String> {
        Err("本次槽没有受击数值能力".into())
    }
    fn incoming_set_damage(&mut self, _amount: i64) -> Result<(), String> {
        Err("本次槽没有受击数值能力".into())
    }
    fn combat_request(&mut self, _target: &str) -> Result<(), String> {
        Err("本次槽没有战斗目标改向能力".into())
    }
    fn combat_cancel(&mut self) -> Result<(), String> {
        Err("本次槽没有战斗能力".into())
    }
    fn combat_apply_damage(&mut self, _target: &str, _amount: i64) -> Result<(), String> {
        Err("本次槽没有策略伤害能力".into())
    }
    fn combat_apply_heal(&mut self, _target: &str, _amount: i64) -> Result<(), String> {
        Err("本次槽没有策略治疗能力".into())
    }
    fn combat_set_retaliation(
        &mut self,
        _requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        Err("本次槽没有反击策略能力".into())
    }
    fn combat_apply_damage_batch(
        &mut self,
        _requests: &[kk_mod_api::combat::DamageRequest],
    ) -> Result<(), String> {
        Err("本次槽没有批量策略伤害能力".into())
    }
    fn combat_list(
        &mut self,
        _after: &str,
        _limit: u32,
    ) -> Result<kk_mod_api::combat::CombatPage, String> {
        Err("本次槽没有战斗读取能力".into())
    }
    fn combat_get(
        &mut self,
        _target: &str,
    ) -> Result<Option<kk_mod_api::combat::CombatView>, String> {
        Err("本次槽没有战斗读取能力".into())
    }
}
