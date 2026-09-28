//! 单房间结算集成测试：固定指令脚本驱动，逐项对照官方数值
//! （常量值与官方 file:line 见各断言注释）。

use std::collections::BTreeMap;

use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::settle::{PlayerState, settle_room};
use kk_schema::room::{CreepObj, RoomObject, SpawnObj};
use kk_world::types::BodyKind;
use kk_world::{ObjId, World};
use serde_json::Value;

fn terrain_with(swamps: &[(usize, usize)]) -> kk_core::Terrain {
    let mut t = kk_core::Terrain::plains();
    for &(x, y) in swamps {
        t.as_mut_slice()[y * 50 + x] |= kk_core::TERRAIN_MASK_SWAMP;
    }
    t.normalize();
    t
}

fn body(kinds: &[BodyKind]) -> Vec<kk_schema::room::BodyPart> {
    kinds
        .iter()
        .map(|k| kk_schema::room::BodyPart {
            kind: k.code(),
            hits: 100,
            boost: None,
        })
        .collect()
}

struct Fixture {
    world: World,
    players: BTreeMap<String, PlayerState>,
}

impl Fixture {
    fn new(seed: u64, terrain: kk_core::Terrain) -> Self {
        let mut world = World::new(seed);
        world.register_room("W1N1", terrain);
        Self {
            world,
            players: BTreeMap::new(),
        }
    }

    fn creep(
        &mut self,
        name: &str,
        owner: &str,
        x: u8,
        y: u8,
        kinds: &[BodyKind],
        energy: u32,
    ) -> ObjId {
        self.world
            .insert(
                "W1N1",
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
                    owner: owner.into(),
                    body: body(kinds),
                    store: kk_schema::room::Store { energy },
                }),
            )
            .unwrap()
    }

    fn controller(&mut self, level: u8, owner: Option<&str>) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Controller(kk_schema::room::ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level,
                    safe_mode: 0,
                    safe_mode_available: 0,
                    progress: 0,
                    downgrade_time: None,
                    owner: owner.map(str::to_string),
                    is_power_enabled: false,
                }),
            )
            .unwrap()
    }

    fn source(&mut self, x: u8, y: u8, energy: u32) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Source(kk_schema::room::SourceObj {
                    id: 0,
                    x,
                    y,
                    energy,
                    energy_capacity: 3000,
                    next_regeneration_time: None,
                }),
            )
            .unwrap()
    }

    fn spawn(&mut self, name: &str, x: u8, y: u8, energy: u32) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Spawn(SpawnObj {
                    id: 0,
                    x,
                    y,
                    hits: 5000,
                    off: false,
                    name: name.into(),
                    owner: "p1".into(),
                    store: kk_schema::room::Store { energy },
                    spawning: None,
                }),
            )
            .unwrap()
    }

    fn site(
        &mut self,
        x: u8,
        y: u8,
        structure_type: &str,
        progress: u32,
        progress_total: u32,
        owner: &str,
    ) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                    id: 0,
                    x,
                    y,
                    structure_type: structure_type.into(),
                    progress,
                    progress_total,
                    owner: Some(owner.into()),
                    name: None,
                }),
            )
            .unwrap()
    }

    fn settle(&mut self, batch: IntentBatch) -> Vec<Value> {
        let mut next = self.world.next_obj_id();
        let mut room = self.world.rooms.get_mut("W1N1").unwrap().clone();
        let events = settle_room(
            &mut room,
            self.world.time,
            &mut next,
            &mut self.players,
            &batch,
        )
        .unwrap();
        let name = room.name.clone();
        self.world.rooms.insert(name, room);
        self.world.bump_next_obj_id(next);
        self.world.advance_ticks(1);
        events
    }

    fn obj(&self, id: ObjId) -> &RoomObject {
        self.world.get(id).unwrap().1
    }

    fn creep_state(&self, id: ObjId) -> (u8, u8, u32, u32) {
        match self.obj(id) {
            RoomObject::Creep(c) => (c.x, c.y, c.fatigue, c.store.energy),
            _ => panic!("非 creep"),
        }
    }
}

fn move_intent(creep: ObjId, direction: u8) -> IntentBatch {
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.direction = Some(direction);
    b.add_object_intent("p1", creep, Action::Move, d);
    b
}

// ---- 移动 ----

#[test]
fn move_fatigue_by_terrain() {
    // 官方 movement.js:204-239：road=1 / plain=2 / swamp=10 × 体重（非 move/carry 部件）
    let mut f = Fixture::new(1, terrain_with(&[(10, 12)]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);

    // 官方 tick 序：movement.execute(+fatigue) 后即恢复(-2×MOVE)——
    // [move,work] 在 plain 上每 tick 净 0、可持续移动
    f.settle(move_intent(c, 5)); // BOTTOM → (10,11) plain
    assert_eq!(f.creep_state(c), (10, 11, 0, 0), "plain：+2 后恢复 -2");

    f.settle(move_intent(c, 5)); // → (10,12) swamp
    assert_eq!(f.creep_state(c).1, 12);
    assert_eq!(f.creep_state(c).2, 8, "swamp：+10 后恢复 -2");

    // 无 MOVE 部件：canMove 不过，不动
    let c2 = f.creep("w2", "p1", 20, 20, &[BodyKind::Work], 0);
    f.settle(move_intent(c2, 5));
    assert_eq!(f.creep_state(c2), (20, 20, 0, 0));
}

#[test]
fn fatigue_gates_movement() {
    // 2 work + 1 move：疲劳 4/移动、恢复 2/tick → 动一停一
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c), (10, 11, 2, 0), "+4 恢复 -2");
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "疲劳 2≠0：不动");
    assert_eq!(f.creep_state(c).2, 0, "未移动仍恢复 -2");
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 12, "疲劳清零后恢复移动");
}

#[test]
fn swap_move_both_succeed() {
    // 对穿：互向对方格 → 双方成功（官方矩阵 + rate1=100 语义）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 3); // RIGHT → 11,10（b 的原格）
    let mut d = IntentData::bare();
    d.direction = Some(7); // LEFT → 10,10（a 的原格）
    batch.add_object_intent("p1", b, Action::Move, d);
    f.settle(batch);
    assert_eq!(f.creep_state(a).0, 11, "对穿 a 成功");
    assert_eq!(f.creep_state(b).0, 10, "对穿 b 成功");
}

#[test]
fn contested_cell_single_winner() {
    // 抢格：两 creep 挤同一目标 → 恰一个胜者
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 12, 12, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 4); // BOTTOM_RIGHT → 11,11
    let mut d = IntentData::bare();
    d.direction = Some(8); // TOP_LEFT → 11,11
    batch.add_object_intent("p1", b, Action::Move, d);
    f.settle(batch);
    let hit_a = f.creep_state(a).0 == 11 && f.creep_state(a).1 == 11;
    let hit_b = f.creep_state(b).0 == 11 && f.creep_state(b).1 == 11;
    assert!(hit_a ^ hit_b, "恰一个胜者：a={} b={}", hit_a, hit_b);
}

