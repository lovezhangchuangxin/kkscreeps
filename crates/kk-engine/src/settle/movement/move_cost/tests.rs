//! 代价候选写面、输入投影与"缺项回退原生"的测试。
use super::*;
use crate::settle::{Engine, Plane, TickState, Working, movement};
use kk_schema::room::{BodyPart, CreepObj, RoomObject};
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};
use std::collections::BTreeMap;

fn creep_with(parts: &[BodyKind]) -> CreepObj {
    CreepObj {
        id: 0,
        x: 0,
        y: 0,
        fatigue: 0,
        age_time: u64::MAX / 2,
        hits: 100,
        spawning: false,
        action_log: 0,
        name: String::new(),
        owner: "p1".into(),
        body: parts
            .iter()
            .map(|kind| BodyPart {
                kind: kind.code(),
                hits: 100,
                boost: None,
            })
            .collect(),
        store: kk_schema::room::Store {
            energy: 0,
            ghodium: 0,
        },
        store_extra: Vec::new(),
        say_message: None,
        say_public: None,
        notify_when_attacked: None,
    }
}

fn put(room: &mut RoomState, id: ObjId, x: u8, y: u8, parts: &[BodyKind]) {
    let mut creep = creep_with(parts);
    creep.x = x;
    creep.y = y;
    room.objects.insert(id, RoomObject::Creep(creep));
}

#[test]
fn cost_input_lists_survivors_with_native_factors() {
    // 被链取消者不进输入；幸存者按对象 ID 升序，附原生代价要素
    // （平原 2、沼泽 10、路 1 覆盖沼泽）与链首拉者。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    // 被拉者 1(work,move) 请求拉者 9 的当前格 (10,11)；拉者让位到
    // (10,12)——配对完好通过复核，cost 输入携带链首拉者。
    put(
        &mut room,
        ObjId(1),
        10,
        10,
        &[BodyKind::Move, BodyKind::Work],
    );
    m.add(ObjId(1), (10, 10), 0, 1);
    put(&mut room, ObjId(2), 40, 40, &[BodyKind::Move]);
    m.add(ObjId(2), (40, 40), 0, -1);
    m.add_pulling(&room, ObjId(9), ObjId(1));
    put(&mut room, ObjId(9), 10, 11, &[BodyKind::Move]);
    m.add(ObjId(9), (10, 11), 0, 1);
    m.rank_winners(&room, &BTreeMap::new());
    m.prefill_admissions(&room, &BTreeMap::new(), &Default::default());
    m.cancel_blocked(&room, &RoomIndex::build(&room.objects), None);
    let input = m.cost_input(&room, &RoomIndex::build(&room.objects));
    assert_eq!(input.movers.len(), 3, "全部幸存者可见");
    let first = &input.movers[0];
    assert_eq!(first.id, ObjId(1).as_str());
    assert_eq!(first.weight, 1, "work 件计重（裸 move_weight 同源）");
    assert_eq!(first.body_length, 2);
    assert_eq!(first.native_fatigue, 2, "平原费率 2");
    assert!(!first.terrain_swamp && !first.has_road);
    assert_eq!(first.pulled_by, Some(ObjId(9).as_str()));
    assert_eq!(input.movers[2].weight, 0, "纯 MOVE 件自重 0");
    assert_eq!(input.movers[2].native_fatigue, 0, "原生实收 0");
    // 无幸存者不构造候选。
    let empty = movement::Movement::new();
    let input = empty.cost_input(&room, &RoomIndex::build(&room.objects));
    assert!(input.movers.is_empty());
    assert!(MoveCost::new(input).unwrap().is_none());
}

