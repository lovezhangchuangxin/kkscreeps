//! 跨房移动：边缘格 creep 的房间过渡在候选上的收集与应用。
//!
//! 官方语义（来源：上游 engine/src/processor/intents/creeps/tick.js 的
//! interRoom 标记）：creep 的移动执行完成后，凡位于边缘格的对象（无论
//! 本 tick 是否移动过——刚到达邻房边缘的对象下一 tick 会再次过渡）按
//! x==0 西、y==0 北、x==49 东、y==49 南的优先级过渡到邻房的镜像格；
//! 角格取首个命中方向。标记时向源房事件日志推 EVENT_EXIT（事件先于
//! 迁移是否可应用发出）。到达格不做占用或地形检查（可以与既有对象
//! 重叠、可以是墙格）。官方排除 NPC 用户（Source Keeper/Invader）；
//! 本仓按保留用户名排除 Source Keeper 与 Invader。portal 触发的跨房
//! 与 powerCreep 的同机制属官方同文件相邻分支，随对应对象形态实现。

use kk_core::RoomName;
use kk_world::{ObjId, RoomState};

/// 一次待应用的跨房过渡：对象从结算来源房移入目标房的镜像格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterRoomMove {
    pub id: ObjId,
    pub to_room: String,
    pub x: u8,
    pub y: u8,
}

/// 计算边缘格 `(x, y)` 在房 `room_name` 中的过渡目标；非边缘格返回 None。
///
/// 房名换算沿用官方坐标约定（W/N 为负并减一），非法房名返回 None——
/// 过渡静默不发生，与未知动作丢弃同待遇。
pub fn edge_target(room_name: &str, x: u8, y: u8) -> Option<(String, u8, u8)> {
    let to = match (x, y) {
        (0, _) => (49, y, -1, 0),
        (_, 0) => (x, 49, 0, -1),
        (49, _) => (0, y, 1, 0),
        (_, 49) => (x, 0, 0, 1),
        _ => return None,
    };
    let parsed = RoomName::parse(room_name).ok()?;
    let name = RoomName::from_xy(parsed.xx + to.2, parsed.yy + to.3).as_str();
    Some((name, to.0, to.1))
}

/// 在全部房间结算后的候选上应用跨房过渡。
///
/// 应用顺序即收集序（房名字典序、房内对象 ID 升序）。对象 ID 全局唯一，
/// 按 ID 在候选中定位源房；候选中已无该对象（本 tick 内死亡或被移除）
/// 时跳过。目标房不在候选中（未注册房间）时对象放回原处——官方世界
/// 全域存在房间，本仓世界仅含注册房，未注册即无处可去。对象的扩展
/// 字段条目随对象迁移，源房不残留孤儿条目。
pub fn apply_inter_room_moves(rooms: &mut [(String, RoomState)], moves: &[InterRoomMove]) {
    for mv in moves {
        let Some(source_idx) = rooms
            .iter()
            .position(|(_, room)| room.objects.contains_key(&mv.id))
        else {
            continue;
        };
        let Some(mut obj) = rooms[source_idx].1.objects.remove(&mv.id) else {
            continue;
        };
        let Some(target_idx) = rooms.iter().position(|(name, _)| name == &mv.to_room) else {
            rooms[source_idx].1.objects.insert(mv.id, obj);
            continue;
        };
        obj.set_pos(mv.x, mv.y);
        rooms[target_idx].1.objects.insert(mv.id, obj);
        if let Some(entries) = rooms[source_idx].1.effects.remove(&mv.id) {
            rooms[target_idx].1.effects.insert(mv.id, entries);
        }
        if let Some(fields) = rooms[source_idx].1.extensions.remove(&mv.id) {
            rooms[target_idx].1.extensions.insert(mv.id, fields);
        }
        // Keeper 巡守记忆与 effects/extensions 同形随迁。当前 NPC 不跨房
        //（movement 显式排除），本迁移是防御：源房滞留会在候选发布校验
        //（记忆引用无效执行者）时整轮拒绝且同输入同失败。
        if let Some(memory) = rooms[source_idx].1.keeper_memory.remove(&mv.id) {
            rooms[target_idx].1.keeper_memory.insert(mv.id, memory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_state_moves_with_host_and_stays_when_destination_is_absent() {
        let mut source = RoomState::new("W1N1", kk_core::Terrain::plains());
        let object = kk_schema::room::RoomObject::Generic(kk_schema::room::GenericObj {
            id: 1,
            x: 0,
            y: 20,
            data: serde_json::json!({"type":"powerCreep"}).to_string(),
        });
        source.insert_with_id(1, object).unwrap();
        let entries = vec![kk_world::EffectEntry {
            effect: Some(3),
            power: 3,
            level: 1,
            end_time: 100,
        }];
        source.effects.insert(ObjId(1), entries.clone());
        let memory = kk_world::domain::keeper_memory::KeeperMemory {
            source_id: Some(9),
            move_cache: None,
        };
        source.keeper_memory.insert(ObjId(1), memory.clone());
        let mut rooms = vec![("W1N1".into(), source)];
        let moves = [InterRoomMove {
            id: ObjId(1),
            to_room: "W2N1".into(),
            x: 49,
            y: 20,
        }];
        apply_inter_room_moves(&mut rooms, &moves);
        assert_eq!(rooms[0].1.effects[&ObjId(1)], entries);
        assert_eq!(rooms[0].1.keeper_memory[&ObjId(1)], memory);
        rooms.push((
            "W2N1".into(),
            RoomState::new("W2N1", kk_core::Terrain::plains()),
        ));
        apply_inter_room_moves(&mut rooms, &moves);
        assert!(rooms[0].1.effects.is_empty());
        assert!(rooms[0].1.keeper_memory.is_empty());
        assert_eq!(rooms[1].1.effects[&ObjId(1)], entries);
        assert_eq!(rooms[1].1.keeper_memory[&ObjId(1)], memory);
        assert_eq!(rooms[1].1.objects[&ObjId(1)].pos(), (49, 20));
    }
}
