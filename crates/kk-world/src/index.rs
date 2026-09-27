//! 50×50 格子索引（ADR-004 第 2 条）：坐标查询从扫全表变查一格。
//!
//! 结算开始时按对象表构建；移动结算集中改坐标后增量更新。

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

    pub fn at(&self, x: usize, y: usize) -> &[ObjId] {
        &self.cells[cell_of(x, y)]
    }

    /// 对象移动后的增量更新。契约：id 当前确实登记在 from 格（结算层保证，
    /// debug 断言兜底；重复调用会在目标格重复登记）。
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

    pub fn insert(&mut self, id: ObjId, at: (u8, u8)) {
        self.cells[cell_of(at.0 as usize, at.1 as usize)].push(id);
    }

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
