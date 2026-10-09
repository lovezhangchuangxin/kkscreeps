//! 通过 Engine 验证 power 域：账号注册表六全局意图、GPL 配额数学、
//! powerSpawn processPower、已生成侧 TTL/死亡与 usePower(PWR_GENERATE_OPS)
//! 的最小面。行为锚点：官方 processor/global-intents/power/*.js、
//! power-spawns/process-power.js、power-creeps/*.js 与 screeps-ok 的
//! GPL/POWERCREEP/POWER-GENERATE 用例族。

use kk_engine::Engine;
use kk_engine::intent::{Action, IntentBatch, IntentData};
use kk_engine::settle::PlayerState;
use kk_engine::settle::PowerCreepRecord;
use kk_engine::settle::powers::gpl_level;
use kk_schema::room::{ControllerObj, GenericObj, RoomObject};
use kk_world::ObjId;
use serde_json::{Value, json};

fn terrain_plain() -> kk_core::Terrain {
    kk_core::Terrain::plains()
}

/// W1N1（p1 主房）+ W2N1（p2）双房世界；玩家预置账号 power。
struct PowerFixture {
    engine: Engine,
    now_ms: u64,
}

impl PowerFixture {
    fn new(power: i64) -> Self {
        let mut world = kk_world::World::new(21);
        world.register_room("W1N1", terrain_plain()).unwrap();
        world.register_room("W2N1", terrain_plain()).unwrap();
        let mut f = Self {
            engine: Engine::new(world),
            now_ms: 1_700_000_000_000,
        };
        f.controller("W1N1", 8);
        f.controller("W2N1", 8);
        f.engine
            .edit_players(|players| {
                players.entry("p1".into()).or_default().power = power;
                players.entry("p2".into()).or_default().power = power;
                Ok(())
            })
            .unwrap();
        f
    }

    fn controller(&mut self, room: &str, level: u8) -> ObjId {
        self.engine
            .insert_object(
                room,
                RoomObject::Controller(ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level,
                    safe_mode: 0,
                    safe_mode_available: 0,
                    progress: 0,
                    downgrade_time: None,
                    owner: Some(room_owner(room).into()),
                    owner_cleared: false,
                    is_power_enabled: true,
                    reservation: None,
                    sign: None,
                    upgrade_blocked: None,
                    safe_mode_cooldown: None,
                }),
            )
            .unwrap()
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

    /// powerSpawn（RCL8 主房满库形态）。
    fn power_spawn(&mut self) -> ObjId {
        self.generic(
            "W1N1",
            20,
            20,
            json!({
                "type": "structure", "structureType": "powerSpawn", "owner": "p1",
                "hits": 5000, "hitsMax": 5000,
                "store": {"power": 100, "energy": 5000},
                "storeCapacityResource": {"power": 100, "energy": 5000},
            }),
        )
    }

    /// 直插已生成 powerCreep（放置层同形数据）。
    fn power_creep(&mut self, room: &str, x: u8, y: u8, name: &str, powers: Value) -> ObjId {
        let level = powers
            .as_object()
            .map(|p| {
                p.values()
                    .filter_map(|v| v.get("level").and_then(Value::as_i64))
                    .sum::<i64>()
            })
            .unwrap_or(0);
        self.generic(
            room,
            x,
            y,
            json!({
                "type": "powerCreep", "owner": room_owner(room), "name": name,
                "className": "operator", "level": level,
                "hits": 1000 + level * 1000, "hitsMax": 1000 + level * 1000,
                "store": {"ops": 10}, "storeCapacity": 100 + level * 100,
                "ageTime": self.engine.world().time() + 5000,
                "notifyWhenAttacked": true,
                "powers": powers,
            }),
        )
    }

