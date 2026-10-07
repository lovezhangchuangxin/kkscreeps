//! 竞争候选的写面与数值域预检；不依赖整轮丢弃掩盖部分写入。
use super::*;
use crate::settle::Working;
use crate::settle::{Engine, Plane, TickState, movement};
use kk_mod_api::movement::contest::MAX_PRIORITY;
use kk_schema::room::{BodyPart, CreepObj, RoomObject};
use kk_world::types::BodyKind;
use kk_world::{ObjId, RoomIndex, RoomState};
use std::collections::BTreeMap;

/// 放置单 MOVE 的 p1 creep；测试矩阵构造专用。
fn put_creep(room: &mut RoomState, id: ObjId, x: u8, y: u8) {
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

fn working_with_contest() -> (Working, movement::Movement) {
    let engine = Engine::default();
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    // 三个相邻原格同时挤 (10,11)：上方、下方与右侧。
    for (n, (x, y, dx, dy)) in [(10u8, 10u8, 0i32, 1i32), (10, 12, 0, -1), (11, 11, -1, 0)]
        .iter()
        .enumerate()
    {
        let id = ObjId(n as u64 + 1);
        room.objects.insert(
            id,
            RoomObject::Creep(CreepObj {
                id: 0,
                x: *x,
                y: *y,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: format!("c{n}"),
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
        m.add(id, (*x, *y), *dx, *dy);
    }
    let w = Working::new(
        room,
        0,
        0,
        TickState::new(BTreeMap::new(), 100),
        Plane {
            buildings: Default::default(),
            links: engine.links,
            stores: engine.stores,
            combat: engine.combat,
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: engine.resources.clone(),
            contrib: engine.contrib,
            extensions: engine.extensions,
            plan: engine.plan,
        },
    );
    (w, m)
}

#[test]
fn contest_input_covers_all_contenders_with_rating_factors() {
    let (w, m) = working_with_contest();
    let input = m.contest_input(&w.room);
    assert!(m.has_contested_cells());
    assert_eq!(input.contests.len(), 1);
    let cell = &input.contests[0];
    assert_eq!((cell.x, cell.y), (10, 11));
    assert_eq!(cell.contenders.len(), 3);
    // 登记序：add 顺序 上方、下方、右侧。
    assert_eq!(cell.contenders[0].origin_y, 10);
    assert_eq!(cell.contenders[1].origin_y, 12);
    assert_eq!(cell.contenders[2].origin_x, 11);
    for info in &cell.contenders {
        assert_eq!(info.owner, "p1");
        assert_eq!(info.move_power, 1);
        assert_eq!(info.weight, 1);
        assert_eq!(info.power_ratio, 1.0);
        assert!(!info.reciprocal, "无对穿：目标格竞争者原格无人请求");
    }
}

#[test]
fn empty_contest_input_produces_no_candidate() {
    let (w, m) = working_with_contest();
    let empty = movement::Movement::new();
    assert!(!empty.has_contested_cells());
    assert!(empty.contest_input(&w.room).contests.is_empty());
    assert!(
        MoveContest::new(empty.contest_input(&w.room))
            .unwrap()
            .is_none()
    );
    // 候选与仲裁互不影响：候选写入不改矩阵。
    let candidate = MoveContest::new(m.contest_input(&w.room)).unwrap().unwrap();
    candidate.set_priority(ObjId(1), 5).unwrap();
    let original = m.debug_matrix();
    let mut priorities = candidate.priorities();
    assert_eq!(priorities.remove(&ObjId(1)), Some(5));
    assert!(priorities.is_empty());
    assert_eq!(m.debug_matrix(), original, "候选不触碰登记矩阵");
}

#[test]
fn candidate_rejects_out_of_domain_and_foreign_ids() {
    let (w, m) = working_with_contest();
    let candidate = MoveContest::new(m.contest_input(&w.room)).unwrap().unwrap();
    assert_eq!(candidate.priority(ObjId(1)), 0);
    candidate.set_priority(ObjId(1), MAX_PRIORITY).unwrap();
    candidate.set_priority(ObjId(2), -MAX_PRIORITY).unwrap();
    assert!(candidate.set_priority(ObjId(1), MAX_PRIORITY + 1).is_err());
    assert!(candidate.set_priority(ObjId(2), -MAX_PRIORITY - 1).is_err());
    // ObjId(99) 不在输入中：拒绝而非静默记录。
    assert!(candidate.set_priority(ObjId(99), 1).is_err());
    assert_eq!(candidate.priority(ObjId(1)), MAX_PRIORITY);
    // 多次写入后值覆盖（before/after 组合语义）。
    candidate.set_priority(ObjId(1), 7).unwrap();
    assert_eq!(candidate.priority(ObjId(1)), 7);
}

#[test]
fn priorities_reorder_winners_with_default_tiebreak() {
    let (w, m) = working_with_contest();
    let mut m2 = m.clone();
    let index = RoomIndex::build(&w.room.objects);
    let old_fatigue = BTreeMap::new();
    // 全部单 MOVE：默认四键平手 → 登记序首个 ObjId(1) 胜。
    m2.check_impl(
        &w.room,
        &index,
        &old_fatigue,
        None,
        &BTreeMap::new(),
        &Default::default(),
    );
    assert_eq!(m2.winners().len(), 1);
    assert_eq!(m2.winners().keys().next().copied(), Some(ObjId(1)));

    // 压低首个、抬高末个：ObjId(3) 反超。
    let mut m3 = m.clone();
    let mut priorities = BTreeMap::new();
    priorities.insert(ObjId(1), -1i64);
    priorities.insert(ObjId(3), 1i64);
    m3.check_impl(
        &w.room,
        &index,
        &old_fatigue,
        None,
        &priorities,
        &Default::default(),
    );
    assert_eq!(m3.winners().keys().next().copied(), Some(ObjId(3)));

    // 仅设一个正值：其余默认 0，同层回落登记序。
    let mut m4 = m.clone();
    let mut priorities = BTreeMap::new();
    priorities.insert(ObjId(2), 1i64);
    m4.check_impl(
        &w.room,
        &index,
        &old_fatigue,
        None,
        &priorities,
        &Default::default(),
    );
    assert_eq!(m4.winners().keys().next().copied(), Some(ObjId(2)));
}

#[test]
fn contest_input_reports_reciprocal_and_request_count_for_crossing() {
    // 对穿 + 竞争：a(10,10)→(10,11)、b(10,11)→(10,10) 互换，c(9,11)→(10,11)
    // 与 a 竞争。a 的原格被 b（站在 a 的目的格上）请求 → reciprocal=true
    // 且 origin_request_count=1；c 无请求原格。默认评分第一键 a 取 100 胜。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    for (n, (x, y, dx, dy)) in [(10u8, 10u8, 0i32, 1i32), (10, 11, 0, -1), (9, 11, 1, 0)]
        .iter()
        .enumerate()
    {
        let id = ObjId(n as u64 + 1);
        put_creep(&mut room, id, *x, *y);
        m.add(id, (*x, *y), *dx, *dy);
    }
    let input = m.contest_input(&room);
    assert_eq!(input.contests.len(), 1, "仅 (10,11) 有竞争者");
    let cell = &input.contests[0];
    let (a, c) = (&cell.contenders[0], &cell.contenders[1]);
    assert_eq!((a.origin_x, a.origin_y), (10, 10));
    assert!(a.reciprocal, "对穿：b 站在 a 的目的格并请求 a 的原格");
    assert_eq!(a.origin_request_count, 1);
    assert!(!c.reciprocal);
    assert_eq!(c.origin_request_count, 0);
    let index = RoomIndex::build(&room.objects);
    let mut m2 = m.clone();
    m2.check_impl(
        &room,
        &index,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
        &Default::default(),
    );
    assert_eq!(
        m2.winners().get(&ObjId(1)),
        Some(&(10u8, 11u8)),
        "对穿键胜出"
    );
}

#[test]
fn contest_input_reports_pull_flags_and_default_rate2_wins() {
    // 拉着走 + 竞争：puller(10,10) 拉动 t(10,11) 并自己让位到 (10,9)；
    // t 与 d(9,10) 同挤 puller 的原格。被拉的 t 默认评分第二键（rate2=1）
    // 胜过 d，且 puller 作为 (10,9) 的获准移动者不再阻挡 t。
    let mut room = RoomState::new("W1N1", kk_core::Terrain::plains());
    let mut m = movement::Movement::new();
    let puller = ObjId(9);
    put_creep(&mut room, puller, 10, 10);
    m.add(puller, (10, 10), 0, -1);
    for (n, (x, y, dx, dy)) in [(10u8, 11u8, 0i32, -1i32), (9, 10, 1, 0)]
        .iter()
        .enumerate()
    {
        let id = ObjId(n as u64 + 1);
        put_creep(&mut room, id, *x, *y);
        m.add(id, (*x, *y), *dx, *dy);
    }
    m.add_pulling(&room, puller, ObjId(1));
    let input = m.contest_input(&room);
    let cell = &input.contests[0];
    let (t, d) = (&cell.contenders[0], &cell.contenders[1]);
    assert!(t.pulled, "被拉者标记");
    assert!(!t.pulling);
    assert!(!d.pulled && !d.pulling);
    assert_eq!(t.move_power, 1);
    assert_eq!(t.weight, 1);
    let index = RoomIndex::build(&room.objects);
    let mut m2 = m.clone();
    m2.check_impl(
        &room,
        &index,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
        &Default::default(),
    );
    assert_eq!(
        m2.winners().get(&ObjId(1)),
        Some(&(10u8, 10u8)),
        "被拉键胜出"
    );
    assert_eq!(
        m2.winners().get(&puller),
        Some(&(10u8, 9u8)),
        "拉者同时让位"
    );
}