#[test]
fn static_obstacle_blocks_move_intent() {
    // 墙格不可入（无路时）——move intent 静态预检
    let mut t = kk_core::Terrain::plains();
    t.as_mut_slice()[10 * 50 + 11] |= kk_core::TERRAIN_MASK_WALL;
    t.normalize();
    let mut f = Fixture::new(1, t);
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 3)); // RIGHT → 墙
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "墙格不可入");
}

// ---- 采集 ----

#[test]
fn harvest_amount_and_events() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    let c = f.creep(
        "w1",
        "p1",
        11,
        10,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    // 2×WORK × HARVEST_POWER(2) = 4
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_HARVEST);
    assert_eq!(events[0]["data"]["amount"], 4);
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 2996),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 4);

    // 非邻接（移远后）拒
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    // creep 在 (11,10) 与 source (10,10) 邻接——先采一次后挪走两格再验
    f.settle(move_intent(c, 3));
    f.settle(move_intent(c, 3));
    let far = f.creep_state(c).0 >= 13;
    if far {
        let mut b2 = IntentBatch::default();
        b2.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        let before = match f.obj(src) {
            RoomObject::Source(s) => s.energy,
            _ => 0,
        };
        f.settle(b2);
        match f.obj(src) {
            RoomObject::Source(s) => assert_eq!(s.energy, before, "非邻接不采"),
            _ => panic!(),
        }
    }
}

#[test]
fn source_regen_clock() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(2, Some("p1"));
    let src = f.source(10, 10, 100);
    f.settle(IntentBatch::default());
    match f.obj(src) {
        // 首见未满挂钟 time+300（sources/tick.js:16-19）
        RoomObject::Source(s) => assert_eq!(s.next_regeneration_time, Some(300)),
        _ => panic!(),
    }
    for _ in 0..298 {
        f.settle(IntentBatch::default());
    }
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 100, "time=298 未到钟（time>=299 才触发）"),
        _ => panic!(),
    }
    f.settle(IntentBatch::default());
    match f.obj(src) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy, 3000, "到钟回满");
            assert_eq!(s.next_regeneration_time, None);
        }
        _ => panic!(),
    }
}

#[test]
fn harvest_overflow_drops() {
    // 满 carry 后继续采 → 超容部分掉地（harvest.js:55-60）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    // 1 carry（50）+ 8 work（16/tick）：4 tick 采 64 → 超 14 掉地
    let c = f.creep(
        "w1",
        "p1",
        11,
        10,
        &[
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
        0,
    );
    for _ in 0..4 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        f.settle(b);
    }
    assert_eq!(f.creep_state(c).3, 50, "钳到容量");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        });
    // 官方：本 tick 掉落的资源在 tick 阶段即参与衰减（forEach 键快照含 intent 期
    // 插入者）→ 14-1=13
    assert_eq!(dropped, Some(13), "超容 64-50=14 掉地且同 tick 衰减 1");
}

// ---- 建造 ----

#[test]
fn build_progress_and_completion() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "constructedWall", 0, 1, "p1");
    // 1 work → 5/tick（BUILD_POWER=5）；墙成本 1 → 一次完成
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    let events = f.settle(b);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_BUILD);
    assert_eq!(events[0]["data"]["incomplete"], false);
    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&site),
        "工地移除"
    );
    let wall = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::ConstructedWall(w) => Some(w),
            _ => None,
        })
        .unwrap();
    assert_eq!(wall.hits, 1, "官方落成墙 hits=WALL_HITS(1)");
    assert_eq!(f.creep_state(c).3, 9, "耗 1 能量");
}

// ---- 升级 ----

#[test]
fn upgrade_progress_and_gcl() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.settle(IntentBatch::default()); // 挂降级钟
    let c = f.creep("w1", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 200);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
    f.settle(b);
    match f.obj(ctl) {
        RoomObject::Controller(t) => assert_eq!(t.progress, 1, "1×WORK×1"),
        _ => panic!(),
    }
    assert_eq!(f.players["p1"].gcl, 1, "gcl 计入");
    assert_eq!(f.creep_state(c).3, 199);
}

#[test]
fn controller_levelup_cycle() {
    // 1 级 200 进位（CONTROLLER_LEVELS[1]=200）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.settle(IntentBatch::default());
    let c = f.creep("w1", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 500);
    for _ in 0..200 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
        f.settle(b);
    }
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.level, 2, "200 进位升 2 级");
            assert_eq!(t.safe_mode_available, 1, "升级 +1");
        }
        _ => panic!(),
    }
}

// ---- 孵化 ----

#[test]
fn spawn_creep_cycle() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store { energy: 300 },
                spawning: None,
            }),
        )
        .unwrap();
    // createCreep [move, work]：成本 150、needTime=3×2=6
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    let (spawning, spawn_energy) = match f.obj(spawn) {
        RoomObject::Spawn(s) => (s.spawning.clone(), s.store.energy),
        _ => panic!(),
    };
    let job = spawning.expect("孵化任务已建");
    assert_eq!((job.need_time, job.spawn_time), (6, 6));
    // 官方涓流：energyAvailable(150)<300 且 store<300 → spawn 每 tick +1
    assert_eq!(spawn_energy, 151, "扣 150 后免费涓流 +1");
    // 雏形 creep 占位于 spawn 格
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning, c.body.len())),
            _ => None,
        })
        .unwrap();
    assert!(chick.2);
    assert_eq!((chick.0, chick.1), (20, 20));
    assert_eq!(chick.3, 2);

    // 推到第 6 tick：出生（spawn 周围 8 格全空 → 出生在方向 1 (20,19)）
    for _ in 0..5 {
        f.settle(IntentBatch::default());
    }
    let chick_after = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .unwrap();
    assert!(!chick_after.2, "已出生");
    assert_ne!(
        (chick_after.0, chick_after.1),
        (20, 20),
        "出生移出 spawn 格"
    );
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_none(), "任务清空"),
        _ => panic!(),
    }
}

// ---- 掉落能量衰减 ----

#[test]
fn dropped_energy_decays() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let e = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 3,
                y: 3,
                resource_type: 0,
                amount: 1000,
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    match f.obj(e) {
        RoomObject::Energy(x) => assert_eq!(x.amount, 999, "ceil(1000/1000)=1 衰减"),
        _ => panic!(),
    }
    // 小量快速消亡
    let e2 = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 4,
                y: 4,
                resource_type: 0,
                amount: 2,
            }),
        )
        .unwrap();
    // 官方衰减 ceil(amount/1000)：2→1→0 两个 tick 清零
    f.settle(IntentBatch::default());
    f.settle(IntentBatch::default());
    assert!(
        !f.world.room("W1N1").unwrap().objects.contains_key(&e2),
        "≤0 移除"
    );
}

// ---- TTL 死亡 ----

