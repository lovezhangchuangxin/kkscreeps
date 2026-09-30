//! 双房双玩家实弹验证（双房批片 2）：跨房 tick 合并规则——tick 契约验收
//! 场景 2 三形态（一玩家影响两房 / 两玩家影响一房 / 无关房独立）+ 玩家
//! 执行顺序不变性 + 多 tick 稳态与能量守恒。全部经 EmbedEngine 真沙箱
//! 全链（per-viewer 投影 → V8 玩家代码 → intent 清洗 → 多房结算 → 统一
//! 提交）；每场景独立引擎、teardown 必达。
//!
//! 夹具口径：own 房 source 摆 3000/3000（有主房容量归一目标=3000）；野房
//! 一律 `controller: false` 且 source 摆 4000/4000（无 controller 房容量
//! 归一目标=KEEPER 4000——摆同值免归一差；野房若保留 controller 则归一
//! NEUTRAL 1500 且钳 energy，口径会被污染）。controller 由 create_shard
//! 硬编码在 (1,1)。
//!
//! 串行参考口径：现单线程协调器的既定序（玩家名序执行 + 意图 handle 序
//! 合并 + 房名序结算）即参考；本模块钉「结果不依赖可打乱的自由度（执行
//! 序经测试旋钮打乱）」+「语义正确（守恒核算）」，作为将来并行化批的
//! 对齐基线。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use kk_runtime::EmbedEngine;

use kk_core::constants::{FIND_CREEPS, FIND_SOURCES, FIND_STRUCTURES};

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

/// 主负载脚本：有能走 upgrade（须 controller）、空仓走 harvest（满仓即停
/// ——超容采集的掉落/衰减路径由引擎级测试锁定
/// settle_tests/combat_heal.rs 的 harvest_source_mixed_holding_overflow_
/// drops_energy_only 等，本链守恒式保持无缺项）；返回可见房名表（JSON 串）。
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

/// 纯采集脚本（野房无 controller，upgrade 臂不可达）；带 memory 标记
///（顺序不变性的跨玩家状态面比对用）与满仓即停门（守恒式无缺项）。
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

/// 场景 A「一玩家影响两房」（验收场景 2 形态一）：p1 于 own 房 W0N1
/// （upgrade 臂）与野房 W1N1（harvest 臂）各有一 creep——同一 RunResult
/// 携带两房对象级意图，合并结算后两房各自推进；可见性=两房（野房经 creep
/// 授视野而非所有权）。
pub(super) fn dual_a_one_player_two_rooms_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
            { "name": "W1N1", "controller": false },
        ],
    }))?;
    // W0N1：controller(1,1) 硬编码；source(2,2)+creep(2,3)——采集/升级
    // 两臂都在程内（harvest≤1、upgrade≤3）
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
    // W1N1：野房（controller:false → 脚本 upgrade 分支不可达）纯采集臂
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

/// 场景 B「两玩家影响一房」（形态二）：p1/p2 各有 own 房（仅 controller），
/// 双方 creep 共处野房 SHARED 采同一 source——竞争扣减=两者之和（意图
/// 按玩家名序处理）；两玩家各自可见 own 房+SHARED。
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

    // 稀缺变体（评审 3 P1-4 清偿）：source=2 只够一人——意图按玩家名序
    // 处理（settle 玩家名序），p1 先采尽、p2 颗粒无收；这是「玩家名序」
    // 处理序的真杀伤线（充裕 source 下任意序同果）
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

/// 场景 C「无关房先完成」（形态三）：B 布局 + 两个独居远房——p3 居
/// E9N9（字典序在共享房 W2N1 **之前**结算=「先完成」取向）、p4 居 W9N9
///（字典序在 W2N1 **之后**=「后完成」取向，评审 3 P1-2 清偿的双向覆盖）。
/// 断言独立：两远房终态各与「单独跑」逐值一致（同 tick 跨房隔离——结算
/// 逐房克隆自已提交态的结构性保证，钉成 embed 级回归）；SHARED 竞争终态
/// 与「无远房跑」逐值一致。
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

/// 顺序不变性（验收场景 2 后半）：B 世界以测试旋钮打乱执行序（p2 先于
/// p1 投影+执行）——终态逐值与名序执行一致（意图合并序固定=handle 序、
/// 结算序固定=房名序；打乱的只有玩家完成序）。
pub(super) fn dual_execution_order_invariance_impl() -> anyhow::Result<()> {
    const TICKS: u64 = 3;
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
    // b：打乱执行序（p2 先于 p1）——建房序也同步打乱（双自由度）
    b.set_execution_order_for_test(Some(vec!["p2".to_string(), "p1".to_string()]));

    let mut codes = BTreeMap::new();
    for p in ["p1", "p2"] {
        codes.insert(p.to_string(), HARVEST_CODE.to_string());
    }
    for _ in 0..TICKS {
        a.run_players(&codes)?;
        b.run_players(&codes)?;
    }
    // 旋钮生效取证（评审 3 P1-3 清偿）：结果面序无关是被测性质，接线回归
    // 须由 last_execution_order 观测——两引擎实际迭代序各自符合注入
    assert_eq!(
        a.last_execution_order_for_test(),
        Some(["p1".to_string(), "p2".to_string()].as_slice()),
        "a 引擎默认序=handle 排序"
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
    // own 房 controller progress 与两玩家 memory（评审 3 P1-1 清偿——方案
    // 明列的逐值断言面）：本夹具下 progress 平凡为 0、memory 为末轮 Game.time
    for room in ["W1N1", "E1N1"] {
        assert_eq!(
            controller_progress(&a, room),
            controller_progress(&b, room),
            "own 房 {room} progress 序不变"
        );
    }
    let mut probe = BTreeMap::new();
    probe.insert("p1".to_string(), "JSON.stringify(Memory.mark)".to_string());
    probe.insert("p2".to_string(), "JSON.stringify(Memory.mark)".to_string());
    let ma = a.run_players(&probe)?;
    let mb = b.run_players(&probe)?;
    for p in ["p1", "p2"] {
        assert_eq!(
            ma[p].expr["value"], mb[p].expr["value"],
            "{p} 的 memory 终态序不变"
        );
    }

    a.teardown();
    b.teardown();
    Ok(())
}

/// 多 tick 稳态（评审 3 P2-2 清偿：A 世界 + B 竞争世界双形）：A=单玩家
/// 两房 ×50 tick，恒等式 Δsource + Δstore + Δprogress = 0（progress 计为
/// 能量汇，1 work part 每 upgrade 恰 1:1）；B=双玩家共享房 ×50 tick，
/// Δsource = Δstore_p1 + Δstore_p2。窗口 50=满仓停采分界（carry 50 /
/// 每 tick 2 ≈ tick 25）的两倍——覆盖 harvest→停的两态；脚本满仓即停
/// （无掉落/衰减项，超容掉落路径归引擎级测试 combat_heal.rs）；窗口内
/// source 不触 300-tick 再生回填、无 spawn/transfer 旁路——恒等式无缺项。
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
    assert_eq!(b.get_game_time(), TICKS, "B 世界时间恰推进 50");
    b.teardown();
    Ok(())
}