    /// 同步放置层的账号记录（已生成态：spawnCooldownTime=null）。
    /// 放置夹具只建 W1N1(p1)/W2N1(p2) 两主。
    fn register_spawned(&mut self, room: &str, id: ObjId, name: &str) {
        let data = self.data_of(id);
        let level = data.get("level").and_then(Value::as_i64).unwrap_or(0);
        let powers = data
            .get("powers")
            .and_then(|p| p.as_object().cloned())
            .map(|map| {
                map.iter()
                    .filter_map(|(pid, e)| {
                        Some((pid.parse::<i64>().ok()?, e.get("level")?.as_i64()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let owner = room_owner(room).to_string();
        self.engine
            .edit_players(move |players| {
                players.entry(owner).or_default().power_creeps.insert(
                    id.0,
                    PowerCreepRecord {
                        name: name.into(),
                        class_name: "operator".into(),
                        level,
                        hits_max: 1000 + level * 1000,
                        store_capacity: 100 + level * 100,
                        powers,
                        spawn_cooldown_time: None,
                        delete_time: None,
                    },
                );
                Ok(())
            })
            .unwrap();
    }

    fn create(&mut self, player: &str, name: &str, class_name: &str) {
        let mut batch = IntentBatch::default();
        batch.users.entry(player.into()).or_default().power.push(
            kk_engine::intent::PowerIntent::CreatePowerCreep {
                name: name.into(),
                class_name: class_name.into(),
            },
        );
        self.run(batch);
    }

    fn spawn_at(&mut self, player: &str, power_spawn: ObjId, name: &str) {
        let mut batch = IntentBatch::default();
        batch.users.entry(player.into()).or_default().power.push(
            kk_engine::intent::PowerIntent::SpawnPowerCreep {
                power_spawn,
                name: name.into(),
            },
        );
        self.run(batch);
    }

    fn suicide(&mut self, player: &str, id: ObjId) {
        let mut batch = IntentBatch::default();
        batch
            .users
            .entry(player.into())
            .or_default()
            .power
            .push(kk_engine::intent::PowerIntent::SuicidePowerCreep { id });
        self.run(batch);
    }

    fn upgrade(&mut self, player: &str, id: ObjId, power: i64) {
        let mut batch = IntentBatch::default();
        batch
            .users
            .entry(player.into())
            .or_default()
            .power
            .push(kk_engine::intent::PowerIntent::UpgradePowerCreep { id, power });
        self.run(batch);
    }

    fn use_power(&mut self, player: &str, id: ObjId, power: i64) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            player,
            id,
            Action::UsePower,
            IntentData {
                power: Some(power),
                ..IntentData::bare()
            },
        );
        self.run(batch);
    }

    fn process_power(&mut self, player: &str, power_spawn: ObjId) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            player,
            power_spawn,
            Action::ProcessPower,
            IntentData::bare(),
        );
        self.run(batch);
    }

    fn run(&mut self, batch: IntentBatch) {
        self.engine
            .tick_with_intents_with_input(
                batch,
                kk_engine::settle::TickInput {
                    now_ms: self.now_ms,
                    warmup: false,
                },
            )
            .unwrap();
    }

    fn tick(&mut self) {
        self.run(IntentBatch::default());
    }

    fn record(&self, player: &str) -> &PlayerState {
        &self.engine.players()[player]
    }

    /// 房内首个控制器（new 时已放置）。
    fn controller_of(&self, room: &str) -> ObjId {
        self.engine
            .world()
            .room(room)
            .unwrap()
            .objects
            .iter()
            .find(|(_, o)| matches!(o, RoomObject::Controller(_)))
            .map(|(id, _)| *id)
            .unwrap()
    }

    /// 控制器布尔标志（None=控制器缺失或标志缺席）。
    fn controller_flag(&self, room: &str, key: &str) -> Option<bool> {
        let ctl = self.controller_of(room);
        match self.engine.world().get(ctl).map(|(_, o)| o) {
            Some(RoomObject::Controller(c)) => match key {
                "is_power_enabled" => Some(c.is_power_enabled),
                _ => None,
            },
            _ => None,
        }
    }

    fn enable_room(&mut self, player: &str, id: ObjId, controller: ObjId) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            player,
            id,
            Action::EnableRoom,
            IntentData {
                id: Some(controller),
                ..IntentData::bare()
            },
        );
        self.run(batch);
    }

