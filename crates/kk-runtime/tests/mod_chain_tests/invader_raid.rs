//! Invader raid 生成器：资格门、出口资格、官方组合金标、预算累计与冷恢复。
//! 行为锚点：backend-npm/lib/cronjobs.js 的 genInvaders 与套件
//! 14.6-invader-raid-spawning（INVADER-RAID-001..010）。
use super::*;
use kk_core::constants as c;
use kk_world::domain::npc::INVADER_OWNER;

const ROOM: &str = "W1N1";
const CORE_ROOM: &str = "W1N2";
/// 套件 ONE_CREEP_RAID_RANDOM：方向 0、无升级、落格 0、boost 0、名字 0.001。
const ONE_CREEP_RANDOM: &[f64] = &[0.0, 0.1, 0.0, 0.0, 0.001, 0.5, 0.5];

/// 套件 edgeTerrain：全房平原、四边墙、仅开放格可作为出口。
fn edge_terrain(open: &[(usize, usize)]) -> Vec<u8> {
    let mut terrain = vec![0u8; 2500];
    for i in 0..50 {
        terrain[i] = kk_core::TERRAIN_MASK_WALL;
        terrain[49 * 50 + i] = kk_core::TERRAIN_MASK_WALL;
        terrain[i * 50] = kk_core::TERRAIN_MASK_WALL;
        terrain[i * 50 + 49] = kk_core::TERRAIN_MASK_WALL;
    }
    for &(x, y) in open {
        terrain[y * 50 + x] = 0;
    }
    terrain
}

pub(super) struct RaidSetup<'a> {
    room: &'a str,
    core_room: &'a str,
    core_level: Option<i64>,
    exit_tiles: &'a [(usize, usize)],
    owner: Option<&'a str>,
    rcl: i64,
    extra_rooms: Vec<Value>,
    source: bool,
    state: Value,
}

impl Default for RaidSetup<'_> {
    fn default() -> Self {
        Self {
            room: ROOM,
            core_room: CORE_ROOM,
            core_level: Some(1),
            exit_tiles: &[(25, 0)],
            owner: None,
            rcl: 1,
            extra_rooms: Vec::new(),
            source: true,
            state: json!({ "raidGoal": 1 }),
        }
    }
}

pub(super) fn setup_raid_room(e: &mut EmbedEngine, opts: RaidSetup) -> anyhow::Result<()> {
    let mut rooms = vec![json!({
        "name": opts.room,
        "terrain": edge_terrain(opts.exit_tiles),
    })];
    if let Some(owner) = opts.owner {
        rooms[0]["owner"] = json!(owner);
        rooms[0]["rcl"] = json!(opts.rcl);
    }
    // extra_rooms 与 core_room 同名时以 extra 定义为准（套件 Map.set 覆盖语义）。
    if opts.core_level.is_some()
        && !opts
            .extra_rooms
            .iter()
            .any(|room| room["name"] == json!(opts.core_room))
    {
        rooms.push(json!({"name": opts.core_room}));
    }
    rooms.extend(opts.extra_rooms.iter().cloned());
    e.create_shard(&json!({"players": ["p1", "p2"], "rooms": rooms}))?;
    if opts.source {
        e.place_object(
            opts.room,
            "structure",
            &json!({"structureType": "source", "pos": [25, 25], "energy": 3000, "energyCapacity": 3000}),
        )?;
    }
    if let Some(level) = opts.core_level {
        e.place_object(
            opts.core_room,
            "invaderCore",
            &json!({"pos": [25, 25], "level": level}),
        )?;
    }
    e.set_invader_raid_state(opts.room, &opts.state)?;
    Ok(())
}

fn invader_creeps(e: &EmbedEngine, room: &str) -> Vec<Value> {
    let mut creeps: Vec<Value> = e
        .find_in_room(room, c::FIND_CREEPS)
        .into_iter()
        .filter(|creep| creep["owner"] == json!(INVADER_OWNER))
        .collect();
    creeps.sort_by_key(|creep| {
        (
            creep["pos"]["y"].as_i64().unwrap_or(0),
            creep["pos"]["x"].as_i64().unwrap_or(0),
        )
    });
    creeps
}

