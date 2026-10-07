//! 拉拽登记候选写面、"否决等同从未登记"的下游等价与配对表双射测试。
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

#[test]
fn pull_input_lists_pairs_in_puller_order_with_context() {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(2), 10, 10, &[BodyKind::Move]);
    put(&mut room, ObjId(1), 10, 11, &[]);
    put(&mut room, ObjId(9), 11, 11, &[BodyKind::Move]);
    put(&mut room, ObjId(5), 12, 11, &[]);
    m.add_pulling(&room, ObjId(9), ObjId(5));
    m.add_pulling(&room, ObjId(2), ObjId(1));
    let input = m.pull_registration_input(&room);
    assert_eq!(input.pulls.len(), 2);
    // 按拉者对象 ID 升序。
    assert_eq!(input.pulls[0].puller_id, ObjId(2).as_str());
    assert_eq!(input.pulls[0].target_id, ObjId(1).as_str());
    assert_eq!((input.pulls[0].puller_x, input.pulls[0].puller_y), (10, 10));
    assert_eq!((input.pulls[0].target_x, input.pulls[0].target_y), (10, 11));
    assert_eq!(input.pulls[0].puller_owner, "p1");
    assert_eq!(input.pulls[1].puller_id, ObjId(9).as_str());
    // 无配对不构造候选。
    let empty = movement::Movement::new();
    assert!(
        PullRegistration::new(empty.pull_registration_input(&room))
            .unwrap()
            .is_none()
    );
}

#[test]
fn denied_pair_equivalent_to_never_registered() {
    // 拉者让位离开 (10,10)，被拉者带疲劳无 MOVE 移向拉者当前格：
    // 配对完好时原生即获准；否决配对后被拉者回到疲劳门槛拒绝，
    // 与从未调用 add_pulling 的基线一致。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let puller = ObjId(9);
    put(&mut room, puller, 10, 10, &[BodyKind::Move]);
    m.add(puller, (10, 10), 0, -1);
    put(&mut room, ObjId(1), 10, 11, &[]);
    m.add(ObjId(1), (10, 11), 0, -1);
    m.add_pulling(&room, puller, ObjId(1));

    // 基线：从未登记配对。
    let mut base_room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut base_m = movement::Movement::new();
    put(&mut base_room, puller, 10, 10, &[BodyKind::Move]);
    base_m.add(puller, (10, 10), 0, -1);
    put(&mut base_room, ObjId(1), 10, 11, &[]);
    base_m.add(ObjId(1), (10, 11), 0, -1);
    base_m.rank_winners(&base_room, &BTreeMap::new());
    let mut base_fatigue = BTreeMap::new();
    base_fatigue.insert(ObjId(1), 5);
    base_m.prefill_admissions(&base_room, &base_fatigue, &Default::default());
    base_m.cancel_blocked(&base_room, &RoomIndex::build(&base_room.objects), None);
    assert!(!base_m.admitted(ObjId(1)), "基线：疲劳无 MOVE 拒绝");

    // 否决配对后逐阶段与基线一致。
    let input = m.pull_registration_input(&room);
    let candidate = PullRegistration::new(input).unwrap().unwrap();
    assert!(!candidate.is_denied(puller), "原生初值恒为未否决");
    candidate.deny(puller).unwrap();
    m.apply_pull_denies(&candidate.into_denies());
    assert!(!m.is_pulled(ObjId(1)), "配对双向移除");
    let index = RoomIndex::build(&room.objects);
    let precheck = m.precheck_input(&room, &index);
    let row = precheck
        .requests
        .iter()
        .find(|r| r.mover_id == ObjId(1).as_str())
        .unwrap();
    assert!(!row.pulled, "预检视野看不到被否决配对");
    m.rank_winners(&room, &BTreeMap::new());
    let mut fatigue = BTreeMap::new();
    fatigue.insert(ObjId(1), 5);
    m.prefill_admissions(&room, &fatigue, &Default::default());
    assert!(!m.admitted(ObjId(1)), "被拉豁免不再生效");
    m.cancel_blocked(&room, &index, None);
    assert_eq!(
        m.winners().get(&ObjId(1)),
        base_m.winners().get(&ObjId(1)),
        "下游与从未登记一致"
    );
}