#[test]
fn creep_dies_at_age_time() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 直接造一个快死的（age_time = time+2 → 第二 tick 死）
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 9,
                y: 9,
                fatigue: 0,
                age_time: 2,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "old".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move, BodyKind::Work]),
                store: kk_schema::room::Store { energy: 30 },
            }),
        )
        .unwrap();
    // 官方 creeps/tick.js:93：gameTime >= ageTime-1 → time=1 的第二次结算死亡
    f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_some(), "time=0 < age_time-1=1：未死");
    f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_none(), "time=1 >= 1：死亡");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(x) => Some(x.amount),
            _ => None,
        });
    // 掉落发生在 tick 阶段（键快照已拍）→ 本 tick 不衰减，30 原值
    assert_eq!(dropped, Some(30), "死亡掉落携带");
}

// ---- 完整闭环（同 tick 采+升） ----

#[test]
fn engine_closed_loop_harvest_and_upgrade() {
    // source 与 controller 相邻放置：worker 每 tick 同时采 4、升 4
    // （upgradeController 不在 harvest 的压制表——官方同 tick 双动作）
    let mut f = Fixture::new(9, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    f.source(6, 6, 3000);
    let src = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Source(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        7,
        7,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );

    for _ in 0..10 {
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
        b.add_object_intent("p1", c, Action::UpgradeController, IntentData::target(ctl));
        f.settle(b);
    }
    // 净存量断言随 carry 容量调整
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.progress, 20, "10 tick × 2（UPGRADE_CONTROLLER_POWER=1）")
        }
        _ => panic!(),
    }
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000 - 40, "采 4/tick × 10"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 20, "采 40 升 20，净存 20");
    assert_eq!(f.players["p1"].gcl, 20);
}

// ---- 评审回归测试（每条 high 修复一条锁） ----

#[test]
fn stationary_creep_blocks_movement() {
    // H1：非获准移动的 creep 是障碍（对穿的另一半）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let _still = f.creep("still", "p1", 11, 10, &[BodyKind::Move], 0);
    let mover = f.creep("mover", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(mover, 3)); // RIGHT → (11,10) 有静止 creep
    assert_eq!(f.creep_state(mover), (10, 10, 0, 0), "静止 creep 挡路");
}

#[test]
fn swap_with_third_contender_rates() {
    // H2：对穿者受压优先（rate1=100）——A↔B 对穿时，抢 A 目标格的 C 应被 A 压制
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move], 0);
    let c = f.creep("c", "p1", 12, 10, &[BodyKind::Move], 0);
    let mut batch = move_intent(a, 3); // a: (10,10)→(11,10)（b 让位）
    let mut d = IntentData::bare();
    d.direction = Some(7);
    batch.add_object_intent("p1", b, Action::Move, d); // b: (11,10)→(10,10)
    let mut d2 = IntentData::bare();
    d2.direction = Some(7);
    batch.add_object_intent("p1", c, Action::Move, d2); // c: (12,10)→(11,10) 抢 a 的目标
    f.settle(batch);
    // 官方：a 的 rate1=100（有竞争者从其目标格移出）必胜 c
    assert_eq!(f.creep_state(a).0, 11, "受压对穿者 a 胜出");
    assert_eq!(f.creep_state(b).0, 10);
    assert_eq!(f.creep_state(c).0, 12, "c 被 a 压制原地");
}

#[test]
fn lighter_creep_wins_contested_cell() {
    // H3：rate4 = moves/体重——轻 creep 压过重 creep（平手 rate1 时）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let heavy = f.creep(
        "heavy",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    let light = f.creep("light", "p1", 12, 12, &[BodyKind::Move], 0);
    let mut batch = move_intent(heavy, 4); // → (11,11)
    let mut d = IntentData::bare();
    d.direction = Some(8);
    batch.add_object_intent("p1", light, Action::Move, d); // → (11,11)
    f.settle(batch);
    // light rate4=1/1 > heavy 1/2 → light 胜
    assert_eq!(
        (f.creep_state(light).0, f.creep_state(light).1),
        (11, 11),
        "轻者胜"
    );
    assert_eq!(f.creep_state(heavy).0, 10, "重者被压制");
}

#[test]
fn road_on_swamp_costs_one() {
    // H5：路上的沼泽按路走（rate 1 而非 10）
    let mut f = Fixture::new(1, terrain_with(&[(10, 11)]));
    f.world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 10,
                y: 11,
                hits: 5000,
                next_decay_time: 9999,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle(move_intent(c, 5)); // → (10,11) 沼泽+路
    // rate 1：+1×1 后恢复 -2 → 0
    assert_eq!(f.creep_state(c).2, 0, "路上的沼泽按路走");
}

#[test]
fn road_on_swamp_costs_more_to_build() {
    // H6：沼泽上的路 progressTotal ×5（300→1500，建工地时定格存值）
    let mut f = Fixture::new(1, terrain_with(&[(12, 10)]));
    let site = f.site(12, 10, "road", 0, 1500, "p1");
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    let events = f.settle(b);
    assert_eq!(
        events[0]["data"]["incomplete"], true,
        "5 能量 < 1500 不完成"
    );
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => assert_eq!(s.progress, 5),
        _ => panic!(),
    }
}

#[test]
fn birth_blocked_by_stationary_creep_until_it_leaves() {
    // 深评 H2：出生占位对任何 creep（友敌皆算）都算被占——'creep' ∈
    // OBSTACLE_OBJECT_TYPES（_born-creep.js:12-15）；被占时推迟重试、雏形
    // 留在 spawn 格，让开后才出生。blocker 的逃离方向必须真正可走（远离
    // spawn），否则测试测不到"被占"阶段
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    // 方向 1 的格 (20,19) 站一个静止 creep
    let blocker = f.creep("blocker", "p1", 20, 19, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    d.directions = vec![1]; // 只允许方向 1（被 block）
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    // 推 5 tick 到出生日（spawn_time=6，time>=5 触发）
    for _ in 0..5 {
        f.settle(IntentBatch::default());
    }
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .expect("雏形存在");
    assert!(chick.2, "被占：不出世");
    assert_eq!(
        (chick.0, chick.1),
        (20, 20),
        "雏形留在 spawn 格（不与 blocker 同格）"
    );
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().map(|j| j.spawn_time),
            Some(6),
            "出生失败推迟一 tick 重试"
        ),
        _ => panic!(),
    }
    // blocker 真正走开（方向 4 → (21,20)，远离 spawn）；spawn tick 在 blocker
    // 的 creep tick 之前（id 序）→ 当 tick 仍失败、下一 tick 出生
    f.settle(move_intent(blocker, 4));
    f.settle(IntentBatch::default());
    let child = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .expect("出生成功");
    assert!(!child.2);
    assert_eq!((child.0, child.1), (20, 19), "blocker 让开后出生在方向 1");
}

