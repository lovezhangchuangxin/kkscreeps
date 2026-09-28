//! 单房间结算集成测试：固定指令脚本驱动，逐项对照官方数值
//! （常量值与官方 file:line 见各断言注释）。

use std::collections::{BTreeMap, BTreeSet};

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
                    store: kk_schema::room::Store { energy, ghodium: 0 },
                }),
            )
            .unwrap()
    }

    fn controller(&mut self, level: u8, owner: Option<&str>) -> ObjId {
        self.controller_full(level, owner, |_| {})
    }

    /// controller 全字段形态：level/owner 之外的持久化面（reservation/sign/
    /// upgrade_blocked/safe_mode_cooldown/downgrade_time/safe_mode…）经闭包
    /// 定制（B2 controller 族测试的公共入口）。
    fn controller_full(
        &mut self,
        level: u8,
        owner: Option<&str>,
        tweak: impl FnOnce(&mut kk_schema::room::ControllerObj),
    ) -> ObjId {
        let mut ctl = kk_schema::room::ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 0,
            downgrade_time: None,
            owner: owner.map(str::to_string),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        };
        tweak(&mut ctl);
        self.world
            .insert("W1N1", RoomObject::Controller(ctl))
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
                    notify_when_attacked: None,
                    id: 0,
                    x,
                    y,
                    hits: 5000,
                    off: false,
                    name: name.into(),
                    owner: "p1".into(),
                    store: kk_schema::room::Store { energy, ghodium: 0 },
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

    /// 中性路（A4-iv notifyWhenAttacked 的无主结构用例）。
    fn road_at(&mut self, x: u8, y: u8) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Road(kk_schema::room::RoadObj {
                    id: 0,
                    x,
                    y,
                    hits: 5000,
                    next_decay_time: 5000,
                    notify_when_attacked: None,
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
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 300,
                    ghodium: 0,
                },
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

// ---- A4-i：spawn 方法族结算（renewCreep/recycleCreep/setSpawnDirections/
// cancelSpawning——官方 processor/intents/spawns/ 逐条对照）----

/// 带孵化任务/定制的 spawn 插入（Fixture::spawn 的 spawning 定制形态）。
fn spawn_with_job(
    f: &mut Fixture,
    x: u8,
    y: u8,
    energy: u32,
    job: Option<kk_schema::room::SpawningJob>,
) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                id: 0,
                x,
                y,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store { energy, ghodium: 0 },
                spawning: job,
                notify_when_attacked: None,
            }),
        )
        .unwrap()
}

/// renewCreep 结算锁值（renew-creep.js）：[work,carry,move] 成本 200 →
/// effect=floor(1.2·1500/3/3)=200、扣费 ceil(1.2·200/3/3)=27；ageTime +200、
/// actionLog.healed 置位；免费涓流（energyAvailable 273<300）+1 → spawn 274。
#[test]
fn renew_creep_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let creep = f.creep(
        "old",
        "p1",
        20,
        21,
        &[BodyKind::Work, BodyKind::Carry, BodyKind::Move],
        0,
    );
    // Fixture::creep 的 age_time 是 u64::MAX/2（寿命尽）——改到 time+100
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(creep) {
        RoomObject::Creep(c) => {
            assert_eq!(c.age_time, 300, "ageTime(100) + effect(600/3=200)");
            assert_eq!(
                c.action_log & 0b1000_0000,
                0b1000_0000,
                "actionLog.healed 置位"
            );
        }
        _ => panic!(),
    }
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(s.store.energy, 274, "扣 27 + 涓流 1"),
        _ => panic!(),
    }
}

/// renewCreep 结算守卫（renew-creep.js 的逐门拒）：孵化中（含同 tick 先
/// createCreep）、CLAIM 部件、非邻接、能量不足、满寿——ageTime 全部不动。
#[test]
fn renew_creep_guards() {
    // ① spawn 孵化中拒（:12-14，先于 target 校验）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(
        &mut f,
        20,
        20,
        300,
        Some(kk_schema::room::SpawningJob {
            name: "busy".into(),
            need_time: 3,
            spawn_time: 100,
            directions: vec![],
        }),
    );
    let creep = f.creep("old", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(creep) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "孵化中的 spawn 不续命"),
        _ => panic!(),
    }

    // ② CLAIM 部件拒（:21-23）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let claimer = f.creep("claimer", "p1", 20, 21, &[BodyKind::Claim], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&claimer)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(claimer));
    f.settle(b);
    match f.obj(claimer) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "CLAIM 部件不可续命"),
        _ => panic!(),
    }

    // ③ 非邻接拒（:18-20）+ ④ 能量不足拒（:30-35）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 0, None);
    let far = f.creep("far", "p1", 30, 30, &[BodyKind::Move], 0);
    let near = f.creep("near", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&far)
        .unwrap()
    {
        c.age_time = 100;
    }
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&near)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(far));
    // near 也拒：energyAvailable 面为 0（该 fixture 无 controller——spawn 同时
    // 被 calc_spawns 标 off，可观察结果与官方 energyAvailable 0 < cost=20 一致；
    // 评审 3 O2：归因注释按实际机制写）
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(near));
    f.settle(b);
    for id in [far, near] {
        match f.obj(id) {
            RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "非邻接/能量不足不续命"),
            _ => panic!(),
        }
    }

    // ⑤ 满寿拒（:25-28：ageTime + effect > gameTime + CREEP_LIFE_TIME）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let full = f.creep("full", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&full)
        .unwrap()
    {
        c.age_time = 1500; // time=0：1500 + 600 > 0 + 1500
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(full));
    f.settle(b);
    match f.obj(full) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 1500, "满寿拒"),
        _ => panic!(),
    }

    // ⑥ 同 tick createCreep 先行置 spawning → renew 被 object.spawning 门拒
    //   （spawns/intents.js 分派序：createCreep → renewCreep）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let creep = f.creep("old", "p1", 20, 21, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&creep)
        .unwrap()
    {
        c.age_time = 100;
    }
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    b.add_object_intent("p1", spawn, Action::RenewCreep, IntentData::target(creep));
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_some(), "createCreep 先行生效"),
        _ => panic!(),
    }
    match f.obj(creep) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 100, "renew 被孵化门拒"),
        _ => panic!(),
    }
}

/// recycleCreep 结算（recycle-creep.js:23 → _die(target, 1.0, false)，B3）：
/// creep 移除、dropRate=1.0 的全额造价回收进墓碑；非邻接拒。
#[test]
fn recycle_creep_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let spawn = spawn_with_job(&mut f, 20, 20, 300, None);
    let trash = f.creep("trash", "p1", 20, 21, &[BodyKind::Move], 50);
    let far = f.creep("far", "p1", 30, 30, &[BodyKind::Move], 7);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::RecycleCreep, IntentData::target(trash));
    let mut b2 = IntentBatch::default();
    b2.add_object_intent("p1", spawn, Action::RecycleCreep, IntentData::target(far));
    f.settle(b);
    assert!(f.world.get(trash).is_none(), "回收目标移除");
    // 墓碑承接（B3）：dropRate 1.0 + 满寿（fixture 的 age_time 极大 ⇒ lifeRate
    // 巨大 ⇒ 每件钳 CREEP_PART_MAX_ENERGY=125）⇒ 50 携带 + 125 回收 = 175；
    // 1 部件 ⇒ ticksToDecay = 5
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.x == 20 && t.y == 21 => Some(t.clone()),
            _ => None,
        })
        .expect("回收死亡产墓碑");
    assert_eq!(tomb.store.energy, 175, "携带 50 + 造价回收 125");
    assert_eq!(tomb.creep_id, Some(trash.0), "尸体信息（creepId）");
    assert_eq!(tomb.creep_name.as_deref(), Some("trash"));
    assert_eq!(
        tomb.decay_time, 5,
        "死亡 tick 0 + TOMBSTONE_DECAY_PER_PART × 1 件"
    );
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 20 && e.y == 21)),
        "不直接掉地（墓碑承接）"
    );
    f.settle(b2);
    assert!(f.world.get(far).is_some(), "非邻接不回收");
    match f.obj(far) {
        RoomObject::Creep(c) => assert_eq!(c.store.energy, 7),
        _ => panic!(),
    }
}

/// setSpawnDirections 结算（set-spawn-directions.js）：合法组替换、
/// 越界组整单忽略、空列表静默忽略。
#[test]
fn set_spawn_directions_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let job = kk_schema::room::SpawningJob {
        name: "child".into(),
        need_time: 3,
        spawn_time: 100,
        directions: vec![1, 2],
    };
    let spawn = spawn_with_job(&mut f, 20, 20, 300, Some(job));

    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.directions = vec![3, 5];
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, d);
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "合法组替换"
        ),
        _ => panic!(),
    }

    // 越界（0）→ 整单忽略（官方 _.any 反向判定——非法元素不静默过滤）
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.directions = vec![0, 1];
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, d);
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "越界组忽略"
        ),
        _ => panic!(),
    }

    // 空列表 → 静默忽略（官方 directions.length > 0 门）
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::SetSpawnDirections, IntentData::bare());
    f.settle(b);
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert_eq!(
            s.spawning.as_ref().unwrap().directions,
            vec![3, 5],
            "空列表忽略"
        ),
        _ => panic!(),
    }
}

/// cancelSpawning 结算（cancel-spawning.js）：雏形 creep 裸移除（无掉落无
/// 事件——官方 bulk.remove 非 _die）+ 孵化任务清空；无任务时 no-op。
#[test]
fn cancel_spawning_settles() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let job = kk_schema::room::SpawningJob {
        name: "child".into(),
        need_time: 3,
        spawn_time: 100,
        directions: vec![1],
    };
    let spawn = spawn_with_job(&mut f, 20, 20, 300, Some(job));
    let chick = f.creep("child", "p1", 20, 20, &[BodyKind::Move], 0);
    if let RoomObject::Creep(c) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&chick)
        .unwrap()
    {
        c.spawning = true;
    }
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::CancelSpawning, IntentData::bare());
    f.settle(b);
    assert!(f.world.get(chick).is_none(), "雏形移除");
    match f.obj(spawn) {
        RoomObject::Spawn(s) => assert!(s.spawning.is_none(), "任务清空"),
        _ => panic!(),
    }
    // 无任务再 cancel：no-op（无对象可动）
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", spawn, Action::CancelSpawning, IntentData::bare());
    f.settle(b);
    assert!(f.world.get(spawn).is_some());
}

/// 雏形寿命模型（官方 create-creep.js:80-98 **不带 ageTime** + creeps/tick.js
/// :88-91 出生后首触赋值——CREEP-SPAWNING-002 的 ticksToLive undefined 根基）：
/// 孵化中 age_time=0；出生 tick 赋 time+1500（CLAIM 部件 600）；spawn 消亡的
/// 孵化中雏形被 creep tick 移除（tick.js:16-19）。
#[test]
fn chick_age_time_model() {
    // ① [move] 雏形：孵化中 0，出生 tick（time 2）赋 2+1500
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("child".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1：任务建（spawn_time 3）、雏形插入
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.name == "child" => Some(*id),
            _ => None,
        })
        .unwrap();
    match f.obj(chick) {
        RoomObject::Creep(c) => {
            assert!(c.spawning);
            assert_eq!(c.age_time, 0, "官方雏形不带 ageTime");
        }
        _ => panic!(),
    }
    f.settle(IntentBatch::default()); // time 1→2（1 < 3-1 未到点）
    f.settle(IntentBatch::default()); // time 2→3：出生（2 >= 3-1）
    match f.obj(chick) {
        RoomObject::Creep(c) => {
            assert!(!c.spawning, "已出生");
            assert_eq!(
                c.age_time, 1502,
                "出生 tick 首触赋值 time(2)+CREEP_LIFE_TIME"
            );
        }
        _ => panic!(),
    }

    // ② CLAIM 部件：CREEP_CLAIM_LIFE_TIME(600)（claim 成本 600——spawn 备足）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 700);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("claimer".into());
    d.body = vec![BodyKind::Claim];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1
    f.settle(IntentBatch::default()); // time 1→2
    f.settle(IntentBatch::default()); // time 2→3：出生（2 >= 3-1）
    let chick = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.name == "claimer" => Some(*id),
            _ => None,
        })
        .unwrap();
    match f.obj(chick) {
        RoomObject::Creep(c) => assert_eq!(c.age_time, 602, "CLAIM 部件 600"),
        _ => panic!(),
    }

    // ③ 孵化中 spawn 消亡 → 雏形被 creep tick 移除（孤儿自愈）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let spawn = f.spawn("Spawn1", 20, 20, 300);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.name = Some("orphan".into());
    d.body = vec![BodyKind::Move];
    b.add_object_intent("p1", spawn, Action::CreateCreep, d);
    f.settle(b); // time 0→1：雏形在（同格 spawn 在——正常孵化）
    let orphan = chick_of(&f, "orphan");
    assert!(f.world.get(orphan).is_some());
    f.world.room_mut("W1N1").unwrap().objects.remove(&spawn); // spawn 消亡
    f.settle(IntentBatch::default()); // time 1→2：孤儿雏形移除
    assert!(
        f.world.get(orphan).is_none(),
        "同格无 spawn 的孵化中雏形移除"
    );
}

fn chick_of(f: &Fixture, name: &str) -> ObjId {
    f.world
        .room("W1N1")
        .unwrap()
        .objects
        .iter()
        .find_map(|(id, o)| match o {
            RoomObject::Creep(c) if c.name == name => Some(*id),
            _ => None,
        })
        .unwrap()
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
                resource: None,
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
                resource: None,
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
                store: kk_schema::room::Store {
                    energy: 30,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    // 官方 creeps/tick.js:93：gameTime >= ageTime-1 → time=1 的第二次结算死亡
    f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_some(), "time=0 < age_time-1=1：未死");
    let events = f.settle(IntentBatch::default());
    assert!(f.world.get(c).is_none(), "time=1 >= 1：死亡");
    // B3（官方 _die.js）：墓碑承接——ticksToLive=1 的 lifeRate=0.2×1/1500，
    // 造价回收 (50+100)×1.33e-4 → floor 0，store 只含携带 30；2 部件 ⇒ 10 tick
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("TTL 死亡产墓碑");
    assert_eq!(tomb.store.energy, 30, "携带进墓碑（回收 floor 0）");
    assert_eq!(tomb.death_time, 1);
    assert_eq!(tomb.decay_time, 1 + 10, "2 部件 × TOMBSTONE_DECAY_PER_PART");
    assert_eq!(tomb.creep_ticks_to_live, 1, "ageTime − gameTime");
    assert_eq!(
        tomb.creep_body,
        vec![BodyKind::Move.code(), BodyKind::Work.code()],
        "尸体 body 类型码"
    );
    // _die.js:97：EVENT_OBJECT_DESTROYED {type: 'creep'}（B3 补齐）
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");
}

