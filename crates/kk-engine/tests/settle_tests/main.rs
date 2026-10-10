//! 单房间结算测试的共享夹具和动作构造工具。
//!
//! Fixture 直接调用 settle_room；跨房市场和世界发布行为由使用 Engine 的测试验证。

use std::collections::{BTreeMap, BTreeSet};

use kk_engine::intent::{Action, IntentBatch, IntentData, NotifyIntent};
use kk_engine::settle::{PlayerState, SettledRoom, settle_room};
use kk_schema::room::{CreepObj, RoomObject, SpawnObj};
use kk_world::types::BodyKind;
use kk_world::{ObjId, World};
use serde_json::Value;

// 测试目标根采用 tests/<name>/main.rs 形态，mod 声明按同目录规则解析，
// 不再使用 #[path] 挂接（S01 结构治理，取代 b1 批次的显式路径结论）。
mod capacity_actions;
mod combat;
mod contrib_link;
mod controller;
mod controller_safemode;
mod creation;
mod creep_actions;
mod deposit_actions;
mod engine_world;
mod ext;
mod factory_actions;
mod lab_actions;
mod mineral_power_gates;
mod minerals_and_boost;
mod mod_buildings;
mod mod_hits;
mod mod_resources;
mod mod_store;
mod movement;
mod movement_inter_room;
mod notify;
mod nuke_actions;
mod observer_actions;
mod power_actions;
mod power_bank;
mod resources;
mod spawn;
mod store_transfer;
mod structure_lifecycle;
mod terminal_market;
mod tombstone_ruin;
mod tower_link_actions;

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
        world.register_room("W1N1", terrain).unwrap();
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

    /// 通过闭包设置控制器的预定、签名、封锁和安全模式等字段。
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

    /// 插入无主道路。
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

    /// 插入矿物；density 范围为 1..=4，None 表示尚未设置再生时钟。
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

    /// 插入提取器，cooldown 是剩余 tick 数。
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

    /// 插入携带命名矿物的 p1 creep。
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
        self.world
            .edit_object(id, |object| {
                if let Some(RoomObject::Creep(c)) = Some(object) {
                    c.store_extra = vec![kk_schema::room::ResourceStack {
                        resource: mineral.into(),
                        amount,
                    }];
                }
                Ok(())
            })
            .unwrap();
        id
    }

    fn settle(&mut self, batch: IntentBatch) -> Vec<Value> {
        // 固定默认墙钟，使签名和重放结果可复现；需要其他时间的测试显式传入 TickInput。
        self.settle_with_input(
            batch,
            kk_engine::settle::TickInput {
                now_ms: 1_700_000_000_000,
                warmup: false,
            },
        )
    }

    fn settle_with_input(
        &mut self,
        batch: IntentBatch,
        input: kk_engine::settle::TickInput,
    ) -> Vec<Value> {
        let state =
            kk_engine::TickState::new(std::mem::take(&mut self.players), self.world.next_obj_id());
        let room = self.world.room("W1N1").unwrap().clone();
        let SettledRoom { room, events, .. } = settle_room(
            room,
            self.world.time(),
            input.now_ms,
            &state,
            &batch,
            &kk_engine::settle::Plane {
                world_span: kk_engine::settle::Plane::UNWRAPPED_SPAN,
                buildings: Default::default(),
                links: kk_world::place::builtin_link_rules(),
                stores: kk_world::place::builtin_store_rules(),
                combat: kk_world::place::builtin_combat_ranges(),
                move_gate: Default::default(),
                quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
                static_passability: Default::default(),
                resources: Default::default(),
                contrib: std::rc::Rc::new(
                    kk_engine::settle::contrib::ContributionDispatch::default(),
                ),
                extensions: std::rc::Rc::new(kk_mod_api::extension::ExtensionCatalog::default()),
                plan: kk_engine::settle::builtin_plan(),
            },
        )
        .unwrap();
        let (players, next, _) = state.into_parts().unwrap();
        self.players = players;
        let name = room.name.clone();
        self.world.replace_room(name, room).unwrap();
        self.world.bump_next_obj_id(next);
        self.world.advance_ticks(1).unwrap();
        events
    }

    /// 以计划的有效规则结算（link 与配额快照同源），供规则覆盖贯穿类
    /// 场景使用。
    fn settle_with_rules(&mut self, batch: IntentBatch, plan: &kk_mod_api::ContributionPlan) {
        let links = kk_mod_api::builtin::LinkRules::from_plan(plan.rules()).unwrap();
        let quotas = std::rc::Rc::new(
            kk_mod_api::builtin::StructureQuotaSnapshot::from_plan(plan.rules()).unwrap(),
        );
        let extensions = kk_mod_api::extension::ExtensionCatalog::default();
        self.settle_with_plane(
            batch,
            kk_engine::settle::Plane {
                world_span: kk_engine::settle::Plane::UNWRAPPED_SPAN,
                buildings: Default::default(),
                links,
                stores: kk_world::place::builtin_store_rules(),
                combat: kk_world::place::builtin_combat_ranges(),
                move_gate: Default::default(),
                quotas,
                static_passability: Default::default(),
                resources: Default::default(),
                contrib: std::rc::Rc::new(
                    kk_engine::settle::contrib::ContributionDispatch::default(),
                ),
                extensions: std::rc::Rc::new(extensions),
                plan: kk_engine::settle::builtin_plan(),
            },
        );
    }

    /// 以显式处理链结算，供处理器执行序类场景使用。
    fn settle_with_dispatch(
        &mut self,
        batch: IntentBatch,
        dispatch: std::rc::Rc<kk_engine::settle::contrib::ContributionDispatch>,
    ) {
        self.settle_with(batch, kk_world::place::builtin_link_rules(), dispatch);
    }

    fn settle_with(
        &mut self,
        batch: IntentBatch,
        links: kk_mod_api::builtin::LinkRules,
        dispatch: std::rc::Rc<kk_engine::settle::contrib::ContributionDispatch>,
    ) {
        let extensions = kk_mod_api::extension::ExtensionCatalog::default();
        self.settle_with_plane(
            batch,
            kk_engine::settle::Plane {
                world_span: kk_engine::settle::Plane::UNWRAPPED_SPAN,
                buildings: Default::default(),
                links,
                stores: kk_world::place::builtin_store_rules(),
                combat: kk_world::place::builtin_combat_ranges(),
                move_gate: Default::default(),
                quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
                static_passability: Default::default(),
                resources: Default::default(),
                contrib: dispatch,
                extensions: std::rc::Rc::new(extensions),
                plan: kk_engine::settle::builtin_plan(),
            },
        );
    }

    /// 以完整贡献平面结算（扩展字段目录非空的场景使用）。
    fn settle_with_plane(&mut self, batch: IntentBatch, plane: kk_engine::settle::Plane) {
        let state =
            kk_engine::TickState::new(std::mem::take(&mut self.players), self.world.next_obj_id());
        let room = self.world.room("W1N1").unwrap().clone();
        let SettledRoom { room, events, .. } = settle_room(
            room,
            self.world.time(),
            1_700_000_000_000,
            &state,
            &batch,
            &plane,
        )
        .unwrap();
        let (players, next, _) = state.into_parts().unwrap();
        self.players = players;
        let name = room.name.clone();
        self.world.replace_room(name, room).unwrap();
        self.world.bump_next_obj_id(next);
        self.world.advance_ticks(1).unwrap();
        drop(events);
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

fn generic_tower(x: u8, y: u8) -> RoomObject {
    RoomObject::Generic(kk_schema::room::GenericObj {
        id: 0,
        x,
        y,
        data: r#"{"type":"structure","structureType":"tower","owner":"p1","hits":1}"#.into(),
    })
}

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
                    // 给 container/rampart 设置远期衰减时钟，避免测试其他动作时结构提前消失。
                    // 0 时钟会立即触发衰减，因此不能省略。
                    "nextDecayTime": 1_000_000,
                })
                .to_string()
                .into(),
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

mod controller_upgrade;

mod controller_invulnerability;
