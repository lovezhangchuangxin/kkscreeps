//! 使用真实玩家沙箱验证跨房间结算、执行顺序和失败恢复。
//!
//! 有主房的 source 设为 3000/3000；无控制器房设为 4000/4000，
//! 与结算时的容量归一化一致。创建分片的控制器位于 (1,1)。
//! 顺序测试只改变玩家执行顺序；意图仍按玩家名合并，房间仍按房名结算。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use kk_runtime::EmbedEngine;

use kk_core::constants::{FIND_CONSTRUCTION_SITES, FIND_CREEPS, FIND_SOURCES, FIND_STRUCTURES};

/// 房内对象某数值字段总和（creep/store.energy 或 source.energy 两种挂位）。
fn field_sum(e: &EmbedEngine, room: &str, find: i64, field: &str) -> i64 {
    e.find_in_room(room, find)
        .iter()
        .filter_map(|o| {
            o.pointer(&format!("/store/{field}"))
                .or_else(|| o.get(field))
                .and_then(Value::as_i64)
        })
        .sum()
}

fn source_energy(e: &EmbedEngine, room: &str) -> i64 {
    field_sum(e, room, FIND_SOURCES, "energy")
}

fn creep_energy_by_owner(e: &EmbedEngine, room: &str, owner: &str) -> i64 {
    e.find_in_room(room, FIND_CREEPS)
        .iter()
        .filter(|o| o.get("owner").and_then(Value::as_str) == Some(owner))
        .filter_map(|o| {
            o.pointer("/store/energy")
                .or_else(|| o.get("energy"))
                .and_then(Value::as_i64)
        })
        .sum()
}

fn controller_progress(e: &EmbedEngine, room: &str) -> i64 {
    e.find_in_room(room, FIND_STRUCTURES)
        .iter()
        .find(|o| o.get("structureType").and_then(Value::as_str) == Some("controller"))
        .and_then(|o| o.get("progress"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

// 有能量时升级，空仓时采集，满仓时停采；不产生超容掉落，便于核算能量收支。
const DUAL_CODE: &str = r#"
Memory.mark = Game.time;
const rooms = Object.keys(Game.rooms).sort();
for (const name in Game.creeps) {
  const c = Game.creeps[name];
  if (c.store.energy > 0 && c.room.controller) c.upgradeController(c.room.controller);
  else if (c.store.getFreeCapacity() > 0) {
    const src = c.room.find(FIND_SOURCES)[0];
    if (src) c.harvest(src);
  }
}
JSON.stringify(rooms)
"#;

// 纯采集脚本；满仓停采，避免掉落影响收支核算。Memory 标记用于比较各玩家结果。
const HARVEST_CODE: &str = r#"
Memory.mark = Game.time;
const rooms = Object.keys(Game.rooms).sort();
for (const name in Game.creeps) {
  const c = Game.creeps[name];
  if (c.store.getFreeCapacity() > 0) {
    const src = c.room.find(FIND_SOURCES)[0];
    if (src) c.harvest(src);
  }
}
JSON.stringify(rooms)
"#;

fn codes_for(player: &str, code: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(player.to_string(), code.to_string());
    m
}

/// 同一玩家的两个 creep 分别采集和升级，验证一个运行结果能影响两房。
/// 野房的视野来自 creep，不来自房间所有权。
pub(super) fn dual_a_one_player_two_rooms_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
            { "name": "W1N1", "controller": false },
        ],
    }))?;
    // creep(2,3) 距 source(2,2) 为 1，距 controller(1,1) 为 2；采集和升级都在范围内。
    e.place_object(
        "W0N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 3000, "energyCapacity": 3000 }),
    )?;
    e.place_object(
        "W0N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "up1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 50 } }),
    )?;
    // W1N1 没有控制器，因此脚本只执行采集。
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    let progress0 = controller_progress(&e, "W0N1");
    let src_w1 = source_energy(&e, "W1N1");
    let runs = e.run_players(&codes_for("p1", DUAL_CODE))?;
    let seen: Vec<String> =
        serde_json::from_str(runs["p1"].expr["value"].as_str().expect("脚本返回 JSON 串"))?;
    assert_eq!(
        seen,
        vec!["W0N1", "W1N1"],
        "一玩家两房可见（野房经 creep 授视野）"
    );

    // 两房各自推进：W0N1 升级（progress +1×work、store 50→49）；W1N1 采集
    //（source −2×work、store 0→2）——同一玩家轮的两房意图各自路由结算
    assert_eq!(
        controller_progress(&e, "W0N1") - progress0,
        1,
        "W0N1 upgrade 臂"
    );
    assert_eq!(creep_energy_by_owner(&e, "W0N1", "p1"), 49, "升级耗 1");
    assert_eq!(src_w1 - source_energy(&e, "W1N1"), 2, "W1N1 harvest 臂");
    assert_eq!(creep_energy_by_owner(&e, "W1N1", "p1"), 2, "采集入仓");

    // RawMemory 提交恰一次：下一轮读回上轮标记（首轮 Game.time=0）
    let runs2 = e.run_players(&codes_for("p1", "JSON.stringify(Memory.mark)"))?;
    assert_eq!(
        runs2["p1"].expr["value"].as_str().unwrap_or("<非串>"),
        "0",
        "双房轮后 memory 已提交"
    );
    assert_eq!(e.get_game_time(), 2, "两轮各恰 +1（评审 3 P2-1 清偿）");

    e.teardown();
    Ok(())
}