    fn renew(&mut self, player: &str, id: ObjId, target: ObjId) {
        let mut batch = IntentBatch::default();
        batch.add_object_intent(
            player,
            id,
            Action::Renew,
            IntentData {
                id: Some(target),
                ..IntentData::bare()
            },
        );
        self.run(batch);
    }

    fn data_of(&self, id: ObjId) -> Value {
        match self.engine.world().get(id).map(|(_, o)| o) {
            Some(RoomObject::Generic(g)) => serde_json::from_str(&g.data).unwrap(),
            other => panic!("Generic 不存在：{other:?}"),
        }
    }
}

fn room_owner(room: &str) -> &str {
    if room == "W1N1" { "p1" } else { "p2" }
}

#[test]
fn gpl_math_matches_official() {
    // 官方 game.js:133/164：level=floor((power/1000)^(1/2))，pow=2。
    assert_eq!(gpl_level(0), 0);
    assert_eq!(gpl_level(999), 0);
    assert_eq!(gpl_level(1000), 1);
    assert_eq!(gpl_level(3999), 1);
    assert_eq!(gpl_level(4000), 2);
    assert_eq!(gpl_level(9000), 3);
    assert_eq!(gpl_level(16000), 4);
}

#[test]
fn create_respects_gpl_quota_and_counts_used_levels() {
    // used = 记录数 + 等级和（screeps-ok GPL-004/005 联合契约：create 每条
    // 占 1、upgrade 每级占 1，无额外计数项）。GPL3 容 3 create。
    let mut f = PowerFixture::new(9000);
    f.create("p1", "A", "operator");
    f.create("p1", "B", "operator");
    f.create("p1", "C", "operator");
    f.create("p1", "D", "operator"); // used=4 >= 3，静默拒绝
    let p = f.record("p1");
    assert_eq!(p.power_creeps.len(), 3);
    assert!(
        f.record("p1")
            .power_creeps
            .values()
            .all(|r| r.spawn_cooldown_time == Some(0))
    );

    // 类名白名单之外拒绝。
    let mut f2 = PowerFixture::new(1000);
    f2.create("p1", "X", "invalid_class");
    assert_eq!(f2.record("p1").power_creeps.len(), 0);
}

#[test]
fn rename_and_delete_gates() {
    let mut f = PowerFixture::new(1000);
    f.create("p1", "Base", "operator");
    let id = *f.record("p1").power_creeps.keys().next().unwrap();

    // 重名拒绝；改名截 50（官方与 xxscreeps 落库口径，预检 >100 拒）。
    f.create("p1", "Other", "operator");
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::RenamePowerCreep {
            id: ObjId(id),
            name: "Other".into(),
        },
    );
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::RenamePowerCreep {
            id: ObjId(id),
            name: "N".repeat(120),
        },
    );
    f.run(batch);
    assert_eq!(f.record("p1").power_creeps[&id].name, "N".repeat(50));

    // delete 排队 24h 墙钟；cancel 可撤；已排队幂等。
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::DeletePowerCreep {
            id: ObjId(id),
            cancel: false,
        },
    );
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::DeletePowerCreep {
            id: ObjId(id),
            cancel: false,
        },
    );
    f.run(batch);
    assert_eq!(
        f.record("p1").power_creeps[&id].delete_time,
        Some(f.now_ms + 86_400_000)
    );
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::DeletePowerCreep {
            id: ObjId(id),
            cancel: true,
        },
    );
    f.run(batch);
    assert_eq!(f.record("p1").power_creeps[&id].delete_time, None);
}

