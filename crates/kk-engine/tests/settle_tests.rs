//! 单房间结算集成测试：固定指令脚本驱动，逐项对照官方数值
//! （常量值与官方 file:line 见各断言注释）。

use std::collections::{BTreeMap, BTreeSet};

use kk_engine::intent::{Action, IntentBatch, IntentData, NotifyIntent};
use kk_engine::settle::{PlayerState, settle_room};
use kk_schema::room::{CreepObj, RoomObject, SpawnObj};
use kk_world::types::BodyKind;
use kk_world::{ObjId, World};
use serde_json::Value;

// 集成测试根（crate root）的 mod 声明解析到 tests/ 平级，故显式 #[path]
#[path = "settle_tests/combat_heal.rs"]
mod combat_heal;
#[path = "settle_tests/combat_ranged.rs"]
mod combat_ranged;
#[path = "settle_tests/controller_safemode.rs"]
mod controller_safemode;
#[path = "settle_tests/m2b2_terminal_market_family.rs"]
mod m2b2_terminal_market_family;
#[path = "settle_tests/m2b_creation_family.rs"]
mod m2b_creation_family;
#[path = "settle_tests/m2b_notify_family.rs"]
mod m2b_notify_family;
#[path = "settle_tests/m2b_structure_actions_family.rs"]
mod m2b_structure_actions_family;
#[path = "settle_tests/m2b_structure_decay_family.rs"]
mod m2b_structure_decay_family;
#[path = "settle_tests/minerals_and_boost.rs"]
mod minerals_and_boost;
#[path = "settle_tests/misc_and_notify.rs"]
mod misc_and_notify;
#[path = "settle_tests/movement_spawn_basics.rs"]
mod movement_spawn_basics;
#[path = "settle_tests/structures_and_attack.rs"]
mod structures_and_attack;
#[path = "settle_tests/tombstone_ruin_engine.rs"]
mod tombstone_ruin_engine;

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
                    store_extra: Vec::new(),
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
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

    /// B4：矿物（density 1..=4；next_regen None = 未挂钟）。
    fn mineral(
        &mut self,
        x: u8,
        y: u8,
        mineral_type: &str,
        amount: u32,
        density: u8,
        next_regen: Option<u64>,
    ) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Mineral(kk_schema::room::MineralObj {
                    id: 0,
                    x,
                    y,
                    mineral_type: mineral_type.into(),
                    mineral_amount: amount,
                    density,
                    next_regeneration_time: next_regen,
                }),
            )
            .unwrap()
    }

    /// B4：提取器（hits 缺省 EXTRACTOR_HITS）。
    fn extractor(&mut self, x: u8, y: u8, owner: Option<&str>, cooldown: u64) -> ObjId {
        self.world
            .insert(
                "W1N1",
                RoomObject::Extractor(kk_schema::room::ExtractorObj {
                    id: 0,
                    x,
                    y,
                    hits: 500,
                    owner: owner.map(str::to_string),
                    cooldown,
                    notify_when_attacked: None,
                }),
            )
            .unwrap()
    }

    /// B4：携带命名矿物的 creep（store_extra 直填——harvest 产出的等价终态）。
    #[allow(clippy::too_many_arguments)]
    fn creep_with_mineral(
        &mut self,
        name: &str,
        x: u8,
        y: u8,
        kinds: &[BodyKind],
        energy: u32,
        mineral: &str,
        amount: u32,
    ) -> ObjId {
        let id = self.creep(name, "p1", x, y, kinds, energy);
        if let Some(RoomObject::Creep(c)) = self.world.get_mut(id) {
            c.store_extra = vec![kk_schema::room::ResourceStack {
                resource: mineral.into(),
                amount,
            }];
        }
        id
    }

    fn settle(&mut self, batch: IntentBatch) -> Vec<Value> {
        // 测试工作台默认注入固定墙钟：结算可复现（个别用例显式改 now_ms 锁
        // 墙钟语义——重放一致性测试在同输入下逐字节一致）
        self.settle_with_input(
            batch,
            kk_engine::settle::TickInput {
                now_ms: 1_700_000_000_000,
            },
        )
    }

    fn settle_with_input(
        &mut self,
        batch: IntentBatch,
        input: kk_engine::settle::TickInput,
    ) -> Vec<Value> {
        let mut next = self.world.next_obj_id();
        let mut room = self.world.room("W1N1").unwrap().clone();
        let events = settle_room(
            &mut room,
            self.world.time(),
            input.now_ms,
            &mut next,
            &mut self.players,
            &batch,
        )
        .unwrap();
        let name = room.name.clone();
        self.world.commit_room(name, room);
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

// ---- 外部复审（ChatGPT）回归：Generic 透传结构的类型名解析 ----

fn generic_tower(x: u8, y: u8) -> RoomObject {
    RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x,
        y,
        data: r#"{"type":"structure","structureType":"tower","owner":"p1","hits":1}"#.into(),
    })
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
                    // M2B P4：衰减钟（官方 DB 不变量——container/rampart 恒带
                    // 绝对 nextDecayTime；缺钟官方立即衰减。夹具给远钟使
                    // 非衰减用例不受相位 8 衰减影响）
                    "nextDecayTime": 1_000_000,
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

fn controller_intent(creep: ObjId, action: Action, target: ObjId) -> IntentBatch {
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", creep, action, IntentData::target(target));
    b
}
