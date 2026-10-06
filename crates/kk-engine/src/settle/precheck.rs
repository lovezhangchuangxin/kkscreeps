//! 移动请求预检候选：单向 block 收紧，被否决请求等同从未登记。
//!
//! 输入中的请求均已通过原生静态预检；能力面只有追加否决，没有放行
//! （意图期放行语义惰性——仲裁期 native 障碍判定对结构/私有 rampart/
//! 墙地形一律复检）。候选在整槽调用期间由 JS 能力写入；成功退出后
//! 引擎把否决集应用进登记矩阵（竞争输入构造之前），正常拒绝丢弃全部
//! 否决。候选不进入检查点。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_world::ObjId;

/// 同一次预检调用内共享的否决候选；不含世界写句柄。
pub struct MovePrecheck {
    input: kk_mod_api::precheck::MovePrecheckInput,
    /// 本次输入中的合法请求者集合，写面不认其他对象。
    movers: BTreeSet<ObjId>,
    blocked: RefCell<BTreeSet<ObjId>>,
}

impl MovePrecheck {
    /// 按预检输入构造候选；空输入（无登记请求）返回 None，不进入 JS。
    /// 请求者 id 必须可解析回引擎对象 ID——输入由 `precheck_input` 从
    /// 登记矩阵生成，此不变量破坏时按故障处理而非静默收窄写面。
    pub fn new(input: kk_mod_api::precheck::MovePrecheckInput) -> Result<Option<Self>, String> {
        if input.requests.is_empty() {
            return Ok(None);
        }
        let mut movers = BTreeSet::new();
        for info in &input.requests {
            let id = ObjId::parse(&info.mover_id)
                .ok_or_else(|| format!("预检输入含不可解析的对象 ID {}", info.mover_id))?;
            movers.insert(id);
        }
        Ok(Some(Self {
            input,
            movers,
            blocked: RefCell::new(BTreeSet::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::precheck::MovePrecheckInput {
        &self.input
    }

    /// 读取当前判定：原生初值恒为否（列表请求均已通过原生静态预检），
    /// 之外只有已追加的否决。
    pub fn is_blocked(&self, id: ObjId) -> bool {
        self.blocked.borrow().contains(&id)
    }

    /// 追加否决；单向能力，不提供放行。id 须为本次输入中的请求者。
    pub fn block(&self, id: ObjId) -> Result<(), String> {
        if !self.movers.contains(&id) {
            return Err(format!("预检目标 {} 不在本次输入中", id.as_str()));
        }
        self.blocked.borrow_mut().insert(id);
        Ok(())
    }

    /// 取出否决集（接管所有权）；只在链成功后由引擎调用一次。
    pub fn into_blocks(&self) -> BTreeSet<ObjId> {
        std::mem::take(&mut *self.blocked.borrow_mut())
    }

    /// 否决集快照；组合诊断与测试用，不消费候选。
    pub fn blocks_snapshot(&self) -> BTreeMap<ObjId, bool> {
        self.movers
            .iter()
            .map(|id| (*id, self.blocked.borrow().contains(id)))
            .collect()
    }
}

#[cfg(test)]
#[path = "precheck/tests.rs"]
mod tests;