#[test]
fn upgrade_syncs_account_and_spawned_object() {
    // GPL3：create 占 2，一次 upgrade 后 used=3 占满。
    let mut f = PowerFixture::new(9000);
    f.create("p1", "U", "operator");
    let id = *f.record("p1").power_creeps.keys().next().unwrap();
    // 类不匹配拒绝。
    f.upgrade("p1", ObjId(id), 13); // PWR_REGEN_SOURCE 类匹配（operator）但等级门槛 [10,..]
    assert_eq!(f.record("p1").power_creeps[&id].level, 0, "等级门槛前拒绝");

    // 生成后升级：账号与房间对象同步（level/hitsMax/storeCapacity/powers）。
    let ps = f.power_spawn();
    f.spawn_at("p1", ps, "U");
    let room_id = *f.record("p1").power_creeps.keys().next().unwrap();
    assert!(
        f.engine.world().get(ObjId(room_id)).is_some(),
        "生成对象在场"
    );
    f.upgrade("p1", ObjId(room_id), 1); // PWR_GENERATE_OPS 门槛 [0,...]
    let rec = &f.record("p1").power_creeps[&room_id];
    assert_eq!(rec.level, 1);
    assert_eq!(rec.hits_max, 2000);
    assert_eq!(rec.store_capacity, 200);
    assert_eq!(rec.powers.get(&1), Some(&1));
    assert_eq!(f.data_of(ObjId(room_id))["level"], json!(1));
    assert_eq!(f.data_of(ObjId(room_id))["hitsMax"], json!(2000));
    assert_eq!(f.data_of(ObjId(room_id))["powers"]["1"]["level"], json!(1));
    // 配额：GPL3 已被占满（len1+level1+counter1=3），再升拒绝。
    f.upgrade("p1", ObjId(room_id), 1);
    assert_eq!(f.record("p1").power_creeps[&room_id].level, 1);
}

#[test]
fn spawn_gates_and_object_shape() {
    // GPL3：两条 create（S/T）恰容；占位测试需第二条记录存在。
    let mut f = PowerFixture::new(9000);
    f.create("p1", "S", "operator");
    // 无 powerSpawn：静默。
    f.spawn_at("p1", ObjId(9999), "S");
    assert_eq!(f.record("p1").power_creeps.len(), 1);
    assert_eq!(
        f.record("p1")
            .power_creeps
            .values()
            .next()
            .unwrap()
            .spawn_cooldown_time,
        Some(0)
    );

    let ps = f.power_spawn();
    // 敌方 powerSpawn：p2 名义下的意图拒绝（归属门）。
    f.spawn_at("p2", ps, "S");
    assert_eq!(
        f.record("p1")
            .power_creeps
            .values()
            .next()
            .unwrap()
            .spawn_cooldown_time,
        Some(0)
    );

    f.spawn_at("p1", ps, "S");
    let (id, rec) = f
        .record("p1")
        .power_creeps
        .iter()
        .next()
        .map(|(k, v)| (*k, v.clone()))
        .unwrap();
    assert_eq!(rec.spawn_cooldown_time, None, "生成态置 null");
    assert_eq!(rec.delete_time, None);
    let data = f.data_of(ObjId(id));
    assert_eq!(data["type"], json!("powerCreep"));
    let placed = f
        .engine
        .world()
        .get(ObjId(id))
        .map(|(_, o)| o)
        .cloned()
        .unwrap();
    let (px, py) = placed.pos();
    assert_eq!((px, py), (20, 20), "位置在 Generic 信封");
    assert_eq!(
        data["ageTime"],
        json!(f.engine.world().time() + 4999),
        "生成 tick 推进后"
    );
    assert_eq!(data["hits"], json!(1000));
    assert_eq!(data["store"], json!({}));

    // 落位格占用：同格已生成时不再生成第二胎。
    f.create("p1", "T", "operator");
    f.spawn_at("p1", ps, "T");
    let still_unspawned = f
        .record("p1")
        .power_creeps
        .values()
        .find(|r| r.name == "T")
        .map(|r| r.spawn_cooldown_time)
        .unwrap();
    assert!(still_unspawned.is_some(), "占位拒绝");
}