#[test]
fn overridden_cost_replaces_native_and_missing_falls_back() {
    // 覆盖替换原生代价；未覆盖项执行原生；getFatigue 缺省即原生值。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(
        &mut room,
        ObjId(1),
        10,
        10,
        &[BodyKind::Move, BodyKind::Work],
    );
    m.add(ObjId(1), (10, 10), 0, -1);
    put(
        &mut room,
        ObjId(2),
        40,
        40,
        &[BodyKind::Move, BodyKind::Work],
    );
    m.add(ObjId(2), (40, 40), 0, -1);
    m.rank_winners(&room, &BTreeMap::new());
    m.prefill_admissions(&room, &BTreeMap::new(), &Default::default());
    m.cancel_blocked(&room, &RoomIndex::build(&room.objects), None);
    let input = m.cost_input(&room, &RoomIndex::build(&room.objects));
    let candidate = MoveCost::new(input).unwrap().unwrap();
    assert_eq!(
        candidate.fatigue_of(ObjId(1)),
        Some(2),
        "缺省为原生代价而非 0"
    );
    candidate.set_fatigue(ObjId(1), 10).unwrap();
    assert_eq!(candidate.fatigue_of(ObjId(1)), Some(10));
    assert_eq!(candidate.fatigue_of(ObjId(2)), Some(2), "未覆盖项原生");
    let costs = candidate.into_costs();
    assert_eq!(costs, BTreeMap::from([(ObjId(1), 10u32)]), "只含显式覆盖");
    m.apply_costs(costs);
    assert_eq!(m.cost_of(ObjId(1)), Some(10));
    assert_eq!(m.cost_of(ObjId(2)), None, "缺项执行原生");
}

#[test]
fn candidate_validates_domain_and_bounds() {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(
        &mut room,
        ObjId(1),
        10,
        10,
        &[BodyKind::Move, BodyKind::Work],
    );
    m.add(ObjId(1), (10, 10), 0, -1);
    m.rank_winners(&room, &BTreeMap::new());
    let input = m.cost_input(&room, &RoomIndex::build(&room.objects));
    let candidate = MoveCost::new(input).unwrap().unwrap();
    // 域外对象与值域边界拒绝写入而非静默收窄。
    assert!(candidate.set_fatigue(ObjId(99), 1).is_err());
    assert!(candidate.set_fatigue(ObjId(1), -1).is_err());
    assert!(
        candidate
            .set_fatigue(ObjId(1), u32::MAX as i64 + 1)
            .is_err()
    );
    assert!(candidate.set_fatigue(ObjId(1), 0).is_ok());
    assert!(candidate.set_fatigue(ObjId(1), u32::MAX as i64).is_ok());
    let snapshot = candidate.costs_snapshot();
    assert_eq!(snapshot[&ObjId(1)], u32::MAX as i64);
}

/// 最小 Working 夹具（与 resources/tests 同型），供疲劳转记链测试。
fn working() -> Working {
    let engine = Engine::default();
    let room = RoomState::new("W1N1", kk_core::Terrain::plains());
    Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            world_span: Plane::UNWRAPPED_SPAN,
            buildings: Default::default(),
            links: engine.links,
            stores: engine.stores,
            combat: engine.combat,
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            static_passability: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan: engine.plan,
        },
    )
}

#[test]
fn chain_transfers_beyond_u32_do_not_wrap_through_recovery() {
    // 链 A←B←C：两个被拉者的代价覆盖（u32::MAX 与 5）同 tick 转记到
    // 链首 A，内部累加 2^32+5；A 随后的疲劳恢复走 resting 分支——
    // 回写必须钳到 u32::MAX，不能经 `as u32` 回绕把惩罚蒸发。
    let mut w = working();
    put(&mut w.room, ObjId(1), 10, 10, &[BodyKind::Move]); // 链首 A
    put(&mut w.room, ObjId(2), 10, 11, &[]); // 被拉 B
    put(&mut w.room, ObjId(3), 11, 11, &[]); // 被拉 C
    w.movement.add_pulling(&w.room, ObjId(1), ObjId(2));
    w.movement.add_pulling(&w.room, ObjId(2), ObjId(3));
    // movement_execute 对每个移动者的收费形态：覆盖值经 add_fatigue
    // 转记给链首。
    w.add_fatigue(ObjId(2), u32::MAX as i64);
    w.add_fatigue(ObjId(3), 5);
    assert_eq!(
        w.tick_fatigue.get(&ObjId(1)),
        Some(&(u32::MAX as i64 + 5)),
        "内部累加跨 2^32"
    );
    // 链首的每 tick 恢复（-2×MOVE 件）走 resting 分支。
    w.add_fatigue(ObjId(1), -2);
    let a = w.creep(ObjId(1)).unwrap();
    assert_eq!(a.fatigue, u32::MAX, "恢复后仍钳在字段上限，不回绕");
}
