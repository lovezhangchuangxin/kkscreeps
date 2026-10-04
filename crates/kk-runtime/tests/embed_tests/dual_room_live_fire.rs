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

#[path = "dual_room_live_fire/ordering_and_conservation.rs"]
mod ordering_and_conservation;
pub(super) use ordering_and_conservation::*;

#[path = "dual_room_live_fire/recovery.rs"]
mod recovery;
pub(super) use recovery::*;