fn expect_creep_basics(creeps: &[Value]) {
    for creep in creeps {
        assert_eq!(creep["owner"], json!(INVADER_OWNER));
        // 官方 cron 不写 ageTime，首拍 creep tick 补设（creeps/tick.js:88）；
        // kk 同构：age_time=0 快照面 ticksToLive 为 null。
        assert_eq!(creep["ticksToLive"], Value::Null);
        let (x, y) = (
            creep["pos"]["x"].as_i64().unwrap(),
            creep["pos"]["y"].as_i64().unwrap(),
        );
        assert!(
            x == 0 || x == 49 || y == 0 || y == 49,
            "落在出口格: {x},{y}"
        );
    }
}

/// INVADER-RAID-001/002/004/005/006/007 的抑制门与放行对照。
#[test]
fn raid_spawner_suppression_gates() -> anyhow::Result<()> {
    struct Case {
        label: &'static str,
        setup: fn(&mut EmbedEngine) -> anyhow::Result<()>,
        expect_spawn: bool,
    }
    let cases = [
        Case {
            label: "扇区无 level>0 核心（远房核心不同扇区）",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        core_room: "W11N11",
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "level 0 核心不满足扇区前置",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        core_level: Some(0),
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "默认阈值差 1 不生成",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        state: json!({"raidGoal": null, "harvestedEnergy": c::INVADERS_ENERGY_GOAL - 1}),
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "已有 Invader creep 抑制新 raid",
            setup: |e| {
                setup_raid_room(e, RaidSetup::default())?;
                e.place_object(
                    ROOM,
                    "creep",
                    &json!({"pos": [10, 10], "owner": INVADER_OWNER, "body": ["move"], "name": "existing"}),
                )?;
                e.set_invader_raid_state(ROOM, &json!({"active": false}))
            },
            // 套件 INVADER-RAID-004：既有 Invader 保留、无新增，总数 1。
            expect_spawn: true,
        },
        Case {
            label: "非 normal 房态抑制",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        state: json!({"raidGoal": 1, "status": "closed"}),
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "活跃房抑制",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        state: json!({"raidGoal": 1, "active": true}),
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "全墙边界无合格出口",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        exit_tiles: &[],
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "邻房有主控制器封锁该出口",
            setup: |e| {
                setup_raid_room(
                    e,
                    RaidSetup {
                        extra_rooms: vec![json!({"name": CORE_ROOM, "owner": "p2", "rcl": 1})],
                        ..Default::default()
                    },
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "邻房预定控制器封锁该出口",
            setup: |e| {
                setup_raid_room(e, RaidSetup::default())?;
                e.set_invader_raid_state(
                    CORE_ROOM,
                    &json!({"controllerReservation": {"owner": "p2", "ticksToEnd": 5000}}),
                )
            },
            expect_spawn: false,
        },
        Case {
            label: "同扇区 level 1 核心放行",
            setup: |e| setup_raid_room(e, RaidSetup::default()),
            expect_spawn: true,
        },
    ];
    for case in cases {
        for binary in [false, true] {
            let mut e = EmbedEngine::new();
            e.set_kkpv_channel(binary);
            (case.setup)(&mut e)?;
            e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
            let creeps = invader_creeps(&e, ROOM);
            assert_eq!(
                creeps.len(),
                usize::from(case.expect_spawn),
                "{}（binary={binary}）",
                case.label
            );
            if case.expect_spawn {
                // 基础断言只针对生成器产物（既有放置 creep 的 TTL 是放置
                // 默认 1500，不走 cron 无 ageTime 形态）。
                let generated: Vec<Value> = creeps
                    .iter()
                    .filter(|creep| {
                        creep["name"]
                            .as_str()
                            .is_some_and(|name| name.starts_with("invader_"))
                    })
                    .cloned()
                    .collect();
                expect_creep_basics(&generated);
                if case.label.contains("已有 Invader") {
                    assert_eq!(generated.len(), 0, "无新生成");
                    assert_eq!(creeps[0]["name"], json!("existing"), "保留的是既有 creep");
                }
            }
        }
    }
    Ok(())
}

/// INVADER-RAID-002/003/008：阈值达标、goal=1 旁路、单出口精确落格。
#[test]
fn raid_spawner_threshold_bypass_and_exact_placement() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(
        &mut e,
        RaidSetup {
            state: json!({"raidGoal": null, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
            exit_tiles: &[(17, 0)],
            ..Default::default()
        },
    )?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    let creeps = invader_creeps(&e, ROOM);
    assert_eq!(creeps.len(), 1);
    expect_creep_basics(&creeps);
    assert_eq!(
        creeps[0]["pos"],
        json!({"x": 17, "y": 0, "roomName": ROOM}),
        "唯一出口格精确落位"
    );

    // goal=1 旁路：零预算也生成（换引擎避免阈值残留）。
    let mut e = EmbedEngine::new();
    setup_raid_room(
        &mut e,
        RaidSetup {
            state: json!({"raidGoal": 1, "harvestedEnergy": 0}),
            ..Default::default()
        },
    )?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    assert_eq!(invader_creeps(&e, ROOM).len(), 1, "goal=1 旁路阈值");
    Ok(())
}

/// INVADER-RAID-009 组合金标：随机流、身体、boost 档与落位逐例对官方矩阵。
#[test]
fn raid_spawner_composition_matches_official_matrix() -> anyhow::Result<()> {
    struct Expected {
        body: &'static str,
        boosts: Option<bool>, // Some(true) 中心 X 档，Some(false) 基础矿，None 不强化
    }
    struct Case {
        label: &'static str,
        room: &'static str,
        core_room: &'static str,
        owner: Option<&'static str>,
        rcl: i64,
        exit_tiles: &'static [(usize, usize)],
        random: &'static [f64],
        expected: &'static [Expected],
    }
    let boost_of = |part: &str, center: bool| -> Option<&'static str> {
        kk_world::domain::invader_raid::raid_boost(part, center)
    };
    let cases = [
        Case {
            label: "非中心无升级：boost 小 Melee",
            room: "W1N1",
            core_room: "W1N2",
            owner: None,
            rcl: 1,
            exit_tiles: &[(25, 0)],
            random: &[0.0, 0.1, 0.0, 0.0, 0.101, 0.5, 0.5],
            expected: &[Expected {
                body: "smallMelee",
                boosts: Some(false),
            }],
        },
        Case {
            label: "有主 RCL3 仍用小身体",
            room: "W1N1",
            core_room: "W1N2",
            owner: Some("p1"),
            rcl: 3,
            exit_tiles: &[(25, 0)],
            random: &[0.0, 0.1, 0.0, 0.0, 0.111, 0.5, 0.5],
            expected: &[Expected {
                body: "smallMelee",
                boosts: Some(false),
            }],
        },
        Case {
            label: "非中心嵌套：1 Ranged、2 Healer",
            room: "W1N1",
            core_room: "W1N2",
            owner: None,
            rcl: 1,
            exit_tiles: &[(20, 0), (25, 0), (30, 0)],
            random: &[
                0.0, 0.95, 0.95, 0.25, 0.0, 0.0, 0.121, 0.9, 0.0, 0.122, 0.0, 0.123, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "smallMelee",
                    boosts: Some(false),
                },
                Expected {
                    body: "smallRanged",
                    boosts: Some(false),
                },
                Expected {
                    body: "smallHealer",
                    boosts: Some(false),
                },
            ],
        },
        Case {
            label: "count=5：下标 2 掷 Ranged",
            room: "W1N1",
            core_room: "W1N2",
            owner: None,
            rcl: 1,
            exit_tiles: &[(15, 0), (20, 0), (25, 0), (30, 0), (35, 0)],
            random: &[
                0.0, 0.95, 0.95, 0.99, 0.0, 0.9, 0.131, 0.4, 0.9, 0.132, 0.9, 0.9, 0.133, 0.9,
                0.134, 0.9, 0.135, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "smallMelee",
                    boosts: None,
                },
                Expected {
                    body: "smallHealer",
                    boosts: None,
                },
                Expected {
                    body: "smallRanged",
                    boosts: None,
                },
                Expected {
                    body: "smallHealer",
                    boosts: None,
                },
                Expected {
                    body: "smallHealer",
                    boosts: None,
                },
            ],
        },
        Case {
            label: "中心小 raid：起手 Ranged、计数收敛到出口数",
            room: "W5N5",
            core_room: "W5N6",
            owner: None,
            rcl: 1,
            exit_tiles: &[(20, 0), (25, 0)],
            random: &[
                0.0, 0.0, 0.0, 0.99, 0.0, 0.0, 0.141, 0.9, 0.0, 0.142, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "smallRanged",
                    boosts: Some(true),
                },
                Expected {
                    body: "smallRanged",
                    boosts: Some(true),
                },
            ],
        },
        Case {
            label: "有主 RCL4 首档升级：大身体无强化",
            room: "W1N1",
            core_room: "W1N2",
            owner: Some("p1"),
            rcl: 4,
            exit_tiles: &[(20, 0), (25, 0)],
            random: &[
                0.0, 0.95, 0.1, 0.0, 0.0, 0.0, 0.151, 0.9, 0.0, 0.152, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "bigMelee",
                    boosts: None,
                },
                Expected {
                    body: "bigRanged",
                    boosts: None,
                },
            ],
        },
        Case {
            label: "中心有主 RCL4：大 Healer 可选、零强化概率",
            room: "W5N5",
            core_room: "W5N6",
            owner: Some("p1"),
            rcl: 4,
            exit_tiles: &[(20, 0), (25, 0)],
            random: &[
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.161, 0.1, 0.0, 0.162, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "bigRanged",
                    boosts: None,
                },
                Expected {
                    body: "bigHealer",
                    boosts: None,
                },
            ],
        },
        // 自派生案例（非套件矩阵）：钉 RCL8 大身体阶梯——boost 0.4、
        // count 掷骰 5 被出口数收敛到 2。
        Case {
            label: "有主 RCL8：count 收敛 + boost 0.4 阶梯",
            room: "W1N1",
            core_room: "W1N2",
            owner: Some("p1"),
            rcl: 8,
            exit_tiles: &[(20, 0), (25, 0)],
            random: &[
                0.0, 0.95, 0.95, 0.99, 0.0, 0.3, 0.171, 0.9, 0.9, 0.172, 0.5, 0.5,
            ],
            expected: &[
                Expected {
                    body: "bigMelee",
                    boosts: Some(false),
                },
                Expected {
                    body: "bigRanged",
                    boosts: None,
                },
            ],
        },
    ];
    for case in cases {
        for binary in [false, true] {
            let mut e = EmbedEngine::new();
            e.set_kkpv_channel(binary);
            setup_raid_room(
                &mut e,
                RaidSetup {
                    room: case.room,
                    core_room: case.core_room,
                    owner: case.owner,
                    rcl: case.rcl,
                    exit_tiles: case.exit_tiles,
                    ..Default::default()
                },
            )?;
            e.run_invader_raid_spawner(Some(case.random.to_vec()))?;
            let creeps = invader_creeps(&e, case.room);
            assert_eq!(
                creeps.len(),
                case.expected.len(),
                "{}（binary={binary}）",
                case.label
            );
            expect_creep_basics(&creeps);
            let expected_tiles: Vec<(i64, i64)> = case
                .exit_tiles
                .iter()
                .map(|&(x, y)| (x as i64, y as i64))
                .collect();
            let actual_tiles: Vec<(i64, i64)> = creeps
                .iter()
                .map(|creep| {
                    (
                        creep["pos"]["x"].as_i64().unwrap(),
                        creep["pos"]["y"].as_i64().unwrap(),
                    )
                })
                .collect();
            assert_eq!(actual_tiles, expected_tiles, "{} 落位", case.label);
            for (creep, expected) in creeps.iter().zip(case.expected) {
                let parts = kk_world::domain::invader_raid::raid_body(expected.body).unwrap();
                let body = creep["body"].as_array().unwrap();
                assert_eq!(body.len(), parts.len(), "{} {}", case.label, expected.body);
                for (part, want) in body.iter().zip(parts) {
                    assert_eq!(part["type"], json!(want), "{} 部件序", case.label);
                    let boost = expected
                        .boosts
                        .and_then(|center_boost| boost_of(want, center_boost))
                        .map(|b| json!(b))
                        .unwrap_or(Value::Null);
                    assert_eq!(part["boost"], boost, "{} {} 强化", case.label, want);
                    if boost.is_null() {
                        // move 部件即使整体强化也不带 boost 键（套件 publicBody 形态）。
                        assert!(part.get("boost").is_none() || part["boost"].is_null());
                    }
                }
                assert_eq!(
                    creep["hitsMax"],
                    json!(parts.len() * 100),
                    "{} hitsMax",
                    case.label
                );
            }
        }
    }
    Ok(())
}

