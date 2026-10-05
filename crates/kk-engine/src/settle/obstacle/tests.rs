//! 障碍槽输入要素、追加取消与让位链传播、候选域测试。
use super::*;
use crate::settle::movement;
use kk_schema::room::{BodyPart, CreepObj, RoomObject};
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};
use std::collections::{BTreeMap, BTreeSet};

fn put(room: &mut RoomState, id: ObjId, x: u8, y: u8) {
    room.objects.insert(
        id,
        RoomObject::Creep(CreepObj {
            id: 0,
            x,
            y,
            fatigue: 0,
            age_time: u64::MAX / 2,
            hits: 100,
            spawning: false,
            action_log: 0,
            name: format!("c{}", id.0),
            owner: "p1".into(),
            body: vec![BodyPart {
                kind: BodyKind::Move.code(),
                hits: 100,
                boost: None,
            }],
            store: kk_schema::room::Store {
                energy: 0,
                ghodium: 0,
            },
            store_extra: Vec::new(),
            say_message: None,
            say_public: None,
            notify_when_attacked: None,
        }),
    );
}

fn wall_terrain_at(x: usize, y: usize) -> kk_core::Terrain {
    let mut codes = [0u8; 2500];
    codes[y * 50 + x] |= kk_core::TERRAIN_MASK_WALL;
    kk_core::Terrain::from_codes(&codes).expect("墙位码合法")
}

#[test]
fn obstacle_input_carries_cell_summary_and_native_verdict() {
    // a(10,10)→(10,11) 空平原格；b(20,20)→(20,21)，格上站着未登记的
    // 静止 creep；c(30,30)→(30,31)，格上站着同时获准让位的 d。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(1), 10, 10);
    m.add(ObjId(1), (10, 10), 0, 1);
    put(&mut room, ObjId(2), 20, 20);
    put(&mut room, ObjId(3), 20, 21); // 静止占位者
    m.add(ObjId(2), (20, 20), 0, 1);
    put(&mut room, ObjId(4), 30, 30);
    m.add(ObjId(4), (30, 30), 0, 1);
    put(&mut room, ObjId(5), 30, 31);
    m.add(ObjId(5), (30, 31), 0, -1); // d 让位离开 (30,31)
    m.rank_winners(&room, &BTreeMap::new());
    m.prefill_admissions(&room, &BTreeMap::new());
    let input = m.obstacle_input(&room, &RoomIndex::build(&room.objects), None);
    let by_id = |id: &str| {
        input
            .movers
            .iter()
            .find(|info| info.id == id)
            .unwrap_or_else(|| panic!("缺 {id}"))
    };
    let a = by_id(&ObjId(1).as_str().to_string());
    assert!(!a.native_blocked);
    assert!(a.objects.is_empty(), "空格无对象摘要");
    assert!(!a.terrain_wall);
    let b = by_id(&ObjId(2).as_str().to_string());
    assert!(b.native_blocked, "静止 creep 阻挡");
    assert_eq!(b.objects.len(), 1);
    assert_eq!(b.objects[0].kind, "creep");
    assert!(!b.objects[0].yielding);
    let d = by_id(&ObjId(5).as_str().to_string());
    // d(30,31)→(30,30)：目的格摘要含 c，c 是胜者且准入终值为真 →
    // 让位标记可见、不阻挡。
    assert!(!d.native_blocked, "获准让位者不阻挡");
    assert!(
        d.objects.iter().any(|o| o.kind == "creep" && o.yielding),
        "让位标记可见"
    );
}

#[test]
fn obstacle_input_reports_wall_terrain_without_road() {
    let mut room = RoomState::new("W1N1", wall_terrain_at(10, 11));
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(1), 10, 10);
    m.add(ObjId(1), (10, 10), 0, 1);
    m.rank_winners(&room, &BTreeMap::new());
    let input = m.obstacle_input(&room, &RoomIndex::build(&room.objects), None);
    let a = &input.movers[0];
    assert!(a.terrain_wall);
    assert!(!a.has_road);
    assert!(a.native_blocked, "无路的墙地形 native 阻挡");
}

#[test]
fn extra_block_cancels_mover_and_dependent_chain() {
    // 交换让位链：a(10,10)→(10,11)，b(10,11)→(10,10)。JS 追加阻挡 a
    // 后：a 取消 → 递归取消 b（a 不再让位、阻挡 (10,10)）。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(1), 10, 10);
    m.add(ObjId(1), (10, 10), 0, 1);
    put(&mut room, ObjId(2), 10, 11);
    m.add(ObjId(2), (10, 11), 0, -1);
    let old_fatigue = BTreeMap::new();
    m.rank_winners(&room, &BTreeMap::new());
    m.prefill_admissions(&room, &old_fatigue);
    let index = RoomIndex::build(&room.objects);
    let input = m.obstacle_input(&room, &index, None);
    // 预填时点两者均不被阻挡（a 让位成立）。
    for info in &input.movers {
        assert!(!info.native_blocked);
    }
    let candidate = MoveObstacle::new(input).unwrap().unwrap();
    candidate.block(ObjId(1)).unwrap();
    let blocks = candidate.into_blocks();
    assert_eq!(blocks.len(), 1);
    m.apply_extra_blocks(blocks);
    m.cancel_blocked(&room, &index, None);
    assert!(
        m.winners().is_empty(),
        "追加阻挡随让位链传播：a 与依赖其让位的 b 全部取消"
    );
    // 无追加的原生对照：两步链移动全部成立。
    let mut m2 = movement::Movement::new();
    m2.add(ObjId(1), (10, 10), 0, 1);
    m2.add(ObjId(2), (10, 11), 0, -1);
    m2.check_impl(&room, &index, &old_fatigue, None, &BTreeMap::new());
    assert_eq!(m2.winners().len(), 2, "原生对照两步链移动成立");
}

#[test]
fn obstacle_candidate_validates_domain_and_empty_input() {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(1), 10, 10);
    m.add(ObjId(1), (10, 10), 0, 1);
    m.rank_winners(&room, &BTreeMap::new());
    let input = m.obstacle_input(&room, &RoomIndex::build(&room.objects), None);
    let candidate = MoveObstacle::new(input).unwrap().unwrap();
    assert!(!candidate.is_blocked(ObjId(1)), "空平原格初判不阻挡");
    // 域外对象拒绝写入。
    assert!(candidate.block(ObjId(99)).is_err());
    candidate.block(ObjId(1)).unwrap();
    assert!(candidate.is_blocked(ObjId(1)));
    let blocks = candidate.into_blocks();
    assert_eq!(blocks, BTreeSet::from([ObjId(1)]));
    assert!(candidate.into_blocks().is_empty(), "接管后追加集已取空");
    // 空输入不构造候选。
    let empty = movement::Movement::new();
    let input = empty.obstacle_input(&room, &RoomIndex::build(&room.objects), None);
    assert!(input.movers.is_empty());
    assert!(MoveObstacle::new(input).unwrap().is_none());
}
