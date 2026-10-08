//! deposit 采集的引擎层结算测试：结算侧冷却静默门与 amount=0 完整流程。
//! 玩家链上的行为（预检、投影、恢复）由 kk-runtime 的 mod_chain_tests 覆盖；
//! 此处经意图直注锁定玩家面不可达的结算侧分支。

use super::*;

fn place_deposit(f: &mut Fixture, x: u8, y: u8, data: Value) -> ObjId {
    f.world
        .insert(
            "W1N1",
            RoomObject::Generic(kk_schema::room::GenericObj {
                id: 0,
                x,
                y,
                data: data.to_string(),
            }),
        )
        .unwrap()
}

fn deposit_data(f: &mut Fixture, id: ObjId) -> Value {
    match f.obj(id) {
        RoomObject::Generic(g) => serde_json::from_str(&g.data).unwrap(),
        _ => panic!("非 deposit"),
    }
}

/// 结算侧冷却门独立于玩家预检：cooldownTime > time 时静默——不落动作日志、
/// 不累计 harvested、不刷新寿命、不发事件（官方 harvest.js:115-117）。
/// 玩家预检与结算门读同一已提交世界、天然一致，此门须经意图直注才可达。
#[test]
fn deposit_settle_gate_silent_during_cooldown() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let d = place_deposit(
        &mut f,
        10,
        10,
        serde_json::json!({"type":"deposit","depositType":"mist","harvested":1000,
            "cooldownTime":10,"decayTime":50000}),
    );
    let c = f.creep("w1", "p1", 10, 11, &[BodyKind::Work, BodyKind::Work], 0);
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(d));
    let events = f.settle(b);
    assert!(events.is_empty(), "deposit 不发 EVENT_HARVEST");
    let v = deposit_data(&mut f, d);
    assert_eq!(v["harvested"], 1000, "冷却期不累计");
    assert_eq!(v["decayTime"], 50000, "寿命不刷新");
    match f.obj(c) {
        RoomObject::Creep(cr) => assert_eq!(cr.action_log, 0, "静默门不落动作日志"),
        _ => panic!("非 creep"),
    }
}

/// 官方 deposit 分支无 amount 门（source/mineral 才有，harvest.js:114-139）：
/// WORK 全熄（hits=0）的 amount=0 采集仍走完整流程——落动作日志、harvested +0、
/// 按当前累计重算冷却并在 >1 时重挂、刷新寿命。真实链上玩家预检的
/// countActiveParts 门先行拦截，此处锁定结算侧语义与官方一致。
#[test]
fn deposit_harvest_zero_amount_still_flows() {
    let mut f = Fixture::new(1, terrain_with(&[]));
    let d = place_deposit(
        &mut f,
        10,
        10,
        serde_json::json!({"type":"deposit","depositType":"mist","harvested":2000,
            "decayTime":100}),
    );
    let c = f
        .world
        .insert(
            "W1N1",
            RoomObject::Creep(CreepObj {
                id: 0,
                x: 10,
                y: 11,
                fatigue: 0,
                age_time: u64::MAX / 2,
                hits: 100,
                spawning: false,
                action_log: 0,
                name: "dead".into(),
                owner: "p1".into(),
                body: vec![
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 0,
                        boost: None,
                    },
                    kk_schema::room::BodyPart {
                        kind: BodyKind::Work.code(),
                        hits: 0,
                        boost: None,
                    },
                ],
                store: Default::default(),
                store_extra: Vec::new(),
                say_message: None,
                say_public: None,
                notify_when_attacked: None,
            }),
        )
        .unwrap();
    let mut b = IntentBatch::default();
    b.add_object_intent("p1", c, Action::Harvest, IntentData::target(d));
    let events = f.settle(b);
    assert!(events.is_empty());
    let v = deposit_data(&mut f, d);
    assert_eq!(v["harvested"], 2000, "amount=0 不累计");
    assert_eq!(
        v["decayTime"], 50000,
        "寿命仍刷新为 time+DEPOSIT_DECAY_TIME"
    );
    assert_eq!(
        v["cooldownTime"], 10,
        "按当前累计重算并重挂：ceil(0.001×2000^1.2)=10"
    );
    match f.obj(c) {
        RoomObject::Creep(cr) => {
            assert_ne!(cr.action_log, 0, "amount=0 仍落动作日志");
        }
        _ => panic!("非 creep"),
    }
}
