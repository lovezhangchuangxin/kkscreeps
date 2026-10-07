//! 移动领域的能力委托；领域状态由 ChainCapability 绑定。
use super::*;

impl kk_mod_host::MovementHost for ModChainContext {
    fn contest_priority(&mut self, id: &str) -> Result<i64, String> {
        let contest = self.capability.contest().ok_or("本次槽没有移动竞争能力")?;
        let id = ObjId::parse(id).ok_or("竞争者 ID 非法")?;
        Ok(contest.priority(id))
    }
    fn contest_set_priority(&mut self, id: &str, priority: i64) -> Result<(), String> {
        let contest = self.capability.contest().ok_or("本次槽没有移动竞争能力")?;
        let id = ObjId::parse(id).ok_or("竞争者 ID 非法")?;
        contest.set_priority(id, priority)
    }
    fn admission_admitted(&mut self, id: &str) -> Result<bool, String> {
        let admission = self
            .capability
            .admission()
            .ok_or("本次槽没有移动准入能力")?;
        let id = ObjId::parse(id).ok_or("执行者 ID 非法")?;
        Ok(admission.admitted(id))
    }
    fn admission_set_admitted(&mut self, id: &str, admitted: bool) -> Result<(), String> {
        let admission = self
            .capability
            .admission()
            .ok_or("本次槽没有移动准入能力")?;
        let id = ObjId::parse(id).ok_or("执行者 ID 非法")?;
        admission.set_admitted(id, admitted)
    }
    fn admission_evaluate(&mut self) -> Result<Vec<String>, String> {
        let admission = self
            .capability
            .admission()
            .ok_or("本次槽没有移动准入能力")?;
        let finals = admission.finals_snapshot();
        let survivors = self.ctx.borrow().movement_simulate_survivors(&finals);
        Ok(survivors.iter().map(|id| id.as_str().to_string()).collect())
    }
    fn obstacle_blocked(&mut self, id: &str) -> Result<bool, String> {
        let obstacle = self.capability.obstacle().ok_or("本次槽没有移动障碍能力")?;
        let id = ObjId::parse(id).ok_or("胜者 ID 非法")?;
        Ok(obstacle.is_blocked(id))
    }
    fn obstacle_block(&mut self, id: &str) -> Result<(), String> {
        let obstacle = self.capability.obstacle().ok_or("本次槽没有移动障碍能力")?;
        let id = ObjId::parse(id).ok_or("胜者 ID 非法")?;
        obstacle.block(id)
    }
    fn precheck_blocked(&mut self, id: &str) -> Result<bool, String> {
        let precheck = self.capability.precheck().ok_or("本次槽没有移动预检能力")?;
        let id = ObjId::parse(id).ok_or("请求者 ID 非法")?;
        Ok(precheck.is_blocked(id))
    }
    fn precheck_block(&mut self, id: &str) -> Result<(), String> {
        let precheck = self.capability.precheck().ok_or("本次槽没有移动预检能力")?;
        let id = ObjId::parse(id).ok_or("请求者 ID 非法")?;
        precheck.block(id)
    }
    fn pull_denied(&mut self, id: &str) -> Result<bool, String> {
        let pull = self
            .capability
            .pull_registration()
            .ok_or("本次槽没有拉拽登记能力")?;
        let id = ObjId::parse(id).ok_or("拉者 ID 非法")?;
        Ok(pull.is_denied(id))
    }
    fn pull_deny(&mut self, id: &str) -> Result<(), String> {
        let pull = self
            .capability
            .pull_registration()
            .ok_or("本次槽没有拉拽登记能力")?;
        let id = ObjId::parse(id).ok_or("拉者 ID 非法")?;
        pull.deny(id)
    }
    fn cost_fatigue_of(&mut self, id: &str) -> Result<i64, String> {
        let cost = self
            .capability
            .move_cost()
            .ok_or("本次槽没有移动代价能力")?;
        let id = ObjId::parse(id).ok_or("移动者 ID 非法")?;
        cost.fatigue_of(id)
            .ok_or("移动者不在本次输入中".to_string())
    }
    fn cost_set_fatigue(&mut self, id: &str, value: i64) -> Result<(), String> {
        let cost = self
            .capability
            .move_cost()
            .ok_or("本次槽没有移动代价能力")?;
        let id = ObjId::parse(id).ok_or("移动者 ID 非法")?;
        cost.set_fatigue(id, value)
    }
    fn movement_pending(&mut self) -> Result<Option<u8>, String> {
        if !matches!(self.capability, ChainCapability::Move) {
            return Err("本次槽没有移动请求能力".into());
        }
        kk_engine::settle::creep::slot_pending_direction(&self.ctx.borrow(), self.id)
    }
    fn movement_request(&mut self, direction: u8) -> Result<bool, String> {
        if !matches!(self.capability, ChainCapability::Move) {
            return Err("本次槽没有移动请求能力".into());
        }
        // 登记不改字段缓存读面（执行者坐标未变），与资源读取同型。
        kk_engine::settle::creep::slot_request_direction(
            &mut self.ctx.borrow_mut(),
            self.id,
            direction,
        )
    }
    fn movement_cancel(&mut self) -> Result<(), String> {
        if !matches!(self.capability, ChainCapability::Move) {
            return Err("本次槽没有移动请求能力".into());
        }
        kk_engine::settle::creep::slot_cancel_direction(&mut self.ctx.borrow_mut(), self.id)
    }
}