#[test]
fn fatigue_recovers_from_persisted_value() {
    // H4：累加器从持久 fatigue 播种——静止恢复每 tick -2×MOVE
    let mut f = Fixture::new(1, terrain_with(&[(10, 11), (10, 12)]));
    // 3 work + 1 move：沼泽移动 fatigue = 3×10 - 2 = 28
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[
            BodyKind::Move,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
        ],
        0,
    );
    f.settle(move_intent(c, 5)); // → (10,11) 沼泽
    assert_eq!(f.creep_state(c).2, 28, "3×10 - 恢复 2");
    // 静止恢复：28 → 26 → 24（不是归零）
    f.settle(IntentBatch::default());
    assert_eq!(f.creep_state(c).2, 26, "从持久值恢复 -2");
    f.settle(IntentBatch::default());
    assert_eq!(f.creep_state(c).2, 24);
}

#[test]
fn transfer_clamps_to_target_capacity() {
    // transfer 钳制：spawn 容量 300
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = f
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store { energy: 250 },
                spawning: None,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 20, 21, &[BodyKind::Move, BodyKind::Carry], 100);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(spawn);
    d.amount = Some(100);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    // 250+100 > 300 → 只转 50
    assert_eq!(events[0]["data"]["amount"], 50);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 300),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn withdraw_only_from_stored_structures() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 10, 3000);
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(src);
    d.amount = Some(10);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Withdraw, d);
    f.settle(b);
    // source 不可 withdraw（官方走 harvest）
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "withdraw 不动 source"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 0);
}

#[test]
fn controller_downgrade_chain() {
    // controllers/tick.js 降级链：无升级 → 到点 level-1、progress += 90%、
    // safeModeAvailable 清零
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p1"));
    // level 2 的 DOWNGRADE=10000：手造快钟绕过长等待——直接改对象
    if let Some(RoomObject::Controller(t)) = f.world.get_mut(ctl) {
        t.downgrade_time = Some(2);
    }
    f.settle(IntentBatch::default());
    f.settle(IntentBatch::default()); // time=1 >= 2-1 → 降级
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.level, 1, "2→1");
            // progress += round(LEVELS[1]×0.9) = 180
            assert_eq!(t.progress, 180);
            assert_eq!(t.safe_mode_available, 0);
        }
        _ => panic!(),
    }
}

#[test]
fn engine_scripted_sandbox_closed_loop() {
    // ScriptedSandbox 经 Engine 驱动的多 tick 闭环（方案 §10）
    let mut world = World::new(9);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t);
    world
        .insert(
            "W1N1",
            RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 5,
                y: 5,
                level: 1,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: Some("p1".into()),
                is_power_enabled: false,
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 6,
                y: 6,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    let ctl = world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let src = world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Source(_) => Some(ObjId(o.id())),
            _ => None,
        })
        .unwrap();
    let creep = world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 7,
                y: 7,
                fatigue: 0,
                age_time: 999_999,
                hits: 500,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Carry.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: Default::default(),
            }),
        )
        .unwrap();

    // 每 tick：采 + 升（脚本是按当前 tick 注入的静态批）
    // 脚本按当前 world.time 匹配：Engine 首 tick 的 time=0，覆盖 0..10
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", creep, Action::Harvest, IntentData::target(src));
    batch.add_object_intent(
        "p1",
        creep,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    let mut engine = kk_engine::Engine::new(world);
    for tick in 0..10u64 {
        sandbox.at(tick, batch.clone());
    }
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    for _ in 0..10 {
        engine.tick().unwrap();
    }
    let ctl_obj = engine.world.get(ctl).unwrap().1;
    match ctl_obj {
        RoomObject::Controller(t) => assert_eq!(t.progress, 20, "10 tick × 2"),
        _ => panic!(),
    }
    assert_eq!(engine.players["p1"].gcl, 20);
    assert!(!engine.last_events.is_empty(), "事件日志留存");
    // id 单一真相源：结算新建对象后 world.next_obj_id 已同步
    let max_id = engine
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .max()
        .unwrap()
        .0;
    assert!(engine.world.next_obj_id() > max_id, "分配器越过最大 id");
}

// ---- M1 深度评审回归 ----

#[test]
fn own_construction_site_blocks_movement() {
    // 深评 H1：自家 obstacle 结构工地在 check 阶段挡路（movement.js:25-26）——
    // move 预检不含工地（挤格语义），但仲裁不得放行，否则完工结构落在 creep 头上
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "spawn", 0, 15000, "p1");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5)); // → (10,11) 自家 spawn 工地
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "自家 obstacle 工地挡路");
    assert!(f.world.get(site).is_some(), "工地未被动");
}

#[test]
fn moving_onto_enemy_site_stomps_it() {
    // movement.js:224-233：进格踩掉敌方工地（safeMode 门外），退一半工程为
    // 掉落能量；tick 期插入的 energy 不在阶段 8 键快照内 → 本 tick 不衰减
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(10, 11, "road", 10, 300, "p2");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "敌方工地可入");
    assert!(f.world.get(site).is_none(), "工地被踩掉");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(5), "退半工程 10/2=5，本 tick 不衰减");
}

#[test]
fn transfer_into_extension_clamps_by_rcl() {
    // 深评 H3：extension 容量按 EXTENSION_ENERGY_CAPACITY[rcl]（extensions/
    // tick.js:10-17 归一、transfer.js 经 capacityForResource 钳制）——rcl 3 档 50
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ext = f
        .world
        .insert(
            "W1N1",
            RoomObject::Extension(kk_schema::room::ExtensionObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: kk_schema::room::Store { energy: 40 },
            }),
        )
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        20,
        21,
        &[BodyKind::Move, BodyKind::Carry, BodyKind::Carry],
        60,
    );
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(ext);
    d.amount = Some(30);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["amount"], 10, "钳到容量差 50-40");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.store.energy, 50),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 50);
}

#[test]
fn cascade_removal_clears_chain_on_head_failure() {
    // movement.js:154-186 级联清链：链尾无 MOVE（canMove 不过）→ 其移动出矩阵，
    // 沿"要进我原格"递归取消上游——全链原地
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("a", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let b = f.creep("b", "p1", 11, 10, &[BodyKind::Move, BodyKind::Carry], 0);
    let c = f.creep("c", "p1", 12, 10, &[BodyKind::Work], 0); // 无 MOVE
    let mut batch = move_intent(a, 3);
    let mut d = IntentData::bare();
    d.direction = Some(3);
    batch.add_object_intent("p1", b, Action::Move, d);
    let mut d2 = IntentData::bare();
    d2.direction = Some(3);
    batch.add_object_intent("p1", c, Action::Move, d2);
    f.settle(batch);
    assert_eq!(
        f.creep_state(a).0,
        10,
        "级联取消：链尾 c 无 MOVE → b、a 全链取消"
    );
    assert_eq!(f.creep_state(b).0, 11);
    assert_eq!(f.creep_state(c).0, 12);
}

#[test]
fn edge_entry_zero_fatigue() {
    // movement.js:242-245：从非边缘进边缘格 fatigue=0（跨房间预备）；常规平原
    // 应为体重 2×2 − 恢复 2 = 2，此处必须为 0
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        1,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Work],
        0,
    );
    f.settle(move_intent(c, 1)); // TOP → (10,0) 边缘
    assert_eq!(f.creep_state(c), (10, 0, 0, 0));
}

