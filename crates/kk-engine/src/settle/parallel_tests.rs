//! 逐房并行结算（R09-lite）的等价锁与资格门测试。
//!
//! 等价锁：同输入下并行与串行产出一致（逐房快照字节、玩家、事件与
//! ID 游标）；资格门：claim 意图、注入随机源、宿主槽任一在场即整拍
//! 串行；回退：并行段触碰 ID 分配即恢复共享态并串行重做，结果与纯
//! 串行一致。

use super::*;
use kk_core::Terrain;

/// 构造 N 间房、每房一个归属控制器（无 ID 分配路径的稳态形态）。
fn world_with_rooms(count: usize) -> kk_world::World {
    let mut world = kk_world::World::new(7);
    for i in 0..count {
        let name = format!("W{i}N1");
        world.register_room(&name, Terrain::plains()).unwrap();
        world
            .insert(
                &name,
                RoomObject::Controller(kk_schema::room::ControllerObj {
                    id: 0,
                    x: 5,
                    y: 5,
                    level: 4,
                    safe_mode: 0,
                    safe_mode_available: 0,
                    progress: 100,
                    downgrade_time: None,
                    owner: Some(format!("p{}", i % 4)),
                    owner_cleared: false,
                    is_power_enabled: false,
                    reservation: None,
                    sign: None,
                    upgrade_blocked: None,
                    safe_mode_cooldown: None,
                }),
            )
            .unwrap();
    }
    world
}

/// 逐房二进制快照 + 玩家 + 事件的指纹（等价锁的比较面）。
fn fingerprint(engine: &Engine) -> Vec<(String, Vec<u8>)> {
    engine
        .world()
        .rooms()
        .iter()
        .map(|(name, room)| {
            let snapshot = room.encode_snapshot(0, 1, 0);
            (name.clone(), kk_schema::room::encode_snapshot(&snapshot))
        })
        .collect()
}

fn drive(engine: &mut Engine, ticks: u64) {
    for _ in 0..ticks {
        engine.tick_with_intents(IntentBatch::default()).unwrap();
    }
}

/// 同输入下并行与串行的完整产出逐字节一致；并行路径确实启用。
#[test]
fn parallel_rooms_match_serial_output() {
    let mut serial = Engine::new(world_with_rooms(40));
    serial.set_settle_workers(Some(1));
    let mut parallel = Engine::new(world_with_rooms(40));
    parallel.set_settle_workers(Some(4));

    drive(&mut serial, 5);
    drive(&mut parallel, 5);

    assert!(parallel.rooms_settled_in_parallel_for_test());
    assert!(!serial.rooms_settled_in_parallel_for_test());
    assert_eq!(fingerprint(&serial), fingerprint(&parallel));
    assert_eq!(serial.players, parallel.players);
    assert_eq!(serial.last_events, parallel.last_events);
    assert_eq!(
        serial.world().next_available_obj_id().unwrap(),
        parallel.world().next_available_obj_id().unwrap()
    );
}

/// 并行段触碰 ID 分配（创建旗）触发串行回退：结果与纯串行一致。
#[test]
fn id_allocation_trips_serial_redo_and_matches() {
    let mut batch = IntentBatch::default();
    batch.add_room_intent(
        "p0",
        crate::intent::RoomIntent::CreateFlag {
            room: "W0N1".into(),
            x: 10,
            y: 10,
            name: "flag".into(),
            color: 1,
            secondary_color: 1,
        },
    );
    let mut serial = Engine::new(world_with_rooms(40));
    serial.set_settle_workers(Some(1));
    let mut parallel = Engine::new(world_with_rooms(40));
    parallel.set_settle_workers(Some(4));

    serial.tick_with_intents(batch.clone()).unwrap();
    parallel.tick_with_intents(batch).unwrap();

    // 创建旗分配了 ID：并行尝试必须回退到串行重做。
    assert!(!parallel.rooms_settled_in_parallel_for_test());
    assert_eq!(fingerprint(&serial), fingerprint(&parallel));
    assert_eq!(
        serial.world().next_available_obj_id().unwrap(),
        parallel.world().next_available_obj_id().unwrap()
    );
}

/// claimController 意图在场即整拍串行（claim 读候选 gcl 的房序语义）。
#[test]
fn claim_intent_forces_serial_path() {
    let mut batch = IntentBatch::default();
    batch.users.entry("p0".into()).or_default().objects.insert(
        ObjId(123),
        std::collections::BTreeMap::from([(
            crate::intent::Action::ClaimController,
            crate::intent::IntentData::target(ObjId(123)),
        )]),
    );
    let mut parallel = Engine::new(world_with_rooms(40));
    parallel.set_settle_workers(Some(4));
    parallel.tick_with_intents(batch).unwrap();
    assert!(!parallel.rooms_settled_in_parallel_for_test());
}

/// 注入随机源（诊断路径）期间整拍串行：注入序列的消费顺序依赖房序。
#[test]
fn injected_random_forces_serial_path() {
    let mut parallel = Engine::new(world_with_rooms(40));
    parallel.set_settle_workers(Some(4));
    parallel.set_processor_random(ProcessorRandom::Injected(std::sync::Arc::new(
        std::sync::Mutex::new(std::collections::VecDeque::new()),
    )));
    parallel.tick_with_intents(IntentBatch::default()).unwrap();
    assert!(!parallel.rooms_settled_in_parallel_for_test());
}

/// 少房间世界自适应串行（线程开销不值得）；自适应并行在 64 房启用。
#[test]
fn worker_count_adapts_to_room_count() {
    let mut small = Engine::new(world_with_rooms(8));
    small.tick_with_intents(IntentBatch::default()).unwrap();
    assert!(!small.rooms_settled_in_parallel_for_test());

    let mut large = Engine::new(world_with_rooms(64));
    large.tick_with_intents(IntentBatch::default()).unwrap();
    assert!(large.rooms_settled_in_parallel_for_test());
}