/// INVADER-RAID-010：成功 raid 清预算、重掷阈值并置活跃；再扫描不生成。
#[test]
fn raid_spawner_resets_budget_and_activates_room() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(
        &mut e,
        RaidSetup {
            state: json!({"raidGoal": null, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
            ..Default::default()
        },
    )?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    assert_eq!(invader_creeps(&e, ROOM).len(), 1);
    // 清 creep、复原非活跃（vanilla 夹具同流程）。
    e.clear_invader_raid_creeps(ROOM)?;
    e.set_invader_raid_state(ROOM, &json!({"active": false}))?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    assert_eq!(
        invader_creeps(&e, ROOM).len(),
        0,
        "预算已清零，默认阈值拦截"
    );
    Ok(())
}

/// 采集预算从真实 harvest 累计：阈值内不生成，达标后生成。
#[test]
fn raid_budget_accumulates_from_real_harvest() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                // 核心放同扇区非邻房：玩家 tick 会让核心预定本房控制器
                //（官方 invader-core/tick 行为），邻房预定会封锁出口。
                core_room: "W2N1",
                state: json!({"raidGoal": 10}),
                ..Default::default()
            },
        )?;
        e.place_object(
            ROOM,
            "creep",
            &json!({"name": "h", "owner": "p1", "pos": [26, 25], "body": ["work", "carry", "move"], "ticksToLive": 2000}),
        )?;
        let source_id = e
            .find_in_room(ROOM, c::FIND_SOURCES)
            .into_iter()
            .next()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .expect("夹具必有 source");
        let harvest = format!("Game.creeps.h.harvest(Game.getObjectById('{source_id}'))");
        // WORK×1 每拍采 2：4 拍 8 < 10 不生成。
        for _ in 0..4 {
            let run = e.run_player("p1", &harvest)?;
            assert_eq!(run.expr["value"], json!(0), "harvest 成功");
        }
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 0, "预算 8 < 10");
        // 第 5 拍累计 10：达标生成。
        e.run_player("p1", &harvest)?;
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 1, "预算 10 达标");
    }
    Ok(())
}

