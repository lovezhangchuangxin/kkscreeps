//! 通过 Engine 验证核弹域：launchNuke 标记→全局跨房插入、落地冲击
//!（全房清扫、5×5 足迹、rampart 先吸收、控制器结算）。行为锚点：官方
//! processor/intents/nukers/launch-nuke.js、nukes/tick.js 与 screeps-ok 的
//! NUKE-LAUNCH/NUKE-IMPACT 用例族。

use kk_engine::Engine;
use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_schema::room::{ControllerObj, GenericObj, RoomObject};
use kk_world::ObjId;
use kk_world::types::BodyKind;
use serde_json::{Value, json};

fn terrain_plain() -> kk_core::Terrain {
    kk_core::Terrain::plains()
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

const NUKE_DAMAGE_0: i64 = 10_000_000;
const NUKE_DAMAGE_2: i64 = 5_000_000;

/// W1N1（发射房，p1）+ W2N1（目标房）双房世界。
struct NukeFixture {
    engine: Engine,
}

impl NukeFixture {
    fn new() -> Self {
        let mut world = kk_world::World::new(11);
        world.register_room("W1N1", terrain_plain()).unwrap();
        world.register_room("W2N1", terrain_plain()).unwrap();
        Self {
            engine: Engine::new(world),
        }
    }

    fn controller(&mut self, level: u8) -> ObjId {
        self.engine
            .insert_object(
                "W1N1",
                RoomObject::Controller(ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level,
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
            .unwrap()
    }

    /// nuker：容量按官方常量，store/cooldownTime 可定制。
    fn nuker(&mut self, store: Value, cooldown_time: Option<u64>) -> ObjId {
        let mut data = json!({
            "type": "structure", "structureType": "nuker", "owner": "p1",
            "hits": 1000, "hitsMax": 1000,
            "store": store,
            "storeCapacityResource": {"energy": 300000, "G": 5000},
        });
        if let Some(t) = cooldown_time {
            data["cooldownTime"] = json!(t);
        }
        self.generic("W1N1", 25, 25, data)
    }

    fn generic(&mut self, room: &str, x: u8, y: u8, data: Value) -> ObjId {
        self.engine
            .insert_object(
                room,
                RoomObject::Generic(GenericObj {
                    id: 0,
                    x,
                    y,
                    data: data.to_string(),
                }),
            )
            .unwrap()
    }

    /// 直插飞行核弹：landTime = 当前时间 + ttl。
    fn nuke_at(&mut self, room: &str, x: u8, y: u8, ttl: u64) -> ObjId {
        let land = self.engine.world().time() + ttl;
        self.generic(
            room,
            x,
            y,
            json!({"type": "nuke", "landTime": land, "launchRoomName": "W3N1"}),
        )
    }

    fn launch(&mut self, nuker: ObjId, target: &str, x: i64, y: i64) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            "p1",
            nuker,
            Action::LaunchNuke,
            IntentData {
                target_room_name: Some(target.into()),
                x: Some(x),
                y: Some(y),
                ..IntentData::bare()
            },
        );
        self.engine.tick_with_intents(batch).unwrap();
    }

    fn tick(&mut self) {
        self.engine
            .tick_with_intents(IntentBatch::default())
            .unwrap();
    }

    fn data_of(&self, id: ObjId) -> Value {
        match self.engine.world().get(id).map(|(_, o)| o) {
            Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
            other => panic!("Generic 不存在：{other:?}"),
        }
    }

    fn exists(&self, id: ObjId) -> bool {
        self.engine.world().get(id).is_some()
    }

    /// 指定房中的 nuke 对象（type=nuke 的 Generic）。
    fn nukes_in(&self, room: &str) -> Vec<(ObjId, Value)> {
        self.engine
            .world()
            .room(room)
            .unwrap()
            .objects
            .iter()
            .filter_map(|(id, o)| match o {
                RoomObject::Generic(g) => {
                    let v: Value = serde_json::from_str(&g.data).ok()?;
                    (v.get("type").and_then(Value::as_str) == Some("nuke")).then_some((*id, v))
                }
                _ => None,
            })
            .collect()
    }

    fn controller_id(&self) -> ObjId {
        self.engine
            .world()
            .room("W1N1")
            .unwrap()
            .objects
            .iter()
            .find_map(|(id, o)| matches!(o, RoomObject::Controller(_)).then_some(*id))
            .expect("测试世界必须有 controller")
    }
}

/// 发射全链：满库发射 → 清库、设绝对冷却、目标房出现 landTime 锚定的
/// nuke 对象（launch-nuke.js 尾段）。
#[test]
fn launch_consumes_stores_cools_and_inserts_cross_room() {
    let mut f = NukeFixture::new();
    f.controller(8);
    let nuker = f.nuker(json!({"energy": 300000, "G": 5000}), None);
    f.launch(nuker, "W2N1", 15, 20);

    let d = f.data_of(nuker);
    assert_eq!(d.pointer("/store/energy"), Some(&json!(0)));
    assert_eq!(d.pointer("/store/G"), Some(&json!(0)));
    assert_eq!(d["cooldownTime"], json!(100_000), "NUKER_COOLDOWN 绝对锚");
    assert!(d.get("launchNuke").is_none() || d["launchNuke"].is_null());

    let nukes = f.nukes_in("W2N1");
    assert_eq!(nukes.len(), 1, "目标房出现一枚核弹");
    let (nuke_id, v) = &nukes[0];
    assert_eq!(v["landTime"], json!(50_000), "NUKE_LAND_TIME 锚定发射 tick");
    assert_eq!(v["launchRoomName"], json!("W1N1"));
    let (x, y) = f.engine.world().get(*nuke_id).unwrap().1.pos();
    assert_eq!((x, y), (15, 20));
    // 发射房不出现核弹（只在目标房对象表）。
    assert_eq!(f.nukes_in("W1N1").len(), 0);
}

/// 发射静默拒绝族：冷却、库存不足（三形态）、RCL 配额、射程、非法房名、
/// 坐标越界——全部不写标记、不清库、目标房无核弹。
#[test]
fn launch_silent_rejections() {
    let full = json!({"energy": 300000, "G": 5000});
    struct Reject {
        label: &'static str,
        level: u8,
        store: Value,
        cooldown: Option<u64>,
        target: &'static str,
        x: i64,
        y: i64,
        // 发射前 store.energy 的期望值（拒后应保持不变）。
        energy: i64,
    }
    let cases = [
        Reject {
            label: "cooldown",
            level: 8,
            store: full.clone(),
            cooldown: Some(50),
            target: "W2N1",
            x: 25,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "no-store",
            level: 8,
            store: json!({}),
            cooldown: None,
            target: "W2N1",
            x: 25,
            y: 25,
            energy: 0,
        },
        Reject {
            label: "energy-only",
            level: 8,
            store: json!({"energy": 300000}),
            cooldown: None,
            target: "W2N1",
            x: 25,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "ghodium-only",
            level: 8,
            store: json!({"G": 5000}),
            cooldown: None,
            target: "W2N1",
            x: 25,
            y: 25,
            energy: 0,
        },
        Reject {
            label: "rcl7-quota",
            level: 7,
            store: full.clone(),
            cooldown: None,
            target: "W2N1",
            x: 25,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "out-of-range",
            level: 8,
            store: full.clone(),
            cooldown: None,
            target: "W12N1",
            x: 25,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "bad-room-name",
            level: 8,
            store: full.clone(),
            cooldown: None,
            target: "no-room",
            x: 25,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "x-out",
            level: 8,
            store: full.clone(),
            cooldown: None,
            target: "W2N1",
            x: 50,
            y: 25,
            energy: 300000,
        },
        Reject {
            label: "y-out",
            level: 8,
            store: full,
            cooldown: None,
            target: "W2N1",
            x: 25,
            y: -1,
            energy: 300000,
        },
    ];
    for c in &cases {
        let mut f = NukeFixture::new();
        f.controller(c.level);
        let nuker = f.nuker(c.store.clone(), c.cooldown);
        f.launch(nuker, c.target, c.x, c.y);
        let d = f.data_of(nuker);
        assert_eq!(
            d.pointer("/store/energy").unwrap_or(&json!(0)),
            &json!(c.energy),
            "{}: 能量不动",
            c.label
        );
        assert!(d.get("launchNuke").is_none(), "{}: 不写标记", c.label);
        assert_eq!(f.nukes_in("W2N1").len(), 0, "{}: 无核弹", c.label);
        if c.label == "ghodium-only" {
            assert_eq!(d.pointer("/store/G"), Some(&json!(5000)), "ghodium 保留");
        }
    }
}

/// 落地冲击全链（landTime-1 tick）：全房 creep 无墓碑死亡、四类临时对象
/// 清除、孵化任务裸清、5×5 足迹（中心 1e7、range2 5e6、range3 无伤）、
/// safeMode 立即结束并清冷却、upgradeBlocked 设置；landTime tick 移除核弹。
#[test]
fn nuke_impact_full_chain() {
    let mut f = NukeFixture::new();
    f.controller(8);
    // 远离爆心的 rampart+creep 同格：creep 仍死（rampart 不保护 creep）。
    let far_rampart = f.generic(
        "W1N1",
        10,
        10,
        json!({"type": "structure", "structureType": "rampart", "owner": "p1",
               "hits": 50_000_000, "hitsMax": 50_000_000, "nextDecayTime": 1_000_000}),
    );
    let creep = f
        .engine
        .insert_object(
            "W1N1",
            RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 10,
                y: 10,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 600,
                spawning: false,
                action_log: 0,
                name: "doomed".into(),
                owner: "p1".into(),
                body: body(&[BodyKind::Tough, BodyKind::Move]),
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // 孵化中 spawn（爆区外高耐久，存活并可观察任务被全房裸清）。
    let spawn = f
        .engine
        .insert_object(
            "W1N1",
            RoomObject::Spawn(kk_schema::room::SpawnObj {
                id: 0,
                x: 5,
                y: 25,
                hits: 5000,
                off: false,
                name: "S1".into(),
                owner: "p1".into(),
                store: Default::default(),
                spawning: Some(kk_schema::room::SpawningJob {
                    name: "chick".into(),
                    need_time: 3,
                    spawn_time: 10,
                    directions: vec![],
                }),
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    // 中心高耐久 rampart（读出中心伤害）、range2 墙、range3 墙。
    let center = f.generic(
        "W1N1",
        25,
        25,
        json!({"type": "structure", "structureType": "rampart", "owner": "p1",
               "hits": NUKE_DAMAGE_0 + 5_000_000, "hitsMax": NUKE_DAMAGE_0 + 5_000_000,
               "nextDecayTime": 1_000_000}),
    );
    let r2 = f.generic(
        "W1N1",
        27,
        25,
        json!({"type": "structure", "structureType": "constructedWall",
               "hits": NUKE_DAMAGE_2 + 1_000_000, "hitsMax": 3000000000i64}),
    );
    let r3 = f.generic(
        "W1N1",
        28,
        25,
        json!({"type": "structure", "structureType": "constructedWall",
               "hits": 1_000_000, "hitsMax": 3000000000i64}),
    );
    let tombstone = f
        .engine
        .insert_object(
            "W1N1",
            RoomObject::Tombstone(kk_schema::room::TombstoneObj {
                id: 0,
                x: 12,
                y: 12,
                death_time: 0,
                decay_time: 1_000_000,
                creep_id: None,
                creep_name: Some("old".into()),
                creep_ticks_to_live: 0,
                creep_body: vec![],
                creep_saying: None,
                owner: Some("p1".into()),
                store: Default::default(),
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let site = f
        .engine
        .insert_object(
            "W1N1",
            RoomObject::ConstructionSite(kk_schema::room::ConstructionSiteObj {
                id: 0,
                x: 13,
                y: 13,
                structure_type: "spawn".into(),
                progress: 0,
                progress_total: 100,
                owner: Some("p1".into()),
                name: None,
            }),
        )
        .unwrap();
    let dropped = f
        .engine
        .insert_object(
            "W1N1",
            RoomObject::Energy(kk_schema::room::EnergyObj {
                id: 0,
                x: 14,
                y: 14,
                resource_type: 0,
                amount: 100,
                resource: None,
            }),
        )
        .unwrap();
    // safeMode 激活 + 冷却占位：核弹落地立即结束并清冷却。
    let ctl = f.controller_id();
    f.engine
        .edit_object(ctl, |object| {
            if let RoomObject::Controller(t) = object {
                t.safe_mode = 500;
                t.safe_mode_cooldown = Some(900);
            }
            Ok(())
        })
        .unwrap();
    // landTime = time+2：两次空 tick 后落地（第二 tick 为 landTime-1）。
    f.nuke_at("W1N1", 25, 25, 2);
    f.tick();
    f.tick();

    assert!(!f.exists(creep), "creep 死亡");
    let room_objs: Vec<&RoomObject> = f
        .engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .collect();
    assert!(
        !room_objs
            .iter()
            .any(|o| matches!(o, RoomObject::Tombstone(_))),
        "房间无墓碑（核弹击杀 + 清扫）"
    );
    assert!(!f.exists(tombstone), "既有墓碑被清扫");
    assert!(!f.exists(site), "工地被清扫");
    assert!(!f.exists(dropped), "掉落资源被清扫");
    assert!(f.exists(spawn), "爆区外 spawn 存活");
    assert!(
        matches!(
            f.engine.world().get(spawn).map(|(_, o)| o),
            Some(RoomObject::Spawn(s)) if s.spawning.is_none()
        ),
        "孵化任务被全房裸清"
    );
    assert!(
        !room_objs.iter().any(|o| matches!(o, RoomObject::Ruin(_))),
        "核弹摧毁不建废墟"
    );
    assert_eq!(
        f.data_of(center).pointer("/hits"),
        Some(&json!(5_000_000)),
        "中心吃 NUKE_DAMAGE[0]"
    );
    assert_eq!(
        f.data_of(r2).pointer("/hits"),
        Some(&json!(1_000_000)),
        "range2 吃 NUKE_DAMAGE[2]"
    );
    assert_eq!(
        f.data_of(r3).pointer("/hits"),
        Some(&json!(1_000_000)),
        "range3 无伤"
    );
    // 远处 rampart 同格 creep 死亡但 rampart 自身在爆区外无伤。
    assert_eq!(
        f.data_of(far_rampart).pointer("/hits"),
        Some(&json!(50_000_000)),
        "爆区外 rampart 无伤"
    );
    match f.engine.world().get(ctl).map(|(_, o)| o) {
        Some(RoomObject::Controller(t)) => {
            assert_eq!(
                t.safe_mode, 1,
                "safeMode 立即结束（锚定 landTime-1 结算 tick）"
            );
            assert!(t.safe_mode_cooldown.is_none(), "冷却清空");
            assert_eq!(t.upgrade_blocked, Some(1 + 200), "核弹升级封锁");
        }
        other => panic!("控制器不存在：{other:?}"),
    }
    // landTime tick 移除核弹。
    f.tick();
    assert_eq!(f.nukes_in("W1N1").len(), 0, "落地后核弹移除");
}

/// rampart 击穿余量同格均摊：每对象各吃全量余量（RAMPART-PROTECT-010）。
#[test]
fn rampart_break_spreads_remainder_equally() {
    let mut f = NukeFixture::new();
    f.controller(8);
    let rampart = f.generic(
        "W1N1",
        25,
        25,
        json!({"type": "structure", "structureType": "rampart", "owner": "p1",
               "hits": NUKE_DAMAGE_0 - 1000, "hitsMax": NUKE_DAMAGE_0,
               "nextDecayTime": 1_000_000}),
    );
    let spawn = f.generic(
        "W1N1",
        25,
        25,
        json!({"type": "structure", "structureType": "spawn", "owner": "p1",
               "hits": 5000, "hitsMax": 5000, "store": {"energy": 0}}),
    );
    let tower = f.generic(
        "W1N1",
        25,
        25,
        json!({"type": "structure", "structureType": "tower", "owner": "p1",
               "hits": 3000, "hitsMax": 3000, "store": {"energy": 0}}),
    );
    f.nuke_at("W1N1", 25, 25, 1);
    f.tick();
    f.tick();
    assert!(!f.exists(rampart), "rampart 被击穿摧毁");
    assert_eq!(
        f.data_of(spawn).pointer("/hits"),
        Some(&json!(4000)),
        "spawn 吃 1000 余量"
    );
    assert_eq!(
        f.data_of(tower).pointer("/hits"),
        Some(&json!(2000)),
        "tower 吃 1000 余量"
    );
}

/// 多弹同 tick 叠加伤害（NUKE-IMPACT-012）。
#[test]
fn two_nukes_cumulative_damage() {
    let mut f = NukeFixture::new();
    f.controller(8);
    let wall = f.generic(
        "W1N1",
        25,
        25,
        json!({"type": "structure", "structureType": "constructedWall",
               "hits": NUKE_DAMAGE_0 * 2 + 1_000_000, "hitsMax": 3000000000i64}),
    );
    f.nuke_at("W1N1", 25, 25, 1);
    f.nuke_at("W1N1", 25, 25, 1);
    f.tick();
    f.tick();
    assert_eq!(
        f.data_of(wall).pointer("/hits"),
        Some(&json!(1_000_000)),
        "两弹各吃 1e7"
    );
}

/// 既有更晚的 upgradeBlocked 不被核弹覆盖（NUKE-IMPACT-011 官方条件
/// `!upgradeBlocked || upgradeBlocked < gameTime`）。
#[test]
fn upgrade_blocked_not_lowered() {
    let mut f = NukeFixture::new();
    f.controller(8);
    let ctl = f.controller_id();
    f.engine
        .edit_object(ctl, |object| {
            if let RoomObject::Controller(t) = object {
                t.upgrade_blocked = Some(1000);
            }
            Ok(())
        })
        .unwrap();
    f.nuke_at("W1N1", 25, 25, 1);
    f.tick();
    f.tick();
    match f.engine.world().get(ctl).map(|(_, o)| o) {
        Some(RoomObject::Controller(t)) => {
            assert_eq!(t.upgrade_blocked, Some(1000), "更晚封锁保持");
        }
        other => panic!("控制器不存在：{other:?}"),
    }
}

/// 射程含界正向：W1N1→W11N1 差恰 10（NUKE_RANGE）放行（官方 `>` 才拒）；
/// 顺带钉 powerCreep（Generic 承载）全房归零。
#[test]
fn range_boundary_launches_and_power_creep_zeroed() {
    let mut f = NukeFixture::new();
    f.controller(8);
    let nuker = f.nuker(json!({"energy": 300000, "G": 5000}), None);
    f.launch(nuker, "W11N1", 25, 25);
    assert_eq!(f.nukes_in("W2N1").len(), 0, "目标房是 W11N1");
    assert_eq!(
        f.data_of(nuker)["cooldownTime"],
        json!(100_000),
        "差恰 10 放行并进入冷却"
    );

    // powerCreep 全房归零（官方 tick.js:25-27，kk 以 Generic 承载）；
    // 归零当拍由全局死亡扫收尾（M2H.a）：对象移除并落墓碑。
    let pc = f.generic(
        "W1N1",
        5,
        5,
        json!({"type": "powerCreep", "name": "PC1", "owner": "p1", "hits": 1000}),
    );
    f.nuke_at("W1N1", 25, 25, 1);
    f.tick();
    assert!(!f.exists(pc), "powerCreep 归零当拍死亡移除");
    let tombstoned = f
        .engine
        .world()
        .room("W1N1")
        .expect("房间存在")
        .objects
        .values()
        .any(|o| matches!(o, RoomObject::Tombstone(t) if t.creep_name.as_deref() == Some("PC1")));
    assert!(tombstoned, "powerCreep 死亡落墓碑");
}

mod invulnerability;