// ---- B3：墓碑/废墟/死亡事件族 ----

/// 墓碑 store 逐资源锁值（_die.js:43-53 的 bodyResources 精确数值）：
/// boosted 件产出 boost 矿物（LAB_BOOST_MINERAL × lifeRate）+ 能量
/// （LAB_BOOST_ENERGY × lifeRate），每件能量 min(CREEP_PART_MAX_ENERGY,
/// cost × lifeRate)；携带 store 全资源并入（第二循环）。
#[test]
fn tombstone_body_recovery_and_boost_minerals() {
    // 自杀路径（suicide.js:14 → _die 缺省 dropRate 0.2）；age_time = 0+1500 ⇒
    // ticksToLive = 1500、无 CLAIM ⇒ lifeTime 1500、lifeRate = 0.2×1500/1500 = 0.2
    let mut f = Fixture::new(1, terrain_with(&[]));
    let mut bodylist = body(&[BodyKind::Work, BodyKind::Move]);
    bodylist[0].boost = Some("XUHO2".into());
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: bodylist,
                store: kk_schema::room::Store {
                    energy: 10,
                    ghodium: 25,
                },
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("suicide 产墓碑");
    // XUHO2 = 30×0.2 = 6；energy = 20×0.2 + min(125, 100×0.2) + min(125, 50×0.2)
    // = 4 + 20 + 10 = 34；携带 10 并入同槽 ⇒ 44；G = 携带 25
    assert_eq!(tomb.store.energy, 44, "回收 34 + 携带 10");
    assert_eq!(tomb.store.ghodium, 25, "携带 G 并入");
    assert_eq!(
        tomb.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "XUHO2".into(),
            amount: 6,
        }],
        "boost 矿物回收（LAB_BOOST_MINERAL × lifeRate）"
    );
    assert_eq!(tomb.creep_ticks_to_live, 1500, "ageTime − gameTime");
    assert_eq!(tomb.decay_time, 10, "2 部件 × TOMBSTONE_DECAY_PER_PART");
}

/// 同格 container 先承接（_die.js:36/45-59：container.hits>0 时按
/// CONTAINER_CAPACITY − 全槽合计 封顶，余量进墓碑——body 回收与携带两段
/// 各自过 container 门）。
#[test]
fn tombstone_container_deposit_first() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // container（Generic 透传）：store.energy=1900 ⇒ 余量 100
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 250000,
                    "store": { "energy": 1900 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    // 满寿自杀（fixture age_time 极大 ⇒ lifeRate 巨大 ⇒ move 件钳 125）：
    // 第一段回收 125 → container 吃 100 后满，25 进墓碑；
    // 第二段携带 150 → 余量 0，150 全进墓碑 ⇒ 175
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 150);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.store.energy),
            _ => None,
        })
        .expect("墓碑仍建（余量进槽）");
    assert_eq!(
        tomb, 175,
        "125 回收（container 吃 100）+ 150 携带（余量 0）"
    );
    match f.obj(container) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(
                data["store"]["energy"], 2000,
                "container 吃满 CONTAINER_CAPACITY"
            );
        }
        _ => panic!(),
    }
}

/// 墓碑衰减到点（tombstones/tick.js:5-16）：全 store 逐资源落地（同格异资源
/// 不并堆——_create-energy 按资源名键）、墓碑移除；新掉落不在本轮对象快照，
/// 当 tick 不衰减。
#[test]
fn tombstone_decay_dumps_store_per_resource() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let t = f
        .world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 10,
                y: 10,
                death_time: 0,
                decay_time: 1, // time=0 结算：0 >= decay_time-1 = 0 即到点
                creep_id: None,
                creep_name: Some("w1".into()),
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 7,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 5,
                }],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(t).is_none(), "衰减到点移除");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![
            ("energy".to_string(), 40),
            ("G".to_string(), 7),
            ("XUHO2".to_string(), 5),
        ],
        "三资源各成一堆（BTreeMap id 序 = 落地插入序：energy→G→extra）"
    );
}

/// 废墟衰减到点（ruins/tick.js）与墓碑同形：全 store 落地 + 移除。
#[test]
fn ruin_decay_dumps_store_and_removes() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let r = f
        .world
        .insert(
            "W1N1",
            RoomObject::Ruin(kk_schema::room::RuinObj {
                id: 0,
                x: 12,
                y: 12,
                structure_id: 99,
                structure_type: "container".into(),
                structure_hits_max: Some(250000),
                destroy_time: 0,
                decay_time: 1,
                owner: Some("p1".into()),
                store: kk_schema::room::Store {
                    energy: 30,
                    ghodium: 0,
                },
                store_extra: vec![kk_schema::room::ResourceStack {
                    resource: "LH".into(),
                    amount: 4,
                }],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(r).is_none(), "废墟衰减到点移除");
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 12 && e.y == 12 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 30), ("LH".to_string(), 4)],
        "能量与 LH 各成一堆（落地插入序）"
    );
}

/// B3 评审 2/3 P1 清偿锁值：pickup 对命名矿物堆（衰减落地产物——code 恒 0 +
/// resource:Some(name)）拒绝——只看 code 会把 XUHO2 洗白成 energy（矿物凭空
/// 转换 + 灭失，普通玩法链：boosted creep 死亡→衰减→搬运工拾取）。
#[test]
fn pickup_rejects_named_mineral_pile() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let pile = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 10,
                resource_type: 0,
                resource: Some("XUHO2".into()),
                amount: 6,
            }),
        )
        .unwrap();
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Carry], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Pickup, IntentData::target(pile));
    f.settle(batch);
    match f.obj(pile) {
        RoomObject::Energy(e) => {
            assert_eq!(kk_engine::settle::energy_resource_name(e), "XUHO2");
            // 6 − 1 = 5：当 tick 掉落衰减 1（energy/tick.js 全资源同式——G 掉落
            // 测试同款口径），未被拾取
            assert_eq!(e.amount, 5, "命名堆不被拾走（−1 为当 tick 衰减）");
        }
        _ => panic!("堆必须仍在"),
    }
    assert_eq!(f.creep_state(c).3, 0, "矿物不得洗白进 energy 槽");
}

/// B3 评审 3-2 锁值：container 余量的**跨资源竞争序**（官方 bodyResources 键
/// 序 = energy 先、矿物按 body 首现序——_die.js:38-76 两段循环间容量是活的）。
/// headroom=10 只够吃部分 energy：energy 先入 container、矿物全进墓碑。
#[test]
fn tombstone_container_cross_resource_competition() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    // container 余量 10（1990/2000）
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 250000,
                    "store": { "energy": 1990 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    // body [Work(XUHO2), Move]、ttl=1500 自杀 → lifeRate=0.2×1500/1500=0.2：
    // energy 回收 = min(125,100×0.2) + 20×0.2 + min(125,50×0.2) = 20+4+10 = 34；
    // XUHO2 = 30×0.2 = 6。键序：energy 34 先过门（container 吃 10 → 墓碑 24、
    // 余量 0）→ XUHO2 6 全进墓碑
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: Some("XUHO2".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    match f.obj(container) {
        RoomObject::Generic(g) => {
            let data: serde_json::Value = serde_json::from_str(&g.data).unwrap();
            assert_eq!(data["store"]["energy"], 2000, "container 吃满");
            assert!(
                data["store"].get("XUHO2").is_none(),
                "键序 energy 先——余量 10 全被 energy 吃掉，矿物不进 container"
            );
        }
        _ => panic!(),
    }
    match f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        }) {
        Some(t) => {
            assert_eq!(t.store.energy, 24, "34−10 进墓碑");
            assert_eq!(
                t.store_extra,
                vec![kk_schema::room::ResourceStack {
                    resource: "XUHO2".into(),
                    amount: 6
                }],
                "矿物 6 全进墓碑（余量竞争可观察面锁值）"
            );
        }
        None => panic!("墓碑必须建"),
    }
}

/// B3 评审 3-3 锁值：引擎产废墟对混合 store（Generic 开放资源面）的承接全链
/// ——container(energy+XUHO2) 击毁 → ruin.store 承接（含 store_extra）→
/// 衰减落地两堆。
#[test]
fn ruin_inherits_mixed_store_and_dumps_on_decay() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let container = f
        .world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x: 10,
                y: 10,
                data: serde_json::json!({
                    "type": "structure", "structureType": "container",
                    "owner": "p1", "hits": 1,
                    "store": { "energy": 20, "XUHO2": 8 },
                })
                .to_string(),
            }),
        )
        .unwrap();
    let dis = f.creep("dis", "p1", 11, 10, &[BodyKind::Work], 0);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", dis, Action::Dismantle, IntentData::target(container));
    let events = f.settle(batch);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED,
        "击毁事件"
    );
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) => Some(r.clone()),
            _ => None,
        })
        .expect("废墟承接");
    assert_eq!(ruin.store.energy, 20);
    assert_eq!(
        ruin.store_extra,
        vec![kk_schema::room::ResourceStack {
            resource: "XUHO2".into(),
            amount: 8
        }],
        "混合 store 的非 energy/G 资源进 store_extra"
    );
    // 加速衰减到点（放置面直改钟——decay 语义本身另有锁值）
    for o in f.world.room_mut("W1N1").unwrap().objects.values_mut() {
        if let RoomObject::Ruin(r) = o {
            r.decay_time = 1;
        }
    }
    f.settle(IntentBatch::default());
    let piles: Vec<(String, u32)> = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some((
                kk_engine::settle::energy_resource_name(e).to_string(),
                e.amount,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        piles,
        vec![("energy".to_string(), 20), ("XUHO2".to_string(), 8)],
        "废墟衰减落地两堆（承接全链闭环）"
    );
}

/// B3 评审 3-4 锁值：CLAIM 部件的寿命分母（CREEP_CLAIM_LIFE_TIME=600——
/// _.any 不看 hits）。ttl=600 自杀 → lifeRate=0.2×600/600=0.2：回收
/// min(125,50×0.2)+min(125,600×0.2)=10+120=130（错分母 1500 会得 4+48=52）。
#[test]
fn claim_body_life_time_denominator() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 600,
                hits: 200,
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
                        kind: BodyKind::Claim.code(),
                        hits: 100,
                        boost: None,
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 0,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    f.settle(batch);
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.store.energy),
            _ => None,
        })
        .expect("墓碑仍建");
    assert_eq!(tomb, 130, "CLAIM 分母 600：(50+600)×0.2");
}

/// B3 评审 3-6 边界：!decayTime（放置面 0）立即到点——官方
/// tombstones/ruins/tick.js:6 `!object.decayTime || ...` 的 falsy 分支。
#[test]
fn tombstone_zero_decay_time_dumps_immediately() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let t = f
        .world
        .insert(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 10,
                y: 10,
                death_time: 0,
                decay_time: 0,
                creep_id: None,
                creep_name: None,
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: None,
                store: kk_schema::room::Store {
                    energy: 12,
                    ghodium: 0,
                },
                store_extra: vec![],
            }),
        )
        .unwrap();
    f.settle(IntentBatch::default());
    assert!(f.world.get(t).is_none(), "decayTime=0 falsy 立即到点");
    let energy = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .filter_map(|o| match o {
            RoomObject::Energy(e) if e.x == 10 && e.y == 10 => Some(e.amount),
            _ => None,
        })
        .sum::<u32>();
    assert_eq!(energy, 12);
}

/// structure.destroy() 的 room intent（destroy-structure.js：可建类型 +
/// controller 归本人 + 房内无敌方 creep）；事件为套件口径（OSS 无 push——
/// ROOM-EVENTLOG-027 期望有，登记报告）。
#[test]
fn destroy_structure_intent_paths() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(3, Some("p1"));
    let ext = extension_at(&mut f, 15, 15, 1000, 80);
    let mut b = IntentBatch::default();
    b.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext,
        },
    );
    let events = f.settle(b);
    assert!(f.world.get(ext).is_none(), "自家结构可毁");
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) => Some(r.clone()),
            _ => None,
        })
        .expect("destroy 产废墟");
    assert_eq!(ruin.structure_type, "extension");
    assert_eq!(ruin.store.energy, 80, "store 承接");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], ext.as_str());
    assert_eq!(events[0]["data"]["type"], "extension");

    // 门 1：房内有敌方 creep → 拒（destroy-structure.js:18）
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    f2.controller(1, Some("p1"));
    let ext2 = extension_at(&mut f2, 15, 15, 1000, 0);
    f2.creep("spy", "p2", 20, 20, &[BodyKind::Move], 0);
    let mut b2 = IntentBatch::default();
    b2.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext2,
        },
    );
    f2.settle(b2);
    assert!(f2.world.get(ext2).is_some(), "敌方 creep 在房拒毁");

    // 门 2：controller 非本人 → 拒（:14）
    let mut f3 = Fixture::new(3, terrain_with(&[]));
    f3.controller(1, Some("p2"));
    let ext3 = extension_at(&mut f3, 15, 15, 1000, 0);
    let mut b3 = IntentBatch::default();
    b3.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: ext3,
        },
    );
    f3.settle(b3);
    assert!(f3.world.get(ext3).is_some(), "非 controller 主人拒毁");

    // 门 3：非 CONSTRUCTION_COST 类型（source）→ 拒（:12）
    let mut f4 = Fixture::new(4, terrain_with(&[]));
    f4.controller(1, Some("p1"));
    let src4 = f4.source(30, 30, 3000);
    let mut b4 = IntentBatch::default();
    b4.add_room_intent(
        "p1",
        kk_engine::intent::RoomIntent::DestroyStructure {
            room: "W1N1".into(),
            id: src4,
        },
    );
    f4.settle(b4);
    assert!(
        f4.world.get(src4).is_some(),
        "source 无 CONSTRUCTION_COST 拒毁"
    );
}