/// raid 房态随完整检查点保存恢复：恢复的预定要继续封锁、恢复的预算要
/// 驱动生成（raid 域丢失则预算归零不生成），恢复引擎与直跑引擎同随机流
/// 生成等值世界且 raid 子树逐字等值。
#[test]
fn raid_state_survives_cold_restore() -> anyhow::Result<()> {
    let build = |binary: bool| -> anyhow::Result<EmbedEngine> {
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                state: json!({"raidGoal": null, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
                ..Default::default()
            },
        )?;
        // 邻房预定随检查点恢复：恢复后出口仍被封锁。
        e.set_invader_raid_state(
            CORE_ROOM,
            &json!({"controllerReservation": {"owner": "p2", "ticksToEnd": 5000}}),
        )?;
        Ok(e)
    };
    for binary in [false, true] {
        let mut early = build(binary)?;
        let saved = early.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        // 阶段一：恢复的预定继续封锁（不消耗随机值，不动房态）。
        restored.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(
            invader_creeps(&restored, ROOM).len(),
            0,
            "恢复后预定封锁仍生效"
        );
        // 阶段二：只解除预定、不重设任何 raid 键——生成必须由恢复的
        // harvested=GOAL（≥ 默认阈值）驱动；raid 域恢复丢失则预算归零不生成。
        restored.set_invader_raid_state(CORE_ROOM, &json!({"controllerReservation": null}))?;
        restored.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        let creeps = invader_creeps(&restored, ROOM);
        assert_eq!(creeps.len(), 1, "恢复的采集预算驱动生成");
        expect_creep_basics(&creeps);
        // 直跑引擎同流程等值（世界 + raid 子树——成功后的阈值重掷同流）。
        early.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        early.set_invader_raid_state(CORE_ROOM, &json!({"controllerReservation": null}))?;
        early.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(
            restored.world_snapshot(),
            early.world_snapshot(),
            "恢复引擎与直跑引擎等值"
        );
        let raid_of =
            |value: &Value| -> Value { value["engine"]["world"]["rooms"][ROOM]["raid"].clone() };
        assert_eq!(
            raid_of(&restored.checkpoint(Value::Null)?),
            raid_of(&early.checkpoint(Value::Null)?),
            "raid 子树逐字等值（阈值/预算/活跃）"
        );
    }
    Ok(())
}