/// 两个玩家在共享房采集同一 source，验证按玩家名处理意图时的竞争结果。
pub(super) fn dual_b_two_players_one_room_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    // 两 creep 各据 source 一侧（均在采集距离 1 内），store 起始 0
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    let src0 = source_energy(&e, "W2N1");
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), HARVEST_CODE.to_string());
    codes.insert("p2".to_string(), HARVEST_CODE.to_string());
    let runs = e.run_players(&codes)?;
    for (player, own) in [("p1", "W1N1"), ("p2", "E1N1")] {
        let seen: Vec<String> = serde_json::from_str(
            runs[player].expr["value"]
                .as_str()
                .expect("脚本返回 JSON 串"),
        )?;
        assert_eq!(seen, vec![own, "W2N1"], "{player} 可见 own 房+共享房");
    }
    // 竞争：source 扣减=两玩家采集和（2+2）；两 creep 各自 +2
    assert_eq!(src0 - source_energy(&e, "W2N1"), 4, "共享 source 双份扣减");
    assert_eq!(creep_energy_by_owner(&e, "W2N1", "p1"), 2, "c_p1 +2");
    assert_eq!(creep_energy_by_owner(&e, "W2N1", "p2"), 2, "c_p2 +2");
    e.teardown();

    // source 只有 2 能量，按玩家名处理时 p1 采尽、p2 得不到能量。
    // 充裕资源无法区分处理顺序，因此另设这个竞争场景。
    let mut s = EmbedEngine::new();
    s.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    s.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 2, "energyCapacity": 4000 }),
    )?;
    s.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    s.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    let mut scodes = BTreeMap::new();
    scodes.insert("p1".to_string(), HARVEST_CODE.to_string());
    scodes.insert("p2".to_string(), HARVEST_CODE.to_string());
    s.run_players(&scodes)?;
    assert_eq!(source_energy(&s, "W2N1"), 0, "稀缺 source 被 p1 采尽");
    assert_eq!(
        creep_energy_by_owner(&s, "W2N1", "p1"),
        2,
        "p1（名序在先）足额"
    );
    assert_eq!(
        creep_energy_by_owner(&s, "W2N1", "p2"),
        0,
        "p2 颗粒无收（名序杀伤线）"
    );
    s.teardown();
    Ok(())
}

