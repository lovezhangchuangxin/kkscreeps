//! 预检候选写面与"否决等同从未登记"的下游等价测试。
use super::*;
use crate::settle::movement;
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

/// 竞争格场景：strong(move,move) 与 weak(move) 都请求 (40,19)，
/// strong 功率高原生胜出；另有独行请求者 lone。
fn contested_room() -> (RoomState, movement::Movement) {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(
        &mut room,
        ObjId(1),
        40,
        20,
        &[BodyKind::Move, BodyKind::Move],
    );
    m.add(ObjId(1), (40, 20), 0, -1);
    put(&mut room, ObjId(2), 41, 20, &[BodyKind::Move]);
    m.add(ObjId(2), (41, 20), -1, -1);
    put(&mut room, ObjId(3), 10, 10, &[BodyKind::Move]);
    m.add(ObjId(3), (10, 10), 1, 0);
    (room, m)
}

#[test]
fn precheck_input_lists_requests_in_id_order_with_context() {
    let (room, m) = contested_room();
    let index = RoomIndex::build(&room.objects);
    let input = m.precheck_input(&room, &index);
    assert_eq!(input.requests.len(), 3, "全部登记请求可见、不截断");
    // 按对象 ID 升序。
    let ids: Vec<&str> = input.requests.iter().map(|r| r.mover_id.as_str()).collect();
    assert_eq!(
        ids,
        [ObjId(1).as_str(), ObjId(2).as_str(), ObjId(3).as_str()]
    );
    let strong = &input.requests[0];
    assert_eq!((strong.origin_x, strong.origin_y), (40, 20));
    assert_eq!((strong.target_x, strong.target_y), (40, 19));
    assert_eq!(strong.move_power, 2, "含 boost 的 MOVE 功率");
    assert_eq!(strong.weight, 1);
    assert_eq!(strong.owner, "p1");
    assert!(!strong.terrain_wall, "平原目的格非墙");
    assert!(!strong.terrain_swamp);
    assert!(!strong.has_road);
    assert!(strong.objects.is_empty(), "空格无占位对象");
    let weak = &input.requests[1];
    assert_eq!((weak.target_x, weak.target_y), (40, 19), "竞争者各自登记");
    assert!(!weak.pulled && !weak.pulling);
}

#[test]
fn blocked_request_never_contests_and_rival_takes_cell() {
    // 请求级语义：否决 strong 的请求使其不参与竞争，weak 成为
    // (40,19) 的唯一登记者而胜出——与障碍槽（挡胜者后无人可动）
    // 形成对照。
    let (room, mut m) = contested_room();
    let index = RoomIndex::build(&room.objects);
    let input = m.precheck_input(&room, &index);
    let candidate = MovePrecheck::new(input).unwrap().unwrap();
    candidate.block(ObjId(1)).unwrap();
    assert!(candidate.is_blocked(ObjId(1)));
    assert!(!candidate.is_blocked(ObjId(2)), "本场景弱者无静态障碍");
    let blocks = candidate.into_blocks();
    m.apply_precheck_blocks(&blocks);

    // 原生基线对照：无否决时 strong 胜出。
    let (base_room, mut base_m) = contested_room();
    base_m.rank_winners(&base_room, &BTreeMap::new());
    assert_eq!(base_m.winners().get(&ObjId(1)), Some(&(40u8, 19u8)));
    assert!(!base_m.winners().contains_key(&ObjId(2)));

    // 应用后、选胜前：被否决者的登记已从矩阵消失（spawn 放置的
    // is_tile_busy 同样读不到），目的格只剩弱者一条。
    assert_eq!(m.request_cell_of(ObjId(1)), None, "被否决请求已移除");
    assert_eq!(m.request_cell_of(ObjId(2)), Some((40u8, 19u8)));
    assert_eq!(
        m.debug_matrix()
            .iter()
            .find(|(c, _)| *c == (40u8, 19u8))
            .map(|(_, ids)| ids.len()),
        Some(1),
        "竞争格只剩唯一登记者"
    );
    m.rank_winners(&room, &BTreeMap::new());
    assert!(
        !m.winners().contains_key(&ObjId(1)),
        "被否决请求等同从未登记"
    );
    assert_eq!(m.winners().get(&ObjId(2)), Some(&(40u8, 19u8)));
    assert_eq!(m.winners().get(&ObjId(3)), Some(&(11u8, 10u8)));
    // 准入槽的整房请求视野同样看不到被否决请求。
    m.prefill_admissions(&room, &BTreeMap::new());
    let admission_input = m.admission_input(&room, &BTreeMap::new());
    assert!(
        admission_input
            .requests
            .iter()
            .all(|r| r.mover_id != ObjId(1).as_str())
    );
}