/// 事件缓冲随房持久化（官方 saveRoomEventLog 每 tick 整体覆写——
/// processor.js:497）：有事件的 tick 落字符串数组、无事件的 tick 清空。
#[test]
fn room_events_buffer_overwritten_per_tick() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Work, BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert_eq!(events.len(), 1);
    let stored = f.world.room("W1N1").unwrap().events.clone();
    assert_eq!(stored.len(), 1, "事件随房落账");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stored[0]).unwrap(),
        events[0],
        "缓冲是结算事件的序列化原文"
    );
    f.settle(IntentBatch::default());
    assert!(
        f.world.room("W1N1").unwrap().events.is_empty(),
        "无事件 tick 覆写为空（官方 hset 整体覆写）"
    );
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
                notify_when_attacked: None,
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
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 250,
                    ghodium: 0,
                },
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
                owner_cleared: false,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
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
                notify_when_attacked: None,
                id: 0,
                x: 20,
                y: 20,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 40,
                    ghodium: 0,
                },
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
                notify_when_attacked: None,
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
                resource: None,
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
                notify_when_attacked: None,
                id: 0,
                x,
                y,
                hits,
                off: false,
                owner: "p2".into(),
                store: kk_schema::room::Store { energy, ghodium: 0 },
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
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED as u16);
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
        kk_engine::settle::creep::ACTION_ATTACK as u16
            | kk_engine::settle::creep::ACTION_ATTACKED as u16
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
fn attack_kills_creep_into_tombstone() {
    // tick.js:134-136：hits 归零 → 暴力死亡，与 TTL 死亡同路径（B3：墓碑承接）
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
    let events = f.settle(attack_intent(a, b));
    assert!(f.world.get(b).is_none(), "1200 ≥ 200：击杀");
    // 墓碑承接（B3）：dropRate 0.2、满寿（fixture age_time 极大 ⇒ lifeRate 巨大
    // ⇒ 每件钳 125）⇒ 40 携带 + 125×2 回收 = 290；2 部件 ⇒ 10 tick
    let tomb = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.x == 11 && t.y == 10 => Some(t.clone()),
            _ => None,
        })
        .expect("暴亡产墓碑");
    assert_eq!(tomb.store.energy, 290, "携带 40 + 造价回收 250");
    assert_eq!(tomb.owner.as_deref(), Some("p2"), "墓碑归死者");
    assert_eq!(tomb.creep_name.as_deref(), Some("victim"));
    // 事件序（官方）：intent 相的 EVENT_ATTACK 先（_damage.js:93）、tick 相的
    // OBJECT_DESTROYED 后（_die.js:97）
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_ATTACK);
    assert_eq!(
        events[1]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[1]["objectId"], b.as_str());
}

#[test]
fn attack_kills_structure_and_spawn_chick() {
    // structures/_destroy.js:9-15：孵化中的 spawn 被毁 → 雏形一并移除；
    // B3：store 进废墟（官方 _destroy.js:37 的 ruin.store = object.store || {}）
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
    // B3（官方 _destroy.js）：spawn 击毁产废墟——结构存照 {id, hitsMax,
    // user} + store 全量承接（300-50 孵化费+1 涓流=251），衰减钟 500 tick
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 20 && r.y == 20 => Some(r.clone()),
            _ => None,
        })
        .expect("击毁产废墟");
    assert_eq!(ruin.structure_type, "spawn");
    assert_eq!(ruin.structure_id, spawn.0, "存照 id = 原结构 id");
    assert_eq!(
        ruin.structure_hits_max,
        Some(kk_core::constants::SPAWN_HITS as u32)
    );
    assert_eq!(ruin.owner.as_deref(), Some("p1"));
    assert_eq!(ruin.destroy_time, 1, "第二个结算 tick");
    assert_eq!(ruin.decay_time, 1 + 500, "RUIN_DECAY");
    assert_eq!(ruin.store.energy, 251, "store 全量承接（不再掉地）");
    assert!(
        !f.world
            .room("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(e) if e.x == 20 && e.y == 20)),
        "store 不再落地（废墟承接）"
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
    // attack.js:30-32 / dismantle.js:24-26：他人 safeMode 生效期拒（引擎层二次门）。
    // attack 与 dismantle 各自独立 settle 验证——两者同 creep 同 tick 会互相压制
    // （dismantle 在 attack 的压制表里），单一空事件断言无法归因到 attack 的门
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let atk = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack, BodyKind::Work], 0);
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
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
    let ext = extension_at(&mut f, 11, 10, 1000, 0);
    // attack 的门：单独 settle
    let events = f.settle(attack_intent(atk, ext));
    assert!(events.is_empty(), "safeMode：攻击拒");
    match f.obj(ext) {
        RoomObject::Extension(e) => assert_eq!(e.hits, 1000),
        _ => panic!(),
    }
    // dismantle 的门：单独 settle
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", atk, Action::Dismantle, IntentData::target(wall));
    let events2 = f.settle(batch);
    assert!(events2.is_empty(), "safeMode：拆解拒");
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
                notify_when_attacked: None,
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
    // 先回流后施伤；击毁时 store 进废墟（B3——官方 _destroy.js:37）
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
    // B3 事件序（官方 _damage.js）：击毁 tick 的 OBJECT_DESTROYED（:51）先于
    // 主 EVENT_ATTACK（:93）
    assert_eq!(events4.len(), 2);
    assert_eq!(
        events4[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events4[0]["data"]["type"], "extension");
    assert_eq!(events4[1]["data"]["damage"], 250, "amount=min(250,250)");
    assert!(f.world.get(ext).is_none(), "击毁");
    assert_eq!(f.creep_state(dis).3, 4, "四 tick 共回流 4");
    // B3：store 进废墟（不再掉地衰减）
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 12 && r.y == 10 => Some(r.clone()),
            _ => None,
        })
        .expect("拆毁产废墟");
    assert_eq!(ruin.structure_type, "extension");
    assert_eq!(ruin.store.energy, 200, "store 200 全量承接");
    assert_eq!(
        ruin.decay_time,
        3 + 500,
        "第 4 个结算 tick（time=3）+ RUIN_DECAY"
    );
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
    // B3 事件序：击毁的 OBJECT_DESTROYED 先于主 EVENT_ATTACK（官方
    // _damage.js:51/:93）；damage 断言移到 attack 事件上
    assert_eq!(events2.len(), 2);
    assert_eq!(
        events2[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events2[0]["data"]["type"], "rampart");
    assert_eq!(
        events2[1]["data"]["damage"], 80,
        "amount=min(100,80) 钳 rampart 血量"
    );
    assert!(f.world.get(rampart2).is_none(), "80-80=0：击毁");
    // B3 评审 3-9：房事件缓冲（RoomState.events——getEventLog 的落账面）与
    // 返回序一致（DESTROYED 先于 ATTACK）
    let buffered: Vec<serde_json::Value> = f
        .world
        .room("W1N1")
        .unwrap()
        .events
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(buffered.len(), 2);
    assert_eq!(
        buffered[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED,
        "缓冲序 = 官方 _damage.js 序（:51 先于 :93）"
    );
    assert!(f.world.get(ext2).is_some(), "本体未动");
    assert_eq!(f.creep_state(dis2).3, 0, "floor(80×0.005)=0 无回流");
    // rampart 击毁同样产废墟（Generic 透传结构——存照按 data 面）
    let ruin = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Ruin(r) if r.x == 12 && r.y == 12 => Some(r.clone()),
            _ => None,
        })
        .expect("rampart 击毁产废墟");
    assert_eq!(ruin.structure_type, "rampart");
    assert_eq!(ruin.owner.as_deref(), Some("p2"), "存照主人（data.owner）");
    // hitsMax 存照：无 controller 的房按 RCL 表无门（官方 ramparts/tick.js:17
    // 按房主归一——本房无主 ⇒ 表值 0；我们无 controller 时 hitsMax 计算得
    // None，投影 undefined——B3 报告登记的边缘差异）
    assert_eq!(ruin.structure_hits_max, None);
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
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_ATTACK as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_ATTACKED as u16);
}

// ---- B1 复审阻塞项追证：官方 _oldHits 是不可观测死机制，损毁部件下一 tick 失效 ----
// 证据链见 damage.rs distribute_hits 注释：recalcBody 冻结 _oldHits 后紧随的
// bulk.update 经 removeHidden 原位剥离（活对象与落库载荷同时失去），下一 tick
// 功率计数恒为 hits>0。以下两用例锁该可观测语义。

#[test]
fn destroyed_attack_part_loses_power_next_tick() {
    // t1：B [attack,attack,tough,tough] 受击 120 → 首个 attack 报废；
    // t2：B 的反击功率 = 1×30（hits>0 计数）而非 2×30；t3：两个 attack 全灭 → 无反击
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("atk", "p1", 10, 10, &[BodyKind::Attack; 4], 0);
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
    // t1：双方满编——反击 2×30=60
    let events1 = f.settle(attack_intent(a, b));
    assert_eq!(
        events1[0]["data"]["damage"], 60,
        "受损当 tick 部件尚未灭：反击 60"
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(
        (hits, parts),
        (280, vec![0, 80, 100, 100]),
        "首个 attack 报废"
    );
    // t2：反击功率 = 存活 attack 部件数 ×30 = 30（不是 _oldHits 冻结的 60）
    let events2 = f.settle(attack_intent(a, b));
    assert_eq!(events2[0]["data"]["damage"], 30, "损毁部件下一 tick 失效");
    assert_eq!(
        events2[1]["data"]["damage"], 120,
        "攻击者未受损：主伤害不变"
    );
    let (hits, parts) = creep_parts(&f, b);
    assert_eq!(
        (hits, parts),
        (160, vec![0, 0, 60, 100]),
        "两个 attack 均灭"
    );
    // t3：无存活 attack 部件 → 无反击事件
    let events3 = f.settle(attack_intent(a, b));
    assert_eq!(events3.len(), 1, "仅主事件");
    assert_eq!(events3[0]["data"]["damage"], 120);
}

#[test]
fn destroyed_work_part_repairs_at_half_power_next_tick() {
    // repair.js:23 的 (hits>0||_oldHits>0) 同族：WORK 部件损毁后下一 tick
    // 修复功率按存活部件数计（_oldHits 已被 removeHidden 剥离）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let fixer = f.creep(
        "fixer",
        "p1",
        11,
        10,
        &[BodyKind::Work, BodyKind::Work, BodyKind::Carry],
        100,
    );
    let enemy = f.creep("atk", "p2", 10, 10, &[BodyKind::Attack; 4], 0);
    // t1：fixer 受击 120 → 300-120=180 → [0,80,100]（首个 work 报废）
    f.settle(attack_intent_by("p2", enemy, fixer));
    let (hits, parts) = creep_parts(&f, fixer);
    assert_eq!((hits, parts), (180, vec![0, 80, 100]));
    // t2：修复功率 = 1×REPAIR_POWER = 100（不是 _oldHits 冻结的 200）
    let wall = f
        .world
        .insert(
            "W1N1",
            RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
                notify_when_attacked: None,
                id: 0,
                x: 12,
                y: 10,
                hits: 1,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", fixer, Action::Repair, IntentData::target(wall));
    let events = f.settle(b);
    assert_eq!(
        events[0]["data"]["amount"], 100,
        "损毁的 WORK 部件下一 tick 不计功率"
    );
    match f.obj(wall) {
        RoomObject::ConstructedWall(w) => assert_eq!(w.hits, 101),
        _ => panic!(),
    }
}

// ---- B1.1：heal / rangedHeal 执行器 ----
// 官方对照：engine/src/processor/intents/creeps/{heal,rangedHeal}.js、
// creeps/tick.js:118-136（_healToApply 施加点：伤害先、治疗后、死亡判定最后）。

fn heal_intent(healer: ObjId, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", healer, Action::Heal, IntentData::target(target));
    b
}

fn ranged_heal_intent(healer: ObjId, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", healer, Action::RangedHeal, IntentData::target(target));
    b
}

#[test]
fn heal_restores_hits_and_redistributes_parts() {
    // heal.js:29 + tick.js:126-139：+HEAL×HEAL_POWER(12) 延迟施加，治疗同样
    // 重排部件分布（从尾部灌满——前列部件先回血）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Tough], 0);
    let enemy = f.creep("atk", "p2", 11, 10, &[BodyKind::Attack], 0);
    let healer = f.creep("medic", "p1", 10, 11, &[BodyKind::Heal], 0);
    // t1：受击 30 → 170，parts [70,100]
    f.settle(attack_intent_by("p2", enemy, victim));
    assert_eq!(creep_parts(&f, victim), (170, vec![70, 100]));
    // t2：近战治疗 +12 → 182，parts [82,100]（前列回血）
    let events = f.settle(heal_intent(healer, victim));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event"], kk_core::constants::EVENT_HEAL);
    assert_eq!(events[0]["objectId"], healer.as_str());
    assert_eq!(events[0]["data"]["targetId"], victim.as_str());
    assert_eq!(events[0]["data"]["amount"], 12, "1×HEAL×12");
    assert_eq!(
        events[0]["data"]["healType"],
        kk_core::constants::EVENT_HEAL_TYPE_MELEE
    );
    assert_eq!(creep_parts(&f, victim), (182, vec![82, 100]));
    // actionLog：heal 位（施疗者）+ healed 位（受疗者）
    let (log_healer, log_victim) = match (f.obj(healer), f.obj(victim)) {
        (RoomObject::Creep(a), RoomObject::Creep(b)) => (a.action_log, b.action_log),
        _ => panic!(),
    };
    assert_eq!(log_healer, kk_engine::settle::creep::ACTION_HEAL as u16);
    assert_eq!(log_victim, kk_engine::settle::creep::ACTION_HEALED as u16);
}

#[test]
fn ranged_heal_range_and_power() {
    // rangedHeal.js:21-23 射程 3；:26 功率 = HEAL×RANGED_HEAL_POWER(4)
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    if let Some(RoomObject::Creep(c)) = f.world.get_mut(victim) {
        c.hits = 50; // 直接造伤
    }
    let medic = f.creep("medic", "p1", 13, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    let events = f.settle(ranged_heal_intent(medic, victim));
    assert_eq!(events[0]["data"]["amount"], 8, "2×HEAL×4");
    assert_eq!(
        events[0]["data"]["healType"],
        kk_core::constants::EVENT_HEAL_TYPE_RANGED
    );
    assert_eq!(creep_parts(&f, victim).0, 58);
    // 射程 4 拒（Chebyshev）
    let far = f.creep("far", "p1", 14, 10, &[BodyKind::Heal], 0);
    assert!(f.settle(ranged_heal_intent(far, victim)).is_empty());
    assert_eq!(creep_parts(&f, victim).0, 58, "射程外不治");
}

#[test]
fn equal_damage_and_heal_same_tick_survives() {
    // COMBAT-SIMULT-002 引擎侧：tick.js:120-136 伤害先施加（可瞬时为负）、
    // 治疗后回补、死亡判定在两者之后——同 tick 治疗量 ≥ 伤害超出量即存活
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    let enemy = f.creep("atk", "p2", 10, 11, &[BodyKind::Attack; 4], 0);
    let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    let mut batch = attack_intent_by("p2", enemy, victim);
    batch.add_object_intent("p1", medic, Action::Heal, IntentData::target(victim));
    let events = f.settle(batch);
    // 100 - 120 + 24 = 4：存活
    assert!(f.world.get(victim).is_some(), "等量抵消：存活");
    assert_eq!(creep_parts(&f, victim).0, 4);
    assert_eq!(creep_parts(&f, victim).1, vec![4]);
    assert_eq!(events.len(), 2, "攻击与治疗事件各一");
    // 对照：去掉治疗即死（同伤害通道）
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    let v2 = f2.creep("v", "p1", 10, 10, &[BodyKind::Tough], 0);
    let e2 = f2.creep("atk", "p2", 10, 11, &[BodyKind::Attack; 4], 0);
    f2.settle(attack_intent_by("p2", e2, v2));
    assert!(f2.world.get(v2).is_none(), "无治疗：死亡");
}

#[test]
fn heal_clamps_at_hits_max() {
    // tick.js:130-132：治疗后钳 hitsMax（=body 部件数×100）；满血再治无变化
    let mut f = Fixture::new(1, terrain_with(&[]));
    let victim = f.creep("v", "p1", 10, 10, &[BodyKind::Tough, BodyKind::Tough], 0);
    if let Some(RoomObject::Creep(c)) = f.world.get_mut(victim) {
        c.hits = 190; // parts 隐含 [90,100]
        c.body[0].hits = 90;
    }
    let medic = f.creep("medic", "p1", 11, 10, &[BodyKind::Heal, BodyKind::Heal], 0);
    f.settle(heal_intent(medic, victim));
    assert_eq!(
        creep_parts(&f, victim),
        (200, vec![100, 100]),
        "钳到 hitsMax"
    );
    // 满血再治：无变化（tick.js:137 不重排），事件照发
    let events = f.settle(heal_intent(medic, victim));
    assert_eq!(events[0]["data"]["amount"], 24);
    assert_eq!(creep_parts(&f, victim), (200, vec![100, 100]));
}

#[test]
fn self_heal_allowed() {
    // heal.js 无自指拒（对照 attack.js:18）：施疗者=受疗者合法，heal/healed
    // 两个 actionLog 位落在同一 creep
    let mut f = Fixture::new(1, terrain_with(&[]));
    let solo = f.creep(
        "solo",
        "p1",
        10,
        10,
        &[BodyKind::Heal, BodyKind::Heal, BodyKind::Tough],
        0,
    );
    let enemy = f.creep("atk", "p2", 11, 10, &[BodyKind::Attack], 0);
    f.settle(attack_intent_by("p2", enemy, solo));
    assert_eq!(creep_parts(&f, solo).0, 270, "300-30");
    f.settle(heal_intent(solo, solo));
    assert_eq!(creep_parts(&f, solo).0, 294, "自愈 +24");
    let log = match f.obj(solo) {
        RoomObject::Creep(c) => c.action_log,
        _ => panic!(),
    };
    assert_eq!(
        log,
        kk_engine::settle::creep::ACTION_HEAL as u16
            | kk_engine::settle::creep::ACTION_HEALED as u16
    );
}

#[test]
fn heal_suppresses_harvest() {
    // intents.js 优先级图：heal 位于链顶——压制 harvest
    let mut f = Fixture::new(1, terrain_with(&[]));
    let src = f.source(10, 11, 3000);
    let ally = f.creep("ally", "p1", 11, 11, &[BodyKind::Tough], 0);
    let hybrid = f.creep(
        "hy",
        "p1",
        10,
        10,
        &[BodyKind::Heal, BodyKind::Work, BodyKind::Carry],
        0,
    );
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", hybrid, Action::Heal, IntentData::target(ally));
    b.add_object_intent("p1", hybrid, Action::Harvest, IntentData::target(src));
    let events = f.settle(b);
    assert!(
        events
            .iter()
            .all(|e| e["event"] != kk_core::constants::EVENT_HARVEST),
        "harvest 被压制"
    );
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_HEAL,
        "heal 照常执行"
    );
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "source 未被动"),
        _ => panic!(),
    }
}

