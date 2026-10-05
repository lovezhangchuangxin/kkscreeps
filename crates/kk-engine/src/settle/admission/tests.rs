//! 准入候选写面与原生门槛预填的时序测试；同源消费不经二次复核。
use super::*;
use crate::settle::movement;
use kk_mod_api::admission::MoveAdmissionInput;
use kk_schema::room::{BodyPart, CreepObj, RoomObject};
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};
use std::collections::BTreeMap;

fn creep_with(parts: &[BodyKind], fatigue: u32) -> CreepObj {
    CreepObj {
        id: 0,
        x: 0,
        y: 0,
        fatigue,
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
    let mut creep = creep_with(parts, 0);
    creep.x = x;
    creep.y = y;
    // 意图时疲劳经 old_fatigue 表传入仲裁，不落在对象字段上。
    room.objects.insert(id, RoomObject::Creep(creep));
}

/// 一疲劳带 MOVE、一无疲劳无 MOVE、一重载 WORK+WORK 三个移动者
/// 分别从 x=10 列向右移动；目的格互不相同且为空，障碍只来自准入终值。
fn gated_room() -> (RoomState, movement::Movement, BTreeMap<ObjId, u32>) {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let mut old_fatigue = BTreeMap::new();
    // 疲劳 2 的 MOVE creep：原生拒绝。
    put(&mut room, ObjId(1), 10, 10, &[BodyKind::Move]);
    m.add(ObjId(1), (10, 10), 1, 0);
    old_fatigue.insert(ObjId(1), 2);
    // 无疲劳无 MOVE：原生拒绝。
    put(&mut room, ObjId(2), 10, 11, &[]);
    m.add(ObjId(2), (10, 11), 1, 0);
    // 无疲劳带双 WORK：重量 2、无 MOVE 功率，原生拒绝。
    put(
        &mut room,
        ObjId(3),
        10,
        12,
        &[BodyKind::Work, BodyKind::Work],
    );
    m.add(ObjId(3), (10, 12), 1, 0);
    (room, m, old_fatigue)
}

fn rank_and_fill(
    room: &RoomState,
    m: &mut movement::Movement,
    old_fatigue: &BTreeMap<ObjId, u32>,
) -> MoveAdmissionInput {
    m.rank_winners(room, &BTreeMap::new());
    m.prefill_admissions(room, old_fatigue);
    m.admission_input(room, old_fatigue)
}

#[test]
fn admission_input_carries_native_gate_factors() {
    let (room, mut m, old_fatigue) = gated_room();
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    assert_eq!(input.movers.len(), 3, "胜者各占一格，全部进入输入");
    let by_id = |id: &str| {
        input
            .movers
            .iter()
            .find(|info| info.id == id)
            .unwrap_or_else(|| panic!("缺 {id}"))
    };
    let tired = by_id(&ObjId(1).as_str().to_string());
    assert_eq!(tired.old_fatigue, 2);
    assert_eq!(tired.active_moves, 1);
    assert_eq!(tired.move_power, 1);
    assert_eq!(tired.weight, 1);
    assert_eq!((tired.origin_x, tired.origin_y), (10, 10));
    assert_eq!((tired.target_x, tired.target_y), (11, 10));
    assert_eq!(tired.owner, "p1");
    let limbless = by_id(&ObjId(2).as_str().to_string());
    assert_eq!(limbless.active_moves, 0);
    assert_eq!(limbless.move_power, 0);
    assert_eq!(limbless.weight, 1, "重量防御值至少为 1");
    let heavy = by_id(&ObjId(3).as_str().to_string());
    assert_eq!(heavy.active_moves, 0);
    assert_eq!(heavy.weight, 2);
    // 三者均不满足原生门槛。
    assert!(!m.admitted(ObjId(1)));
    assert!(!m.admitted(ObjId(2)));
    assert!(!m.admitted(ObjId(3)));
}

#[test]
fn native_path_cancels_all_and_check_impl_stays_equivalent() {
    let (room, mut m, old_fatigue) = gated_room();
    let index = RoomIndex::build(&room.objects);
    m.check_impl(&room, &index, &old_fatigue, None, &BTreeMap::new());
    assert!(m.winners().is_empty(), "原生门槛下三者全部取消");

    // 分阶段调用与整链组合等价。
    let (room, mut m, old_fatigue) = gated_room();
    let index = RoomIndex::build(&room.objects);
    rank_and_fill(&room, &mut m, &old_fatigue);
    m.cancel_blocked(&room, &index, None);
    assert!(m.winners().is_empty());
}

#[test]
fn override_admits_fatigued_mover_and_cancel_consumes_finals() {
    let (room, mut m, old_fatigue) = gated_room();
    let index = RoomIndex::build(&room.objects);
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    // JS 侧：疲劳者放行，其余保持原生拒绝。
    let native = m.admission_finals().clone();
    let candidate = MoveAdmission::new(input, native).unwrap().unwrap();
    candidate.set_admitted(ObjId(1), true).unwrap();
    assert!(candidate.admitted(ObjId(1)), "覆盖后可读回终值");
    assert!(!candidate.admitted(ObjId(2)), "未覆盖项保持原生值");
    m.apply_admissions(candidate.into_finals());
    assert!(m.admitted(ObjId(1)));
    m.cancel_blocked(&room, &index, None);
    assert_eq!(
        m.winners().get(&ObjId(1)),
        Some(&(11u8, 10u8)),
        "准入覆盖使疲劳移动者保留获准"
    );
    assert!(!m.winners().contains_key(&ObjId(2)));
}

#[test]
fn candidate_validates_mover_domain_and_empty_input() {
    let (room, mut m, old_fatigue) = gated_room();
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    let native = m.admission_finals().clone();
    let candidate = MoveAdmission::new(input, native).unwrap().unwrap();
    // 域外对象拒绝写入而非静默收窄。
    assert!(candidate.set_admitted(ObjId(99), true).is_err());
    assert!(candidate.set_admitted(ObjId(1), false).is_ok());
    assert!(!candidate.admitted(ObjId(1)));
    // 空输入（无胜者）不构造候选。
    let empty = movement::Movement::new();
    let input = empty.admission_input(&room, &BTreeMap::new());
    assert!(input.movers.is_empty());
    assert!(
        MoveAdmission::new(input, BTreeMap::new())
            .unwrap()
            .is_none()
    );
    // 缺项准入 fail-closed。
    assert!(!empty.admitted(ObjId(1)));
}

#[test]
fn effective_pulled_follows_pair_validation_timing() {
    // 拉着走：puller(10,10) 让位到 (10,9)，被拉的 t(10,11) 带疲劳无 MOVE
    // 移向拉者当前格。配对完好时原生即获准；拉者改拉他人后配对破裂，
    // 同一移动者转为拒绝——准入要素的 pulled 标记必须复现这个时序。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let puller = ObjId(9);
    put(&mut room, puller, 10, 10, &[BodyKind::Move]);
    m.add(puller, (10, 10), 0, -1);
    put(&mut room, ObjId(1), 10, 11, &[]);
    m.add(ObjId(1), (10, 11), 0, -1);
    let mut old_fatigue = BTreeMap::new();
    old_fatigue.insert(ObjId(1), 5);
    m.add_pulling(&room, puller, ObjId(1));
    let index = RoomIndex::build(&room.objects);
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    let t = input
        .movers
        .iter()
        .find(|info| info.id == ObjId(1).as_str())
        .unwrap();
    assert!(t.pulled, "配对完好：有效被拉标记");
    assert!(m.admitted(ObjId(1)), "被拉免除疲劳与 MOVE 门槛");
    m.cancel_blocked(&room, &index, None);
    assert_eq!(m.winners().get(&ObjId(1)), Some(&(10u8, 10u8)));

    // 配对破裂：拉者先拉 t 再改拉 ObjId(2)（未登记移动）——add_pulling
    // 覆写拉者指向但不清理旧标记，t 保留 pulled_by 残标；配对一致性
    // 复核失败 → 有效被拉为假。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let puller = ObjId(9);
    put(&mut room, puller, 10, 10, &[BodyKind::Move]);
    put(&mut room, ObjId(1), 10, 11, &[]);
    put(&mut room, ObjId(2), 9, 9, &[BodyKind::Move]);
    m.add(puller, (10, 10), 0, -1);
    m.add(ObjId(1), (10, 11), 0, -1);
    m.add_pulling(&room, puller, ObjId(1));
    m.add_pulling(&room, puller, ObjId(2));
    let mut old_fatigue = BTreeMap::new();
    old_fatigue.insert(ObjId(1), 5);
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    let t = input
        .movers
        .iter()
        .find(|info| info.id == ObjId(1).as_str())
        .unwrap();
    assert!(!t.pulled, "配对破裂：标记被复核摘除");
    assert!(!m.admitted(ObjId(1)), "疲劳无 MOVE 回到原生拒绝");
}

#[test]
fn admission_override_cannot_bypass_occupied_cell() {
    // f(work) 请求移入 (10,10)，静止 creep s 占据该格且无请求。
    // JS 放行只解除门槛，不绕过障碍/让位语义：s 不是胜者、不会让位，
    // f 的请求仍被阻塞链取消。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(1), 10, 11, &[]);
    put(&mut room, ObjId(2), 10, 10, &[]);
    m.add(ObjId(1), (10, 11), 0, -1);
    let old_fatigue = BTreeMap::new();
    let input = rank_and_fill(&room, &mut m, &old_fatigue);
    assert!(!m.admitted(ObjId(1)), "原生拒绝无件移动者");
    let native = m.admission_finals().clone();
    let candidate = MoveAdmission::new(input, native).unwrap().unwrap();
    candidate.set_admitted(ObjId(1), true).unwrap();
    m.apply_admissions(candidate.into_finals());
    let index = RoomIndex::build(&room.objects);
    m.cancel_blocked(&room, &index, None);
    assert!(!m.winners().contains_key(&ObjId(1)), "放行不绕过被占目的格");
}

#[test]
fn non_creep_winner_gets_defensive_factors_and_foreign_read_fails_closed() {
    // 非 creep 对象（flag）直接登记请求并胜出：要素按防御值填充、
    // 原生准入为否。域外对象读取同样按未获准返回。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    room.objects.insert(
        ObjId(5),
        RoomObject::Flag(kk_schema::room::FlagObj {
            id: 0,
            x: 10,
            y: 10,
            name: "flag".into(),
            color: 1,
            secondary_color: 1,
            owner: None,
        }),
    );
    m.add(ObjId(5), (10, 10), 1, 0);
    let input = rank_and_fill(&room, &mut m, &BTreeMap::new());
    assert_eq!(input.movers.len(), 1);
    let info = &input.movers[0];
    assert_eq!(info.owner, "");
    assert_eq!(info.active_moves, 0);
    assert_eq!(info.move_power, 0);
    assert_eq!(info.weight, 1);
    assert!(!info.pulled);
    assert!(!m.admitted(ObjId(5)), "非 creep 原生不获准");
    let candidate = MoveAdmission::new(input, BTreeMap::new()).unwrap().unwrap();
    assert!(!candidate.admitted(ObjId(99)), "域外对象读取 fail-closed");
}