/// 共享房前后各添加一个独立房，与单独运行的结果比较，检查跨房状态隔离。
pub(super) fn dual_c_unrelated_room_independent_impl() -> anyhow::Result<()> {
    const TICKS: u64 = 5;
    // 完整世界：p1/p2 竞争 W2N1 + p3 独居 E9N9（先结算）+ p4 独居 W9N9（后结算）
    let mut full = EmbedEngine::new();
    full.create_shard(&json!({
        "players": ["p1", "p2", "p3", "p4"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
            { "name": "E9N9", "controller": false },
            { "name": "W9N9", "controller": false },
        ],
    }))?;
    full.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    full.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    full.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    full.place_object(
        "E9N9",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    full.place_object(
        "E9N9",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p3", "owner": "p3",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    full.place_object(
        "W9N9",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    full.place_object(
        "W9N9",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p4", "owner": "p4",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    let mut codes = BTreeMap::new();
    for p in ["p1", "p2", "p3", "p4"] {
        codes.insert(p.to_string(), HARVEST_CODE.to_string());
    }
    for _ in 0..TICKS {
        full.run_players(&codes)?;
    }

    // 对照 1/2：独居房（先结算 E9N9 与后结算 W9N9 两取向）各与「单独跑」
    // 逐值一致（跨房同 tick 隔离）
    for (room, player) in [("E9N9", "p3"), ("W9N9", "p4")] {
        let mut solo = EmbedEngine::new();
        solo.create_shard(&json!({
            "players": [player],
            "rooms": [{ "name": room, "controller": false }],
        }))?;
        solo.place_object(
            room,
            "structure",
            &json!({ "pos": [2, 2], "structureType": "source",
                "energy": 4000, "energyCapacity": 4000 }),
        )?;
        solo.place_object(
            room,
            "creep",
            &json!({ "pos": [2, 3], "name": "c_me", "owner": player,
                "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
        )?;
        for _ in 0..TICKS {
            solo.run_players(&codes_for(player, HARVEST_CODE))?;
        }
        assert_eq!(
            source_energy(&full, room),
            source_energy(&solo, room),
            "无关房 {room} 的 source 终态与单独跑一致（跨房同 tick 隔离）"
        );
        assert_eq!(
            creep_energy_by_owner(&full, room, player),
            creep_energy_by_owner(&solo, room, player),
            "无关房 {room} 的 creep 终态与单独跑一致"
        );
        assert_eq!(full.get_game_time(), solo.get_game_time(), "时间推进一致");
        solo.teardown();
    }

    // 对照 2：无 Z9 的 B 世界——SHARED 竞争不受无关房存在影响
    let mut bworld = EmbedEngine::new();
    bworld.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    bworld.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    bworld.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    bworld.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    let mut bcodes = BTreeMap::new();
    for p in ["p1", "p2"] {
        bcodes.insert(p.to_string(), HARVEST_CODE.to_string());
    }
    for _ in 0..TICKS {
        bworld.run_players(&bcodes)?;
    }
    assert_eq!(
        source_energy(&full, "W2N1"),
        source_energy(&bworld, "W2N1"),
        "SHARED 竞争终态不受无关房影响"
    );
    assert_eq!(
        full.get_game_time(),
        bworld.get_game_time(),
        "时间不受无关房影响"
    );
    assert_eq!(
        (
            creep_energy_by_owner(&full, "W2N1", "p1"),
            creep_energy_by_owner(&full, "W2N1", "p2")
        ),
        (
            creep_energy_by_owner(&bworld, "W2N1", "p1"),
            creep_energy_by_owner(&bworld, "W2N1", "p2")
        ),
        "SHARED 双方 creep 终态不受无关房影响"
    );

    full.teardown();
    bworld.teardown();
    Ok(())
}

/// 反转玩家执行顺序，比较世界结果和各玩家 Memory。
/// source 只有 2 能量的场景用来区分执行顺序与意图处理顺序：始终应由 p1 先采到。
pub(super) fn dual_execution_order_invariance_impl() -> anyhow::Result<()> {
    const TICKS: u64 = 3;

    // 资源充裕时，两名玩家都能采集。
    let mut a = EmbedEngine::new();
    a.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    let mut b = EmbedEngine::new();
    b.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
        ],
    }))?;
    for e in [&mut a, &mut b] {
        e.place_object(
            "W2N1",
            "structure",
            &json!({ "pos": [2, 2], "structureType": "source",
                "energy": 4000, "energyCapacity": 4000 }),
        )?;
        e.place_object(
            "W2N1",
            "creep",
            &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
                "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
        )?;
        e.place_object(
            "W2N1",
            "creep",
            &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
                "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
        )?;
    }
    b.set_execution_order_for_test(Some(vec!["p2".to_string(), "p1".to_string()]));

    // 为两个玩家写入不同 Memory 标记，以便发现结果被交叉提交。
    let mut codes = BTreeMap::new();
    for (p, tag) in [("p1", "A"), ("p2", "B")] {
        codes.insert(
            p.to_string(),
            format!(
                "Memory.tag = '{tag}'; const c = Game.creeps[Object.keys(Game.creeps)[0]]; \
                 if (c.store.getFreeCapacity() > 0) {{ const s = c.room.find(FIND_SOURCES)[0]; \
                 if (s) c.harvest(s); }} 'run'"
            ),
        );
    }
    for _ in 0..TICKS {
        a.run_players(&codes)?;
        b.run_players(&codes)?;
    }
    // 默认引擎不记录覆盖顺序；覆盖引擎应返回设置的排列。
    assert_eq!(
        a.last_execution_order_for_test(),
        None,
        "a 未启用观测（默认路径零分配不记录）"
    );
    assert_eq!(
        b.last_execution_order_for_test(),
        Some(["p2".to_string(), "p1".to_string()].as_slice()),
        "b 引擎旋钮序生效（p2 先于 p1 执行）"
    );
    assert_eq!(
        source_energy(&a, "W2N1"),
        source_energy(&b, "W2N1"),
        "source 终态序不变"
    );
    assert_eq!(
        (
            creep_energy_by_owner(&a, "W2N1", "p1"),
            creep_energy_by_owner(&a, "W2N1", "p2")
        ),
        (
            creep_energy_by_owner(&b, "W2N1", "p1"),
            creep_energy_by_owner(&b, "W2N1", "p2")
        ),
        "双方 creep 终态序不变"
    );
    assert_eq!(a.get_game_time(), b.get_game_time(), "时间序不变");
    for room in ["W1N1", "E1N1"] {
        assert_eq!(
            controller_progress(&a, room),
            controller_progress(&b, room),
            "own 房 {room} progress 序不变"
        );
    }
    let mut probe = BTreeMap::new();
    probe.insert("p1".to_string(), "Memory.tag".to_string());
    probe.insert("p2".to_string(), "Memory.tag".to_string());
    let ma = a.run_players(&probe)?;
    let mb = b.run_players(&probe)?;
    assert_eq!(
        (
            ma["p1"].expr["value"].as_str(),
            ma["p2"].expr["value"].as_str()
        ),
        (Some("A"), Some("B")),
        "a memory 标记玩家区分"
    );
    assert_eq!(
        (
            mb["p1"].expr["value"].as_str(),
            mb["p2"].expr["value"].as_str()
        ),
        (Some("A"), Some("B")),
        "b memory 标记序不变（交叉错接可辨）"
    );
    a.teardown();
    b.teardown();

    // source 只有 2 能量时，反转执行顺序也应由 p1 采尽。
    // 若意图改为按执行完成顺序处理，结果会从 (2,0) 变成 (0,2)。
    let mut c = EmbedEngine::new();
    c.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    let mut d = EmbedEngine::new();
    d.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    for e in [&mut c, &mut d] {
        e.place_object(
            "W2N1",
            "structure",
            &json!({ "pos": [2, 2], "structureType": "source",
                "energy": 2, "energyCapacity": 4000 }),
        )?;
        e.place_object(
            "W2N1",
            "creep",
            &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
                "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
        )?;
        e.place_object(
            "W2N1",
            "creep",
            &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
                "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
        )?;
    }
    d.set_execution_order_for_test(Some(vec!["p2".to_string(), "p1".to_string()]));
    let mut scodes = BTreeMap::new();
    for p in ["p1", "p2"] {
        scodes.insert(p.to_string(), HARVEST_CODE.to_string());
    }
    c.run_players(&scodes)?;
    d.run_players(&scodes)?;
    for e in [&c, &d] {
        assert_eq!(
            (
                creep_energy_by_owner(e, "W2N1", "p1"),
                creep_energy_by_owner(e, "W2N1", "p2")
            ),
            (2, 0),
            "稀缺下名序合并（p1 足额 p2 无收）——执行完成序不改变处理序"
        );
        assert_eq!(source_energy(e, "W2N1"), 0, "稀缺 source 采尽");
    }
    c.teardown();
    d.teardown();
    Ok(())
}

/// 连续运行 50 tick，检查 source、creep 和控制器之间的能量收支。
/// 窗口覆盖满仓后停采，但不触发 source 再生；脚本不超容采集，故无需计入掉落和衰减。
pub(super) fn dual_multi_tick_conservation_impl() -> anyhow::Result<()> {
    const TICKS: u64 = 50;
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
            { "name": "W1N1", "controller": false },
        ],
    }))?;
    e.place_object(
        "W0N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 3000, "energyCapacity": 3000 }),
    )?;
    e.place_object(
        "W0N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "up1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    let s0 = source_energy(&e, "W0N1") + source_energy(&e, "W1N1");
    let st0 = creep_energy_by_owner(&e, "W0N1", "p1") + creep_energy_by_owner(&e, "W1N1", "p1");
    let p0 = controller_progress(&e, "W0N1");
    let codes = codes_for("p1", DUAL_CODE);
    let mut last_progress = p0;
    for i in 0..TICKS {
        e.run_players(&codes)?;
        let p = controller_progress(&e, "W0N1");
        assert!(p >= last_progress, "progress 单调不降（tick {}）", i + 1);
        last_progress = p;
    }
    let s1 = source_energy(&e, "W0N1") + source_energy(&e, "W1N1");
    let st1 = creep_energy_by_owner(&e, "W0N1", "p1") + creep_energy_by_owner(&e, "W1N1", "p1");
    let p1_ = controller_progress(&e, "W0N1");
    assert_eq!(
        (s1 - s0) + (st1 - st0) + (p1_ - p0),
        0,
        "能量守恒：Δsource + Δstore + Δprogress = 0（双房合计）"
    );
    assert!(p1_ > p0, "50 tick 内 upgrade 臂真实工作");
    assert_eq!(e.get_game_time(), TICKS, "时间恰推进 50");
    assert_eq!(
        e.find_in_room("W0N1", FIND_CREEPS).len() + e.find_in_room("W1N1", FIND_CREEPS).len(),
        2,
        "无意外对象增生"
    );
    e.teardown();

    // B 竞争世界：双玩家共享房 50 tick——Δsource = Δstore_p1 + Δstore_p2
    let mut b = EmbedEngine::new();
    b.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "E1N1", "rcl": 1, "owner": "p2" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    b.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    b.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "c_p1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    b.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [1, 2], "name": "c_p2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;
    let bs0 = source_energy(&b, "W2N1");
    let (bst_p1_0, bst_p2_0) = (
        creep_energy_by_owner(&b, "W2N1", "p1"),
        creep_energy_by_owner(&b, "W2N1", "p2"),
    );
    let mut bcodes = BTreeMap::new();
    for p in ["p1", "p2"] {
        bcodes.insert(p.to_string(), HARVEST_CODE.to_string());
    }
    for _ in 0..TICKS {
        b.run_players(&bcodes)?;
    }
    assert_eq!(
        bs0 - source_energy(&b, "W2N1"),
        (creep_energy_by_owner(&b, "W2N1", "p1") - bst_p1_0)
            + (creep_energy_by_owner(&b, "W2N1", "p2") - bst_p2_0),
        "B 竞争世界守恒：Δsource = Δstore_p1 + Δstore_p2"
    );
    // 收支等式在无人采集时也成立，因此另检查两人各采满 50，总采集量为 100。
    assert_eq!(
        creep_energy_by_owner(&b, "W2N1", "p1"),
        50,
        "p1 满仓（替换脚本为空转会得 0——空转即红）"
    );
    assert_eq!(creep_energy_by_owner(&b, "W2N1", "p2"), 50, "p2 满仓");
    assert_eq!(
        bs0 - source_energy(&b, "W2N1"),
        100,
        "总采集恰 100（25 tick × 2 人 × 2）"
    );
    assert_eq!(b.get_game_time(), TICKS, "B 世界时间恰推进 50");
    b.teardown();
    Ok(())
}