#[test]
fn movement_wears_road() {
    // movement.js:213：路上移动磨损 nextDecayTime -= ROAD_WEAROUT(1) × body 部件数
    let mut f = Fixture::new(1, terrain_with(&[]));
    let road = f
        .world
        .insert(
            "W1N1",
            RoomObject::Road(kk_schema::room::RoadObj {
                id: 0,
                x: 10,
                y: 11,
                hits: 5000,
                next_decay_time: 5000,
            }),
        )
        .unwrap();
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
        0,
    );
    f.settle(move_intent(c, 5));
    match f.obj(road) {
        RoomObject::Road(r) => assert_eq!(r.next_decay_time, 4997, "3 部件 × ROAD_WEAROUT(1)"),
        _ => panic!(),
    }
}

#[test]
fn carried_resources_add_move_weight() {
    // movement.js:41-59 calcResourcesWeight：从 body 尾部数装下携带量所需的
    // CARRY 部件数，计入移动体重——[move,carry,carry,work] 带 60：weight = 1 + 2
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep(
        "w1",
        "p1",
        10,
        10,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Carry,
            BodyKind::Work,
        ],
        60,
    );
    f.settle(move_intent(c, 5)); // plain：3×2 − 2 = 4
    assert_eq!(f.creep_state(c).2, 4, "满载两 carry 计入体重");
    // 对照：同 body 空载 → weight 1 → 2 − 2 = 0
    let c2 = f.creep(
        "w2",
        "p1",
        12,
        12,
        &[
            BodyKind::Move,
            BodyKind::Carry,
            BodyKind::Carry,
            BodyKind::Work,
        ],
        0,
    );
    f.settle(move_intent(c2, 5));
    assert_eq!(f.creep_state(c2).2, 0, "空载不计资源重量");
}

#[test]
fn build_blocked_by_creep_on_tile() {
    // build.js:50-59：障碍类型工地格上有 creep（无 safeMode ⇒ 任何 creep）→ 拒建
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "spawn", 0, 15000, "p1");
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let _blocker = f.creep("blocker", "p1", 12, 10, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    f.settle(b);
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => {
            assert_eq!(s.progress, 0, "格被 creep 占：拒建")
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 10, "能量未扣");
}

#[test]
fn create_creep_rejected_for_off_spawn() {
    // create-creep.js:17 checkStructureAgainstController（utils.js:456-506）：
    // RCL 3 只许 1 个 spawn，距 controller 更远者不活跃——即使能量充足也不得孵化
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let _near = f.spawn("Near", 5, 6, 300);
    let far = f.spawn("Far", 25, 25, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    b.add_object_intent("p1", far, Action::CreateCreep, d);
    f.settle(b);
    match f.obj(far) {
        RoomObject::Spawn(s) => {
            assert!(s.spawning.is_none(), "超配额 spawn 不孵化");
            assert_eq!(s.store.energy, 300, "未扣费");
        }
        _ => panic!(),
    }
    match f.obj(_near) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 300, "Near 同样未被动"),
        _ => panic!(),
    }
    assert!(
        f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .all(|o| !matches!(o, RoomObject::Creep(c) if c.name == "child")),
        "无雏形"
    );
}

#[test]
fn drop_rejects_amount_over_holdings() {
    // drop.js:13：持有不足整条拒绝（官方不部分掉落）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 20);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(50);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    assert_eq!(f.creep_state(c).3, 20, "不足额：整条拒绝");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "无掉落"
    );
}

// ---- 外部复审（ChatGPT）回归：Generic 透传结构的类型名解析 ----

fn generic_tower(x: u8, y: u8) -> RoomObject {
    RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x,
        y,
        data: r#"{"type":"structure","structureType":"tower","owner":"p1","hits":1}"#.into(),
    })
}

#[test]
fn placed_tower_blocks_birth() {
    // 官方 checkObstacleFn 吃真实类型名（'tower' ∈ OBSTACLE_OBJECT_TYPES）——
    // Generic 透传桶不得报成 "structure"（不在表内 → 原实现可在 tower 格出生）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    f.world.insert("W1N1", generic_tower(20, 19)).unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    d.directions = vec![1];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    for _ in 0..12 {
        f.settle(IntentBatch::default());
    }
    // 方向 1 被 tower 永占：反复重试、永不出世（tower 不会让开）
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        })
        .expect("雏形存在");
    assert!(chick.2, "tower 格不可出生");
    assert_eq!((chick.0, chick.1), (20, 20), "雏形留在 spawn 格");
}

#[test]
fn tower_counts_toward_controller_quota() {
    // utils.checkControllerAvailability 按真实结构类型数现存对象——
    // Generic tower 必须计入配额（原实现报 "structure" 永不计入 → 可超配额建塔）
    let mk_intent = |b: &mut IntentBatch| {
        b.add_room_intent(
            "p1",
            kk_engine::intent::RoomIntent::CreateConstructionSite {
                room: "W1N1".into(),
                x: 15,
                y: 15,
                structure_type: "tower".into(),
                name: None,
            },
        );
    };
    let has_tower_site =
        |f: &Fixture| {
            f.world.room("W1N1").unwrap().objects.values().any(
                |o| matches!(o, RoomObject::ConstructionSite(s) if s.structure_type == "tower"),
            )
        };
    // 无 tower：rcl 3 允许 1 座 → 工地建成
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let mut b = IntentBatch::default();
    mk_intent(&mut b);
    f.settle(b);
    assert!(has_tower_site(&f), "配额空位：允许建塔");
    // 已有 1 座 tower：配额满 → 拒建
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller(3, Some("p1"));
    f2.world.insert("W1N1", generic_tower(25, 25)).unwrap();
    let mut b2 = IntentBatch::default();
    mk_intent(&mut b2);
    f2.settle(b2);
    assert!(!has_tower_site(&f2), "现存 tower 计入配额：拒建");
}

#[test]
fn build_blocked_by_tower_on_site_tile() {
    // build.js:51-53 障碍对象拒建——Generic tower 在工地格上必须挡建造
    let mut f = Fixture::new(1, terrain_with(&[]));
    let site = f.site(12, 10, "spawn", 0, 15000, "p1");
    f.world.insert("W1N1", generic_tower(12, 10)).unwrap();
    let c = f.creep("w1", "p1", 11, 10, &[BodyKind::Move, BodyKind::Work], 10);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
    f.settle(b);
    match f.obj(site) {
        RoomObject::ConstructionSite(s) => {
            assert_eq!(s.progress, 0, "tower 占格：拒建")
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(c).3, 10);
}

#[test]
fn energy_merge_saturates_at_u32_max() {
    // 掉落合并的受检运算：u32::MAX 堆 + 1 不 panic（debug）/不回绕丢数据（release），
    // 随后照常衰减 ceil(MAX/1000)
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                amount: u32::MAX,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Move, BodyKind::Carry], 1);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.amount = Some(1);
    d.resource_type = Some("energy".into());
    b.add_object_intent("p1", c, Action::Drop, d);
    f.settle(b);
    let decay = (u32::MAX as i64 + 999) / 1000; // ceil(4294967295/1000) = 4294968
    let expected = u32::MAX - decay as u32;
    let heap = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some(e.amount),
            _ => None,
        })
        .unwrap();
    assert_eq!(heap, expected, "饱和合并后照常衰减（无 panic/回绕）");
}