#[test]
fn ttl_death_creates_tombstone_and_cooldown() {
    let mut f = PowerFixture::new(1000);
    let pc = f.power_creep("W1N1", 25, 25, "Doomed", json!({"1": {"level": 1}}));
    f.register_spawned("W1N1", pc, "Doomed");
    // ageTime = 放置时 time+5000；直接把 ageTime 提前到下一 tick。
    let next = f.engine.world().time() + 1;
    f.engine
        .edit_object(pc, |o| {
            if let RoomObject::Generic(g) = o {
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["ageTime"] = json!(next);
                g.data = v.to_string();
            }
            Ok(())
        })
        .unwrap();
    f.tick();
    assert!(f.engine.world().get(pc).is_none(), "TTL 死亡移除");
    let tomb = f
        .engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Tombstone(t) if t.creep_name.as_deref() == Some("Doomed") => {
                Some(t.clone())
            }
            _ => None,
        })
        .expect("墓碑在场");
    assert_eq!(tomb.death_time, f.engine.world().time() - 1);
    assert_eq!(tomb.store_extra[0].resource, "ops");
    assert_eq!(tomb.store_extra[0].amount, 10);
    let rec = f.record("p1").power_creeps[&pc.0].clone();
    assert_eq!(rec.spawn_cooldown_time, Some(f.now_ms + 28_800_000));
}

#[test]
fn suicide_kills_spawned_creep() {
    let mut f = PowerFixture::new(1000);
    let pc = f.power_creep("W1N1", 25, 25, "Bye", json!({}));
    f.register_spawned("W1N1", pc, "Bye");
    f.suicide("p1", pc);
    assert!(f.engine.world().get(pc).is_none());
    assert_eq!(
        f.record("p1").power_creeps[&pc.0].spawn_cooldown_time,
        Some(f.now_ms + 28_800_000)
    );
    // 敌方自杀意图拒绝（归属门）。
    let pc2 = f.power_creep("W1N1", 26, 25, "Mine", json!({}));
    f.register_spawned("W1N1", pc2, "Mine");
    f.suicide("p2", pc2);
    assert!(f.engine.world().get(pc2).is_some());
}

#[test]
fn process_power_double_deducts_and_accumulates_account() {
    let mut f = PowerFixture::new(0);
    let ps = f.power_spawn();
    f.process_power("p1", ps);
    assert_eq!(f.data_of(ps)["store"]["power"], json!(99));
    assert_eq!(f.data_of(ps)["store"]["energy"], json!(4950));
    assert_eq!(f.record("p1").power, 1, "账号 power 累计");

    // 库存不足静默拒绝（power 归零后）。
    f.engine
        .edit_object(ps, |o| {
            if let RoomObject::Generic(g) = o {
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["power"] = json!(0);
                g.data = v.to_string();
            }
            Ok(())
        })
        .unwrap();
    f.process_power("p1", ps);
    assert_eq!(f.record("p1").power, 1);
}

#[test]
fn generate_ops_effect_and_overflow_drop() {
    let mut f = PowerFixture::new(1000);
    // 等级 1：容量 200，初始 0 ops → 生成 1 ops 无溢出。
    let pc = f.power_creep("W1N1", 25, 25, "Gen", json!({"1": {"level": 1}}));
    f.register_spawned("W1N1", pc, "Gen");
    f.use_power("p1", pc, 1);
    assert_eq!(f.data_of(pc)["store"]["ops"], json!(11), "10 初始 + 1 生成");
    assert_eq!(
        f.data_of(pc)["powers"]["1"]["cooldownTime"],
        json!(f.engine.world().time() - 1 + 50)
    );

    // 溢出：等级 5 容量 600，余 3 → 生成 8，掉 5。
    let mut f2 = PowerFixture::new(1000);
    let pc2 = f2.power_creep("W1N1", 25, 25, "Full", json!({"1": {"level": 5}}));
    f2.engine
        .edit_object(pc2, |o| {
            if let RoomObject::Generic(g) = o {
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["store"]["ops"] = json!(597);
                g.data = v.to_string();
            }
            Ok(())
        })
        .unwrap();
    f2.register_spawned("W1N1", pc2, "Full");
    f2.use_power("p1", pc2, 1);
    assert_eq!(f2.data_of(pc2)["store"]["ops"], json!(600), "钳容量");
    let pile = f2
        .engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .values()
        .find_map(|o| match o {
            RoomObject::Energy(e) => Some((e.resource.clone(), e.amount)),
            _ => None,
        })
        .expect("溢出掉落");
    // 掉落堆同拍按 ENERGY_DECAY 衰减一次（GENERATE-003 同款）：5−1=4。
    assert_eq!(pile, (Some("ops".into()), 4));
}

