//! 移动请求与房间策略宿主能力；默认拒绝，具体槽显式开放。

pub trait MovementHost {
    fn contest_priority(&mut self, _id: &str) -> Result<i64, String> {
        Err("本次槽没有移动竞争能力".into())
    }
    fn contest_set_priority(&mut self, _id: &str, _priority: i64) -> Result<(), String> {
        Err("本次槽没有移动竞争能力".into())
    }
    fn admission_admitted(&mut self, _id: &str) -> Result<bool, String> {
        Err("本次槽没有移动准入能力".into())
    }
    fn admission_set_admitted(&mut self, _id: &str, _admitted: bool) -> Result<(), String> {
        Err("本次槽没有移动准入能力".into())
    }
    fn admission_evaluate(&mut self) -> Result<Vec<String>, String> {
        Err("本次槽没有移动准入能力".into())
    }
    fn obstacle_blocked(&mut self, _id: &str) -> Result<bool, String> {
        Err("本次槽没有移动障碍能力".into())
    }
    fn obstacle_block(&mut self, _id: &str) -> Result<(), String> {
        Err("本次槽没有移动障碍能力".into())
    }
    fn obstacle_evaluate(&mut self) -> Result<Vec<String>, String> {
        Err("本次槽没有移动障碍能力".into())
    }
    fn precheck_blocked(&mut self, _id: &str) -> Result<bool, String> {
        Err("本次槽没有移动预检能力".into())
    }
    fn precheck_block(&mut self, _id: &str) -> Result<(), String> {
        Err("本次槽没有移动预检能力".into())
    }
    fn precheck_allow_static(&mut self, _id: &str) -> Result<(), String> {
        Err("本次槽没有移动预检能力".into())
    }
    fn pull_denied(&mut self, _id: &str) -> Result<bool, String> {
        Err("本次槽没有拉拽登记能力".into())
    }
    fn pull_deny(&mut self, _id: &str) -> Result<(), String> {
        Err("本次槽没有拉拽登记能力".into())
    }
    fn cost_fatigue_of(&mut self, _id: &str) -> Result<i64, String> {
        Err("本次槽没有移动代价能力".into())
    }
    fn cost_set_fatigue(&mut self, _id: &str, _value: i64) -> Result<(), String> {
        Err("本次槽没有移动代价能力".into())
    }
    fn movement_pending(&mut self) -> Result<Option<u8>, String> {
        Err("本次槽没有移动请求能力".into())
    }
    fn movement_request(&mut self, _direction: u8) -> Result<bool, String> {
        Err("本次槽没有移动请求能力".into())
    }
    fn movement_cancel(&mut self) -> Result<(), String> {
        Err("本次槽没有移动请求能力".into())
    }
}