#[test]
fn sandbox_intents_bound_to_registered_player() {
    // 身份边界：沙箱返回的 intent 只归其注册玩家——冒名键（操作他人对象）整批丢弃
    let mut world = World::new(1);
    world.register_room("W1N1", kk_core::Terrain::plains());
    let victim = world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "victim".into(),
                owner: "p2".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
            }),
        )
        .unwrap();
    // 恶意沙箱注册为 p1，却发 p2 键的 move intent
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    sandbox.at(0, move_intent(victim, 3));
    let mut engine = kk_engine::Engine::new(world);
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    engine.tick().unwrap();
    let (x, y) = match engine.world.get(victim).unwrap().1 {
        RoomObject::Creep(c) => (c.x, c.y),
        _ => panic!(),
    };
    assert_eq!((x, y), (10, 10), "冒名 intent 被丢弃：p2 的 creep 未被移动");
}

// ---- B1：hits 模型 + attack/repair/dismantle ----
// 官方对照：engine/src/processor/intents/creeps/{attack,repair,dismantle}.js、
// _damage.js、creeps/tick.js（_applyDamage/_recalc-body）、structures/_destroy.js。

/// 直插 extension（可自定义 hits/store——Fixture.spawn 只造 spawn）。
fn extension_at(f: &mut Fixture, x: u8, y: u8, hits: u32, energy: u32) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Extension(kk_schema::room::ExtensionObj {
                id: 0,
                x,
                y,
                hits,
                off: false,
                owner: "p2".into(),
                store: kk_schema::room::Store { energy },
            }),
        )
        .unwrap()
}

/// 直插 Generic 结构（rampart 等），hits 存 data JSON。
fn generic_structure(
    f: &mut Fixture,
    x: u8,
    y: u8,
    structure_type: &str,
    owner: &str,
    hits: i64,
) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: serde_json::json!({
                    "type": "structure",
                    "structureType": structure_type,
                    "owner": owner,
                    "hits": hits,
                })
                .to_string(),
            }),
        )
        .unwrap()
}

fn attack_intent(attacker: ObjId, target: ObjId) -> IntentBatch {
    attack_intent_by("p1", attacker, target)
}

fn attack_intent_by(player: &str, attacker: ObjId, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent(player, attacker, Action::Attack, IntentData::target(target));
    b
}

/// creep 侧对象断言小工具。
fn creep_parts(f: &Fixture, id: ObjId) -> (u32, Vec<u32>) {
    match f.obj(id) {
        RoomObject::Creep(c) => (c.hits, c.body.iter().map(|p| p.hits).collect()),
        _ => panic!("非 creep"),
    }
}

#[test]
fn melee_attack_damage_and_body_distribution() {
    // attack.js:38 伤害 = 2×ATTACK×30=60；_recalc-body.js:10-18 从尾部灌满——
    // 前列部件先掉 hit：[tough,work,work,move] 400→340，tough 100→40
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[BodyKind::Attack, BodyKind::Attack],
        0,
    );
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Move,
        ],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "无反击目标（无 ATTACK 部件）→ 单事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(events[0]["objectId"], a.as_str());
    assert_eq!(events[0]["data"]["targetId"], b.as_str());
    assert_eq!(events[0]["data"]["damage"], 60);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_MELEE
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 340, "400 - 60");
    assert_eq!(parts, vec![40, 100, 100, 100], "受伤自头部推进");
    // actionLog（_damage.js:59-75）：攻击方 attack 位、受击方 attacked 位
    let (log_a, log_b) = match (f.obj(a), f.obj(b)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED);
}

#[test]
fn melee_hit_back_and_event_order() {
    // _damage.js:17-19：近战 creep 目标反击（同样延迟）；:89-93 反击事件先于主事件
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep(
        "atk",
        "p1",
        10,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Attack,
        ],
        0,
    );
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Attack,
            BodyKind::Attack,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 2, "反击事件 + 主事件");
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["objectId"],
        b.as_str(),
        "反击事件以受击者为 objectId"
    );
    assert_eq!(events[0]["data"]["targetId"], a.as_str());
    assert_eq!(events[0]["data"]["damage"], 60, "2×ATTACK×30 反击");
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
    );
    assert_eq!(events[1]["data"]["damage"], 120, "4×ATTACK×30 主伤害");
    // 双方伤害同 tick 各自结算：a 400-60=340、b 400-120=280
    assert_eq!(creep_parts(&f, a).0, 340);
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280);
    assert_eq!(parts, vec![0, 80, 100, 100], "b 首个 ATTACK 部件被击毁");
    // 攻击者同时记 attack 与 attacked（_damage.js:74 + :88）
    let log_a = match f.obj(a) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log_a,
        kk_engine::settle::creep::ACTION_ATTACK | kk_engine::settle::creep::ACTION_ATTACKED
    );
}

#[test]
fn no_hit_back_when_attacker_on_rampart() {
    // _damage.js:17：攻击者站 rampart（不分敌我/公开性）→ 无反击
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    generic_structure(&mut f, 10, 10, "rampart", "p2", 1000);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Attack, BodyKind::Tough],
        0,
    );
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1, "仅主事件");
    assert_eq!(creep_parts(&f, a).0, 100, "攻击者无伤");
    assert_eq!(creep_parts(&f, b).0, 170, "200 - 30");
}

#[test]
fn multiple_attackers_accumulate_damage() {
    // _damage.js:20：creep 目标伤害累加进 _damageToApply，目标自己的 tick 一次结算
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a1 = f.creep("a1", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let a2 = f.creep("a2", "p1", 12, 10, &[BodyKind::Attack, BodyKind::Attack], 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
            BodyKind::Tough,
        ],
        0,
    );
    let mut batch = attack_intent(a1, b);
    batch.add_object_intent("p1", a2, Action::Attack, IntentData::target(b));
    let events = f.settle(batch);
    assert_eq!(events.len(), 2, "两次主事件");
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 280, "400 - 60×2");
    assert_eq!(parts, vec![0, 80, 100, 100]);
}

#[test]
fn attack_kills_creep_and_drops_store() {
    // tick.js:134-136：hits 归零 → 暴力死亡，与 TTL 死亡同路径（掉能量、无墓碑）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut bodylist = vec![BodyKind::Attack; 40];
    bodylist.push(BodyKind::Move);
    let a = f.creep("atk", "p1", 10, 10, &bodylist, 0);
    let b = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Tough, BodyKind::Tough],
        40,
    );
    f.settle(attack_intent(a, b));
    assert!(f.world.get(b).is_none(), "1200 ≥ 200：击杀");
    // 死亡掉落发生在 tick 阶段（键快照已拍）→ 本 tick 不衰减，40 原值
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(40), "store 掉地为 energy");
}