#[test]
fn checkpoint_backfills_power_fields() {
    // 旧检查点（M2H.a 前）不含 power/power_creeps 键；
    // deny_unknown_fields 只拒未知键，serde(default) 按缺省回填。
    // 构造 M2H.a 之前的真实形态：默认态序列化后剥离两个新键。
    let mut legacy = serde_json::to_value(PlayerState::default()).unwrap();
    {
        let map = legacy.as_object_mut().unwrap();
        for key in ["power", "power_creeps"] {
            map.remove(key);
        }
    }
    let state: PlayerState = serde_json::from_value(legacy).expect("旧检查点可载入");
    assert_eq!(state.power, 0);
    assert!(state.power_creeps.is_empty());
    // 往返：新字段写出的检查点同样可载入。
    let mut state2 = state.clone();
    state2.power = 16000;
    state2.power_creeps.insert(
        7,
        PowerCreepRecord {
            name: "PC".into(),
            class_name: "operator".into(),
            level: 1,
            hits_max: 2000,
            store_capacity: 200,
            powers: [(1, 1)].into_iter().collect(),
            spawn_cooldown_time: None,
            delete_time: None,
        },
    );
    let round: PlayerState =
        serde_json::from_value(serde_json::to_value(&state2).unwrap()).unwrap();
    assert_eq!(round.power, 16000);
    assert_eq!(round.power_creeps[&7].name, "PC");
    assert_eq!(round.power_creeps[&7].spawn_cooldown_time, None);
}

#[test]
fn enable_room_sets_flag_and_safe_mode_rejects() {
    let mut f = PowerFixture::new(1000);
    let ctl = f.controller_of("W1N1");
    let pc = f.power_creep("W1N1", 5, 6, "En", json!({}));
    f.register_spawned("W1N1", pc, "En");
    f.engine
        .edit_object(ctl, |o| {
            if let RoomObject::Controller(c) = o {
                c.is_power_enabled = false;
            }
            Ok(())
        })
        .unwrap();

    // 敌方控制器 + 安全模式激活：静默拒绝（标志保持关闭）。
    f.engine
        .edit_object(ctl, |o| {
            if let RoomObject::Controller(c) = o {
                c.owner = Some("p2".into());
                c.safe_mode = 10_000;
            }
            Ok(())
        })
        .unwrap();
    f.enable_room("p1", pc, ctl);
    assert_eq!(f.controller_flag("W1N1", "is_power_enabled"), Some(false));

    // 安全模式过期后放行：isPowerEnabled 置位并记动作。
    f.engine
        .edit_object(ctl, |o| {
            if let RoomObject::Controller(c) = o {
                c.safe_mode = 0;
            }
            Ok(())
        })
        .unwrap();
    f.enable_room("p1", pc, ctl);
    assert_eq!(f.controller_flag("W1N1", "is_power_enabled"), Some(true));
    assert_eq!(
        f.data_of(pc)["actionLog"]["attack"],
        json!({"x": 5, "y": 5})
    );
}

