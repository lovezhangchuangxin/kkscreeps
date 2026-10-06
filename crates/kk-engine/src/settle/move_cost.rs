//! 移动代价候选：逐幸存者疲劳数值替换，缺项回退原生代价。
//!
//! 输入为取消链结算后的终局幸存者；能力面只有设置覆盖值（升/降双向，
//! 可替换规则），值域 0..=u32::MAX（疲劳存储为 u32）。疲劳转记给链首
//! 拉者、道路磨损、工地踩踏与跨房疲劳归零保持原生，不在替换面内。
//! 候选在整槽调用期间由 JS 能力写入；成功退出后引擎把覆盖表交给
//! 执行阶段消费，正常拒绝丢弃全部覆盖。候选不进入检查点。

use std::cell::RefCell;
use std::collections::BTreeMap;

use kk_world::ObjId;

/// 同一次代价调用内共享的覆盖候选；不含世界写句柄。
pub struct MoveCost {
    input: kk_mod_api::move_cost::MoveCostInput,
    /// 本次输入中的幸存者集合与其原生代价，写面不认其他对象。
    movers: BTreeMap<ObjId, i64>,
    costs: RefCell<BTreeMap<ObjId, u32>>,
}

impl MoveCost {
    /// 按代价输入构造候选；空输入（无幸存者）返回 None，不进入 JS。
    /// 移动者 id 必须可解析回引擎对象 ID——输入由 `cost_input` 从
    /// 胜者表生成，此不变量破坏时按故障处理而非静默收窄写面。
    pub fn new(input: kk_mod_api::move_cost::MoveCostInput) -> Result<Option<Self>, String> {
        if input.movers.is_empty() {
            return Ok(None);
        }
        let mut movers = BTreeMap::new();
        for info in &input.movers {
            let id = ObjId::parse(&info.id)
                .ok_or_else(|| format!("代价输入含不可解析的对象 ID {}", info.id))?;
            movers.insert(id, info.native_fatigue);
        }
        Ok(Some(Self {
            input,
            movers,
            costs: RefCell::new(BTreeMap::new()),
        }))
    }

    pub fn input(&self) -> &kk_mod_api::move_cost::MoveCostInput {
        &self.input
    }

    /// 读取当前代价：未覆盖项为原生代价（不是 0）。
    pub fn fatigue_of(&self, id: ObjId) -> Option<i64> {
        self.costs
            .borrow()
            .get(&id)
            .map(|cost| *cost as i64)
            .or_else(|| self.movers.get(&id).copied())
    }

    /// 设置覆盖；id 须为本次输入中的幸存者，值域 0..=u32::MAX。
    pub fn set_fatigue(&self, id: ObjId, value: i64) -> Result<(), String> {
        if !self.movers.contains_key(&id) {
            return Err(format!("代价目标 {} 不在本次输入中", id.as_str()));
        }
        if !(0..=u32::MAX as i64).contains(&value) {
            return Err("移动代价须为 0..=u32::MAX 内的整数".into());
        }
        self.costs.borrow_mut().insert(id, value as u32);
        Ok(())
    }

    /// 取出覆盖表（接管所有权）；只在链成功后由引擎调用一次。
    /// 只含被显式设置过的幸存者，缺项即原生。
    pub fn into_costs(&self) -> BTreeMap<ObjId, u32> {
        std::mem::take(&mut *self.costs.borrow_mut())
    }

    /// 覆盖快照；组合诊断与测试用，不消费候选。
    pub fn costs_snapshot(&self) -> BTreeMap<ObjId, i64> {
        self.movers
            .keys()
            .map(|id| (*id, self.fatigue_of(*id).unwrap_or_default()))
            .collect()
    }
}

#[cfg(test)]
#[path = "move_cost/tests.rs"]
mod tests;
