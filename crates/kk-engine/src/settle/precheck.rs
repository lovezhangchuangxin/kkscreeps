//! 移动请求预检候选：静态放行与不可撤销的显式否决分别累积。
//!
//! Hosted 登记保留原生拒绝快照，正常拒绝按该快照恢复默认过滤；
//! 成功后在竞争前移除被否决请求，并将静态放行传给仲裁与预演。
//! 动态占格仍由取消链复核；候选只在本房本 tick 存活。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_world::ObjId;

/// 同一次预检调用内共享的决策候选；不含世界写句柄。
pub struct MovePrecheck {
    input: kk_mod_api::precheck::MovePrecheckInput,
    /// 本次输入中的合法请求者集合，写面不认其他对象。
    movers: BTreeSet<ObjId>,
    blocked: RefCell<BTreeSet<ObjId>>,
    registration_blocks: BTreeSet<ObjId>,
    static_blocks: BTreeSet<ObjId>,
    static_allowed: RefCell<BTreeSet<ObjId>>,
}

/// 竞争前的请求过滤与仲裁阶段的静态放行；两者同一次候选提交。
pub struct PrecheckDecisions {
    pub blocks: BTreeSet<ObjId>,
    pub static_allowed: BTreeSet<ObjId>,
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
        let mut registration_blocks = BTreeSet::new();
        let mut static_blocks = BTreeSet::new();
        for info in &input.requests {
            let id = ObjId::parse(&info.mover_id)
                .ok_or_else(|| format!("预检输入含不可解析的对象 ID {}", info.mover_id))?;
            movers.insert(id);
            if info.native_registration_blocked {
                registration_blocks.insert(id);
            }
            if info.native_registration_blocked || info.native_static_blocked {
                static_blocks.insert(id);
            }
        }
        Ok(Some(Self {
            input,
            movers,
            blocked: RefCell::new(BTreeSet::new()),
            registration_blocks,
            static_blocks,
            static_allowed: RefCell::new(BTreeSet::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::precheck::MovePrecheckInput {
        &self.input
    }

    /// 读取静态初判与显式否决的组合终值；不推测动态让位结果。
    pub fn is_blocked(&self, id: ObjId) -> bool {
        self.blocked.borrow().contains(&id)
            || (self.static_blocks.contains(&id) && !self.static_allowed.borrow().contains(&id))
    }

    /// 追加不可撤销的否决；静态放行不能清除它。id 须在本次输入中。
    pub fn block(&self, id: ObjId) -> Result<(), String> {
        if !self.movers.contains(&id) {
            return Err(format!("预检目标 {} 不在本次输入中", id.as_str()));
        }
        self.blocked.borrow_mut().insert(id);
        Ok(())
    }

    /// 只放行此请求的静态玩法障碍，不能清除其他处理器的 block 或动态占格。
    pub fn allow_static(&self, id: ObjId) -> Result<(), String> {
        if !self.movers.contains(&id) {
            return Err(format!("静态放行目标 {} 不在本次输入中", id.as_str()));
        }
        self.static_allowed.borrow_mut().insert(id);
        Ok(())
    }

    /// 正常拒绝丢弃全部 JS 候选，并兑现登记时的原生拒绝快照。
    pub fn decisions(&self, completed: bool) -> PrecheckDecisions {
        let static_allowed = if completed {
            self.static_allowed.borrow().clone()
        } else {
            BTreeSet::new()
        };
        let mut blocks: BTreeSet<_> = self
            .registration_blocks
            .difference(&static_allowed)
            .copied()
            .collect();
        if completed {
            blocks.extend(self.blocked.borrow().iter().copied());
        }
        PrecheckDecisions {
            blocks,
            static_allowed,
        }
    }

    /// 取出显式否决集（诊断用）；正式提交必须同时使用 decisions 的静态放行。
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