#[test]
fn blocked_puller_breaks_pulled_chain_at_arbitration() {
    // 否决拉者请求：拉者原地不动成为未让位占据者，被拉者在仲裁
    // 链内被取消——与"拉者请求从未登记"的下游完全一致。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let puller = ObjId(9);
    put(&mut room, puller, 10, 10, &[BodyKind::Move]);
    m.add(puller, (10, 10), 0, -1);
    put(&mut room, ObjId(1), 10, 11, &[]);
    m.add(ObjId(1), (10, 11), 0, -1);
    m.add_pulling(&room, puller, ObjId(1));
    let index = RoomIndex::build(&room.objects);
    let input = m.precheck_input(&room, &index);
    let pulled_row = input
        .requests
        .iter()
        .find(|r| r.mover_id == ObjId(1).as_str())
        .unwrap();
    assert!(pulled_row.pulled, "登记时点的原始被拉标记");
    let candidate = MovePrecheck::new(input).unwrap().unwrap();
    candidate.block(puller).unwrap();
    m.apply_precheck_blocks(&candidate.into_blocks());

    // 否决只移除请求登记，不动 pull 配对：被拉者仍以有效被拉身份
    // 通过准入门槛，但拉者不再让位，链内必被取消——与拉者请求从未
    // 登记的基线逐步一致（基线里 pull 配对同样保留）。
    let baseline = |m: &mut movement::Movement, room: &RoomState| {
        m.rank_winners(room, &BTreeMap::new());
        m.prefill_admissions(room, &BTreeMap::new());
        assert!(m.admitted(ObjId(1)), "配对完好：被拉免除部件门槛");
        m.cancel_blocked(room, &RoomIndex::build(&room.objects), None);
        m.winners().clone()
    };
    // 基线：拉者发出 pull 但从未登记移动请求。
    let mut base_room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut base_m = movement::Movement::new();
    put(&mut base_room, puller, 10, 10, &[BodyKind::Move]);
    put(&mut base_room, ObjId(1), 10, 11, &[]);
    base_m.add(ObjId(1), (10, 11), 0, -1);
    base_m.add_pulling(&base_room, puller, ObjId(1));
    let base_winners = baseline(&mut base_m, &base_room);

    m.rank_winners(&room, &BTreeMap::new());
    m.prefill_admissions(&room, &BTreeMap::new());
    assert!(m.admitted(ObjId(1)), "否决不动配对：被拉门槛仍豁免");
    m.cancel_blocked(&room, &index, None);
    assert_eq!(
        m.winners(),
        &base_winners,
        "拉者被否决后下游与从未登记一致：均为空幸存"
    );
    assert!(m.winners().is_empty());
}

#[test]
fn candidate_validates_domain_and_empty_input() {
    let (room, m) = contested_room();
    let index = RoomIndex::build(&room.objects);
    let input = m.precheck_input(&room, &index);
    let candidate = MovePrecheck::new(input).unwrap().unwrap();
    // 域外对象拒绝写入而非静默收窄。
    assert!(candidate.block(ObjId(99)).is_err());
    assert!(candidate.block(ObjId(1)).is_ok());
    let snapshot = candidate.blocks_snapshot();
    assert!(snapshot[&ObjId(1)]);
    assert!(!snapshot[&ObjId(2)]);
    assert_eq!(candidate.into_blocks(), [ObjId(1)].into_iter().collect());
    // 空输入（无登记请求）不构造候选。
    let empty = movement::Movement::new();
    let input = empty.precheck_input(&room, &index);
    assert!(input.requests.is_empty());
    assert!(MovePrecheck::new(input).unwrap().is_none());
}