#[test]
fn zero_power_heal_still_emits_event() {
    // heal.js:34 无条件发事件（引擎层无部件预检——API 层的事）：无 HEAL 部件
    // 的 heal intent 仍产 actionLog 与 0 治疗量事件
    let mut f = Fixture::new(1, terrain_with(&[]));
    let quack = f.creep("quack", "p1", 10, 10, &[BodyKind::Work, BodyKind::Carry], 0);
    let ally = f.creep("ally", "p1", 11, 10, &[BodyKind::Tough], 0);
    let events = f.settle(heal_intent(quack, ally));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"]["amount"], 0);
    assert_eq!(creep_parts(&f, ally).0, 100, "0 治疗不进结算路径");
    let (log_a, log_b) = match (f.obj(quack), f.obj(ally)) {
        (RoomObject::Creep(x), RoomObject::Creep(y)) => (x.action_log, y.action_log),
        _ => panic!(),
    };
    assert_eq!(log_a, kk_engine::settle::creep::ACTION_HEAL as u16);
    assert_eq!(log_b, kk_engine::settle::creep::ACTION_HEALED as u16);
}

#[test]
fn heal_target_validation_rejections() {
    // heal.js 各前置门：spawning 施疗者（:11-13）、非 creep 目标（:17-19）、
    // spawning 目标（:17-19）、射程外（:21-23）、他人 safeMode（:25-27）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let medic = f.creep("medic", "p1", 10, 10, &[BodyKind::Heal], 0);
    let ally = f.creep("ally", "p1", 12, 10, &[BodyKind::Tough], 0);
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
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: Default::default(),
            }),
        )
        .unwrap();
    let ext = extension_at(&mut f, 11, 11, 1000, 0);
    // 近战射程 2 拒
    assert!(f.settle(heal_intent(medic, ally)).is_empty(), "射程 2 拒");
    // 非 creep 目标拒
    assert!(f.settle(heal_intent(medic, ext)).is_empty(), "结构不可治");
    // spawning 目标拒
    assert!(
        f.settle(heal_intent(medic, chick)).is_empty(),
        "spawning 拒"
    );
    // spawning 施疗者拒
    let mut b0 = IntentBatch::default();
    b0.add_object_intent("p1", chick, Action::Heal, IntentData::target(medic));
    assert!(f.settle(b0).is_empty(), "spawning 施疗者拒");
    // 他人 safeMode 拒（heal.js:25-27 与 attack 同形）
    let ctl = f.controller(2, Some("p2"));
    if let Some(RoomObject::Controller(t)) = f.world.get_mut(ctl) {
        t.safe_mode = 1000;
    }
    let near = f.creep("near", "p1", 11, 10, &[BodyKind::Tough], 0);
    assert!(f.settle(heal_intent(medic, near)).is_empty(), "safeMode 拒");
    assert_eq!(creep_parts(&f, near).0, 100);
}

// ---- B2：controller 结算族（claim / reserve / attack / sign / generateSafeMode
// / activateSafeMode 激活链 / 踩工地 safeMode 门 / source 容量 reservation 分支）----

fn ctl_of(f: &Fixture) -> &kk_schema::room::ControllerObj {
    f.world
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Controller(t) => Some(t),
            _ => None,
        })
        .expect("测试世界必须有 controller")
}

fn controller_action(player: &str, ctl: ObjId, action: Action, data: IntentData) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent(player, ctl, action, data);
    b
}

#[test]
fn claim_controller_success_and_state_reset() {
    // claimController.js:45-53：level=1、owner 换人、progress=0、
    // downgradeTime=null、reservation=null；GCL 门（:41）：rooms=0 时
    // calcNeededGcl(1) = 1e6 × 0^2.4 = 0——任何 gcl 放行
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.progress = 777;
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 123,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Move], 0);
    f.players.insert("p1".into(), PlayerState::default());
    let events = f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert!(events.is_empty(), "claim 无事件（官方处理器不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.level, 1, "claim → level 1");
    assert_eq!(t.owner.as_deref(), Some("p1"));
    assert_eq!(t.progress, 0, "progress 清零");
    // claim 落地 downgradeTime=null（:51），同 tick 的 controller tick 随即按
    // tick.js:33-36 初始化挂钟（time + DOWNGRADE[1] + 1 = 20001）
    assert_eq!(t.downgrade_time, Some(20_001), "清空后同 tick 重挂钟");
    assert_eq!(t.reservation, None, "自己的预定随 claim 清除");
}

fn controller_intent(creep: ObjId, action: Action, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", creep, action, IntentData::target(target));
    b
}

#[test]
fn claim_controller_rejections() {
    // claimController.js 逐分支：level>0（:25-27）、无 CLAIM 部件（:28-30）、
    // 他人预定（:31-33）、非邻接（:19-21）、目标非 controller（:16-18）
    let mut f = Fixture::new(1, terrain_with(&[]));
    // level > 0 拒
    let ctl = f.controller(1, Some("p2"));
    let c1 = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c1, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner.as_deref(), Some("p2"), "已占房不可夺");
    // 无 CLAIM 部件拒（无主房）
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c2 = f.creep("w2", "p1", 5, 6, &[BodyKind::Move], 0);
    f.settle(controller_intent(c2, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "无 CLAIM 部件拒");
    // 他人预定拒
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 999,
        });
    });
    let c3 = f.creep("w3", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c3, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "他人预定拒");
    // 非邻接（Chebyshev 2）拒
    f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c4 = f.creep("w4", "p1", 5, 8, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c4, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "距离 2 拒");
    // 目标非 controller（source）拒
    let src = f.source(10, 10, 100);
    let c5 = f.creep("w5", "p1", 10, 11, &[BodyKind::Claim], 0);
    assert!(
        f.settle(controller_intent(c5, Action::ClaimController, src))
            .is_empty()
    );
}

#[test]
fn claim_controller_gcl_gate() {
    // claimController.js:34-43：claimedRooms+1 级 GCL 不足拒。
    // calcNeededGcl(2) = GCL_MULTIPLY × 1^GCL_POW = 1e6（utils.js:661-663）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into()); // 已有一房
    p.gcl = 999_999;
    f.players.insert("p1".into(), p);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner, None, "gcl 999999 < 1e6 拒");
    // 门槛值放行（官方 < 严格比较：恰好 1e6 不拒）
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into());
    p.gcl = 1_000_000;
    f.players.insert("p1".into(), p);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    assert_eq!(ctl_of(&f).owner.as_deref(), Some("p1"), "gcl == 1e6 放行");
}

#[test]
fn claim_controller_own_room_rejected() {
    // claimController.js:26-28：level > 0 拒——自己已拥有的房再 claim 同路
    //（分支评审 3 补测：既有测试只锁他人房形态）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::ClaimController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.owner.as_deref(), Some("p1"), "owner 不变");
    assert_eq!(t.level, 1, "level>0 拒对自家房同效（不重置）");
}

#[test]
fn claim_controller_gcl_reads_last_tick_set() {
    // 官方 quirk：GCL 门读上一 tick 定集（bulkUsers.addToSet 不改内存对象，
    // bulk.js:93-101 亲验）——同 tick 双 claim 都按旧 rooms 判定（rooms=
    // {W2N2}、gcl 恰好 1e6 时两条都放行）。锁定"settle 期不更新
    // PlayerState.rooms"（重建在 Engine tick 末；若被挪进 settle_room，
    // 第二条 claim 会读到增长的 rooms 而拒，此测试转红）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl1 = f.controller(0, None);
    let ctl2 = f.controller(0, None);
    let c1 = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let c2 = f.creep("w2", "p1", 5, 4, &[BodyKind::Claim], 0);
    let mut p = PlayerState::default();
    p.rooms.insert("W2N2".into());
    p.gcl = 1_000_000;
    f.players.insert("p1".into(), p);
    let mut batch = controller_intent(c1, Action::ClaimController, ctl1);
    batch.add_object_intent("p1", c2, Action::ClaimController, IntentData::target(ctl2));
    f.settle(batch);
    let t1 = ctl_of(&f);
    assert_eq!(t1.owner.as_deref(), Some("p1"), "第一条过门");
    // 第二条也过门：GCL 读数是 settle 前的定集（官方同 tick 不涨）
    let t2 = f
        .world
        .room("W1N1")
        .unwrap()
        .objects
        .get(&ctl2)
        .and_then(|o| match o {
            kk_schema::room::RoomObject::Controller(c) => Some(c.clone()),
            _ => None,
        })
        .expect("第二个 controller 存在");
    assert_eq!(
        t2.owner.as_deref(),
        Some("p1"),
        "同 tick 双 claim 都按上一 tick 定集放行"
    );
}

#[test]
fn attack_controller_zero_claim_rejected() {
    // attackController.js:34/:42：effect 为 0（无存活 CLAIM）整票拒——
    // 无削减、无封锁、无事件（评审 3 补测：若删掉 effect==0 守卫，
    // upgrade_blocked 断言抓回）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = Some(20_000);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    let ev = f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).downgrade_time, Some(20_000), "零 CLAIM 不削减");
    assert_eq!(ctl_of(&f).upgrade_blocked, None, "零 CLAIM 不挂封锁");
    assert!(ev.is_empty());
}

