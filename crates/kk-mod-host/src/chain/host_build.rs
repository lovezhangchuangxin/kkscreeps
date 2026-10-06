//! 建造宿主能力；默认拒绝，具体槽显式开放。

pub trait BuildHost {
    fn build_request(&mut self, _target: &str) -> Result<(), String> {
        Err("本次槽没有建造目标改向能力".into())
    }
    fn build_cancel(&mut self) -> Result<(), String> {
        Err("本次槽没有建造能力".into())
    }
}
