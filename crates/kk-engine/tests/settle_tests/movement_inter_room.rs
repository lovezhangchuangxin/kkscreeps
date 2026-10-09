//! 跨房移动过渡：边缘格 creep 的邻房迁移、镜像落点、身份与负载保持，
//! 以及未注册目的房的保守处置。

use super::*;
use kk_world::RoomState;

/// 双房夹具：W1N1 与其西邻 W2N1 各自独立结算后应用跨房阶段。
/// 结算路径与单房 Fixture 相同，再走 inter_room 的候选应用与逐房提交。
struct TwoRooms {
    world: World,
    players: BTreeMap<String, PlayerState>,
}

impl TwoRooms {
    fn new(terrain: kk_core::Terrain) -> Self {
        let mut world = World::new(1);
        world.register_room("W1N1", terrain.clone()).unwrap();
        world.register_room("W2N1", terrain).unwrap();
        Self {
            world,
            players: BTreeMap::new(),
        }
    }

    fn creep(&mut self, room: &str, name: &str, x: u8, y: u8, kinds: &[BodyKind]) -> ObjId {
        self.world
            .insert(
                room,
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x,
                    y,
                    fatigue: 0,
                    age_time: u64::MAX / 2,
                    hits: kinds.len() as u32 * 100,
                    spawning: false,
                    action_log: 0,
                    name: name.into(),
                    owner: "p1".into(),
                    body: body(kinds),
                    store: kk_schema::room::Store {
                        energy: 25,
                        ghodium: 0,
                    },
                    store_extra: Vec::new(),
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            )
            .unwrap()
    }

    fn settle(&mut self, batch: IntentBatch) {
        let state =
            kk_engine::TickState::new(std::mem::take(&mut self.players), self.world.next_obj_id());
        let plane = kk_engine::settle::Plane {
            buildings: Default::default(),
            links: kk_world::place::builtin_link_rules(),
            stores: kk_world::place::builtin_store_rules(),
            combat: kk_world::place::builtin_combat_ranges(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: Default::default(),
            contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
            extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan: kk_engine::settle::builtin_plan(),
        };
        let mut rooms = Vec::new();
        let mut moves = Vec::new();
        for name in ["W1N1", "W2N1"] {
            let mut room = self.world.room(name).unwrap().clone();
            room.name = name.into();
            let (settled, _events, mut pending) = settle_room(
                room,
                self.world.time(),
                1_700_000_000_000,
                &state,
                &batch,
                &plane,
            )
            .unwrap();
            moves.append(&mut pending);
            rooms.push((name.to_string(), settled));
        }
        kk_engine::settle::inter_room::apply_inter_room_moves(&mut rooms, &moves);
        let (players, next, _) = state.into_parts().unwrap();
        self.players = players;
        self.world.commit_tick_rooms(rooms, next, true).unwrap();
    }
}

fn creep_in(world: &World, room: &str, id: ObjId) -> (u8, u8, u32, u64, String, u32) {
    let obj = world.room(room).unwrap().objects.get(&id).unwrap();
    let RoomObject::Creep(c) = obj else {
        panic!("应为 creep")
    };
    (
        c.x,
        c.y,
        c.fatigue,
        c.age_time,
        c.name.clone(),
        c.store.energy,
    )
}

#[test]
fn inter_room_move_lands_in_neighbor_mirror_cell() {
    // 官方 interRoom：移动踏上边缘格后同 tick 过渡到邻房镜像位；
    // 4 WORK + 1 MOVE 的身体若无边缘归零规则，疲劳应为 4×2-2=6，
    // 归零规则下为 0。来源：上游 engine/src/processor/intents/creeps/tick.js。
    let mut f = TwoRooms::new(terrain_with(&[]));
    let c = f.creep(
        "W1N1",
        "w1",
        1,
        10,
        &[
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
    );
    f.settle(move_intent(c, 7)); // LEFT → (0,10) 西边缘

    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&c),
        "源房不再持有对象"
    );
    let (x, y, fatigue, age, name, energy) = creep_in(&f.world, "W2N1", c);
    assert_eq!((x, y), (49, 10), "西邻镜像位 (49,10)");
    assert_eq!(fatigue, 0, "进入边缘格疲劳归零");
    // 身份与寿命、负载随对象原样保持。
    assert_eq!(name, "w1");
    assert_eq!(energy, 25);
    assert!(age > f.world.time(), "寿命未重置");
}

#[test]
fn inter_room_stationary_edge_creep_transitions() {
    // 官方对位于边缘格的对象每 tick 过渡，不要求本 tick 有移动意图——
    // 刚到达邻房边缘的对象下一 tick 会再次过渡（出口弹跳）。
    // 来源：上游 engine/src/processor/intents/creeps/tick.js。
    let mut f = TwoRooms::new(terrain_with(&[]));
    let c = f.creep("W2N1", "w1", 49, 10, &[BodyKind::Move]);
    f.settle(IntentBatch::default());

    assert!(
        !f.world.room("W2N1").unwrap().objects.contains_key(&c),
        "源房不留副本"
    );
    let (x, y, ..) = creep_in(&f.world, "W1N1", c);
    assert_eq!((x, y), (0, 10), "东边缘静立对象过渡到东邻镜像位");
}