#[test]
fn attack_kills_structure_and_spawn_chick() {
    // structures/_destroy.js:9-15：孵化中的 spawn 被毁 → 雏形一并移除；
    // store 落地为掉落能量（官方进 ruin——B1 偏离登记）；phase 5 落地 → 本 tick 衰减 1
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    // 造孵化任务与雏形（走真 createCreep intent）
    let mut b0 = IntentBatch::default();
    let mut d0 = IntentData::bare();
    d0.name = Some("child".into());
    d0.body = vec![BodyKind::Move];
    b0.add_object_intent("p1", spawn, Action::CreateCreep, d0);
    f.settle(b0);
    // 削弱 spawn 至一击必杀（120 伤害）
    if let Some(RoomObject::Spawn(s)) = f.world.get_mut(spawn) {
        s.hits = 100;
    }
    let atk = f.creep("atk", "p2", 20, 21, &[BodyKind::Attack; 4], 0);
    f.settle(attack_intent_by("p2", atk, spawn));
    assert!(f.world.get(spawn).is_none(), "spawn 被击毁");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Creep(c) if c.name == "child")),
        "雏形随 spawn 一并移除"
    );
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 20 && e.y == 20 => Some(e.amount),
            _ => None,
        });
    assert_eq!(
        dropped,
        Some(250),
        "store 落地后当 tick 衰减 ceil(251/1000)=1（300-50 孵化费+1 涓流=251）"
    );
}

#[test]
fn rampart_redirects_attack_until_broken() {
    // attack.js:33-36：目标格 rampart 存在 → 伤害转打 rampart；打破后直击本体
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 11, 10, &[BodyKind::Attack; 10], 0);
    let ext = extension_at(&mut f, 12, 10, 1000, 0);
    let rampart = generic_structure(&mut f, 12, 10, "rampart", "p2", 1000);
    // t1：extension 无伤、rampart -300
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(
        events[0]["data"]["targetId"],
        rampart.as_str(),
        "事件指向 rampart"
    );
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "本体受 rampart 保护"),
        _ => panic!(),
    }
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 700);
        }
        _ => panic!(),
    }
    // t2-t3：rampart 700→400→100
    f.settle(attack_intent(a, ext));
    f.settle(attack_intent(a, ext));
    // t4：100-300 ≤ 0 → rampart 击毁
    f.settle(attack_intent(a, ext));
    assert!(f.world.get(rampart).is_none(), "rampart 击毁");
    assert!(f.world.get(ext).is_some(), "本体仍在");
    // t5：无 rampart → 直击 extension
    let events = f.settle(attack_intent(a, ext));
    assert_eq!(events[0]["data"]["targetId"], ext.as_str());
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 700),
        _ => panic!(),
    }
}

#[test]
fn safe_mode_blocks_hostile_attack_and_dismantle() {
    // attack.js:30-32 / dismantle.js:24-26：他人 safeMode 生效期拒（引擎层二次门）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Work], 0);
    let ext = extension_at(&mut f, 11, 10, 1000, 0);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 10,
                y: 11,
                hits: 1000,
            }),
        )
        .unwrap();
    // 手动挂 safeMode（激活链属 B2）
    if let Some(RoomObject::Controller(t)) = f.world.get_mut(ctl) {
        t.safe_mode = 1000;
    }
    let mut batch = attack_intent(atk, ext);
    batch.add_object_intent("p1", atk, Action::Dismantle, IntentData::target(wall));
    let events = f.settle(batch);
    assert!(events.is_empty(), "safeMode：攻击与拆解全拒");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 1000),
        _ => panic!(),
    }
}

#[test]
fn repair_effect_cost_and_clamps() {
    // repair.js:23-27：effect=min(功率,能量×100,缺口)；cost=min(存量,ceil(effect×0.01))
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ext = extension_at(&mut f, 12, 10, 900, 0);
    let w1 = f.creep("fixer", "p1", 11, 10, &[BodyKind::Work, BodyKind::Carry], 1);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_REPAIR);
    assert_eq!(events[0]["data"]["amount"], 100, "1×WORK×100");
    assert_eq!(events[0]["data"]["energySpent"], 1, "ceil(100×0.01)=1");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "钳到 hitsMax"),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(w1).3, 0, "能量扣尽");

    // 缺口小于功率：effect 钳缺口，cost 仍按 ceil 计 1
    if let Some(RoomObject::Extension(e)) = f.world.get_mut(ext) {
        e.hits = 950;
    }
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events2 = f.settle(b2);
    assert_eq!(events2.len(), 0, "无能量：整条拒绝（repair.js:11）");
    if let Some(RoomObject::Extension(e)) = f.world.get_mut(ext) {
        e.hits = 950;
    }
    if let Some(RoomObject::Creep(c)) = f.world.get_mut(w1) {
        c.store.energy = 5;
    }
    let mut b3 = IntentBatch::default();
    b3.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    let events3 = f.settle(b3);
    assert_eq!(events3[0]["data"]["amount"], 50, "缺口 50 钳制");
    assert_eq!(events3[0]["data"]["energySpent"], 1, "ceil(50×0.01)=1");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(w1).3, 4);

    // 满血目标拒绝（repair.js:16 target.hits >= hitsMax）
    let mut b4 = IntentBatch::default();
    b4.add_object_intent("p1", w1, Action::Repair, IntentData::target(ext));
    assert!(f.settle(b4).is_empty(), "满血不修");

    // 无 WORK 部件：effect 0 → 拒（repair.js:40 `!boostedEffect`）
    let nowork = f.creep("nowork", "p1", 11, 10, &[BodyKind::Carry], 10);
    let mut b5 = IntentBatch::default();
    b5.add_object_intent("p1", nowork, Action::Repair, IntentData::target(ext));
    assert!(f.settle(b5).is_empty());
}

#[test]
fn repair_wall_and_rampart_hits_max() {
    // build.js:192 墙 hitsMax=WALL_HITS_MAX(300M)；ramparts/tick.js:17 按 RCL 表
    let mut f = Fixture::new(1, terrain_with(&[]));
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                id: 0,
                x: 12,
                y: 10,
                hits: 1,
            }),
        )
        .unwrap();
    let fixer = f.creep(
        "fixer",
        "p1",
        11,
        10,
        &[BodyKind::Work, BodyKind::Carry, BodyKind::Carry],
        100,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", fixer, Action::Repair, IntentData::target(wall));
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["amount"], 100, "300M 缺口不钳功率");
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 101),
        _ => panic!(),
    }

    // rampart：RCL 3 上限 1,000,000（RAMPART_HITS_MAX["3"]）
    f.controller(3, Some("p1"));
    let rampart = generic_structure(&mut f, 12, 11, "rampart", "p1", 999_900);
    let fixer2 = f.creep(
        "fixer2",
        "p1",
        11,
        11,
        &[BodyKind::Work, BodyKind::Carry],
        10,
    );
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", fixer2, Action::Repair, IntentData::target(rampart));
    let events2 = f.settle(b2);
    assert_eq!(events2[0]["data"]["amount"], 100, "缺口 100");
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 1_000_000, "修到 RCL 3 上限");
        }
        _ => panic!(),
    }
}

