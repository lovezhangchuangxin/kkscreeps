//! 50×50 格子索引（ADR-004 第 2 条）：坐标查询从扫全表变查一格。
//!
//! 结算按对象表构建，并在移动、插入、删除后显式维护。索引不持有对象，
//! 不检查 ID 唯一性，也不会跟随对象表自动更新。

use kk_core::{ROOM_SIZE, TERRAIN_LEN};
use kk_schema::room::RoomObject;
use std::collections::BTreeMap;

use crate::world::ObjId;

#[derive(Debug, Default, Clone)]
pub struct RoomIndex {
    /// 每格的对象 id 列表（保持插入序；数量小，线性扫描足够）。
    cells: Vec<Vec<ObjId>>,
}

impl RoomIndex {
    /// 从房间对象表建立索引，各格初始按对象 ID 升序排列。
    /// 调用方须保证所有坐标在 0..50 内。
    pub fn build(objects: &BTreeMap<ObjId, RoomObject>) -> Self {
        let mut idx = Self {
            cells: vec![Vec::new(); TERRAIN_LEN],
        };
        for (id, object) in objects {
            let (x, y) = object.pos();
            idx.cells[cell_of(x as usize, y as usize)].push(*id);
        }
        idx
    }

    /// 返回一格的对象 ID，后续更新按追加顺序排列。
    /// 须先用 build 构造索引，且 x、y 都小于 ROOM_SIZE。
    pub fn at(&self, x: usize, y: usize) -> &[ObjId] {
        &self.cells[cell_of(x, y)]
    }

    /// 更新移动后的索引，不修改对象坐标。
    /// 调用方须保证 ID 已在 from 格登记且尚未写入 to 格，两个坐标均合法。
    /// 缺少来源登记只在 debug 构建中断言，release 构建仍会向目标格追加 ID。
    pub fn apply_move(&mut self, id: ObjId, from: (u8, u8), to: (u8, u8)) {
        let old = &mut self.cells[cell_of(from.0 as usize, from.1 as usize)];
        debug_assert!(
            old.contains(&id),
            "apply_move：对象 {id:?} 不在来源格 ({from:?})"
        );
        if let Some(pos) = old.iter().position(|i| *i == id) {
            old.remove(pos);
        }
        self.cells[cell_of(to.0 as usize, to.1 as usize)].push(id);
    }

    /// 向指定格追加 ID，不去重；调用方须提供合法坐标和已构建的索引。
    pub fn insert(&mut self, id: ObjId, at: (u8, u8)) {
        self.cells[cell_of(at.0 as usize, at.1 as usize)].push(id);
    }

    /// 删除指定格中的首个匹配 ID；未找到时不修改，坐标须合法。
    pub fn remove(&mut self, id: ObjId, at: (u8, u8)) {
        let cell = &mut self.cells[cell_of(at.0 as usize, at.1 as usize)];
        if let Some(pos) = cell.iter().position(|i| *i == id) {
            cell.remove(pos);
        }
    }
}

fn cell_of(x: usize, y: usize) -> usize {
    debug_assert!(x < ROOM_SIZE && y < ROOM_SIZE);
    y * ROOM_SIZE + x
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::room::EnergyObj;

    #[test]
    fn build_query_and_move() {
        let mut objects = BTreeMap::new();
        let a = ObjId(1);
        let b = ObjId(2);
        objects.insert(
            a,
            RoomObject::Energy(EnergyObj {
                id: 1,
                x: 5,
                y: 6,
                resource_type: 0,
                amount: 1,
                resource: None,
            }),
        );
        objects.insert(
            b,
            RoomObject::Energy(EnergyObj {
                id: 2,
                x: 5,
                y: 6,
                resource_type: 0,
                amount: 2,
                resource: None,
            }),
        );
        let mut idx = RoomIndex::build(&objects);
        assert_eq!(idx.at(5, 6), &[a, b]);
        assert!(idx.at(0, 0).is_empty());
        idx.apply_move(a, (5, 6), (7, 8));
        assert_eq!(idx.at(5, 6), &[b]);
        assert_eq!(idx.at(7, 8), &[a]);
    }
}
