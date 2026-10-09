//! 移动域测试：拉拽配对、疲劳与仲裁行为。

use super::*;
use kk_engine::settle::movement;
use kk_world::{ObjId, RoomIndex, RoomState};

#[test]
fn pull_pair_moves_fatigue_free_creep() {
    // 拉拽官方语义（pull.js + movement.js）：拉者 A 记 pull，被拉 B 记
    // move(A)（id 形态）——A 移动让位，B 挤入 A 原格；B 无 MOVE 部件也带疲劳
    // 照样移动（canMove 的 _pulled 分支），B 的移动疲劳沿链记在 A 头上
    //（_add-fatigue 归链）
    let mut f = Fixture::new(1, terrain_with(&[]));
    let a = f.creep("A", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0); // 拉者
    let b = f.creep("B", "p1", 11, 10, &[BodyKind::Work], 0); // 被拉（无 MOVE）
    f.world
        .edit_object(b, |object| {
            if let RoomObject::Creep(cc) = object {
                cc.fatigue = 2; // 被拉者带疲劳
            }
            Ok(())
        })
        .unwrap();

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

#[test]
fn stationary_creep_blocks_movement() {
    // 未获准移动的 creep 占据原格，阻止其他 creep 移入。
    let mut f = Fixture::new(1, terrain_with(&[]));
    let _still = f.creep("still", "p1", 11, 10, &[BodyKind::Move], 0);
    let mover = f.creep("mover", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle(move_intent(mover, 3)); // RIGHT → (11,10) 有静止 creep
    assert_eq!(f.creep_state(mover), (10, 10, 0, 0), "静止 creep 挡路");
}

#[test]
fn swap_with_third_contender_rates() {
    // A 与 B 对穿时，A 的目标格还有竞争者 C，A 的 rate1=100，优先于 C。
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
    // 前面评分平手时比较 MOVE 功率/重量，较轻的 creep 获准移动。
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
    // 沼泽上有道路时使用道路疲劳率 1，覆盖沼泽率 10。
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
fn birth_blocked_by_stationary_creep_until_it_leaves() {
    // 指定出生格有静止 creep 时延期，移走后出生。
    // 阻挡者的移走方向须远离 spawn，避免逃离动作本身被阻挡而无法验证解除占位。
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
    // 疲劳从已保存的值继续递减，静止 tick 不得直接清零。
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

// G30.d 移动仲裁同功能默认参考：原生 check_impl 的仲裁吞吐微基准，
// release 构建下有统计意义（cargo test --release -- --ignored --nocapture），
// 输出耗时不作门禁。
/// 构造 8 个竞争格 × 每格 4 竞争者的登记矩阵：每格从左上三个邻格与
/// 左侧一格同时挤入。
fn contested_movement(room: &mut RoomState) -> movement::Movement {
    let mut m = movement::Movement::new();
    let mut next = 1;
    for cell in 0..8u8 {
        let (gx, gy) = (10 + cell * 2, 15u8);
        for (dx, dy) in [(1i32, 1i32), (0, 1), (-1, 1), (1, 0)] {
            let (x, y) = ((gx as i32 - dx) as u8, (gy as i32 - dy) as u8);
            let id = ObjId(next);
            next += 1;
            room.objects.insert(
                id,
                RoomObject::Creep(CreepObj {
                    id: 0,
                    x,
                    y,
                    fatigue: 0,
                    age_time: u64::MAX / 2,
                    hits: 100,
                    spawning: false,
                    action_log: 0,
                    name: format!("c{cell}-{x}-{y}"),
                    owner: "p1".into(),
                    body: body(&[BodyKind::Move]),
                    store: kk_schema::room::Store {
                        energy: 0,
                        ghodium: 0,
                    },
                    store_extra: Vec::new(),
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
            );
            m.add(id, (x, y), dx, dy);
        }
    }
    m
}

#[test]
#[ignore = "手动基准：cargo test --release -p kk-engine --test settle_tests movement_benchmark -- --ignored --nocapture"]
fn movement_benchmark_native_arbitration() {
    let mut room = RoomState::new("W1N1", terrain_with(&[]));
    let movement = contested_movement(&mut room);
    let index = RoomIndex::build(&room.objects);
    let old_fatigue = BTreeMap::new();
    let iterations = if cfg!(debug_assertions) { 10 } else { 2000 };
    // 预热与正确性检查不计入窗口。
    let mut probe = movement.clone();
    probe.check_impl(
        &room,
        &index,
        &old_fatigue,
        None,
        &BTreeMap::new(),
        &Default::default(),
    );
    assert_eq!(probe.winners().len(), 8, "每个竞争格恰一胜者");
    let start = std::time::Instant::now();
    for _ in 0..iterations {
        let mut m = movement.clone();
        m.check_impl(
            &room,
            &index,
            &old_fatigue,
            None,
            &BTreeMap::new(),
            &Default::default(),
        );
        debug_assert_eq!(m.winners().len(), 8);
    }
    let elapsed = start.elapsed();
    println!(
        "movement check_impl 默认参考：8 竞争格 ×4 竞争者 ×{iterations} 次 = {:.2} µs/房",
        elapsed.as_nanos() as f64 / iterations as f64 / 1000.0,
    );
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

// ---- 计划级墙地形静态通行声明（movement.md 静态通行声明） ----

/// 带地形通行声明的贡献平面；其余域取内置默认。
fn passability_plane(walls: bool) -> kk_engine::settle::Plane {
    kk_engine::settle::Plane {
        world_span: kk_engine::settle::Plane::UNWRAPPED_SPAN,
        buildings: Default::default(),
        links: kk_world::place::builtin_link_rules(),
        stores: kk_world::place::builtin_store_rules(),
        combat: kk_world::place::builtin_combat_ranges(),
        move_gate: Default::default(),
        quotas: std::rc::Rc::new(kk_mod_api::builtin::StructureQuotaSnapshot::default()),
        static_passability: kk_mod_api::builtin::StaticPassability {
            wall_passable: walls,
            ..Default::default()
        },
        resources: Default::default(),
        contrib: std::rc::Rc::new(kk_engine::settle::contrib::ContributionDispatch::default()),
        extensions: Default::default(),
        plan: kk_engine::settle::builtin_plan(),
    }
}

fn wall_terrain(swamp_at_wall: Option<(usize, usize)>) -> kk_core::Terrain {
    // (11,10) 竖墙；可选格改为墙|沼泽。
    let mut t = kk_core::Terrain::plains();
    t.as_mut_slice()[10 * 50 + 11] |= kk_core::TERRAIN_MASK_WALL;
    if let Some((x, y)) = swamp_at_wall {
        assert_eq!((x, y), (11, 10), "墙|沼泽格固定在 (11,10)");
        t.as_mut_slice()[10 * 50 + 11] |= kk_core::TERRAIN_MASK_SWAMP;
    }
    t.normalize();
    t
}

#[test]
fn static_passability_allows_move_onto_wall_with_plain_fatigue() {
    // 声明放行墙位：move 意图可入墙格，疲劳按平原费率（重量 1 × 2）。
    let mut f = Fixture::new(1, wall_terrain(None));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle_with_plane(move_intent(c, 3), passability_plane(true));
    // 平原净疲劳：+2（重量 1×平原率 2）−2（1 个 MOVE 恢复）= 0。
    assert_eq!(
        f.creep_state(c),
        (11, 10, 0, 0),
        "墙位按平原放行，净疲劳与平原一致"
    );
    // 原生对照：默认平面同意图被静态障碍拒绝。
    let mut f = Fixture::new(1, wall_terrain(None));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle_with_plane(move_intent(c, 3), passability_plane(false));
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "默认墙位不可入");
}

#[test]
fn static_passability_keeps_swamp_fatigue_on_wall_swamp_tile() {
    // 墙|沼泽位放行后按沼泽语义计疲劳（重量 1 × 10）。
    let mut f = Fixture::new(1, wall_terrain(Some((11, 10))));
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle_with_plane(move_intent(c, 3), passability_plane(true));
    // 沼泽净疲劳：+10（重量 1×沼泽率 10）−2（1 个 MOVE 恢复）= 8。
    assert_eq!(f.creep_state(c), (11, 10, 8, 0), "墙|沼泽按沼泽费率");
}

#[test]
fn static_passability_does_not_release_dynamic_blocking() {
    // 声明只放行静态地形面：动态占格（未获准移动的 creep）仍阻挡。
    let mut f = Fixture::new(1, wall_terrain(None));
    let _still = f.creep("still", "p1", 11, 10, &[BodyKind::Move], 0);
    let mover = f.creep("mover", "p1", 10, 10, &[BodyKind::Move], 0);
    f.settle_with_plane(move_intent(mover, 3), passability_plane(true));
    assert_eq!(f.creep_state(mover), (10, 10, 0, 0), "占格 creep 仍挡路");
}

#[test]
fn static_passability_keeps_structure_obstacles_blocking() {
    // 声明只作用于地形墙项：内建障碍结构不随声明放行（登记与仲裁的
    // 结构判定不读通行声明）。
    let mut f = Fixture::new(1, wall_terrain(None));
    let _wall = generic_structure(&mut f, 11, 10, "constructedWall", "p1", 1);
    let c = f.creep("w1", "p1", 10, 10, &[BodyKind::Move, BodyKind::Work], 0);
    f.settle_with_plane(move_intent(c, 3), passability_plane(true));
    assert_eq!(f.creep_state(c), (10, 10, 0, 0), "结构障碍仍阻挡");
}