/// 两房同轮孵化和建造，检查资源消耗、对象生命周期及跨房新 ID 的唯一性。
pub(super) fn dual_lifecycle_spawn_and_construction_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
            { "name": "W1N1", "rcl": 4, "owner": "p1" },
        ],
    }))?;
    // builder 占住 spawn 上方，指定 LEFT 应在 (2,3) 出生；
    // 忽略方向而使用默认顺序会选 (4,2)，因此这个布局能检出方向丢失。
    for room in ["W0N1", "W1N1"] {
        e.place_object(
            room,
            "structure",
            &json!({ "pos": [3, 3], "structureType": "spawn",
                "owner": "p1", "name": format!("S_{room}"), "store": { "energy": 300 } }),
        )?;
        e.place_object(
            room,
            "creep",
            &json!({ "pos": [3, 2], "name": format!("builder_{room}"), "owner": "p1",
                "body": ["work", "carry", "move"], "store": { "energy": 20 } }),
        )?;
        // 工地预置 progress 2990、总量 3000，两个 build tick 后应成为结构。
        e.place_object(
            room,
            "constructionSite",
            &json!({ "pos": [1, 2], "structureType": "extension",
                "owner": "p1", "progress": 2990 }),
        )?;
    }

    const CODE: &str = r#"
for (const roomName of Object.keys(Game.rooms).sort()) {
  const room = Game.rooms[roomName];
  const spawn = Object.values(room.find(FIND_MY_SPAWNS))[0];
  if (spawn && !spawn.spawning && !Game.creeps['born_' + roomName]) {
    spawn.spawnCreep(['work', 'carry', 'move'], 'born_' + roomName, {directions: [LEFT]});
  }
  for (const site of room.find(FIND_CONSTRUCTION_SITES)) {
    for (const name in Game.creeps) {
      const c = Game.creeps[name];
      if (c.room.name === roomName && c.store.energy >= 5) c.build(site);
    }
  }
}
'lifecycle'
"#;
    let codes = codes_for("p1", CODE);
    // 建造链：2 tick 后两房工地均转正
    for _ in 0..2 {
        e.run_players(&codes)?;
    }
    for room in ["W0N1", "W1N1"] {
        assert_eq!(
            e.find_in_room(room, FIND_CONSTRUCTION_SITES).len(),
            0,
            "{room} 工地已消费"
        );
        let extensions = e
            .find_in_room(room, FIND_STRUCTURES)
            .iter()
            .filter(|o| o.get("structureType").and_then(Value::as_str) == Some("extension"))
            .count();
        assert_eq!(extensions, 1, "{room} extension 转正出现");
        assert_eq!(
            creep_energy_by_owner(&e, room, "p1"),
            10,
            "{room} builder 耗 2×5 建造能量"
        );
    }

    // 孵化链：needTime=9，第 12 tick 前两房各出生一只
    for _ in 0..10 {
        e.run_players(&codes)?;
    }
    let mut born_ids = Vec::new();
    for room in ["W0N1", "W1N1"] {
        let creeps = e.find_in_room(room, FIND_CREEPS);
        assert_eq!(creeps.len(), 2, "{room} 新 creep 出生");
        let born = creeps
            .iter()
            .find(|o| {
                o.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| n.starts_with("born_"))
            })
            .unwrap_or_else(|| panic!("{room} 出生 creep 在世界中"));
        assert_eq!(
            (
                born.pointer("/pos/x").and_then(Value::as_i64),
                born.pointer("/pos/y").and_then(Value::as_i64)
            ),
            (Some(2), Some(3)),
            "{room} 出生落位 directions [LEFT]——TOP 被 builder 压住，可判别（缺省序将落 (4,2)）"
        );
        assert_eq!(
            born.get("spawning"),
            Some(&Value::Bool(false)),
            "{room} 出生完成"
        );
        born_ids.push(born["id"].as_str().expect("id 字符串").to_string());
        let spawn_energy: i64 = e
            .find_in_room(room, FIND_STRUCTURES)
            .iter()
            .filter(|o| o.get("structureType").and_then(Value::as_str) == Some("spawn"))
            .filter_map(|o| o.pointer("/store/energy").and_then(Value::as_i64))
            .sum();
        assert!(
            (111..=112).contains(&spawn_energy),
            "{room} spawn 能量 300−200+涓流（实测 {spawn_energy}）"
        );
    }
    // 跨房 id 全局唯一（同轮跨房创建的出生 creep id 互异）
    assert_ne!(
        born_ids[0], born_ids[1],
        "两房新对象 id 全局唯一（跨房 id 分配器）"
    );

    e.teardown();
    Ok(())
}