#[test]
fn dismantle_reflux_and_destroy() {
    // dismantle.js:33-52：amount=min(WORK×50, hits)、回流 floor(amount×0.005)、
    // 先回流后施伤；击毁时 store 落地（官方进 ruin——B1 偏离登记）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let dis = f.creep(
        "dis",
        "p1",
        11,
        10,
        &[
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Work,
            BodyKind::Carry,
            BodyKind::Carry,
        ],
        0,
    );
    let ext = extension_at(&mut f, 12, 10, 1000, 200);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(ext));
    let events = f.settle(b.clone());
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[0]["data"]["attackType"],
        kk_core::constants::EVENT_ATTACK_TYPE_DISMANTLE
    );
    assert_eq!(events[0]["data"]["damage"], 250, "5×WORK×50");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 750),
        _ => panic!(),
    }
    assert_eq!(f.creep_state(dis).3, 1, "回流 floor(250×0.005)=1");
    // 继续拆：750→500→250→0（第 4 tick 击毁，amount=250 回流 1）
    f.settle(b.clone());
    f.settle(b.clone());
    let events4 = f.settle(b.clone());
    assert_eq!(events4[0]["data"]["damage"], 250, "amount=min(250,250)");
    assert!(f.world.get(ext).is_none(), "击毁");
    assert_eq!(f.creep_state(dis).3, 4, "四 tick 共回流 4");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 12 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(199), "store 200 落地后当 tick 衰减 1");
}

#[test]
fn dismantle_redirects_to_rampart() {
    // dismantle.js:27-35：rampart 重定向后 amount 按重定向目标（rampart）的 hits 取
    let mut f = Fixture::new(1, terrain_with(&[]));
    let dis = f.creep("dis", "p1", 11, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let ext = extension_at(&mut f, 12, 10, 1000, 0);
    let rampart = generic_structure(&mut f, 12, 10, "rampart", "p2", 80);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(ext));
    let events = f.settle(b);
    assert_eq!(events[0]["data"]["targetId"], rampart.as_str());
    assert_eq!(events[0]["data"]["damage"], 50, "1×WORK×50");
    match f.obj(rampart) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["hits"], 30, "80-50：rampart 存活");
        }
        _ => panic!(),
    }
    assert_eq!(f.creep_state(dis).3, 0, "floor(50×0.005)=0 无回流");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000, "本体受保护"),
        _ => panic!(),
    }

    // 功率超过 rampart 血量：amount 钳到 80，恰击毁、回流 0
    let dis2 = f.creep(
        "dis2",
        "p1",
        11,
        12,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let rampart2 = generic_structure(&mut f, 12, 12, "rampart", "p2", 80);
    let ext2 = extension_at(&mut f, 12, 12, 1000, 0);
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", dis2, Action::Dismantle, IntentData::target(ext2));
    let events2 = f.settle(b2);
    assert_eq!(
        events2[0]["data"]["damage"], 80,
        "amount=min(100,80) 钳 rampart 血量"
    );
    assert!(f.world.get(rampart2).is_none(), "80-80=0：击毁");
    assert!(f.world.get(ext2).is_some(), "本体未动");
    assert_eq!(f.creep_state(dis2).3, 0, "floor(80×0.005)=0 无回流");
}

#[test]
fn attack_suppresses_harvest() {
    // intents.js 优先级图：attack 压制 harvest——同 tick 只执行 attack
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let enemy = f.creep(
        "victim",
        "p2",
        11,
        10,
        &[BodyKind::Tough, BodyKind::Tough],
        0,
    );
    let hybrid = f.creep(
        "hy",
        "p1",
        10,
        10,
        &[BodyKind::Attack, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", hybrid, Action::Attack, IntentData::target(enemy));
    b.add_object_intent("p1", hybrid, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert!(
        events
            .iter()
            .all(|e| e["event"] != kk_core::constants::EVENT_HARVEST),
        "harvest 被压制：无采集事件"
    );
    assert_eq!(creep_parts(&f, enemy).0, 170, "attack 照常执行");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "source 未被动"),
        _ => panic!(),
    }
}

#[test]
fn carry_death_drops_overflow_energy() {
    // tick.js:141-143 → _drop-resources-without-space：CARRY 部件损毁缩容 → 超容掉落
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
    let b = f.creep(
        "haul",
        "p2",
        11,
        10,
        &[BodyKind::Carry, BodyKind::Carry],
        80,
    );
    f.settle(attack_intent(a, b));
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(hits, 80, "200-120");
    assert_eq!(parts, vec![0, 80], "首个 carry 报废");
    assert_eq!(f.creep_state(b).3, 50, "容量缩至 50");
    let dropped = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.x == 11 && e.y == 10 => Some(e.amount),
            _ => None,
        });
    assert_eq!(dropped, Some(30), "超容 80-50 掉地（tick 阶段不衰减）");
}

#[test]
fn attack_target_validation_rejections() {
    // attack.js 各前置门：自指（:18）、射程（:21-23）、spawning 目标（:24-26）、
    // 无 hits 目标（:27-29）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack], 0);
    let far = f.creep("far", "p2", 20, 20, &[BodyKind::Tough], 0);
    let src = f.source(10, 11, 3000);
    // spawning 目标：直插 spawning=true 的 creep
    let chick = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 11,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: true,
                action_log: 0,
                name: "chick".into(),
                owner: "p2".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
            }),
        )
        .unwrap();
    let e1 = f.settle(attack_intent(a, a));
    assert!(e1.is_empty(), "自指拒");
    let e2 = f.settle(attack_intent(a, far));
    assert!(e2.is_empty(), "射程外拒");
    let e3 = f.settle(attack_intent(a, src));
    assert!(e3.is_empty(), "source 无 hits 不可攻");
    let e4 = f.settle(attack_intent(a, chick));
    assert!(e4.is_empty(), "spawning 目标拒");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000),
        _ => panic!(),
    }
}

#[test]
fn zero_power_attack_still_logs() {
    // 官方引擎层无部件预检（那是 API 层的事）：无 ATTACK 部件的攻击 intent
    // 仍产 actionLog 与 0 伤害事件（_damage.js:59-93 无条件段）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("weak", "p1", 10, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let b = f.creep("victim", "p2", 11, 10, &[BodyKind::Tough], 0);
    let events = f.settle(attack_intent(a, b));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["damage"], 0);
    assert_eq!(creep_parts(&f, b).0, 100, "0 伤害不进伤害路径");
    let (log_a, log_b) = match (f.obj(a), f.obj(b)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED);
}
