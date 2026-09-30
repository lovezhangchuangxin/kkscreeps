//! 渲染器夹具导出（F2 前端批唯一的引擎侧触点，方案 docs/plans/f2-renderer.md §4）。
//!
//! 只读公开 API：`World` 构建（settle_tests Fixture 同款摆法——脚本对象摆放）
//! → `Engine` 空意图推进 N tick（零 V8：sandboxes 空 + 默认 IntentBatch，走
//! 与 embed/测试同一条 settle_room 结算链）→ 逐 tick 落 `world_json()`。
//! 输出 JSONL（`{"tick":n,"world":{…}}` 每行一条，含 tick 0 初始态），由
//! 前端 `packages/renderer/scripts/build-fixtures.mjs` 经 differ 转成
//! `{room, ticks:[snapshot, …deltas]}` 夹具入库。
//!
//! 确定性（方案 §9）：固定 seed + 固定 [`TickInput`] 墙钟注入（ADR-013 决策 5）
//! + 结算全程 BTreeMap 序 + 无时间戳——两次导出逐字节一致（本模块测试断言）。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::settle::{Engine, TickInput};
use kk_schema::room::{
    ConstructionSiteObj, ControllerObj, CreepObj, EnergyObj, ExtensionObj, GenericObj, MineralObj,
    RoadObj, RoomObject, SpawnObj, StorageObj, Store, TombstoneObj,
};
use kk_world::ObjId;
use kk_world::World;
use kk_world::types::BodyKind;

/// 墙钟固定值（2023-11-14T22:13:20Z 量级整数）：ADR-013 决策 5 的显式注入面，
/// 夹具重放/回归的唯一时间真值——绝不在导出路径使用 `TickInput::now`。
const FIXED_NOW_MS: u64 = 1_700_000_000_000;

/// 世界构建器（settle_tests Fixture 同款摆法的脚本摆放函数）。
/// 返回需要意图脚本驱动的对象句柄（名 → id；静态夹具返回空表）。
type WorldBuilder = fn(&mut World) -> BTreeMap<String, ObjId>;