/// 官方阈值重掷乘数怪癖：10% 概率乘 floor(r>0.5?2:0.5)，半量分支
/// floor(0.5)=0 把阈值**写成 0**——但读回侧 `goal || C` 的 JS 真值回退
/// 把存储 0 视同默认阈值（本意减半实际等效重置为默认）；翻倍分支把
/// 阈值真实抬高到 2×base。两条路径用同预算区分：预算恰为 GOAL 时
/// 归零分支（读回默认 100000）达标、翻倍分支（146000）拦截。
#[test]
fn raid_goal_reroll_multiplier_quirk() -> anyhow::Result<()> {
    // 尾段 [base=0.05, check=0.05, mult=0.4]：base=floor(73000)，
    // check <0.1 命中乘数、mult≤0.5 → ×0 → 存储 0。
    let zeroing = [0.0, 0.1, 0.0, 0.0, 0.001, 0.05, 0.05, 0.4];
    // 尾段 [base=0.05, check=0.05, mult=0.6] → 存储 73000×2=146000。
    let doubling = [0.0, 0.1, 0.0, 0.0, 0.001, 0.05, 0.05, 0.6];
    for binary in [false, true] {
        // 归零：第二次扫描预算恰为 GOAL 时，真值回退读默认 100000 达标。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                state: json!({"raidGoal": 1}),
                ..Default::default()
            },
        )?;
        e.run_invader_raid_spawner(Some(zeroing.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 1);
        e.clear_invader_raid_creeps(ROOM)?;
        e.set_invader_raid_state(
            ROOM,
            &json!({"active": false, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
        )?;
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(
            invader_creeps(&e, ROOM).len(),
            1,
            "存储 0 读回默认阈值，预算 GOAL 恰好达标"
        );
        // 翻倍：同预算被 146000 拦截（0 不会被当免门槛、146000 真实生效）。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                state: json!({"raidGoal": 1}),
                ..Default::default()
            },
        )?;
        e.run_invader_raid_spawner(Some(doubling.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 1);
        e.clear_invader_raid_creeps(ROOM)?;
        e.set_invader_raid_state(
            ROOM,
            &json!({"active": false, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
        )?;
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(
            invader_creeps(&e, ROOM).len(),
            0,
            "阈值 146000 拦住预算 GOAL"
        );
    }
    Ok(())
}

/// 成功 raid 置活跃：清 creep 且重灌预算后不动 active，仍不生成；
/// 显式复原非活跃后才生成。
#[test]
fn raid_success_activates_room() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(&mut e, RaidSetup::default())?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    e.clear_invader_raid_creeps(ROOM)?;
    // 预算灌到任何重掷阈值之上（重掷上限约 26 万），不动 active。
    e.set_invader_raid_state(ROOM, &json!({"harvestedEnergy": 1_000_000_000}))?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    assert_eq!(invader_creeps(&e, ROOM).len(), 0, "成功置活跃拦截后续扫描");
    e.set_invader_raid_state(ROOM, &json!({"active": false}))?;
    e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
    assert_eq!(invader_creeps(&e, ROOM).len(), 1, "复原非活跃后生成");
    Ok(())
}

/// 出口方向组序（top/bottom/left/right 插入序）与首格随机映射：
/// roll 0 选首组 top、roll 0.99 选次组 bottom；同组内 floor(r×len)
/// 的中间值选第二格。
#[test]
fn raid_exit_direction_order_and_first_square_mapping() -> anyhow::Result<()> {
    for binary in [false, true] {
        // 双向开放（top/bottom），roll 0 选 top、0.99 选 bottom。
        for (roll, expect_y) in [(0.0f64, 0i64), (0.99, 49)] {
            let mut e = EmbedEngine::new();
            e.set_kkpv_channel(binary);
            setup_raid_room(
                &mut e,
                RaidSetup {
                    exit_tiles: &[(25, 0), (25, 49)],
                    state: json!({"raidGoal": 1}),
                    ..Default::default()
                },
            )?;
            e.run_invader_raid_spawner(Some(vec![roll, 0.1, 0.0, 0.0, 0.001, 0.5, 0.5]))?;
            let creeps = invader_creeps(&e, ROOM);
            assert_eq!(creeps.len(), 1);
            assert_eq!(creeps[0]["pos"]["y"], json!(expect_y), "roll={roll}");
        }
        // 发现序：left 在 i=5 先开口、top 在 i=25 后开口——官方键插入序
        // 是 [left, top]，roll 0 选 left（固定方位序会错选 top）。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                exit_tiles: &[(0, 5), (25, 0)],
                state: json!({"raidGoal": 1}),
                ..Default::default()
            },
        )?;
        e.run_invader_raid_spawner(Some(vec![0.0, 0.1, 0.0, 0.0, 0.001, 0.5, 0.5]))?;
        let creeps = invader_creeps(&e, ROOM);
        assert_eq!(creeps.len(), 1);
        assert_eq!(creeps[0]["pos"]["x"], json!(0), "发现序首组是先开口的 left");
        assert_eq!(creeps[0]["pos"]["y"], json!(5));
        // 单向三格，首格 roll 0.6 → floor(1.8)=1 → 中格。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(
            &mut e,
            RaidSetup {
                exit_tiles: &[(20, 0), (25, 0), (30, 0)],
                state: json!({"raidGoal": 1}),
                ..Default::default()
            },
        )?;
        e.run_invader_raid_spawner(Some(vec![0.0, 0.1, 0.6, 0.0, 0.001, 0.5, 0.5]))?;
        let creeps = invader_creeps(&e, ROOM);
        assert_eq!(creeps.len(), 1);
        assert_eq!(creeps[0]["pos"]["x"], json!(25), "首格 roll 0.6 选第二格");
    }
    Ok(())
}