#[test]
fn attack_controller_reservation_drain_below_zero_cleared_same_tick() {
    // 预约削减穿透 0：官方 endTime − effect 得负数入内存、同 tick 被
    // controllers/tick 预定清除分支消解（清除条件在 0/负数下同真）——
    // 我方 u64 饱和 0，终态等价（评审 3 补测：锁"削穿即清除"官方可观察值）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 1,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(
        ctl_of(&f).reservation,
        None,
        "1 − 2 削穿 → 同 tick 清除（官方负数同终态）"
    );
}

#[test]
fn reserve_controller_success_and_cap() {
    // reserveController.js:35-52：新预定 endTime = gameTime+1 再 += effect；
    // 事件 amount=effect；上限 gameTime+CONTROLLER_RESERVE_MAX(5000)（:43-45）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Move], 0);
    let events = f.settle(controller_intent(c, Action::ReserveController, ctl));
    let t = ctl_of(&f);
    // time=0：base=1，effect=1×CONTROLLER_RESERVE(1) → 2
    assert_eq!(
        t.reservation,
        Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 2,
        })
    );
    assert_eq!(
        events,
        vec![serde_json::json!({
            "event": kk_core::constants::EVENT_RESERVE_CONTROLLER,
            "objectId": c.as_str(),
            "data": { "amount": 1 },
        })],
        "EVENT_RESERVE_CONTROLLER(8) amount=CLAIM×1"
    );
    // actionLog.reserveController 位（官方独立键，位图 bit8）
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 1 << 8),
        _ => panic!(),
    }
    // 续订：既有预约 end=100，5 部件 → 105（:49 累加）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 100,
        });
    });
    let c2 = f2.creep("w2", "p1", 5, 6, [BodyKind::Claim; 5].as_slice(), 0);
    f2.settle(controller_intent(c2, Action::ReserveController, ctl2));
    assert_eq!(
        ctl_of(&f2).reservation.as_ref().map(|r| r.end_time),
        Some(105),
        "续订 += 5"
    );
    // 上限窗：end=4998 + effect2 = 5000 恰好放行；end=4999 + effect2 拒
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 4998,
        });
    });
    let c3 = f3.creep("w3", "p1", 5, 6, [BodyKind::Claim; 2].as_slice(), 0);
    f3.settle(controller_intent(c3, Action::ReserveController, ctl3));
    assert_eq!(
        ctl_of(&f3).reservation.as_ref().map(|r| r.end_time),
        Some(5000),
        "恰好压线放行"
    );
    let mut f4 = Fixture::new(1, terrain_with(&[]));
    let ctl4 = f4.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 4999,
        });
    });
    let c4 = f4.creep("w4", "p1", 5, 6, [BodyKind::Claim; 2].as_slice(), 0);
    let ev = f4.settle(controller_intent(c4, Action::ReserveController, ctl4));
    assert_eq!(
        ctl_of(&f4).reservation.as_ref().map(|r| r.end_time),
        Some(4999),
        "超窗拒：预约不动"
    );
    assert!(ev.is_empty(), "拒分支无事件");
}

#[test]
fn reserve_controller_rejections() {
    // reserveController.js:26-33：已占房拒（房主也不行）、他人预定拒、
    // 无 CLAIM 部件拒（:31-33 无事件无 actionLog）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(1, Some("p1"));
    let mine = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(mine, Action::ReserveController, ctl));
    assert_eq!(ctl_of(&f).reservation, None, "已占房不可预定");
    // 他人预定拒
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev = f2.settle(controller_intent(c2, Action::ReserveController, ctl2));
    assert_eq!(
        ctl_of(&f2).reservation.as_ref().map(|r| r.user.as_str()),
        Some("p2"),
        "他人预定不可覆盖"
    );
    assert!(ev.is_empty());
    // 无 CLAIM 部件：effect=0 拒，actionLog 不置位
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller(0, None);
    let c3 = f3.creep("w3", "p1", 5, 6, &[BodyKind::Move], 0);
    f3.settle(controller_intent(c3, Action::ReserveController, ctl3));
    assert_eq!(ctl_of(&f3).reservation, None);
    match f3.obj(c3) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 0, "零功率无 actionLog"),
        _ => panic!(),
    }
}

#[test]
fn reservation_expires_and_owned_room_clears_it() {
    // controllers/tick.js:10-12：gameTime >= endTime-1 到期清除（位于无主
    // 早退之前）；房被占（claim）同样清除
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 1, // time=0：0 >= 1-1=0 → 到期
        });
    });
    f.settle(IntentBatch::default());
    assert_eq!(ctl_of(&f).reservation, None, "到期清除");
    // 未到期保留：end=100 > time+1
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 100,
        });
    });
    f2.settle(IntentBatch::default());
    assert!(ctl_of(&f2).reservation.is_some(), "未到期保留");
}

#[test]
fn attack_controller_owned_downgrade_and_block() {
    // attackController.js:41-49：downgradeTime -= CLAIM×CONTROLLER_CLAIM_DOWNGRADE
    //(300)；_upgradeBlocked = gameTime+1000（tick 持久化）；:50 actionLog.attack；
    // :52 EVENT_ATTACK_CONTROLLER
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = Some(20_000);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    let events = f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.downgrade_time, Some(20_000 - 2 * 300), "2 CLAIM × 300");
    assert_eq!(t.upgrade_blocked, Some(1000), "time=0 + 1000");
    assert!(
        events.iter().any(
            |e| e["event"] == kk_core::constants::EVENT_ATTACK_CONTROLLER
                && e["objectId"] == c.as_str()
        ),
        "EVENT_ATTACK_CONTROLLER(3)（官方无 data 键——我方封套空对象）"
    );
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 1 << 3, "attack 位"),
        _ => panic!(),
    }
    // 升级封锁双门（upgradeController.js:25-27 + activateSafeMode.js:17）：
    // 下一 tick 升级被拒（progress 不动）
    let worker = f.creep("up", "p2", 5, 6, &[BodyKind::Work, BodyKind::Carry], 50);
    let mut b = IntentBatch::default();
    b.add_object_intent(
        "p2",
        worker,
        Action::UpgradeController,
        IntentData::target(ctl),
    );
    f.settle(b);
    assert_eq!(ctl_of(&f).progress, 0, "封锁期升级拒");
}

#[test]
fn attack_controller_reservation_drain() {
    // attackController.js:33-40：预约削减 endTime -= CLAIM×CONTROLLER_RESERVE；
    // 无主房不设 upgradeBlocked
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(
        t.reservation,
        Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 498,
        }),
        "500 − 2×1，user 保留（官方 merge 语义）"
    );
    assert_eq!(t.upgrade_blocked, None, "无主分支不设封锁");
}

#[test]
fn attack_controller_rejections() {
    // attackController.js:22-28：原始房拒；他人 safeMode 拒；
    // upgradeBlocked > gameTime 拒（对房主同样生效）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).downgrade_time, None, "无主无预定不可攻");
    // 他人 safeMode（:25-26）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 1000;
        t.downgrade_time = Some(20_000);
    });
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev = f2.settle(controller_intent(c2, Action::AttackController, ctl2));
    assert_eq!(ctl_of(&f2).downgrade_time, Some(20_000), "safeMode 拒");
    assert!(ev.is_empty());
    // 房主自己也被 upgradeBlocked 拒（:26-27）
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = f3.controller_full(1, Some("p1"), |t| {
        t.downgrade_time = Some(20_000);
        t.upgrade_blocked = Some(500);
    });
    let c3 = f3.creep("w3", "p1", 5, 6, &[BodyKind::Claim], 0);
    let ev3 = f3.settle(controller_intent(c3, Action::AttackController, ctl3));
    assert_eq!(ctl_of(&f3).downgrade_time, Some(20_000), "封锁期拒");
    assert!(ev3.is_empty());
}

#[test]
fn attack_controller_null_downgrade_time_loses_room_next_tick() {
    // 官方 quirk：downgradeTime 未挂钟（新 claim 房的当 tick）时 attack 的
    // `null − effect` 得负数 → 降级链即刻触发（attackController.js:46 +
    // controllers/tick.js:49-63）。u64 饱和到 0——触发条件同真
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = None;
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(c, Action::AttackController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.level, 0, "当 tick 即走降级链归零");
    assert_eq!(t.owner, None, "主权丢失");
    assert!(
        t.owner_cleared,
        "降级归零置 owner 哨兵（tick.js:56 user=null）"
    );
    assert_eq!(
        t.safe_mode_cooldown,
        Some(kk_core::constants::SAFE_MODE_COOLDOWN as u64),
        "降级到 0 挂冷却（tick.js:61）"
    );
}

#[test]
fn downgrade_then_claim_resets_owner_sentinel() {
    // owner 三态往返锁：降级归零（cleared=true）→ 重新 claim（cleared=false）。
    // 防未来 claim 路径漏复位导致落库态长期不一致（投影 Some 分支优先，
    // 漏复位属潜伏态——owner_cleared 断言是唯一可见面）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.downgrade_time = None; // 当 tick即归零（同上 quirk）
    });
    let attacker = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(attacker, Action::AttackController, ctl));
    assert_eq!(ctl_of(&f).owner, None);
    assert!(ctl_of(&f).owner_cleared, "归零后哨兵置位");
    // p1 重新 claim（claimController.js:45-53 只写 user——哨兵同步复位）
    let claimer = f.creep("w2", "p1", 5, 6, &[BodyKind::Claim], 0);
    f.settle(controller_intent(claimer, Action::ClaimController, ctl));
    let t = ctl_of(&f);
    assert_eq!(t.owner, Some("p1".into()));
    assert!(!t.owner_cleared, "claim 复位哨兵");
    assert_eq!(t.level, 1);
}

#[test]
fn attack_controller_suppressed_by_heal() {
    // intents.js:5 压制图：attackController 被 heal 压制——同 tick heal 存在
    // 则 attackController 不执行（玩家 API 早已返回成功）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 500,
        });
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim, BodyKind::Heal], 0);
    let ally = f.creep("ally", "p1", 5, 5, &[BodyKind::Tough], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::AttackController, IntentData::target(ctl));
    b.add_object_intent("p1", c, Action::Heal, IntentData::target(ally));
    let events = f.settle(b);
    assert_eq!(
        ctl_of(&f).reservation.as_ref().map(|r| r.end_time),
        Some(500),
        "attackController 被 heal 压制：预约不动"
    );
    assert!(
        events
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_HEAL),
        "heal 照常执行"
    );
    assert!(
        !events
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_ATTACK_CONTROLLER),
        "无 attackController 事件"
    );
}

#[test]
fn sign_controller_sets_and_clears() {
    // signController.js:23-28：{user, text, time: gameTime}；空/缺省 sign 落
    // null（官方 `intent.sign ?` 判空 + userString 的 ||"" 折叠）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(2, Some("p2"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::target(ctl);
    d.sign = Some("reserved sector".into());
    b.add_object_intent("p1", c, Action::SignController, d);
    f.settle(b);
    // datetime 是结算时墙钟（不可锁值）——比较业务三元组
    assert_eq!(
        ctl_of(&f)
            .sign
            .as_ref()
            .map(|s| (s.user.as_str(), s.text.as_str(), s.time)),
        Some(("p1", "reserved sector", 0)),
    );
    assert!(
        ctl_of(&f).sign.as_ref().unwrap().datetime.is_some(),
        "签名结算落 datetime"
    );
    // 清除：sign 缺省
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::SignController, IntentData::target(ctl));
    f.settle(b);
    assert_eq!(ctl_of(&f).sign, None, "缺省 sign = 清除");
    // 再签 + 空串清除
    let mut b2 = IntentBatch::default();
    let mut d2 = IntentData::target(ctl);
    d2.sign = Some("again".into());
    b2.add_object_intent("p1", c, Action::SignController, d2);
    f.settle(b2);
    assert!(ctl_of(&f).sign.is_some());
    let mut b3 = IntentBatch::default();
    let mut d3 = IntentData::target(ctl);
    d3.sign = Some(String::new());
    b3.add_object_intent("p1", c, Action::SignController, d3);
    f.settle(b3);
    assert_eq!(ctl_of(&f).sign, None, "空串 = 清除（官方 falsy）");
    // 距离 2 拒（signController.js:18-20）
    let far = f.creep("far", "p1", 5, 8, &[BodyKind::Move], 0);
    let mut b4 = IntentBatch::default();
    let mut d4 = IntentData::target(ctl);
    d4.sign = Some("x".into());
    b4.add_object_intent("p1", far, Action::SignController, d4);
    f.settle(b4);
    assert_eq!(ctl_of(&f).sign, None, "非邻接拒");
}

#[test]
fn sign_text_truncated_to_100_chars() {
    // system.js:74 `sign: 'userString'` → 截 100（UTF-16 码元 vs Unicode
    // chars 的差异沿用既有灰区登记——BMP 文本两者等值）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    // 截断发生在清洗层（system.js:74 `sign: 'userString'`）——经生产清洗入口
    // from_json 构造（引擎层不截、官方同形）
    let raw = serde_json::json!({
        c.as_str(): { "signController": { "id": ctl.as_str(), "sign": "x".repeat(150) } }
    });
    let b = IntentBatch::from_json("p1", &raw);
    f.settle(b);
    assert_eq!(
        ctl_of(&f).sign.as_ref().map(|s| s.text.chars().count()),
        Some(100),
        "签名文本截 100"
    );
}

#[test]
fn generate_safe_mode_consumes_ghodium() {
    // generateSafeMode.js:19-24：store["G"] >= SAFE_MODE_COST(1000) →
    // safeModeAvailable += 1、扣 1000 ghodium；不足拒（无部分消耗）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(3, Some("p1"));
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f.world.get_mut(c).unwrap() {
        cr.store.ghodium = 1500;
    }
    f.settle(controller_intent(c, Action::GenerateSafeMode, ctl));
    assert_eq!(ctl_of(&f).safe_mode_available, 1, "available 0→1");
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.store.ghodium, 500, "1500 − 1000"),
        _ => panic!(),
    }
    // 不足拒
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller(3, Some("p1"));
    let c2 = f2.creep("w2", "p1", 5, 6, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f2.world.get_mut(c2).unwrap() {
        cr.store.ghodium = 999;
    }
    f2.settle(controller_intent(c2, Action::GenerateSafeMode, ctl2));
    assert_eq!(ctl_of(&f2).safe_mode_available, 0, "999 < 1000 拒");
    // 非邻接拒
    let c3 = f2.creep("w3", "p1", 5, 8, &[BodyKind::Move], 0);
    if let RoomObject::Creep(cr) = f2.world.get_mut(c3).unwrap() {
        cr.store.ghodium = 5000;
    }
    f2.settle(controller_intent(c3, Action::GenerateSafeMode, ctl2));
    assert_eq!(ctl_of(&f2).safe_mode_available, 0, "距离 2 拒");
}