/// 逐 tick 意图脚本（运动/战斗夹具——R1 §12 决策①登记的本文件扩展点）。
type IntentScript = fn(tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch;

/// 夹具清单：名（=输出文件名/前端夹具键）、seed、世界构建器、意图脚本。
struct FixtureSpec {
    name: &'static str,
    seed: u64,
    build: WorldBuilder,
    script: Option<IntentScript>,
}

const fn fixture(name: &'static str, seed: u64, build: WorldBuilder) -> FixtureSpec {
    FixtureSpec {
        name,
        seed,
        build,
        script: None,
    }
}

const FIXTURES: &[FixtureSpec] = &[
    fixture("plains", 1, build_plains),
    fixture("standard", 2, build_standard),
    fixture("swamp-walls", 3, build_swamp_walls),
    fixture("dense-edge", 4, build_dense_edge),
    FixtureSpec {
        name: "creeps",
        seed: 5,
        build: build_creeps,
        script: Some(creep_tick_script),
    },
    FixtureSpec {
        name: "actions",
        seed: 6,
        build: build_actions,
        script: Some(action_tick_script),
    },
];

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

fn creep(name: &str, owner: &str, x: u8, y: u8, kinds: &[BodyKind]) -> RoomObject {
    creep_with_energy(name, owner, x, y, kinds, 0)
}

fn creep_with_energy(
    name: &str,
    owner: &str,
    x: u8,
    y: u8,
    kinds: &[BodyKind],
    energy: u32,
) -> RoomObject {
    RoomObject::Creep(CreepObj {
        id: 0,
        x,
        y,
        fatigue: 0,
        age_time: 1500,
        hits: kinds.len() as u32 * 100,
        spawning: false,
        action_log: 0,
        name: name.into(),
        owner: owner.into(),
        body: body(kinds),
        store: Store { energy, ghodium: 0 },
        store_extra: Vec::new(),
        notify_when_attacked: None,
        say_message: None,
        say_public: None,
    })
}

/// R4 运动与战斗夹具（房间 W5N5 全平原）：移动/朝向插值、受击罩三色、
/// 死亡淡出的实弹取材。布局（互不相扰的两组舞台）：
///   - 走位者×2（p1，move×4）：右/下/左/上循环——方向逐 tick 变化（朝向
///     旋转取材）；两组相位错开。
///   - 近战组：attacker(p1 [attack,move]) 每 tick 咬 victimA(p2 [move]，100
///     hits，ATTACK_POWER=30 → 第 4 tick 死亡——removedIds→死亡淡出)。
///   - 双向组：attacker2 咬 victimB(p2 [tough,tough,move]，300 hits) 且
///     healer2 每 tick 治 victimB → 同 tick attacked+healed=黄罩。
///   - 治疗组：healer(p1 [heal,move]，储能) 每 tick 治 tank(p1 [tough,move])
///     → 纯绿罩（heal.js:34 事件无条件，满血也发）。
///
/// 意图对死亡目标的续发由结算链静默拒（room.rs resolve else-return）。
fn build_creeps(world: &mut World) -> BTreeMap<String, ObjId> {
    world
        .register_room("W5N5", kk_core::Terrain::plains())
        .unwrap();
    let mut handles = BTreeMap::new();
    let put =
        |world: &mut World, handles: &mut BTreeMap<String, ObjId>, name: &str, o: RoomObject| {
            let id = world.insert("W5N5", o).unwrap();
            handles.insert(name.to_string(), id);
        };
    put(
        world,
        &mut handles,
        "walker1",
        creep(
            "walker1",
            "p1",
            10,
            10,
            &[
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
            ],
        ),
    );
    put(
        world,
        &mut handles,
        "walker2",
        creep(
            "walker2",
            "p1",
            14,
            10,
            &[
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
                BodyKind::Move,
            ],
        ),
    );
    put(
        world,
        &mut handles,
        "attacker",
        creep(
            "attacker",
            "p1",
            20,
            20,
            &[BodyKind::Attack, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "victimA",
        creep("victimA", "p2", 20, 21, &[BodyKind::Move]),
    );
    put(
        world,
        &mut handles,
        "attacker2",
        creep(
            "attacker2",
            "p1",
            23,
            20,
            &[BodyKind::Attack, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "victimB",
        creep(
            "victimB",
            "p2",
            22,
            20,
            &[BodyKind::Tough, BodyKind::Tough, BodyKind::Move],
        ),
    );
    put(
        world,
        &mut handles,
        "healer",
        creep_with_energy(
            "healer",
            "p1",
            25,
            25,
            &[BodyKind::Heal, BodyKind::Move],
            1000,
        ),
    );
    put(
        world,
        &mut handles,
        "tank",
        creep("tank", "p1", 25, 26, &[BodyKind::Tough, BodyKind::Move]),
    );
    put(
        world,
        &mut handles,
        "healer2",
        creep_with_energy(
            "healer2",
            "p1",
            22,
            21,
            &[BodyKind::Heal, BodyKind::Move],
            1000,
        ),
    );
    handles
}

/// 逐 tick 意图脚本：走位循环 + 战斗/治疗。方向钟面（constants.rs TOP=1 顺
/// 时针到 TOP_LEFT=8）：3=右、5=下、7=左、1=上（movement.rs offsets）。
fn creep_tick_script(tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch {
    let mut batch = IntentBatch::default();
    let id = |name: &str| -> ObjId {
        *handles
            .get(name)
            .unwrap_or_else(|| panic!("意图脚本句柄缺失: {name}"))
    };
    const LOOP1: [u8; 4] = [3, 5, 7, 1];
    const LOOP2: [u8; 4] = [5, 7, 1, 3];
    let phase = (tick as usize) % 4;
    for (name, loop_) in [("walker1", &LOOP1), ("walker2", &LOOP2)] {
        let mut data = IntentData::bare();
        data.direction = Some(loop_[phase]);
        batch.add_object_intent("p1", id(name), Action::Move, data);
    }
    batch.add_object_intent(
        "p1",
        id("attacker"),
        Action::Attack,
        IntentData::target(id("victimA")),
    );
    batch.add_object_intent(
        "p1",
        id("attacker2"),
        Action::Attack,
        IntentData::target(id("victimB")),
    );
    batch.add_object_intent(
        "p1",
        id("healer"),
        Action::Heal,
        IntentData::target(id("tank")),
    );
    batch.add_object_intent(
        "p1",
        id("healer2"),
        Action::Heal,
        IntentData::target(id("victimB")),
    );
    batch
}

fn terrain_with(walls: &[(usize, usize)], swamps: &[(usize, usize)]) -> kk_core::Terrain {
    let mut t = kk_core::Terrain::plains();
    for &(x, y) in walls {
        t.as_mut_slice()[y * 50 + x] |= kk_core::TERRAIN_MASK_WALL;
    }
    for &(x, y) in swamps {
        t.as_mut_slice()[y * 50 + x] |= kk_core::TERRAIN_MASK_SWAMP;
    }
    t.normalize();
    t
}

/// R5 real event chain. Public intent API only; includes a completed site that
/// disappears from the current index and ranged healing. RangedAttack/MassAttack
/// currently have no settle handlers on this branch; those use a synthetic gallery.
fn build_actions(world: &mut World) -> BTreeMap<String, ObjId> {
    const ROOM: &str = "W6N6";
    world
        .register_room(ROOM, kk_core::Terrain::plains())
        .unwrap();
    let mut handles = BTreeMap::new();
    let mut put = |name: &str, object| {
        handles.insert(name.to_string(), world.insert(ROOM, object).unwrap());
    };
    for (name, x, y, kinds, energy) in [
        ("attacker", 9, 10, vec![BodyKind::Attack, BodyKind::Move], 0),
        (
            "ranged-healer",
            8,
            13,
            vec![BodyKind::Heal, BodyKind::Move],
            0,
        ),
        (
            "harvester",
            14,
            10,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            0,
        ),
        (
            "builder",
            13,
            16,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
        (
            "repairer",
            20,
            15,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
        (
            "upgrader",
            29,
            24,
            vec![BodyKind::Work, BodyKind::Carry, BodyKind::Move],
            50,
        ),
    ] {
        put(name, creep_with_energy(name, "p1", x, y, &kinds, energy));
    }
    let tank_body = vec![BodyKind::Tough; 20];
    for (name, x, y, owner) in [
        ("ranged-target", 10, 10, "p2"),
        ("heal-target", 10, 13, "p1"),
    ] {
        put(name, creep(name, owner, x, y, &tank_body));
    }
    put(
        "source",
        RoomObject::Source(kk_schema::room::SourceObj {
            id: 0,
            x: 15,
            y: 10,
            energy: 3000,
            energy_capacity: 3000,
            next_regeneration_time: None,
        }),
    );
    put(
        "site",
        RoomObject::ConstructionSite(ConstructionSiteObj {
            id: 0,
            x: 14,
            y: 17,
            structure_type: "extension".into(),
            progress: 2995,
            progress_total: 3000,
            owner: Some("p1".into()),
            name: None,
        }),
    );
    put(
        "road",
        RoomObject::Road(RoadObj {
            id: 0,
            x: 21,
            y: 16,
            hits: 1,
            next_decay_time: 5000,
            notify_when_attacked: None,
        }),
    );
    put(
        "controller",
        RoomObject::Controller(ControllerObj {
            id: 0,
            x: 30,
            y: 24,
            level: 5,
            safe_mode: 0,
            safe_mode_available: 0,
            progress: 1200,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        }),
    );
    handles
}

fn action_tick_script(_tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch {
    let mut batch = IntentBatch::default();
    for (source, action, target) in [
        ("attacker", Action::Attack, "ranged-target"),
        ("ranged-healer", Action::RangedHeal, "heal-target"),
        ("harvester", Action::Harvest, "source"),
        ("builder", Action::Build, "site"),
        ("repairer", Action::Repair, "road"),
        ("upgrader", Action::UpgradeController, "controller"),
    ] {
        batch.add_object_intent(
            "p1",
            handles[source],
            action,
            IntentData::target(handles[target]),
        );
    }
    batch
}

/// 全平原：地形零特殊格 + 最小对象面——R2 plain/噪声渲染的基线夹具。
fn build_plains(world: &mut World) -> BTreeMap<String, ObjId> {
    world
        .register_room("W1N1", kk_core::Terrain::plains())
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 20,
                y: 20,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            RoomObject::Spawn(SpawnObj {
                notify_when_attacked: None,
                id: 0,
                x: 25,
                y: 25,
                hits: 5000,
                off: false,
                name: "Spawn1".into(),
                owner: "p1".into(),
                store: Store {
                    energy: 300,
                    ghodium: 0,
                },
                spawning: None,
            }),
        )
        .unwrap();
    world
        .insert(
            "W1N1",
            creep(
                "worker",
                "p1",
                25,
                26,
                &[BodyKind::Move, BodyKind::Work, BodyKind::Carry],
            ),
        )
        .unwrap();
    BTreeMap::new()
}

/// 标准房：controller/spawn/extension/storage/source/mineral/extractor/路/
/// 工地/墙/塔与城墙（Generic 透传）——R3 静态对象全家的主夹具。
fn build_standard(world: &mut World) -> BTreeMap<String, ObjId> {
    let walls: Vec<(usize, usize)> = (0..50)
        .flat_map(|i| [(i, 0usize), (i, 49usize), (0usize, i), (49usize, i)])
        .collect();
    world
        .register_room("W2N2", terrain_with(&walls, &[]))
        .unwrap();
    let ins = |world: &mut World, o: RoomObject| world.insert("W2N2", o).unwrap();
    ins(
        world,
        RoomObject::Controller(ControllerObj {
            id: 0,
            x: 5,
            y: 5,
            level: 5,
            safe_mode: 0,
            safe_mode_available: 1,
            progress: 1200,
            downgrade_time: None,
            owner: Some("p1".into()),
            owner_cleared: false,
            is_power_enabled: false,
            reservation: None,
            sign: None,
            upgrade_blocked: None,
            safe_mode_cooldown: None,
        }),
    );
    ins(
        world,
        RoomObject::Spawn(SpawnObj {
            notify_when_attacked: None,
            id: 0,
            x: 25,
            y: 25,
            hits: 5000,
            off: false,
            name: "Spawn1".into(),
            owner: "p1".into(),
            store: Store {
                energy: 180,
                ghodium: 0,
            },
            spawning: Some(kk_schema::room::SpawningJob {
                name: "bootstrapper".into(),
                // 官方不变式 needTime ≡ spawnTime − 创建时 time（time 0 创建）
                need_time: 10,
                spawn_time: 10,
                directions: vec![1, 3, 5, 7],
            }),
        }),
    );
    // 孵化雏形（同格同名，官方 _calc_spawns 出生面）：t10 出生——R3/R5 的
    // 孵化进度/孵化弧动态取材（评审 A 建议 4）。
    ins(
        world,
        RoomObject::Creep(CreepObj {
            id: 0,
            x: 25,
            y: 25,
            fatigue: 0,
            age_time: 0,
            hits: 400,
            spawning: true,
            action_log: 0,
            name: "bootstrapper".into(),
            owner: "p1".into(),
            body: body(&[
                BodyKind::Work,
                BodyKind::Carry,
                BodyKind::Move,
                BodyKind::Move,
            ]),
            store: Store {
                energy: 0,
                ghodium: 0,
            },
            store_extra: Vec::new(),
            notify_when_attacked: None,
            say_message: None,
            say_public: None,
        }),
    );
    for (x, y, energy) in [
        (24usize, 24usize, 50u32),
        (26, 24, 0),
        (24, 26, 50),
        (26, 26, 0),
        (25, 23, 0),
    ] {
        ins(
            world,
            RoomObject::Extension(ExtensionObj {
                id: 0,
                x: x as u8,
                y: y as u8,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: Store { energy, ghodium: 0 },
                notify_when_attacked: None,
            }),
        );
    }
    ins(
        world,
        RoomObject::Storage(StorageObj {
            id: 0,
            x: 23,
            y: 25,
            hits: 10000,
            owner: "p1".into(),
            store: Store {
                energy: 12000,
                ghodium: 30,
            },
            notify_when_attacked: None,
            store_extra: vec![kk_schema::room::ResourceStack {
                resource: "U".into(),
                amount: 200,
            }],
        }),
    );
    ins(
        world,
        RoomObject::Source(kk_schema::room::SourceObj {
            id: 0,
            x: 20,
            y: 20,
            energy: 2400,
            energy_capacity: 3000,
            next_regeneration_time: None,
        }),
    );
    ins(
        world,
        RoomObject::Source(kk_schema::room::SourceObj {
            id: 0,
            x: 30,
            y: 30,
            energy: 3000,
            energy_capacity: 3000,
            next_regeneration_time: None,
        }),
    );
    ins(
        world,
        RoomObject::Mineral(MineralObj {
            id: 0,
            x: 40,
            y: 40,
            mineral_type: "H".into(),
            mineral_amount: 35000,
            density: 3,
            next_regeneration_time: None,
        }),
    );
    ins(
        world,
        RoomObject::Extractor(kk_schema::room::ExtractorObj {
            id: 0,
            x: 40,
            y: 40,
            hits: 500,
            owner: Some("p1".into()),
            cooldown: 0,
            notify_when_attacked: None,
        }),
    );
    // 路：M1 磨损语义=被踩才衰减（movement.rs next_decay_time 递减），空意图
    // 不产生 decay——next_decay_time 取远期常量，移除动态留待 R4 运动意图夹具
    for (x, y) in [(24usize, 25usize), (23, 25), (22, 25), (21, 24), (20, 23)] {
        ins(
            world,
            RoomObject::Road(RoadObj {
                id: 0,
                x: x as u8,
                y: y as u8,
                hits: 5000,
                next_decay_time: 5000,
                notify_when_attacked: None,
            }),
        );
    }
    ins(
        world,
        RoomObject::ConstructionSite(ConstructionSiteObj {
            id: 0,
            x: 27,
            y: 25,
            structure_type: "extension".into(),
            progress: 200,
            progress_total: 3000,
            owner: Some("p1".into()),
            name: None,
        }),
    );
    ins(
        world,
        RoomObject::ConstructedWall(kk_schema::room::ConstructedWallObj {
            id: 0,
            x: 35,
            y: 35,
            hits: 10000,
            notify_when_attacked: None,
        }),
    );
    // 结算范围外类型走 Generic 透传桶（2B 转正前形态）：塔 + 私有城墙。
    // data 键按官方渲染消费面补齐（评审 A 建议 3）：官方对象归属键是 `user`
    // （tower.metadata isNpc/playerColor、rampart.metadata rampartColor）、
    // 塔能量条读 storeCapacityResource.energy（metadata 公式直读）、rampart
    // 顶罩跑 isPublic（shouldRun: isPublic）——owner 为 kk typed 投影口径的
    // 冗余键，R3 描述符登记 user/owner 映射。
    ins(world, RoomObject::Generic(GenericObj {
        id: 0,
        x: 26,
        y: 27,
        data: r#"{"type":"structure","structureType":"tower","user":"p1","owner":"p1","hits":3000,"hitsMax":3000,"store":{"energy":600},"storeCapacityResource":{"energy":1000}}"#.into(),
    }));
    ins(world, RoomObject::Generic(GenericObj {
        id: 0,
        x: 24,
        y: 27,
        data: r#"{"type":"structure","structureType":"rampart","user":"p1","owner":"p1","isPublic":false,"hits":5000,"hitsMax":5000}"#.into(),
    }));
    ins(
        world,
        creep(
            "harvester",
            "p1",
            26,
            25,
            &[
                BodyKind::Work,
                BodyKind::Work,
                BodyKind::Carry,
                BodyKind::Move,
            ],
        ),
    );
    ins(
        world,
        creep(
            "upgrader",
            "p1",
            6,
            6,
            &[BodyKind::Work, BodyKind::Carry, BodyKind::Move],
        ),
    );
    BTreeMap::new()
}

/// 沼泽墙混合：内陆沼泽斑块 + 沼泽上的路（hitsMax 地形系数）+ 边界墙——
/// R2 swamp 路径合并/滚动噪声与 R3 路系数的夹具。
fn build_swamp_walls(world: &mut World) -> BTreeMap<String, ObjId> {
    let mut walls = Vec::new();
    for i in 0..50 {
        walls.extend([(i, 0usize), (i, 49usize), (0usize, i), (49usize, i)]);
    }
    let mut swamps = Vec::new();
    for x in 14..=22 {
        for y in 14..=20 {
            // 斑块边缘不规则（(x+y)%3 缺口），避免教科书式矩形
            if (x + y) % 3 != 0 {
                swamps.push((x, y));
            }
        }
    }
    swamps.extend([
        (30usize, 12usize),
        (31, 12),
        (32, 13),
        (10, 35),
        (10, 36),
        (11, 36),
    ]);
    world
        .register_room("W3N3", terrain_with(&walls, &swamps))
        .unwrap();
    world
        .insert(
            "W3N3",
            RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 18,
                y: 17,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    // 沼泽上的路：四格均经地形码核实为沼泽（斑块规则 (x+y)%3!=0），hitsMax=
    // ROAD_HITS×SWAMP_RATIO=25000（world_json road_hits_max），hits=hitsMax
    // 新路态（评审 A 建议 1 清偿）；decay 语义见 standard 路注释
    for (x, y) in [(17usize, 17usize), (16, 15), (15, 14), (14, 14)] {
        world
            .insert(
                "W3N3",
                RoomObject::Road(RoadObj {
                    id: 0,
                    x: x as u8,
                    y: y as u8,
                    hits: 25000,
                    next_decay_time: 5000,
                    notify_when_attacked: None,
                }),
            )
            .unwrap();
    }
    world
        .insert(
            "W3N3",
            creep(
                "swamper",
                "p1",
                17,
                18,
                &[BodyKind::Move, BodyKind::Move, BodyKind::Work],
            ),
        )
        .unwrap();
    world
        .insert(
            "W3N3",
            creep(
                "scout",
                "p2",
                30,
                30,
                &[BodyKind::Move, BodyKind::Move, BodyKind::Move],
            ),
        )
        .unwrap();
    BTreeMap::new()
}

/// 密集边界：四边墙留 2 格出口（官方 exit 形态）+ 中心密集对象群 + 边缘散点
/// （掉落资源/墓碑）——R2 出口贴边与 R3/R4 群体渲染的夹具。
fn build_dense_edge(world: &mut World) -> BTreeMap<String, ObjId> {
    let mut walls = Vec::new();
    for i in 0..50 {
        // 出口缺口：top/bottom x=24,25；left/right y=24,25
        if i != 24 && i != 25 {
            walls.extend([(i, 0usize), (i, 49usize), (0usize, i), (49usize, i)]);
        }
    }
    // 内部墙簇（右上象限）
    for x in 34..=40 {
        for y in 10..=14 {
            if (x + y) % 4 != 0 {
                walls.push((x, y));
            }
        }
    }
    world
        .register_room("W4N4", terrain_with(&walls, &[]))
        .unwrap();
    let ins = |world: &mut World, o: RoomObject| world.insert("W4N4", o).unwrap();
    ins(
        world,
        RoomObject::Controller(ControllerObj {
            id: 0,
            x: 24,
            y: 24,
            level: 4,
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
    );
    ins(
        world,
        RoomObject::Spawn(SpawnObj {
            notify_when_attacked: None,
            id: 0,
            x: 22,
            y: 22,
            hits: 5000,
            off: false,
            name: "Spawn1".into(),
            owner: "p1".into(),
            store: Store {
                energy: 300,
                ghodium: 0,
            },
            spawning: None,
        }),
    );
    for (x, y) in [
        (21usize, 21usize),
        (23, 21),
        (21, 23),
        (23, 23),
        (20, 22),
        (22, 20),
    ] {
        ins(
            world,
            RoomObject::Extension(ExtensionObj {
                id: 0,
                x: x as u8,
                y: y as u8,
                hits: 1000,
                off: false,
                owner: "p1".into(),
                store: Store {
                    energy: 50,
                    ghodium: 0,
                },
                notify_when_attacked: None,
            }),
        );
    }
    // 边缘散点：掉落能量 ×3 + 墓碑（出口通道旁）
    for (x, y, amount) in [(25usize, 2usize, 300u32), (2, 25, 180), (47, 26, 90)] {
        ins(
            world,
            RoomObject::Energy(EnergyObj {
                id: 0,
                x: x as u8,
                y: y as u8,
                resource_type: 0,
                amount,
                resource: None,
            }),
        );
    }
    ins(
        world,
        RoomObject::Tombstone(TombstoneObj {
            id: 0,
            x: 26,
            y: 25,
            death_time: 0,
            decay_time: 90,
            creep_id: None,
            creep_name: Some("fallen".into()),
            creep_ticks_to_live: 800,
            creep_body: vec![BodyKind::Move.code(), BodyKind::Work.code()],
            creep_saying: None,
            owner: Some("p1".into()),
            store: Store {
                energy: 120,
                ghodium: 0,
            },
            store_extra: Vec::new(),
        }),
    );
    for (i, (x, y, kinds)) in [
        (20usize, 25usize, vec![BodyKind::Move, BodyKind::Work]),
        (25, 20, vec![BodyKind::Move, BodyKind::Carry]),
        (23, 26, vec![BodyKind::Attack, BodyKind::Move]),
        (
            26,
            23,
            vec![BodyKind::RangedAttack, BodyKind::Heal, BodyKind::Move],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        ins(
            world,
            creep(
                &format!("edge{i}"),
                if i == 2 { "p2" } else { "p1" },
                x as u8,
                y as u8,
                &kinds,
            ),
        );
    }
    BTreeMap::new()
}

/// 单夹具导出为 JSONL 行（tick 0 初始态 + 每 tick 推进后各一条）。
/// IO 与渲染分离：确定性测试直接对比本函数两次输出。
fn render_fixture(
    world: World,
    handles: BTreeMap<String, ObjId>,
    script: Option<IntentScript>,
    ticks: u32,
) -> Vec<String> {
    let mut engine = Engine::new(world);
    let mut lines = Vec::with_capacity(ticks as usize + 1);
    let mut emit = |tick: u32, world: &World| {
        let mut line = String::new();
        // serde_json::to_string 写 Value（确定序），String::from_utf8 恒成功
        let json = world.world_json().to_string();
        let _ = write!(line, r#"{{"tick":{tick},"world":{json}}}"#);
        lines.push(line);
    };
    emit(0, engine.world());
    for t in 1..=ticks {
        let batch = script.map(|f| f(t, &handles)).unwrap_or_default();
        engine
            .tick_with_intents_with_input(
                batch,
                TickInput {
                    now_ms: FIXED_NOW_MS,
                },
            )
            .expect("夹具世界结算不应失败（空意图或静态脚本）");
        emit(t, engine.world());
    }
    lines
}

/// 导出全部夹具到目录（每夹具一个 `<名>.jsonl`）。返回成功写出的文件数。
pub fn run(out_dir: &str, ticks: u32) -> i32 {
    std::fs::create_dir_all(out_dir).unwrap_or_else(|e| panic!("创建输出目录 {out_dir} 失败: {e}"));
    for spec in FIXTURES {
        let mut world = World::new(spec.seed);
        let handles = (spec.build)(&mut world);
        let lines = render_fixture(world, handles, spec.script, ticks);
        let path = Path::new(out_dir).join(format!("{}.jsonl", spec.name));
        std::fs::write(&path, lines.join("\n") + "\n")
            .unwrap_or_else(|e| panic!("写入 {} 失败: {e}", path.display()));
        println!("✓ {} ({} ticks)", path.display(), ticks);
    }
    println!("导出完成：{} 夹具 × {}+1 tick 快照", FIXTURES.len(), ticks);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性（方案 §9）：同 seed 同 TickInput 两次导出逐字节一致。
    #[test]
    fn 导出确定性_两次逐字节一致() {
        for spec in FIXTURES {
            let build_world = || {
                let mut w = World::new(spec.seed);
                let handles = (spec.build)(&mut w);
                (w, handles)
            };
            let (wa, ha) = build_world();
            let (wb, hb) = build_world();
            let la = render_fixture(wa, ha, spec.script, 5);
            let lb = render_fixture(wb, hb, spec.script, 5);
            assert_eq!(la, lb, "夹具 {} 两次导出应逐字节一致", spec.name);
            assert_eq!(la.len(), 6, "夹具 {} 应为 tick0+5 tick 共 6 条", spec.name);
        }
    }

    /// 时间序：world_json.time 随 tick 单调推进（回放链的时间轴依据）。
    #[test]
    fn 导出时间序单调推进() {
        let mut world = World::new(1);
        let handles = build_plains(&mut world);
        let lines = render_fixture(world, handles, None, 3);
        let times: Vec<u64> = lines
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["world"]["time"].as_u64().unwrap()
            })
            .collect();
        assert_eq!(times, vec![0, 1, 2, 3], "time 应逐 tick +1");
    }

    #[test]
    fn actions_真实事件及完工目标坐标() {
        let mut world = World::new(6);
        let handles = build_actions(&mut world);
        let site = format!("kk{:016x}", handles["site"].0);
        let lines = render_fixture(world, handles, Some(action_tick_script), 1);
        let v: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        let events = v["world"]["rooms"]["W6N6"]["events"].as_array().unwrap();
        for code in [1, 4, 5, 6, 7, 9] {
            assert!(events.iter().any(|e| e["event"] == code), "缺少事件 {code}");
        }
        let build = events.iter().find(|e| e["event"] == 4).unwrap();
        assert_eq!(build["data"]["x"], 14);
        assert_eq!(build["data"]["y"], 17);
        assert_eq!(build["data"]["incomplete"], false);
        assert!(
            v["world"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|o| o["id"] != site)
        );
        let heal = events.iter().find(|e| e["event"] == 6).unwrap();
        assert_eq!(heal["data"]["healType"], 2);
    }

    /// R4 运动夹具行为面：意图注入后走位者位移、victimA 死亡移除、
    /// 攻击/治疗事件入投影——渲染端运动/受击罩/死亡淡出的数据依据。
    #[test]
    fn creeps_夹具_位移_死亡_事件() {
        let mut world = World::new(5);
        let handles = build_creeps(&mut world);
        let lines = render_fixture(world, handles, Some(creep_tick_script), 10);
        let parse = |i: usize| -> serde_json::Value { serde_json::from_str(&lines[i]).unwrap() };
        let obj_at = |v: &serde_json::Value, name: &str| -> Option<serde_json::Value> {
            v["world"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["name"] == name)
                .cloned()
        };
        // 走位者 tick1 各进一格（walker1 方向 5=下、walker2 方向 7=左——
        // phase=tick%4 的首步）
        let w0 = obj_at(&parse(0), "walker1").unwrap();
        let w1 = obj_at(&parse(1), "walker1").unwrap();
        assert_eq!((w0["x"].as_i64(), w0["y"].as_i64()), (Some(10), Some(10)));
        assert_eq!((w1["x"].as_i64(), w1["y"].as_i64()), (Some(10), Some(11)));
        let v2 = obj_at(&parse(1), "walker2").unwrap();
        assert_eq!((v2["x"].as_i64(), v2["y"].as_i64()), (Some(13), Some(10)));
        // victimA tick0 在场、前 5 tick 内死亡移除（30×4≥100 hits）
        assert!(obj_at(&parse(0), "victimA").is_some());
        let died = (1..=10)
            .find(|t| obj_at(&parse(*t), "victimA").is_none())
            .unwrap_or_else(|| panic!("victimA 应在夹具期间死亡移除"));
        assert!(died <= 5, "victimA 应在前 5 tick 内死亡，实际 tick {died}");
        // 攻击/治疗事件进投影（EVENT_ATTACK=1 / EVENT_HEAL=6）
        let has_event = |v: &serde_json::Value, code: i64| {
            v["world"]["rooms"]["W5N5"]["events"]
                .as_array()
                .map(|evs| evs.iter().any(|e| e["event"].as_i64() == Some(code)))
                .unwrap_or(false)
        };
        assert!(has_event(&parse(1), 1), "tick1 应有 EVENT_ATTACK");
        assert!(has_event(&parse(1), 6), "tick1 应有 EVENT_HEAL");
    }
}