/// 种子命令部分更新语义：第二次种子未提供的键保持前值——raidGoal 1
/// 旁路与灌入的预算都必须穿过只改 active 的第二次种子继续生效。
#[test]
fn raid_seed_partial_update_keeps_unprovided_keys() -> anyhow::Result<()> {
    for binary in [false, true] {
        // goal=1 旁路穿过第二次种子。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(&mut e, RaidSetup::default())?;
        e.set_invader_raid_state(ROOM, &json!({"raidGoal": 1, "harvestedEnergy": 0}))?;
        e.set_invader_raid_state(ROOM, &json!({"active": false}))?;
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 1, "raidGoal=1 穿过部分更新");
        // 灌入的预算同样穿过（清掉 goal 覆盖后靠预算生成）。
        let mut e = EmbedEngine::new();
        e.set_kkpv_channel(binary);
        setup_raid_room(&mut e, RaidSetup::default())?;
        e.set_invader_raid_state(
            ROOM,
            &json!({"raidGoal": null, "harvestedEnergy": c::INVADERS_ENERGY_GOAL}),
        )?;
        e.set_invader_raid_state(ROOM, &json!({"active": false}))?;
        e.run_invader_raid_spawner(Some(ONE_CREEP_RANDOM.to_vec()))?;
        assert_eq!(invader_creeps(&e, ROOM).len(), 1, "灌入预算穿过部分更新");
    }
    Ok(())
}

/// 注入序列耗尽报错且世界保持原状；非法随机值整单拒绝。
#[test]
fn raid_spawner_rejects_exhausted_or_invalid_random() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    setup_raid_room(&mut e, RaidSetup::default())?;
    let before = e.world_snapshot();
    // 空序列：首次取值即耗尽。
    assert!(e.run_invader_raid_spawner(Some(Vec::new())).is_err());
    assert_eq!(e.world_snapshot(), before, "耗尽失败保留原世界");
    // 值域外拒绝。
    assert!(e.run_invader_raid_spawner(Some(vec![1.0])).is_err());
    assert!(e.run_invader_raid_spawner(Some(vec![-0.1])).is_err());
    assert!(e.run_invader_raid_spawner(Some(vec![f64::NAN])).is_err());
    // 未注入 random：种子派生流可用（生成 1 只，后续可复跑）。
    e.run_invader_raid_spawner(None)?;
    assert_eq!(invader_creeps(&e, ROOM).len(), 1, "种子流路径可用");
    Ok(())
}