// 静态放行必须进入正式取消链及准入预演；动态占格仍会级联取消。
#[test]
fn static_allowance_matches_preview_and_keeps_dynamic_cancellation() {
    for occupied in [false, true] {
        let mut codes = vec![0; 2500];
        codes[10 * 50 + 11] = kk_core::TERRAIN_MASK_WALL;
        let mut room = RoomState::new("W1N1", kk_core::Terrain::from_codes(&codes).unwrap());
        let mut m = movement::Movement::new();
        put(&mut room, ObjId(1), 10, 10, &[BodyKind::Move]);
        put(&mut room, ObjId(2), 9, 10, &[BodyKind::Move]);
        m.add(ObjId(1), (10, 10), 1, 0);
        m.add(ObjId(2), (9, 10), 1, 0);
        if occupied {
            put(&mut room, ObjId(3), 11, 10, &[BodyKind::Work]);
        }
        let index = RoomIndex::build(&room.objects);
        let candidate = MovePrecheck::new(m.precheck_input(&room, &index))
            .unwrap()
            .unwrap();
        assert!(candidate.is_blocked(ObjId(1)));
        candidate.allow_static(ObjId(1)).unwrap();
        assert!(!candidate.is_blocked(ObjId(1)));
        m.apply_precheck_decisions(candidate.decisions(true));
        m.rank_winners(&room, &BTreeMap::new());
        m.prefill_admissions(&room, &BTreeMap::new());
        let preview = m.simulate_survivors(
            &room,
            &index,
            None,
            &[(ObjId(1), true), (ObjId(2), true)].into_iter().collect(),
        );
        m.cancel_blocked(&room, &index, None);
        assert_eq!(
            preview,
            m.winners().keys().copied().collect::<BTreeSet<_>>()
        );
        assert_eq!(m.winners().len(), if occupied { 0 } else { 2 });
    }
}

#[test]
fn static_allowance_cannot_clear_explicit_veto_and_decline_restores_registration() {
    let (room, mut m) = contested_room();
    // 模拟登记时挡下、预检时障碍已消失：不能按当前空格误放行。
    m.mark_registration_blocked(ObjId(1), true);
    let input = m.precheck_input(&room, &RoomIndex::build(&room.objects));
    assert!(input.requests[0].native_registration_blocked);
    assert!(!input.requests[0].native_static_blocked);
    let c = MovePrecheck::new(input).unwrap().unwrap();
    assert!(c.is_blocked(ObjId(1)));
    assert!(c.allow_static(ObjId(99)).is_err());
    c.allow_static(ObjId(1)).unwrap();
    assert!(!c.is_blocked(ObjId(1)));
    assert!(c.decisions(true).blocks.is_empty());
    assert!(c.decisions(false).blocks.contains(&ObjId(1)));
    assert!(c.decisions(false).static_allowed.is_empty());
    c.block(ObjId(1)).unwrap();
    c.allow_static(ObjId(1)).unwrap();
    assert!(c.is_blocked(ObjId(1)));
    assert!(c.decisions(true).blocks.contains(&ObjId(1)));
    m.apply_precheck_decisions(c.decisions(true));
    m.rank_winners(&room, &BTreeMap::new());
    assert_eq!(m.winners().get(&ObjId(2)), Some(&(40, 19)));
}

#[test]
fn overwriting_request_clears_static_registration_snapshot() {
    let (room, mut m) = contested_room();
    m.mark_registration_blocked(ObjId(1), true);
    m.add(ObjId(1), (40, 20), 1, 0);
    let input = m.precheck_input(&room, &RoomIndex::build(&room.objects));
    assert!(!input.requests[0].native_registration_blocked);
    assert!(
        !MovePrecheck::new(input)
            .unwrap()
            .unwrap()
            .is_blocked(ObjId(1))
    );
}

#[test]
fn losing_contenders_static_allowance_cannot_leak_to_winner() {
    for allowed in [ObjId(1), ObjId(2)] {
        let mut codes = vec![0; 2500];
        codes[19 * 50 + 40] = kk_core::TERRAIN_MASK_WALL;
        let (plain, mut m) = contested_room();
        let mut room = RoomState::new("W1N1", kk_core::Terrain::from_codes(&codes).unwrap());
        room.objects = plain.objects;
        let index = RoomIndex::build(&room.objects);
        let c = MovePrecheck::new(m.precheck_input(&room, &index))
            .unwrap()
            .unwrap();
        c.allow_static(allowed).unwrap();
        m.apply_precheck_decisions(c.decisions(true));
        m.rank_winners(&room, &BTreeMap::new());
        m.prefill_admissions(&room, &BTreeMap::new());
        m.cancel_blocked(&room, &index, None);
        assert_eq!(m.winners().contains_key(&ObjId(1)), allowed == ObjId(1));
        assert!(
            !m.winners().contains_key(&ObjId(2)),
            "放行落选者不会重新选胜"
        );
    }
}