/// 后结算房失败时，前一房的结果也不得发布；重试一并提交两房且不重跑玩家代码。
/// 丢弃失败轮的场景另检查 Memory 未提前提交。
pub(super) fn dual_room_failure_recovery_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 4, "owner": "p1" },
            { "name": "W2N1", "controller": false },
        ],
    }))?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 3000, "energyCapacity": 3000 }),
    )?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "up1", "owner": "p1",
            "body": ["work", "carry", "move"], "store": { "energy": 50 } }),
    )?;
    e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    const P1_CODE: &str = r#"
globalThis.n1 = (globalThis.n1 || 0) + 1;
Memory.k1 = globalThis.n1;
Game.creeps['up1'].upgradeController(Game.rooms['W1N1'].controller);
'p1'
"#;
    const P2_CODE: &str = r#"
globalThis.n2 = (globalThis.n2 || 0) + 1;
Memory.k2 = globalThis.n2;
const c = Game.creeps['har2'];
if (c.store.getFreeCapacity() > 0) c.harvest(c.room.find(FIND_SOURCES)[0]);
'p2'
"#;
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), P1_CODE.to_string());
    codes.insert("p2".to_string(), P2_CODE.to_string());
    // 轮 1：正常推进（n1=n2=1，W1N1 progress+1、W2N1 source−2）
    e.run_players(&codes)?;
    let (p0, s0, t0) = (
        controller_progress(&e, "W1N1"),
        source_energy(&e, "W2N1"),
        e.get_game_time(),
    );

    // 轮 2：房级失败注入（W2N1 名序在后——W1N1 已良好结算仍须回退）
    e.fail_next_settle_room_for_test("W2N1");
    let err = e.run_players(&codes).unwrap_err();
    assert!(err.to_string().contains("W2N1"), "房级故障注入命中: {err}");
    assert!(e.has_pending_round(), "失败轮候选缓存");
    assert_eq!(e.get_game_time(), t0, "时间不推进");
    assert_eq!(
        controller_progress(&e, "W1N1"),
        p0,
        "先结算房 W1N1 的良好结果随整体回退（两阶段跨房提交）"
    );
    assert_eq!(source_energy(&e, "W2N1"), s0, "失败房 W2N1 状态不变");

    // 轮 2 重试：只重结算——执行恰一次（n1/n2 保持 2，不重跑玩家）
    e.run_players(&codes)?;
    assert_eq!(
        controller_progress(&e, "W1N1"),
        p0 + 1,
        "重试后两房一并提交（W1N1）"
    );
    assert_eq!(
        source_energy(&e, "W2N1"),
        s0 - 2,
        "重试后两房一并提交（W2N1）"
    );
    assert_eq!(e.get_game_time(), t0 + 1, "重试恰推进一 tick");
    assert!(!e.has_pending_round(), "候选已消费");
    // 两轮执行后各沙箱计数器都应为 2；重试结算不能使它增加到 3。
    let mut probe = BTreeMap::new();
    probe.insert(
        "p1".to_string(),
        "JSON.stringify([globalThis.n1, Memory.k1])".to_string(),
    );
    probe.insert(
        "p2".to_string(),
        "JSON.stringify([globalThis.n2, Memory.k2])".to_string(),
    );
    let runs = e.run_players(&probe)?;
    for (p, label) in [("p1", "n1"), ("p2", "n2")] {
        let v: Vec<u64> = serde_json::from_str(runs[p].expr["value"].as_str().expect("JSON 串"))?;
        assert_eq!(v, vec![2, 2], "{label} 执行恰两次（轮1+轮2），失败轮未重跑");
    }

    // 再次失败后显式丢弃，再读 Memory.k 应仍为 2，证明失败候选未提前提交。
    e.fail_next_settle_for_test();
    e.run_players(&codes).unwrap_err();
    assert!(e.has_pending_round());
    e.discard_pending_round();
    assert!(!e.has_pending_round());
    let mut probe2 = BTreeMap::new();
    probe2.insert(
        "p1".to_string(),
        "JSON.stringify([globalThis.n1, Memory.k1])".to_string(),
    );
    probe2.insert(
        "p2".to_string(),
        "JSON.stringify([globalThis.n2, Memory.k2])".to_string(),
    );
    let runs = e.run_players(&probe2)?;
    for (p, label) in [("p1", "n1"), ("p2", "n2")] {
        let v: Vec<u64> = serde_json::from_str(runs[p].expr["value"].as_str().expect("JSON 串"))?;
        assert_eq!(
            v,
            vec![3, 2],
            "{label}：globalThis=3（含被丢弃的轮 3 执行，V8 不回滚），Memory 恒 2（失败轮候选未提交即被丢弃）"
        );
    }

    e.teardown();
    Ok(())
}