#[test]
fn candidate_validates_domain_and_empty_input() {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(9), 10, 10, &[BodyKind::Move]);
    put(&mut room, ObjId(1), 10, 11, &[]);
    m.add_pulling(&room, ObjId(9), ObjId(1));
    let input = m.pull_registration_input(&room);
    let candidate = PullRegistration::new(input).unwrap().unwrap();
    // 域外对象拒绝写入而非静默收窄。
    assert!(candidate.deny(ObjId(99)).is_err());
    assert!(candidate.deny(ObjId(1)).is_err(), "被拉者不是否决键");
    assert!(candidate.deny(ObjId(9)).is_ok());
    let snapshot = candidate.denies_snapshot();
    assert!(snapshot[&ObjId(9)]);
    assert_eq!(candidate.into_denies(), [ObjId(9)].into_iter().collect());
}

#[test]
fn add_pulling_overwrites_keep_maps_bijective() {
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(9), 10, 10, &[BodyKind::Move]);
    put(&mut room, ObjId(1), 10, 11, &[]);
    put(&mut room, ObjId(2), 9, 9, &[BodyKind::Move]);
    put(&mut room, ObjId(8), 11, 10, &[BodyKind::Move]);

    // 拉者换目标：旧目标的被拉标记即时清除，无残标到达复核。
    m.add_pulling(&room, ObjId(9), ObjId(1));
    assert!(m.is_pulled(ObjId(1)));
    m.add_pulling(&room, ObjId(9), ObjId(2));
    assert!(!m.is_pulled(ObjId(1)), "换目标清除旧被拉标记");
    assert!(m.is_pulled(ObjId(2)));

    // 目标换拉者：旧拉者的拉动标记即时清除。
    m.add_pulling(&room, ObjId(8), ObjId(2));
    assert!(!m.is_pulled(ObjId(1)));
    assert_eq!(m.pull_registration_input(&room).pulls.len(), 1);
    let pair = &m.pull_registration_input(&room).pulls[0];
    assert_eq!(pair.puller_id, ObjId(8).as_str(), "换拉者清除旧拉动标记");

    // 重复登记同一配对是幂等操作。
    m.add_pulling(&room, ObjId(8), ObjId(2));
    assert_eq!(m.pull_registration_input(&room).pulls.len(), 1);
}

#[test]
fn pair_validation_is_traversal_order_independent() {
    // 拉者改拉后两个旧/新目标都是胜者：旧目标的无残标保证新配对
    // 不因遍历顺序被连带清除（旧行为里先复核的失败配对会清掉
    // pull_of，胜者 ID 升序即对象创建序，结果任意）。
    for (stale, current) in [(ObjId(1), ObjId(5)), (ObjId(5), ObjId(1))] {
        let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
        let mut m = movement::Movement::new();
        let puller = ObjId(9);
        put(&mut room, puller, 10, 10, &[BodyKind::Move]);
        m.add(puller, (10, 10), 0, -1);
        // 旧目标请求别处并胜出；新目标空身体带疲劳请求拉者当前格，
        // 准入完全依赖被拉豁免——旧实现里残标复核会连带清除当前
        // 配对，低 id 布局下豁免消失即红。
        put(&mut room, stale, 10, 11, &[BodyKind::Move]);
        m.add(stale, (10, 11), 1, 0);
        put(&mut room, current, 9, 10, &[]);
        m.add(current, (9, 10), 1, 0);
        m.add_pulling(&room, puller, stale);
        m.add_pulling(&room, puller, current);
        assert!(!m.is_pulled(stale), "登记期已清除旧标记");
        m.rank_winners(&room, &BTreeMap::new());
        let mut fatigue = BTreeMap::new();
        fatigue.insert(current, 5);
        m.prefill_admissions(&room, &fatigue, &Default::default());
        assert!(
            m.admitted(current),
            "新目标配对不受旧目标复核影响（id 布局 {:?}/{:?}）",
            stale,
            current
        );
    }
}

#[test]
fn pull_input_skips_pairs_whose_puller_left_the_room() {
    // 拉者对象已移除的配对不进输入、不可否决；配对复核按"保留现有
    // 标记"处理，由链语义兜底。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    put(&mut room, ObjId(9), 10, 10, &[BodyKind::Move]);
    put(&mut room, ObjId(1), 10, 11, &[]);
    m.add_pulling(&room, ObjId(9), ObjId(1));
    room.objects.remove(&ObjId(9));
    let input = m.pull_registration_input(&room);
    assert!(input.pulls.is_empty());
    assert!(PullRegistration::new(input).unwrap().is_none());
    assert!(m.is_pulled(ObjId(1)), "被拉标记保留，复核跳过");
}
