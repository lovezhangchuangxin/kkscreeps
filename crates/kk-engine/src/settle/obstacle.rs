//! 仲裁障碍追加阻挡候选：单向 block，不可放行 native 判定的阻挡。
//!
//! 放行 native 阻挡格会让移动者进入未让位对象占据的格、破坏坐标
//! 表示有效性，因此能力面只有追加；让位链的增量语义（取消 A 后
//! A 反过来阻挡其原格上的请求）保留在引擎链内 native 判定。候选
//! 在整槽调用期间由 JS 能力写入；成功退出后引擎把追加集并入取消
//! 链，正常拒绝丢弃全部追加。候选不进入检查点。

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use kk_world::ObjId;

/// 同一次障碍调用内共享的追加候选；不含世界写句柄。
pub struct MoveObstacle {
    input: kk_mod_api::obstacle::MoveObstacleInput,
    /// 本次输入中的合法胜者集合，写面不认其他对象。
    movers: BTreeSet<ObjId>,
    /// 预填时点的 native 初判（只读）；链内让位取消只会收紧。
    native_flags: BTreeMap<ObjId, bool>,
    blocked: RefCell<BTreeSet<ObjId>>,
}

impl MoveObstacle {
    /// 按障碍输入构造候选；空输入（无胜者）返回 None，不进入 JS。
    /// 胜者 id 必须可解析回引擎对象 ID——输入由 `obstacle_input`
    /// 从胜者表生成，此不变量破坏时按故障处理而非静默收窄写面。
    pub fn new(input: kk_mod_api::obstacle::MoveObstacleInput) -> Result<Option<Self>, String> {
        if input.movers.is_empty() {
            return Ok(None);
        }
        let mut movers = BTreeSet::new();
        let mut native_flags = BTreeMap::new();
        for info in &input.movers {
            let id = ObjId::parse(&info.id)
                .ok_or_else(|| format!("障碍输入含不可解析的对象 ID {}", info.id))?;
            movers.insert(id);
            native_flags.insert(id, info.native_blocked);
        }
        Ok(Some(Self {
            input,
            movers,
            native_flags,
            blocked: RefCell::new(BTreeSet::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::obstacle::MoveObstacleInput {
        &self.input
    }

    /// 读取当前判定：预填时点的 native 初判或已追加的阻挡。
    pub fn is_blocked(&self, id: ObjId) -> bool {
        self.native_flags.get(&id).copied().unwrap_or(false) || self.blocked.borrow().contains(&id)
    }

    /// 追加阻挡；单向能力，不提供放行。id 须为本次输入中的胜者。
    pub fn block(&self, id: ObjId) -> Result<(), String> {
        if !self.movers.contains(&id) {
            return Err(format!("障碍目标 {} 不在本次输入中", id.as_str()));
        }
        self.blocked.borrow_mut().insert(id);
        Ok(())
    }

    /// 取出追加集（接管所有权）；只在链成功后由引擎调用一次。
    pub fn into_blocks(&self) -> BTreeSet<ObjId> {
        std::mem::take(&mut *self.blocked.borrow_mut())
    }
}

#[cfg(test)]
mod tests;
