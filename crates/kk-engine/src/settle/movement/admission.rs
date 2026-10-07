//! 移动准入终值候选：以原生 can_move 结果初始化，JS 只在其上覆盖。
//!
//! 候选在整槽调用期间由 JS 能力读写；成功退出后引擎以终值整体替换
//! 原生预填，正常拒绝丢弃覆盖并保留原生值。准入终值同时被链取消与
//! 执行阶段消费（同源），候选不进入检查点。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_world::ObjId;

/// 同一次准入调用内共享的终值候选；不含世界写句柄。
pub struct MoveAdmission {
    input: kk_mod_api::movement::admission::MoveAdmissionInput,
    /// 本次输入中的合法执行者集合，写面不认其他对象。
    movers: BTreeSet<ObjId>,
    finals: RefCell<BTreeMap<ObjId, bool>>,
}

impl MoveAdmission {
    /// 按准入输入与原生终值构造候选；空输入（无胜者）返回 None，
    /// 不进入 JS。执行者 id 必须可解析回引擎对象 ID——输入由
    /// `prefill_admissions`/`admission_input` 从胜者表生成，此不变量破坏时按故障
    /// 处理而非静默收窄写面。
    pub fn new(
        input: kk_mod_api::movement::admission::MoveAdmissionInput,
        native: BTreeMap<ObjId, bool>,
    ) -> Result<Option<Self>, String> {
        if input.movers.is_empty() {
            return Ok(None);
        }
        let mut movers = BTreeSet::new();
        for info in &input.movers {
            let id = ObjId::parse(&info.id)
                .ok_or_else(|| format!("准入输入含不可解析的对象 ID {}", info.id))?;
            movers.insert(id);
        }
        Ok(Some(Self {
            input,
            movers,
            finals: RefCell::new(native),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::movement::admission::MoveAdmissionInput {
        &self.input
    }

    /// 读取当前准入终值；候选以原生值初始化，未覆盖项即原生结果，
    /// 输入外对象按未获准返回。
    pub fn admitted(&self, id: ObjId) -> bool {
        self.finals.borrow().get(&id).copied().unwrap_or(false)
    }

    /// 覆盖准入终值；可多次调整以组合处理器。id 必须是本次输入中的
    /// 执行者。
    pub fn set_admitted(&self, id: ObjId, value: bool) -> Result<(), String> {
        if !self.movers.contains(&id) {
            return Err(format!("准入目标 {} 不在本次输入中", id.as_str()));
        }
        self.finals.borrow_mut().insert(id, value);
        Ok(())
    }

    /// 当前终值候选的快照（原生初始项与 JS 覆盖项的并集）；供只读
    /// 链预演读取，不改变候选本身。
    pub fn finals_snapshot(&self) -> BTreeMap<ObjId, bool> {
        self.finals.borrow().clone()
    }

    /// 取出终值候选（接管所有权）；含原生初始项与 JS 覆盖项的并集。
    /// 只在链成功后由引擎调用一次，候选不再复用。
    pub fn into_finals(&self) -> BTreeMap<ObjId, bool> {
        std::mem::take(&mut *self.finals.borrow_mut())
    }
}

#[cfg(test)]
mod tests;