/// 双房失败轮中，检查错误重试保留缓存、签名复用首次墙钟，以及两房一并提交。
pub(super) fn dual_b06_variants_impl() -> anyhow::Result<()> {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    let now_ms = || {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2", "p3"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N1", "controller": false },
            { "name": "E1N1", "rcl": 1, "owner": "p3" },
        ],
    }))?;
    // p1 的 signer 与控制器相邻，p2 在野房采集，p3 已注册但不参与首次执行。
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [2, 1], "name": "signer", "owner": "p1",
            "body": ["move"], "store": { "energy": 0 } }),
    )?;
    e.place_object(
        "W2N1",
        "structure",
        &json!({ "pos": [2, 2], "structureType": "source",
            "energy": 4000, "energyCapacity": 4000 }),
    )?;
    e.place_object(
        "W2N1",
        "creep",
        &json!({ "pos": [2, 3], "name": "har2", "owner": "p2",
            "body": ["work", "carry", "move"], "store": { "energy": 0 } }),
    )?;

    let sign_code = "globalThis.n = (globalThis.n||0)+1; Game.creeps['signer'].signController(Game.rooms['W1N1'].controller, 'dual'); 'sign'";
    let harvest_code = "const c = Game.creeps['har2']; if (c.store.getFreeCapacity() > 0) c.harvest(c.room.find(FIND_SOURCES)[0]); 'harvest'";
    // 预热一轮（挡首跑装配耗时出 datetime 判窗）
    let mut warm = BTreeMap::new();
    for p in ["p1", "p2", "p3"] {
        warm.insert(p.to_string(), "'warmup'".to_string());
    }
    e.run_players(&warm)?;

    // 轮 A：p1 签名 + p2 采集（p3 不在 codes——pending 只含 {p1,p2}）→ 失败
    let first_attempt_ms = now_ms();
    e.fail_next_settle_for_test();
    let mut codes = BTreeMap::new();
    codes.insert("p1".to_string(), sign_code.to_string());
    codes.insert("p2".to_string(), harvest_code.to_string());
    let err = e.run_players(&codes).unwrap_err();
    assert!(err.to_string().contains("故障注入"), "{err}");
    assert!(e.has_pending_round(), "失败轮候选缓存");

    // ①被拒重试：请求含 pending 外玩家（p3）→ bail 且缓存幸存
    let mut with_p3 = BTreeMap::new();
    for (p, c) in [("p1", sign_code), ("p2", harvest_code), ("p3", "'idle'")] {
        with_p3.insert(p.to_string(), c.to_string());
    }
    let rejected = e.run_players(&with_p3).unwrap_err();
    assert!(
        rejected.to_string().contains("p3"),
        "键集守卫拒绝 pending 外玩家: {rejected}"
    );
    assert!(
        e.has_pending_round(),
        "被拒请求不销毁缓存（B0-6 ①双玩家形态）"
    );

    // ②TickInput 复用：重试（{p1,p2} 合法子集）——datetime 应为首次尝试
    // 的记录值（早于重试起点），两房意图一并提交
    std::thread::sleep(Duration::from_millis(150));
    let retry_started_ms = now_ms();
    let s_before = source_energy(&e, "W2N1");
    e.run_players(&codes)?;
    assert_eq!(
        s_before - source_energy(&e, "W2N1"),
        2,
        "重试提交野房采集意图"
    );
    let run = e.run_player("p1", "JSON.stringify([globalThis.n, new Date(Game.rooms['W1N1'].controller.sign.datetime).getTime()])")?;
    let v: Vec<u64> = serde_json::from_str(run.expr["value"].as_str().expect("JSON 串"))?;
    assert_eq!(
        v[0], 1,
        "signer 计数恰 1（轮A 一次——预热轮为纯字面量代码不计数；失败轮未重跑）"
    );
    assert!(
        v[1] < retry_started_ms.saturating_sub(100),
        "datetime 用首次尝试的记录值（dt={} 早于重试起点 {}；重采样则不早于）",
        v[1],
        retry_started_ms
    );
    assert!(
        v[1] >= first_attempt_ms.saturating_sub(1_000),
        "datetime 合理性：不早于首轮起点附近（{first_attempt_ms}）"
    );

    e.teardown();
    Ok(())
}