#[test]
fn inter_room_corner_takes_west_first() {
    // 官方方向优先级：x==0 西先于 y==0 北——角格 (0,0) 向西过渡。
    // 来源：上游 engine/src/processor/intents/creeps/tick.js 的 if/else 序。
    let mut f = TwoRooms::new(terrain_with(&[]));
    let c = f.creep("W1N1", "w1", 0, 0, &[BodyKind::Move]);
    f.settle(IntentBatch::default());

    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&c),
        "源房不留副本"
    );
    let (x, y, ..) = creep_in(&f.world, "W2N1", c);
    assert_eq!((x, y), (49, 0), "角格按西向过渡到西邻");
}

#[test]
fn inter_room_unregistered_target_keeps_creep() {
    // 目的房未注册（本仓世界仅含注册房）：对象留在源房，不丢弃。
    let mut world = World::new(1);
    world.register_room("W1N1", terrain_with(&[])).unwrap();
    let mut players = BTreeMap::new();
    let c = world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 49,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let state = kk_engine::TickState::new(std::mem::take(&mut players), world.next_obj_id());
    let mut room = world.room("W1N1").unwrap().clone();
    room.name = "W1N1".into();
    let (settled, _events, moves) = settle_room(
        room,
        world.time(),
        1_700_000_000_000,
        &state,
        &IntentBatch::default(),
        &kk_engine::settle::Plane {
            buildings: Default::default(),
            links: kk_world::place::builtin_link_rules(),
            stores: kk_world::place::builtin_store_rules(),
            combat: kk_world::place::builtin_combat_ranges(),
            move_gate: Default::default(),
            quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
            terrain_passability: Default::default(),
            resources: Default::default(),
            contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
            extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
            plan: kk_engine::settle::builtin_plan(),
        },
    )
    .unwrap();
    let mut rooms = vec![("W1N1".to_string(), settled)];
    kk_engine::settle::inter_room::apply_inter_room_moves(&mut rooms, &moves);
    assert_eq!(moves.len(), 1, "边缘对象仍登记过渡");
    assert_eq!(moves[0].to_room, "W0N1", "东邻名换算（W1=−2，+1=−1 即 W0）");
    assert!(
        rooms[0].1.objects.contains_key(&c),
        "未注册目的房时对象留在源房"
    );
    let obj = rooms[0].1.objects.get(&c).unwrap();
    assert_eq!(obj.pos(), (49, 10), "放回时不改坐标");
}

#[test]
fn inter_room_north_and_south_mirror_targets() {
    // 北 (y==0) 与南 (y==49) 的镜像换算；W1N1 的北邻是 W1N2、南邻是 W1N0。
    let north = kk_engine::settle::inter_room::edge_target("W1N1", 25, 0).unwrap();
    assert_eq!(north, ("W1N2".to_string(), 25, 49));
    let south = kk_engine::settle::inter_room::edge_target("W1N1", 25, 49).unwrap();
    assert_eq!(south, ("W1N0".to_string(), 25, 0));
    assert!(
        kk_engine::settle::inter_room::edge_target("W1N1", 25, 25).is_none(),
        "非边缘格不产生过渡"
    );
    let corner = kk_engine::settle::inter_room::edge_target("W1N1", 0, 49).unwrap();
    assert_eq!(corner, ("W2N1".to_string(), 49, 49), "角格西向优先于南向");
}

#[test]
fn inter_room_apply_moves_extension_fields_with_object() {
    // 扩展字段条目随对象迁移：目标房收录、源房不残留孤儿条目；
    // 引用缺席对象的过渡被跳过且不 panic。
    let mut rooms = vec![
        (
            "W1N1".to_string(),
            RoomState::new("W1N1", terrain_with(&[])),
        ),
        (
            "W2N1".to_string(),
            RoomState::new("W2N1", terrain_with(&[])),
        ),
    ];
    let id = ObjId(7);
    let obj = RoomObject::Creep(CreepObj {
        id: 0,
        x: 0,
        y: 10,
        fatigue: 0,
        age_time: u64::MAX / 2,
        hits: 100,
        spawning: false,
        action_log: 0,
        name: "w1".into(),
        owner: "p1".into(),
        body: body(&[BodyKind::Move]),
        store: kk_schema::room::Store {
            energy: 0,
            ghodium: 0,
        },
        store_extra: Vec::new(),
        say_message: None,
        say_public: None,
        notify_when_attacked: None,
    });
    rooms[0].1.objects.insert(id, obj);
    rooms[0].1.extensions.insert(
        id,
        std::iter::once(("mod:beacon".to_string(), serde_json::json!({ "level": 3 }))).collect(),
    );
    let moves = vec![
        kk_engine::settle::inter_room::InterRoomMove {
            id,
            to_room: "W2N1".into(),
            x: 49,
            y: 10,
        },
        // 引用已不存在对象的过渡：跳过、不 panic。
        kk_engine::settle::inter_room::InterRoomMove {
            id: ObjId(999),
            to_room: "W2N1".into(),
            x: 1,
            y: 1,
        },
    ];
    kk_engine::settle::inter_room::apply_inter_room_moves(&mut rooms, &moves);
    assert!(!rooms[0].1.objects.contains_key(&id), "源房对象已迁走");
    assert!(rooms[1].1.objects.contains_key(&id), "目标房收录对象");
    assert_eq!(rooms[1].1.objects.get(&id).unwrap().pos(), (49, 10));
    assert!(
        rooms[1].1.extensions.contains_key(&id),
        "扩展字段条目随对象迁移"
    );
    assert!(
        !rooms[0].1.extensions.contains_key(&id),
        "源房不残留扩展字段孤儿条目"
    );
}