#[test]
fn activate_safe_mode_full_chain() {
    // activateSafeMode.js:6-24（意图半步）+ controllers/tick.js:25-31（消费
    // 半步）：available −1、safeMode = time+SAFE_MODE_DURATION(20000)、
    // safeModeCooldown = time+SAFE_MODE_COOLDOWN(50000)。
    // time 取 10：gameTime 0 时未挂冷却的 null 在官方 `cooldown >= gameTime`
    // 数值比较中是 0 >= 0 → 拒（JS null 强制转换角落，行为已由
    // activate_safe_mode_rejections 的 cooldown 用例锁住）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001); // 健康窗：>= time + 20000/2 − 5000
    });
    let events = f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert!(events.is_empty(), "激活链无事件（官方两半步都不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.safe_mode_available, 0, "消耗一枚");
    assert_eq!(t.safe_mode, 10 + 20_000, "time + SAFE_MODE_DURATION");
    assert_eq!(
        t.safe_mode_cooldown,
        Some(10 + 50_000),
        "time + SAFE_MODE_COOLDOWN"
    );
}

#[test]
fn activate_safe_mode_rejections() {
    // activateSafeMode.js 逐门：无 available（:11）、冷却中（:14）、
    // upgradeBlocked（:17）、降级危险窗（:20）、无主/0 级（:8）
    fn base(f: &mut Fixture, tweak: &dyn Fn(&mut kk_schema::room::ControllerObj)) -> ObjId {
        f.world.time = 10;
        f.controller_full(1, Some("p1"), |t| {
            t.safe_mode_available = 1;
            t.downgrade_time = Some(10 + 20_001);
            tweak(t);
        })
    }
    // 无 available
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = base(&mut f, &|t| t.safe_mode_available = 0);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode, 0, "无 available 拒");
    // 冷却中（cooldown >= gameTime）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = base(&mut f2, &|t| t.safe_mode_cooldown = Some(100)); // >= 10
    f2.settle(controller_action(
        "p1",
        ctl2,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f2).safe_mode, 0, "冷却中拒");
    // upgradeBlocked
    let mut f3 = Fixture::new(1, terrain_with(&[]));
    let ctl3 = base(&mut f3, &|t| t.upgrade_blocked = Some(500));
    f3.settle(controller_action(
        "p1",
        ctl3,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f3).safe_mode, 0, "封锁期拒");
    // 降级危险窗：level1 阈值 time+5000；downgradeTime=time+4999 拒 / +5000 放行
    let mut f4 = Fixture::new(1, terrain_with(&[]));
    let ctl4 = base(&mut f4, &|t| t.downgrade_time = Some(10 + 4_999));
    f4.settle(controller_action(
        "p1",
        ctl4,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f4).safe_mode, 0, "危险窗拒（5009 < 5010）");
    let mut f5 = Fixture::new(1, terrain_with(&[]));
    let ctl5 = base(&mut f5, &|t| t.downgrade_time = Some(10 + 5_000));
    f5.settle(controller_action(
        "p1",
        ctl5,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(
        ctl_of(&f5).safe_mode,
        10 + 20_000,
        "恰好压线放行（< 严格比较）"
    );
    // 无主 / 0 级
    let mut f6 = Fixture::new(1, terrain_with(&[]));
    let ctl6 = f6.controller_full(0, None, |t| t.safe_mode_available = 1);
    f6.settle(controller_action(
        "p1",
        ctl6,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f6).safe_mode, 0, "无主 0 级拒");
    // downgradeTime 未挂钟（null → 0 < time+5000）拒——新 claim 房当 tick不可激活
    let mut f7 = Fixture::new(1, terrain_with(&[]));
    let ctl7 = base(&mut f7, &|t| t.downgrade_time = None);
    f7.settle(controller_action(
        "p1",
        ctl7,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f7).safe_mode, 0, "未挂钟拒（官方 null 数值比较）");
}

#[test]
fn same_tick_attack_cancels_safe_mode_activation() {
    // 官方同序语义：attackController 的 _upgradeBlocked 被 tick.js:20-23 先
    // 持久化、:25 激活门读到它 → 激活取消（available 不减、safeMode 不挂）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001);
    });
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut b = controller_intent(c, Action::AttackController, ctl);
    b.add_object_intent("p1", ctl, Action::ActivateSafeMode, IntentData::bare());
    f.settle(b);
    let t = ctl_of(&f);
    assert_eq!(t.upgrade_blocked, Some(10 + 1000), "攻击封锁已持久化");
    assert_eq!(t.safe_mode_available, 1, "激活被同 tick 攻击取消：不消耗");
    assert_eq!(t.safe_mode, 0, "safeMode 未挂");
}

#[test]
fn enemy_stomp_blocked_under_safe_mode() {
    // movement.js:224：`!roomController || controller.user === mover ||
    // !(safeMode > gameTime)` 才允许踩敌方工地——B2 补齐的门（灰区清偿项）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 1000;
    });
    let _ = ctl;
    let site = f.site(10, 11, "road", 10, 300, "p2");
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(c, 5));
    assert_eq!(f.creep_state(c).1, 11, "进格本身不受门影响");
    assert!(f.world.get(site).is_some(), "敌方 safeMode 下工地受保护");
    // 门前对照：safeMode 过期 → 工地被踩
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let ctl2 = f2.controller_full(1, Some("p2"), |t| {
        t.safe_mode = 0;
    });
    let _ = ctl2;
    let site2 = f2.site(10, 11, "road", 10, 300, "p2");
    let c2 = f2.creep("w2", "p1", 10, 10, &[BodyKind::Move], 0);
    f2.settle(move_intent(c2, 5));
    assert!(f2.world.get(site2).is_none(), "无 safeMode 照常踩");
}

#[test]
fn source_capacity_3000_under_reservation() {
    // sources/tick.js:46-49：`user || reservation` → SOURCE_ENERGY_CAPACITY
    //(3000)；无主无预定 → 1500（B2 清偿项）。预约未过期 → 两种 tick 次序下
    // 都存活，断言与对象 id 序无关
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 5000,
        });
    });
    let src = f.source(10, 10, 1200);
    // fixture 的 source 挂 3000 容量——先压到 1500 模拟无预定期的归一
    if let RoomObject::Source(s) = f.world.get_mut(src).unwrap() {
        s.energy_capacity = 1500;
    }
    f.settle(IntentBatch::default());
    match f.obj(src) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy_capacity, 3000, "有效预定 → 3000");
            assert_eq!(s.energy, 1200, "抬升分支不钳 energy");
        }
        _ => panic!(),
    }
    // 无主无预定 → 1500 + 钳 energy（:41-45）
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    f2.controller(0, None);
    let src2 = f2.source(10, 10, 2000);
    if let RoomObject::Source(s) = f2.world.get_mut(src2).unwrap() {
        s.energy_capacity = 3000;
    }
    f2.settle(IntentBatch::default());
    match f2.obj(src2) {
        RoomObject::Source(s) => {
            assert_eq!(s.energy_capacity, 1500, "无主无预定 → 1500");
            assert_eq!(s.energy, 1500, "降容量钳 energy");
        }
        _ => panic!(),
    }
}

#[test]
fn harvest_rejected_under_foreign_reservation() {
    // harvest.js:31-34：controller 被他人预定 → harvest 拒（B2 补的分支）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller_full(0, None, |t| {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 5000,
        });
    });
    let src = f.source(10, 10, 3000);
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Carry], 0);
    let events = f.settle(controller_intent(c, Action::Harvest, src));
    assert!(events.is_empty(), "他人预定房 harvest 拒");
    match f.obj(src) {
        RoomObject::Source(s) => assert_eq!(s.energy, 3000, "能量未被动"),
        _ => panic!(),
    }
    // 自己的预定放行
    if let Some(t) = f
        .world
        .room_mut("W1N1")
        .unwrap()
        .objects
        .values_mut()
        .find_map(|o| match o {
            RoomObject::Controller(t) => Some(t),
            _ => None,
        })
    {
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p1".into(),
            end_time: 5000,
        });
    }
    let events2 = f.settle(controller_intent(c, Action::Harvest, src));
    assert!(
        events2
            .iter()
            .any(|e| e["event"] == kk_core::constants::EVENT_HARVEST),
        "自己的预定放行"
    );
}

#[test]
fn downgrade_sets_safe_mode_cooldown() {
    // controllers/tick.js:64-69：降一级也清 available 并挂冷却（M1 只做了
    // available 清零，B2 补冷却）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(2, Some("p1"), |t| {
        t.safe_mode_available = 2;
        t.downgrade_time = Some(0); // time=0：0 >= 0-1 触发降级
    });
    let _ = ctl;
    f.settle(IntentBatch::default());
    let t = ctl_of(&f);
    assert_eq!(t.level, 1, "2 → 1");
    assert_eq!(t.safe_mode_available, 0);
    assert_eq!(
        t.safe_mode_cooldown,
        Some(kk_core::constants::SAFE_MODE_COOLDOWN as u64),
        "降级挂冷却"
    );
    // 进度回退：progress += round(CONTROLLER_LEVELS[1] × 0.9) = 180
    assert_eq!(t.progress, 180, "LEVELS[1]=200 × 0.9 → 180");
    // 新挂钟 = 0 + DOWNGRADE[1]/2 + 1 = 10001
    assert_eq!(t.downgrade_time, Some(10_001));
}

#[test]
fn engine_recomputes_player_rooms_after_tick() {
    // user.rooms 不动点（claimController.js:34-35 的 GCL 读数 +
    // driver addRoomToUser/removeRoomFromUser 的等价承载）：Engine 每 tick 后
    // 按世界真值重建——同 tick 内读数保持上一 tick 定集（官方 bulk addToSet
    // 不改内存对象，bulk.js:101-107）
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone());
    world.register_room("W2N2", t);
    for name in ["W1N1", "W2N2"] {
        world
            .insert(
                name,
                RoomObject::Controller(kk_schema::room::ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level: if name == "W1N1" { 1 } else { 0 },
                    safe_mode: 0,
                    safe_mode_available: 0,
                    progress: 0,
                    downgrade_time: None,
                    owner_cleared: false,
                    owner: if name == "W1N1" {
                        Some("p1".into())
                    } else {
                        None
                    },
                    is_power_enabled: false,
                    reservation: None,
                    sign: None,
                    upgrade_blocked: None,
                    safe_mode_cooldown: None,
                }),
            )
            .unwrap();
    }
    let mut engine = Engine::new(world);
    engine.players.entry("p1".to_string()).or_default().gcl = 1_000_000;
    // 空 tick 后 rooms 重建 = {W1N1}
    engine.tick().unwrap();
    assert_eq!(
        engine.players["p1"].rooms,
        BTreeSet::from(["W1N1".to_string()])
    );
    // W2N2 claim：rooms=1 → 需要 GCL 1e6（恰好放行）
    let w2_ctl = *engine
        .world
        .room("W2N2")
        .unwrap()
        .objects
        .keys()
        .next()
        .unwrap();
    let claimer = engine
        .world
        .insert(
            "W2N2",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 5,
                y: 6,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Claim]),
                store: Default::default(),
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent(
        "p1",
        claimer,
        Action::ClaimController,
        IntentData::target(w2_ctl),
    );
    engine.tick_with_intents(b).unwrap();
    assert_eq!(
        engine.players["p1"].rooms,
        BTreeSet::from(["W1N1".to_string(), "W2N2".to_string()]),
        "claim 后纳入 rooms"
    );
    match engine
        .world
        .room("W2N2")
        .unwrap()
        .objects
        .values()
        .find(|o| matches!(o, RoomObject::Controller(_)))
    {
        Some(RoomObject::Controller(c)) => {
            assert_eq!(c.level, 1);
            assert_eq!(c.owner.as_deref(), Some("p1"));
        }
        _ => panic!(),
    }
}

#[test]
fn activate_safe_mode_owner_gate_at_dispatch() {
    // processor.js:248：`object.user && object.user != userId` → 整对象 intent
    // 跳过——敌人对他人 controller 发 activateSafeMode 在分派层丢弃
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(1, Some("p2"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(10 + 20_001);
    });
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert_eq!(t.safe_mode, 0, "敌方 intent 分派层丢弃");
    assert_eq!(t.safe_mode_available, 1, "available 未被动");
}

#[test]
fn activate_safe_mode_null_cooldown_at_time_zero_rejects() {
    // activateSafeMode.js:14 的 JS null 数值比较角落：未挂冷却（null → 0）
    // 时 `0 >= gameTime` 在 gameTime 0 为真——拒绝。我方 None 统一按 null
    // 语义建模（undefined 初态的分歧仅放置态可达，见 controller.rs 注释）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller_full(1, Some("p1"), |t| {
        t.safe_mode_available = 1;
        t.downgrade_time = Some(20_001);
    });
    assert_eq!(f.world.time, 0);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::ActivateSafeMode,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode, 0, "time=0 + 冷却 None（0>=0）拒");
}

// ---- A4-iii：unclaim（官方 processor/intents/controllers/unclaim.js）----