/// 跨房移动过渡的端到端：玩家 move 上边缘格，同 tick 过渡到邻房镜像位；
/// 玩家下一轮视图在结算前读取，看到的是过渡后的位置；随后该轮结算使
/// 位于新边缘格的对象再次过渡（官方出口弹跳语义）。
pub(super) fn dual_inter_room_transition_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W1N1", "rcl": 1, "owner": "p1" },
            { "name": "W2N1", "rcl": 1, "owner": "p1" },
        ],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [1, 10], "name": "Traveler", "owner": "p1", "body": ["move"] }),
    )?;

    let mut codes = std::collections::BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "Game.creeps['Traveler'].move(LEFT)".to_string(),
    );
    let run = e.run_players(&codes)?;
    assert_eq!(run["p1"].expr["value"], json!(0), "move(LEFT) 返回 OK");

    // 同 tick 过渡：世界状态中对象已位于西邻镜像位。
    let after = e.get_object(&creep_id).expect("对象仍存在");
    let pos = &after["pos"];
    assert_eq!(pos["roomName"], json!("W2N1"), "过渡到西邻房");
    assert_eq!(pos["x"], json!(49), "西邻镜像 x=49");
    assert_eq!(pos["y"], json!(10), "y 不变");

    // 下一轮玩家视图：玩家代码先于结算运行，读到过渡后的位置；
    // 该轮结算使边缘对象再次过渡（弹回源房）。
    let mut codes = std::collections::BTreeMap::new();
    codes.insert(
        "p1".to_string(),
        "JSON.stringify((function (c) { return [c.pos.roomName, c.pos.x, c.pos.y, c.name]; }) \
         (Game.getObjectById(Game.creeps['Traveler'].id)))"
            .to_string(),
    );
    let run = e.run_players(&codes)?;
    let view: Vec<serde_json::Value> =
        serde_json::from_str(run["p1"].expr["value"].as_str().expect("JSON 串"))?;
    assert_eq!(
        serde_json::Value::Array(view),
        json!(["W2N1", 49, 10, "Traveler"]),
        "玩家视图在结算前读到过渡后位置"
    );
    let bounced = e.get_object(&creep_id).expect("对象仍存在");
    assert_eq!(
        bounced["pos"]["roomName"],
        json!("W1N1"),
        "下一轮结算弹回源房"
    );
    assert_eq!(bounced["pos"]["x"], json!(0), "弹回镜像位 x=0");
    assert_eq!(bounced["pos"]["y"], json!(10));

    e.teardown();
    Ok(())
}