#[test]
fn renew_resets_ttl_and_requires_adjacency() {
    let mut f = PowerFixture::new(1000);
    let ps = f.power_spawn();
    let near = f.power_creep("W1N1", 20, 21, "Near", json!({}));
    let far = f.power_creep("W1N1", 25, 25, "Far", json!({}));
    f.register_spawned("W1N1", near, "Near");
    f.register_spawned("W1N1", far, "Far");
    let time = f.engine.world().time();
    for id in [near, far] {
        f.engine
            .edit_object(id, |o| {
                if let RoomObject::Generic(g) = o {
                    let mut v: Value = serde_json::from_str(&g.data).unwrap();
                    v["ageTime"] = json!(time + 100);
                    g.data = v.to_string();
                }
                Ok(())
            })
            .unwrap();
    }

    f.renew("p1", near, ps);
    // healed 动作记录按官方每 tick 重置，断言须在下一次 tick 前读。
    assert_eq!(f.data_of(near)["ageTime"], json!(time + 5000), "相邻续命");
    assert_eq!(
        f.data_of(near)["actionLog"]["healed"],
        json!({"x": 20, "y": 21})
    );

    f.renew("p1", far, ps);
    assert_eq!(f.data_of(far)["ageTime"], json!(time + 100), "非相邻拒绝");
}

#[test]
fn use_power_respects_power_enabled_and_cooldown() {
    let mut f = PowerFixture::new(1000);
    let ctl = f.controller_of("W1N1");
    let pc = f.power_creep("W1N1", 25, 25, "Gate", json!({"1": {"level": 1}}));
    f.register_spawned("W1N1", pc, "Gate");

    // powerEnabled=false：静默拒绝（不发冷却不生成）。
    f.engine
        .edit_object(ctl, |o| {
            if let RoomObject::Controller(c) = o {
                c.is_power_enabled = false;
            }
            Ok(())
        })
        .unwrap();
    f.use_power("p1", pc, 1);
    assert_eq!(
        f.data_of(pc)["store"]["ops"],
        json!(10),
        "powerEnabled 关闭时不结算"
    );

    f.engine
        .edit_object(ctl, |o| {
            if let RoomObject::Controller(c) = o {
                c.is_power_enabled = true;
            }
            Ok(())
        })
        .unwrap();
    f.use_power("p1", pc, 1);
    assert_eq!(f.data_of(pc)["store"]["ops"], json!(11));
    let cooldown = f.data_of(pc)["powers"]["1"]["cooldownTime"]
        .as_u64()
        .unwrap();
    // power 动作记录按官方每 tick 重置，断言须在下一次 tick 前读。
    assert_eq!(
        f.data_of(pc)["actionLog"]["power"],
        json!({"id": 1, "x": 25, "y": 25}),
        "结算成功记动作（无目标取自身坐标）"
    );

    // 冷却期内二次使用：静默拒绝，ops 不变。
    f.use_power("p1", pc, 1);
    assert_eq!(f.data_of(pc)["store"]["ops"], json!(11));
    assert_eq!(
        f.data_of(pc)["powers"]["1"]["cooldownTime"],
        json!(cooldown)
    );
}

#[test]
fn delete_expires_consumes_record_and_interpolates_power() {
    // 官方 cronjobs.js:533-553：到期删记录并按 GPL 插值降 power。
    // power=4000（GPL2）删 1 条 level0 记录：change=round(4000-1000-0)=3000
    // → power=1000（GPL1，progress 比例折算到 (1,2) 段的同点）。
    let mut f = PowerFixture::new(4000);
    f.create("p1", "Doomed", "operator");
    let id = *f.record("p1").power_creeps.keys().next().unwrap();
    let mut batch = IntentBatch::default();
    batch.users.entry("p1".into()).or_default().power.push(
        kk_engine::intent::PowerIntent::DeletePowerCreep {
            id: ObjId(id),
            cancel: false,
        },
    );
    f.run(batch);
    assert!(f.record("p1").power_creeps[&id].delete_time.is_some());

    // 未到期不删。
    f.run(IntentBatch::default());
    assert!(f.record("p1").power_creeps.contains_key(&id));

    // 推进墙钟 25h 后：记录删除、power 插值扣减。
    f.now_ms += 25 * 3600 * 1000;
    f.run(IntentBatch::default());
    assert!(
        !f.record("p1").power_creeps.contains_key(&id),
        "到期记录被删"
    );
    assert_eq!(f.record("p1").power, 1000, "GPL 插值降级");
}