#[test]
fn unclaim_clears_ownership_and_sets_cooldown() {
    // unclaim.js:15-25：user=null（→ owner_cleared 哨兵）、level=0、progress=0、
    // downgradeTime=null、safeMode=null、safeModeAvailable=0、冷却
    // gameTime+SAFE_MODE_COOLDOWN（roomInfo.novice 免冷却路径——我方无新手区
    // 恒挂钟）、isPowerEnabled=false。**不清 reservation/sign/upgradeBlocked**
    // （官方 bulk update 键表无此三项——与降级归零路径 tick.js:52-63 的刻意
    // 差异：sign 跨 unclaim 存续是玩家可观察语义；reservation 由 controllers/
    // tick 的到期/已占条件另行清除，unclaim 后无主不满足）。removeRoomFromUser
    // （:15）由 Engine rooms 重建承载（engine_recomputes_player_rooms_after_tick）
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 100;
    let ctl = f.controller_full(4, Some("p1"), |t| {
        t.progress = 12_345;
        t.safe_mode = 500;
        t.safe_mode_available = 3;
        t.downgrade_time = Some(9_000);
        t.is_power_enabled = true;
        t.reservation = Some(kk_schema::room::Reservation {
            user: "p2".into(),
            end_time: 10_000,
        });
        t.sign = Some(kk_schema::room::Sign {
            user: "p1".into(),
            text: "hi".into(),
            time: 50,
            datetime: None,
        });
        t.upgrade_blocked = Some(700);
    });
    // 同 tick 追加 activateSafeMode：processor.js:307-309 unclaim 先序——归零
    // 后 activate 的 `!object.user || !object.level` 再拒（官方同序）
    let mut b = controller_action("p1", ctl, Action::Unclaim, IntentData::bare());
    b.add_object_intent("p1", ctl, Action::ActivateSafeMode, IntentData::bare());
    let events = f.settle(b);
    assert!(events.is_empty(), "unclaim 无事件（官方处理器不 push）");
    let t = ctl_of(&f);
    assert_eq!(t.owner, None);
    assert!(
        t.owner_cleared,
        "官方 user=null 落库哨兵 → 玩家侧 my === false"
    );
    assert_eq!(t.level, 0);
    assert_eq!(t.progress, 0);
    assert_eq!(
        t.safe_mode, 0,
        "同 tick activateSafeMode 被 unclaim 先序压掉"
    );
    assert_eq!(t.safe_mode_available, 0);
    assert_eq!(t.downgrade_time, None);
    assert_eq!(
        t.safe_mode_cooldown,
        Some(100 + 50_000),
        "冷却 = time + SAFE_MODE_COOLDOWN（无新手区恒挂钟）"
    );
    assert!(!t.is_power_enabled);
    assert_eq!(
        t.sign.as_ref().map(|s| s.text.as_str()),
        Some("hi"),
        "sign 存续"
    );
    assert_eq!(
        t.upgrade_blocked,
        Some(700),
        "upgradeBlocked 不清（官方键表无）"
    );
    assert_eq!(
        t.reservation.as_ref().map(|r| r.end_time),
        Some(10_000),
        "reservation 存续（未到期且 unclaim 后无主——tick 清除条件不满足）"
    );
}

#[test]
fn unclaim_rejections() {
    // unclaim.js:9-12 `!object.user || !object.level` 早退（不挂冷却、不置
    // 哨兵）；分派层（processor.js:248 `object.user && object.user != userId`）
    // 他人 controller 的 intent 整对象跳过
    // ① 无主 + 0 级拒
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller(0, None);
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert!(!t.owner_cleared, "无主：哨兵不置（从未拥有）");
    assert_eq!(t.safe_mode_cooldown, None, "无主：不挂冷却");
    // ② 有主但 0 级拒
    f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(0, Some("p1"), |_| {});
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    assert_eq!(ctl_of(&f).safe_mode_cooldown, None, "0 级：不挂冷却");
    // ③ 他人 controller：分派层丢弃
    f = Fixture::new(1, terrain_with(&[]));
    f.world.time = 10;
    let ctl = f.controller_full(3, Some("p2"), |t| {
        t.safe_mode_available = 1;
    });
    f.settle(controller_action(
        "p1",
        ctl,
        Action::Unclaim,
        IntentData::bare(),
    ));
    let t = ctl_of(&f);
    assert_eq!(t.owner.as_deref(), Some("p2"), "敌方 intent 分派层丢弃");
    assert_eq!(t.level, 3);
    assert_eq!(t.safe_mode_available, 1);
}

#[test]
fn engine_safemode_gate4_backstop_drops_cross_room_activation() {
    // 门④可信侧兜底（A4-iii 评审 2 必修锁值）：官方 game 层在 isolated-vm
    // 可信 realm，门④（任一自家 controller safeMode 生效 → ERR_BUSY，
    // structures.js:223-225）处理器不复核；我方 prelude 与玩家共享 realm，
    // 迭代器污染可令门④扫描失明——Engine 层按世界真值兜底：玩家已有生效
    // 中的 safeMode → 其 ActivateSafeMode intent 全部丢弃（双房 safeMode
    // 并存是官方不可能态）
    use kk_engine::Engine;
    let mut world = World::new(3);
    let mut t = kk_core::Terrain::plains();
    t.normalize();
    world.register_room("W1N1", t.clone());
    world.register_room("W2N2", t);
    let mk_ctl = |safe_mode: u64, level: u8| {
        RoomObject::Controller(kk_schema::room::ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level,
            safe_mode,
            safe_mode_available: 1,
            progress: 0,
            downgrade_time: Some(20_000),
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        })
    };
    // W1N1：safeMode 已生效（time=3 起算挂 1000）；W2N2：五门真值可通过
    let a = world.insert("W1N1", mk_ctl(3 + 1000, 1)).unwrap();
    let b = world.insert("W2N2", mk_ctl(0, 1)).unwrap();
    let mut engine = Engine::new(world);
    // 恶意场景：绕过 prelude 门④直接构造 B 房的 ActivateSafeMode intent
    //（embed 侧收集器闭包私有本不可达——本测试模拟的就是门④失明的等价批）
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", b, Action::ActivateSafeMode, IntentData::bare());
    engine.tick_with_intents(batch).unwrap();
    let ctl = engine.world.room("W2N2").unwrap().objects.get(&b).unwrap();
    match ctl {
        RoomObject::Controller(c) => {
            assert_eq!(c.safe_mode, 0, "A 房生效中 → B 房激活被 Engine 兜底丢弃");
            assert_eq!(c.safe_mode_available, 1, "可用数不消耗");
        }
        _ => panic!("controller 不见了"),
    }
    // 对照：A 房自身的 safeMode 不受影响
    let ctl_a = engine.world.room("W1N1").unwrap().objects.get(&a).unwrap();
    match ctl_a {
        RoomObject::Controller(c) => assert_eq!(c.safe_mode, 3 + 1000),
        _ => panic!(),
    }
}
// ---- A4-iv：移动杂项族（say/suicide/pull/notifyWhenAttacked） ----

#[test]
fn say_intent_sets_action_log_bit() {
    // say.js:18-21：actionLog.say 落位（u16 位图 bit9）；spawning 拒；
    // message 缺省按官方 userString 折叠 "" 同样落位（isString 门恒过）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    let b1 = f.creep("b1", "p1", 11, 11, &[BodyKind::Move], 0);

    let mut batch = IntentBatch::default();
    let mut d = IntentData::bare();
    d.message = Some("hello".into());
    d.is_public = Some(true);
    batch.add_object_intent("p1", c, Action::Say, d);
    let d2 = IntentData::bare(); // 缺省 message（None ≙ 官方 ""）
    batch.add_object_intent("p1", b1, Action::Say, d2);
    assert!(f.world.get(c).is_some(), "settle 前对象在");
    assert!(f.world.get(b1).is_some(), "settle 前b1在");
    let events_dbg = f.settle(batch);
    println!("DBG events: {}", serde_json::Value::Array(events_dbg));
    assert!(f.world.get(c).is_some(), "settle 后 c 还在");

    let action_log = |fx: &Fixture, id: ObjId| match fx.obj(id) {
        RoomObject::Creep(cc) => cc.action_log,
        _ => panic!("非 creep"),
    };
    println!(
        "DBG after settle: c={} b1={}",
        f.world.get(c).is_some(),
        f.world.get(b1).is_some()
    );
    // 官方 say 键带 message(截 10)/isPublic——位图只落事件位（灰区登记）
    assert_eq!(action_log(&f, c), 1 << 9, "say 落 ACTION_SAY 位");
    assert_eq!(
        action_log(&f, b1),
        1 << 9,
        "缺省 message（None ≙ 官方 \"\"）同落位"
    );

    // spawning creep：处理器拒（无 actionLog）。A4-i 的孵化模型并入后，孤立
    // 的 spawning 雏形会在 tick 相被孤儿移除——本例给它配同格带匹配任务
    // （name "sp1"）的 spawn：intent 相 say 拒先于 tick 相的孵化处理，断言
    // 窗口内 s 恒在
    let mut f2 = Fixture::new(1, terrain_with(&[]));
    let spawn_id = f2
        .world
        .insert(
            "W1N1",
            RoomObject::Spawn(kk_schema::room::SpawnObj {
                id: 0,
                x: 10,
                y: 10,
                hits: 0,
                off: false,
                name: "S".into(),
                owner: "p1".into(),
                store: kk_schema::room::Store::default(),
                spawning: Some(kk_schema::room::SpawningJob {
                    name: "sp1".into(),
                    need_time: 3,
                    spawn_time: 5,
                    directions: vec![],
                }),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let s = f2
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: true,
                action_log: 0,
                name: "sp1".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Move]),
                store: kk_schema::room::Store::default(),
            }),
        )
        .unwrap();
    let _ = spawn_id;
    let mut batch2 = IntentBatch::default();
    batch2.add_object_intent(
        "p1",
        s,
        Action::Say,
        IntentData {
            message: Some("x".into()),
            ..IntentData::bare()
        },
    );
    f2.settle(batch2);
    assert_eq!(
        action_log(&f2, s),
        0,
        "spawning 中 say 不落位（say.js:11-13）"
    );
}

#[test]
fn suicide_intent_kills_creep_same_tick() {
    // suicide.js：intent 相即 _die——对象当 tick 消亡（B3：墓碑承接 + 死亡事件）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 40);
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", c, Action::Suicide, IntentData::bare());
    let events = f.settle(batch);
    assert!(f.world.get(c).is_none(), "suicide 当 tick 死亡");
    // 墓碑承接（B3）：dropRate 0.2、满寿（fixture age_time 极大 ⇒ lifeRate 巨大
    // ⇒ 每件钳 125）⇒ 40 携带 + 125×2 回收 = 290；2 部件 ⇒ 10 tick
    let tomb = f
        .world
        .rooms
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) => Some(t.clone()),
            _ => None,
        })
        .expect("suicide 产墓碑");
    assert_eq!(tomb.store.energy, 290, "携带 40 + 造价回收 250");
    assert!(
        !f.world
            .rooms
            .get("W1N1")
            .unwrap()
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Energy(_))),
        "不直接掉地（墓碑承接）"
    );
    // _die.js:97：EVENT_OBJECT_DESTROYED（B3 补齐——M1 灰区销账）
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]["event"],
        kk_core::constants::EVENT_OBJECT_DESTROYED
    );
    assert_eq!(events[0]["objectId"], c.as_str());
    assert_eq!(events[0]["data"]["type"], "creep");

    // 同 tick 后续 intents 不再作用（对象消亡）——**有意分歧**：官方执行序
    // creepActions 里 upgradeController 排 suicide 之后（intents.js:15-17），
    // 但官方处理器持死引用快照继续执行、对其他对象的效果落地（processor.js:241/
    // _die.js:12-13 不清引用、upgradeController.js 无存活复检——同 tick 官方
    // 升级照常生效）；我方对象消亡即跳过（A4-iv 评审 1/3 双确认，compatibility.md
    // 登记为有意分歧：处理器持 pre-death 快照侵入面大、真实 bot 罕见）——本断言
    // 锁的是我方登记后的分歧行为，非官方语义
    let mut f2 = Fixture::new(2, terrain_with(&[]));
    let ctl = f2.controller(1, Some("p1"));
    let c2 = f2.creep("w2", "p1", 6, 6, &[BodyKind::Move, BodyKind::Work], 100);
    let mut batch2 = IntentBatch::default();
    batch2.add_object_intent("p1", c2, Action::Suicide, IntentData::bare());
    batch2.add_object_intent("p1", c2, Action::UpgradeController, IntentData::target(ctl));
    f2.settle(batch2);
    assert!(f2.world.get(c2).is_none(), "对象消亡");
    match f2.obj(ctl) {
        RoomObject::Controller(t) => {
            assert_eq!(t.progress, 0, "死后 upgradeController 静默跳过")
        }
        _ => panic!(),
    }
}

#[test]
fn pull_pair_moves_fatigue_free_creep() {
    // 拉拽官方语义（pull.js + movement.js）：拉者 A 记 pull，被拉 B 记
    // move(A)（id 形态）——A 移动让位，B 挤入 A 原格；B 无 MOVE 部件也带疲劳
    // 照样移动（canMove 的 _pulled 分支），B 的移动疲劳沿链记在 A 头上
    //（_add-fatigue 归链）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0); // 拉者
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0); // 被拉（无 MOVE）
    if let RoomObject::Creep(cc) = f
        .world
        .rooms
        .get_mut("W1N1")
        .unwrap()
        .objects
        .get_mut(&b)
        .unwrap()
    {
        cc.fatigue = 2; // 被拉者带疲劳
    }

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m = IntentData::bare();
    m.id = Some(a); // 被拉 creep 的 move(拉者)——id 形态
    batch.add_object_intent("p1", b, Action::Move, m);
    let mut m2 = IntentData::bare();
    m2.direction = Some(7); // LEFT：A 从 (10,10) → (9,10)
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    // A 移至 (9,10)；B 挤入 A 原格 (10,10)
    assert_eq!(f.creep_state(a).0, 9, "拉者左移");
    assert_eq!(f.creep_state(b).0, 10, "被拉者补位（无 MOVE + 疲劳照移）");
    // 疲劳（官方 movement.js:235-245 无 weight||1——纯 MOVE 移动 0 疲劳）：
    // A 自身移动 1 部件×2 + B 的移动 1×2 归链到 A = 4，A 的 1 MOVE 恢复 -2 → 2
    //（无归链则 A 仅 2-2=0——2 锁的就是归链发生）；B 自身疲劳不动（2）且无
    // 恢复（无 MOVE）
    assert_eq!(f.creep_state(a).2, 2, "拉者累计双方移动疲劳后恢复 -2");
    assert_eq!(
        f.creep_state(b).2,
        2,
        "被拉者疲劳不动（归链 + 无 MOVE 无恢复）"
    );
}

