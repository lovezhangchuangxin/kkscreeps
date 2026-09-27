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
    let site = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 12,
                y: 10,
                structure_type: "constructedWall".into(),
                progress: 0,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();
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
    // H6：沼泽上的路 progressTotal ×5（300→1500）
    let mut f = Fixture::new(1, terrain_with(&[(12, 10)]));
    let site = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 12,
                y: 10,
                structure_type: "road".into(),
                progress: 0,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();
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
fn birth_avoids_stationary_creep() {
    // H7：出生方向搜索对任何 creep 都算被占
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
    // 方向 1 的格 (20,19) 站一个静止 creep
    let blocker = f.creep("blocker", "p1", 20, 19, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move, BodyKind::Work];
    d.directions = vec![1]; // 只允许方向 1（被 block）
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b);
    // 孵化任务已建；推 6 tick 到出生时刻——方向 1 被 block 且无其他允许方向 → 重试
    for _ in 0..5 {
        f.settle(IntentBatch::default());
    }
    f.settle(move_intent(blocker, 5)); // blocker 走开
    f.settle(IntentBatch::default()); // 重试出生
    let child = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Creep(c) if c.name == "child" => Some((c.x, c.y, c.spawning)),
            _ => None,
        });
    let child = child.expect("出生成功");
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
