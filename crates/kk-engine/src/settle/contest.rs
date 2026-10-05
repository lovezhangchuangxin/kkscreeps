//! 移动竞争优先级候选：只记录各竞争者的优先级数值，不直接改写仲裁。
//!
//! 候选在整槽调用期间由 JS 能力写入；成功退出后引擎按终值完成仲裁，
//! 正常拒绝丢弃全部数值并回退默认评分。世界写入仍由既有仲裁与对象
//! tick 阶段统一发生，候选不进入检查点。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_mod_api::contest::MAX_PRIORITY;
use kk_world::ObjId;

use super::RoomCtx;

/// 同一次竞争调用内共享的优先级候选；不含世界写句柄。
pub struct MoveContest {
    input: kk_mod_api::contest::MoveContestInput,
    /// 本次输入中的合法竞争者集合，写面不认其他对象。
    contenders: BTreeSet<ObjId>,
    priorities: RefCell<BTreeMap<ObjId, i64>>,
}

impl MoveContest {
    /// 按竞争输入构造候选；空输入（无竞争格）返回 None，不进入 JS。
    /// 竞争者 id 必须可解析回引擎对象 ID——输入由 `contest_input` 从
    /// 登记矩阵生成，此不变量破坏时按故障处理而非静默收窄写面。
    pub fn new(input: kk_mod_api::contest::MoveContestInput) -> Result<Option<Self>, String> {
        if input.contests.is_empty() {
            return Ok(None);
        }
        let mut contenders = BTreeSet::new();
        for info in input
            .contests
            .iter()
            .flat_map(|cell| cell.contenders.iter())
        {
            let id = ObjId::parse(&info.id)
                .ok_or_else(|| format!("竞争输入含不可解析的对象 ID {}", info.id))?;
            contenders.insert(id);
        }
        Ok(Some(Self {
            input,
            contenders,
            priorities: RefCell::new(BTreeMap::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::contest::MoveContestInput {
        &self.input
    }

    /// 读取当前优先级；未设置为 0（原生默认层）。
    pub fn priority(&self, id: ObjId) -> i64 {
        self.priorities.borrow().get(&id).copied().unwrap_or(0)
    }

    /// 写入优先级；可多次调整以组合处理器。id 必须是本次输入中的
    /// 竞争者，数值须在 ±2^53-1 的 JS 安全整数域内。
    pub fn set_priority(&self, id: ObjId, value: i64) -> Result<(), String> {
        if !self.contenders.contains(&id) {
            return Err(format!("竞争优先级目标 {} 不在本次输入中", id.as_str()));
        }
        if !(-MAX_PRIORITY..=MAX_PRIORITY).contains(&value) {
            return Err("竞争优先级须为 ±(2^53-1) 内的 JS 安全整数".into());
        }
        self.priorities.borrow_mut().insert(id, value);
        Ok(())
    }

    /// 取出终值候选；只含被显式设置过的竞争者。
    pub fn priorities(&self) -> BTreeMap<ObjId, i64> {
        self.priorities.borrow().clone()
    }
}

/// 房间竞争仲裁入口：有贡献槽时经整槽宿主生成优先级候选再仲裁，
/// 无贡献或无竞争格直接走原生默认。必须在不持有候选借用时调用。
pub fn arbitrate_room(ctx: &RoomCtx) -> Result<(), String> {
    let contrib = ctx.borrow().plane.contrib.clone();
    match contrib.run_contest(ctx)? {
        Some(priorities) => ctx.borrow_mut().check_movement(&priorities),
        None => {
            let empty = BTreeMap::new();
            ctx.borrow_mut().check_movement(&empty);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "contest/tests.rs"]
mod tests;