#[test]
fn pull_pairing_breaks_on_destination_mismatch() {
    // movement.js:176-180：被拉对象的 move 目标格 ≠ 拉者当前格 → 解除配对
    // → 常规 canMove（无 MOVE 部件拒）——B 不动；A 独自移动
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m = IntentData::bare();
    m.direction = Some(3); // RIGHT：B 朝 (12,10)——不是拉者的当前格
    batch.add_object_intent("p1", b, Action::Move, m);
    let mut m2 = IntentData::bare();
    m2.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    assert_eq!(f.creep_state(a).0, 9, "拉者照常移动");
    assert_eq!(f.creep_state(b).0, 11, "配对解除后 B 无 MOVE 不动");
    assert_eq!(f.creep_state(b).2, 0, "B 无疲劳发生");
}

#[test]
fn pulled_creep_without_move_intent_stays() {
    // 只有 pull 没有被拉方的 move：位移不发生（官方矩阵只收 move intent；
    // _pull/_pulled 只影响仲裁 tie-break 与 canMove）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    let mut m2 = IntentData::bare();
    m2.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, m2);
    f.settle(batch);

    assert_eq!(f.creep_state(a).0, 9);
    assert_eq!(f.creep_state(b).0, 11, "无 move intent 的被拉者不动");
}

#[test]
fn pull_cycle_chain_rejected() {
    // movement.js:70-75 checkRecursiveTarget：A→B→C 后 C 拉 A 成环——配对
    // 拒绝（C 的 pull 无效；A/B 链不受影响照常工作）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move], 0);
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Move], 0);
    let c = f.creep("C", "p1", 10, 11, &[BodyKind::Move], 0);

    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", a, Action::Pull, IntentData::target(b));
    batch.add_object_intent("p1", b, Action::Pull, IntentData::target(c));
    batch.add_object_intent("p1", c, Action::Pull, IntentData::target(a)); // 成环
    // 三方 move：B move(A)、C move(B)、A move(7)
    let mut mb = IntentData::bare();
    mb.id = Some(a);
    batch.add_object_intent("p1", b, Action::Move, mb);
    let mut mc = IntentData::bare();
    mc.id = Some(b);
    batch.add_object_intent("p1", c, Action::Move, mc);
    let mut ma = IntentData::bare();
    ma.direction = Some(7);
    batch.add_object_intent("p1", a, Action::Move, ma);
    f.settle(batch);

    // A (10,10)→(9,10)；B (11,10)→(10,10)（A 的让位格）；C (10,11) 的目标
    // 是 B 的原格 (11,10)——C→B 配对成立（环只在 C 拉 A 处断），B 让位可入。
    // 全员纯 MOVE：每步疲劳 0（movement.js:235-245 无 weight||1）
    assert_eq!(f.creep_state(a), (9, 10, 0, 0), "A 移动");
    assert_eq!(f.creep_state(b).0, 10, "B 补 A 的位");
    assert_eq!(f.creep_state(c).0, 11, "C 补 B 的位（链式拉拽传递）");
}

#[test]
fn notify_when_attacked_intent_sets_flag() {
    // processor.js:313-314：CONSTRUCTION_COST 类型 + 所有权门 → 落库
    let mut f = Fixture::new(1, terrain_with(&[]));
    let sp = f.spawn("S1", 20, 20, 300);
    let rd = f.road_at(21, 20);
    let ctl = f.controller(3, Some("p1"));

    let mut batch = IntentBatch::default();
    let mut d = IntentData::bare();
    d.enabled = Some(false);
    batch.add_object_intent("p1", sp, Action::NotifyWhenAttacked, d);
    let mut d2 = IntentData::bare();
    d2.enabled = Some(true);
    batch.add_object_intent("p1", rd, Action::NotifyWhenAttacked, d2.clone());
    // controller 不在 CONSTRUCTION_COST——通用段类型门拒（API 面不暴露，
    // 伪造 intent 的兜底）
    batch.add_object_intent("p1", ctl, Action::NotifyWhenAttacked, d2);
    f.settle(batch);

    assert_eq!(
        match f.obj(sp) {
            RoomObject::Spawn(s) => s.notify_when_attacked,
            _ => panic!(),
        },
        Some(false),
        "spawn 落 enabled=false"
    );
    assert_eq!(
        match f.obj(rd) {
            RoomObject::Road(r) => r.notify_when_attacked,
            _ => panic!(),
        },
        Some(true),
        "中性路同样可落（无主不拦——官方 :248）"
    );

    // 他人对象：分派层通用段跳过（p2 对 p1 的 spawn）
    let mut batch2 = IntentBatch::default();
    let mut d3 = IntentData::bare();
    d3.enabled = Some(true);
    batch2.add_object_intent("p2", sp, Action::NotifyWhenAttacked, d3);
    f.settle(batch2);
    assert_eq!(
        match f.obj(sp) {
            RoomObject::Spawn(s) => s.notify_when_attacked,
            _ => panic!(),
        },
        Some(false),
        "他人 intent 跳过"
    );
}

#[test]
fn built_structure_born_with_notify_attacked_true() {
    // build.js:116：玩家建造落成的结构出生即 notifyWhenAttacked: true
    let mut f = Fixture::new(1, terrain_with(&[]));
    f.controller(1, Some("p1"));
    let builder = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 200);
    // 进度 45/50：1 WORK×BUILD_POWER(5) 恰好补完剩余 5 → 落成
    let site = f.site(11, 10, "extension", 45, 50, "p1");
    let mut batch = IntentBatch::default();
    batch.add_object_intent("p1", builder, Action::Build, {
        let mut d = IntentData::target(site);
        d.x = Some(11);
        d.y = Some(10);
        d
    });
    f.settle(batch);
    let ext = f
        .world
        .rooms
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Extension(e) => Some(e.notify_when_attacked),
            _ => None,
        });
    assert_eq!(ext, Some(Some(true)), "落成 extension 出生即 true");
}

// ---- GPT 外部复审清偿（m2/a4-review）锁值 ----

#[test]
fn ghodium_resource_family_settles() {
    // P1-1：G 资源的 transfer/withdraw/drop 全链结算（官方 transfer.js:8/
    // withdraw.js:8/drop.js:10 按资源类型结算——此前执行器静默丢弃）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let st = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store::default(),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // G transfer：creep 40G → storage 20G
    let c = f.creep("w1", "p1", 21, 20, &[BodyKind::Move, BodyKind::Carry], 0);
    if let Some(RoomObject::Creep(cc)) = f.world.get_mut(c) {
        cc.store.ghodium = 40;
    }
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(st);
    d.amount = Some(20);
    d.resource_type = Some("G".into());
    b.add_object_intent("p1", c, Action::Transfer, d);
    f.settle(b);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 20, "creep G 40→20");
            assert_eq!(s.store.ghodium, 20, "storage G 0→20");
        }
        _ => panic!(),
    }
    // G withdraw：storage 20 → creep 全量
    let mut b2 = IntentBatch::default();
    let mut d2 = IntentData::bare();
    d2.id = Some(st);
    d2.amount = Some(20);
    d2.resource_type = Some("G".into());
    b2.add_object_intent("p1", c, Action::Withdraw, d2);
    f.settle(b2);
    match (f.obj(c), f.obj(st)) {
        (RoomObject::Creep(cc), RoomObject::Storage(s)) => {
            assert_eq!(cc.store.ghodium, 40, "withdraw 回满");
            assert_eq!(s.store.ghodium, 0);
        }
        _ => panic!(),
    }
    // G drop：掉落为 resource_type=2 的资源堆（同格同资源并堆）
    let mut b3 = IntentBatch::default();
    let mut d3 = IntentData::bare();
    d3.amount = Some(5);
    d3.resource_type = Some("G".into());
    b3.add_object_intent("p1", c, Action::Drop, d3);
    f.settle(b3);
    let pile = f
        .world
        .rooms
        .get("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) if e.resource_type == 2 => Some(e.amount),
            _ => None,
        });
    assert_eq!(
        pile,
        Some(4),
        "G 5 掉落 → 当 tick 衰减 ceil(5/1000)=1 → 4（官方全资源同式）"
    );
    assert_eq!(
        match f.obj(c) {
            RoomObject::Creep(cc) => cc.store.ghodium,
            _ => 0,
        },
        35
    );
}

#[test]
fn boost_effectiveness_unified() {
    // P1-2：强化部件统一效能——官方 movement.js:41-59（重量按强化容量扣）与
    // tick.js:105（恢复按 fatigue 效能）。KH 容量×2 / ZO 疲劳×2
    let mut f = Fixture::new(1, terrain_with(&[]));
    // 1 carry(KH) + 2 move(ZO)：携带 80（无强化需 2 carry=weight 2；KH ×2
    // 容量 100 ⇒ weight 1）+ 1 move 够驱 ⇒ move 后疲劳 = (1+1) - 2×2 = 0
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: 9_000,
                hits: 300,
                spawning: false,
                action_log: 0,
                name: "b1".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Carry.code(),
                        hits: 100,
                        boost: Some("KH".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Move.code(),
                        hits: 100,
                        boost: Some("ZO".into()),
                    },
                ],
                store: kk_schema::room::Store {
                    energy: 80,
                    ghodium: 0,
                },
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.direction = Some(3);
    b.add_object_intent("p1", c, Action::Move, d);
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!((cc.x, cc.y), (11, 10), "direction 3=RIGHT 落地");
            assert_eq!(
                cc.fatigue, 0,
                "weight=1（KH 容量×2）− 恢复 2×2（ZO 效能×2）= 1−4 → 0"
            );
        }
        _ => panic!(),
    }
}

#[test]
fn sign_settlement_persists_datetime() {
    // 灰区纠错清偿：官方 signController.js:23 结算时落一次 Date.now() 毫秒
    let mut f = Fixture::new(1, terrain_with(&[]));
    let ctl = f.controller(0, None);
    let c = f.creep("w1", "p1", 5, 6, &[BodyKind::Claim], 0);
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(ctl);
    d.sign = Some("hello".into());
    b.add_object_intent("p1", c, Action::SignController, d);
    f.settle(b);
    match f.obj(ctl) {
        RoomObject::Controller(t) => {
            let s = t.sign.as_ref().expect("签名落库");
            assert!(
                s.datetime.is_some(),
                "datetime 结算时落一次（非 getter 合成）"
            );
        }
        _ => panic!(),
    }
}

// ---- GPT 二轮复审清偿锁值 ----

#[test]
fn boosted_build_two_step_official_values() {
    // 官方 build.js:67-84 两步：基础量定耗能（energy -= base effect），
    // 增量数组定进度。GPT 探针值：5 能量单 WORK——LH(×1.5) 进度 7、
    // XLH2O(×2) 进度 10、能耗均 5
    for (boost, want_progress) in [("LH", 7), ("XLH2O", 10)] {
        let mut f = Fixture::new(1, terrain_with(&[]));
        let site = f
            .world
            .insert(
                "W1N1",
                RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                    id: 0,
                    x: 10,
                    y: 10,
                    structure_type: "road".into(),
                    progress: 0,
                    progress_total: 100,
                    owner: Some("p1".into()),
                    name: None,
                }),
            )
            .unwrap();
        let c = f
            .world
            .insert(
                "W1N1",
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x: 10,
                    y: 11,
                    fatigue: 0,
                    age_time: 9_000,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: "w".into(),
                    owner: "p1".into(),
                    body: vec![kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 100,
                        boost: Some(boost.into()),
                    }],
                    store: kk_schema::room::Store {
                        energy: 5,
                        ghodium: 0,
                    },
                }),
            )
            .unwrap();
        let _ = site;
        let mut b = IntentBatch::default();
        b.add_object_intent("p1", c, Action::Build, IntentData::target(site));
        f.settle(b);
        let progress = f
            .world
            .rooms
            .get("W1N1")
            .unwrap()
            .objects
            .get(&site)
            .map(|o| match o {
                RoomObject::ConstructionSite(s) => s.progress,
                _ => 0,
            })
            .unwrap_or(0);
        let energy = match f.obj(c) {
            RoomObject::Creep(cc) => cc.store.energy,
            _ => 0,
        };
        assert_eq!(progress as i64, want_progress, "{boost} 进度");
        assert_eq!(energy, 0, "{boost} 能量扣基础量 5");
    }
}

#[test]
fn transfer_mixed_holdings_respect_total_capacity() {
    // 官方 transfer.js:32：通用容量目标按 calcResources 全槽求和——
    // 满 energy 的 storage 只能收差额 G（GPT 二轮 P1-2 探针场景）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let st = f
        .world
        .insert(
            "W1N1",
            RoomObject::Storage(kk_schema::room::StorageObj {
                id: 0,
                x: 20,
                y: 20,
                hits: 10_000,
                owner: "p1".into(),
                store: kk_schema::room::Store {
                    energy: 999_990,
                    ghodium: 0,
                },
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let c1 = f.creep("w1", "p1", 21, 20, &[BodyKind::Move, BodyKind::Carry], 0);
    if let Some(RoomObject::Creep(cc)) = f.world.get_mut(c1) {
        cc.store.ghodium = 20;
    }
    // 单笔 20G：余量 1e6-999990=10 ⇒ 只收 10
    let mut b = IntentBatch::default();
    let mut d = IntentData::bare();
    d.id = Some(st);
    d.amount = Some(20);
    d.resource_type = Some("G".into());
    b.add_object_intent("p1", c1, Action::Transfer, d);
    f.settle(b);
    match f.obj(st) {
        RoomObject::Storage(s) => {
            assert_eq!(s.store.energy, 999_990);
            assert_eq!(s.store.ghodium, 10, "全槽总量钳制：余量 10 G");
        }
        _ => panic!(),
    }
    match f.obj(c1) {
        RoomObject::Creep(cc) => assert_eq!(cc.store.ghodium, 10, "退回未收的 10"),
        _ => panic!(),
    }
}

#[test]
fn pickup_ghodium_routes_to_ghodium_slot() {
    // 官方 pickup.js:9/23-28：carry 全槽求和、按堆 resourceType 入槽
    let mut f = Fixture::new(1, terrain_with(&[]));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Carry], 30);
    let pile = f
        .world
        .insert(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 10,
                y: 11,
                resource_type: 2,
                amount: 10,
                resource: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Pickup, IntentData::target(pile));
    f.settle(b);
    match f.obj(c) {
        RoomObject::Creep(cc) => {
            assert_eq!(cc.store.energy, 30, "energy 槽不动");
            assert_eq!(cc.store.ghodium, 10, "G 入 ghodium 槽（GPT 二轮 P1-3）");
        }
        _ => panic!(),
    }
    assert!(f.world.get(pile).is_none(), "堆清零移除");
}
