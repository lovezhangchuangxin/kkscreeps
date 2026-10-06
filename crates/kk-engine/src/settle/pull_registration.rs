//! 拉拽登记候选：单向 deny 收紧，被否决配对等同从未登记。
//!
//! 输入中的配对均已通过原生登记校验（相邻、非孵化、循环链拒绝）且
//! 双方仍在房间内；拉者对象已移除的配对不进输入也不可否决（该边界
//! 由配对复核按"保留现有标记"处理）。能力面只有追加否决，没有放行
//! 或新增——新增拉拽没有属主意图，疲劳转移会向未行动玩家计费，属
//! 宿主约束关闭方向。候选在整槽调用期间由 JS 能力写入；成功退出后
//! 引擎把否决集应用进双向配对表（预检之前），正常拒绝丢弃全部否决。
//! 候选不进入检查点。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_world::ObjId;

/// 同一次拉拽登记调用内共享的否决候选；不含世界写句柄。
pub struct PullRegistration {
    input: kk_mod_api::pull_registration::PullRegistrationInput,
    /// 本次输入中的合法拉者集合，写面不认其他对象。
    pullers: BTreeSet<ObjId>,
    denied: RefCell<BTreeSet<ObjId>>,
}

impl PullRegistration {
    /// 按登记输入构造候选；空输入（无有效配对）返回 None，不进入 JS。
    /// 拉者 id 必须可解析回引擎对象 ID——输入由 `pull_registration_input`
    /// 从配对表生成，此不变量破坏时按故障处理而非静默收窄写面。
    pub fn new(
        input: kk_mod_api::pull_registration::PullRegistrationInput,
    ) -> Result<Option<Self>, String> {
        if input.pulls.is_empty() {
            return Ok(None);
        }
        let mut pullers = BTreeSet::new();
        for info in &input.pulls {
            let id = ObjId::parse(&info.puller_id)
                .ok_or_else(|| format!("拉拽登记输入含不可解析的拉者 ID {}", info.puller_id))?;
            pullers.insert(id);
        }
        Ok(Some(Self {
            input,
            pullers,
            denied: RefCell::new(BTreeSet::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::pull_registration::PullRegistrationInput {
        &self.input
    }

    /// 读取当前判定：原生初值恒为未否决（列表配对均已通过原生登记），
    /// 之外只有已追加的否决。
    pub fn is_denied(&self, id: ObjId) -> bool {
        self.denied.borrow().contains(&id)
    }

    /// 追加否决；单向能力，不提供放行或新增。id 须为本次输入中的拉者。
    pub fn deny(&self, id: ObjId) -> Result<(), String> {
        if !self.pullers.contains(&id) {
            return Err(format!("拉拽否决目标 {} 不在本次输入中", id.as_str()));
        }
        self.denied.borrow_mut().insert(id);
        Ok(())
    }

    /// 取出否决集（接管所有权）；只在链成功后由引擎调用一次。
    pub fn into_denies(&self) -> BTreeSet<ObjId> {
        std::mem::take(&mut *self.denied.borrow_mut())
    }

    /// 否决集快照；组合诊断与测试用，不消费候选。
    pub fn denies_snapshot(&self) -> BTreeMap<ObjId, bool> {
        self.pullers
            .iter()
            .map(|id| (*id, self.denied.borrow().contains(id)))
            .collect()
    }
}

#[cfg(test)]
#[path = "pull_registration/tests.rs"]
mod tests;
