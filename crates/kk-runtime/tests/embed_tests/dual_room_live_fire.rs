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
/// 结算序固定=房名序；打乱的只有玩家完成序——b 引擎建房数组序也同步
/// 打乱作第二自由度，但 rooms BTreeMap 使其语义惰性）。**双臂**（外部复审 P1
/// 清偿）：充裕臂的可交换负载证明粗粒度一致；**稀缺臂**（source=2 只够
/// 一人）才是完成序敏感负载——若协调器按完成顺序处理意图，反序引擎将
/// 得 (p1,p2)=(0,2)，断言即红。memory 用玩家区分标记（交叉错接可辨）。
pub(super) fn dual_execution_order_invariance_impl() -> anyhow::Result<()> {
    const TICKS: u64 = 3;

    // ── 充裕臂 ──
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

    // 玩家区分标记（外部复审 P1 清偿：交叉错接可辨——两玩家 memory 值不同）
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
    // 旋钮生效取证：观测门控后（复审 P2-2 清偿——默认路径零分配、旋钮
    // 启用才记录）默认引擎 a 返回 None、旋钮引擎 b 返回注入序
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

    // ── 稀缺臂（复审 P1 清偿）：source=2 只够一人——完成序敏感负载 ──
    // 意图合并序固定=玩家名序：无论执行完成序如何，p1（名序在先）足额、
    // p2 颗粒无收。若按完成顺序处理意图，反序引擎将得 (0,2)，断言即红
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
    // 正锚（外部复审 P2 清偿——恒等式对整臂空转恒真，须独立锚定采集
    // 真实发生）：两人各满仓 50（carry 容量），总采集恰 100、末段停采
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

/// 对象生命周期（片 3，**双房两联装**——外部复审 P1 清偿：原单房实现
/// 与方案「双房上下文」承诺不符）：p1 同时在 W0N1 与 W1N1 各持
/// spawn+builder+工地——同轮跨房孵化/建造，分别断言生命周期、资源变化，
/// 并断言两房新对象 id 全局唯一（跨房 id 分配器语义）。
pub(super) fn dual_lifecycle_spawn_and_construction_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W0N1", "rcl": 4, "owner": "p1" },
            { "name": "W1N1", "rcl": 4, "owner": "p1" },
        ],
    }))?;
    // 两房同构负载：spawn(3,3) + builder(3,2)（压住 TOP 格——出生判别力
    // 面，R1-P2-2 清偿：directions [LEFT] 应落 (2,3)；directions 被静默
    // 丢弃时缺省序首空格为 (4,2)，断言即红）+ 工地(1,2 progress 2990)
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
        // 工地：预置 progress 2990（总量 3000）——两个 build tick 即转正
        //（API 建造面为 stub，见 §5 登记）
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

/// 结算失败恢复·双房全链（片 3，验收场景 3 的多房形态）：房级注入
/// （W2N1 名序在后）→ **先结算房 W1N1 的良好结果也整体回退**（两阶段
/// 跨房提交）→ 世界/时间保旧 → 重试只重结算（执行恰一次，globalThis
/// 计数）→ 两房一并提交；memory 候选随缓存、失败时点保旧由尾段 discard
/// 臂断言（b05 单房同构）。
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
    // 执行恰一次的终检：读回两沙箱计数器（轮1+轮2 共 2 次，无第 3 次）
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

    // memory 失败时点保旧（评审 3 P1-1 清偿，b05 单房 discard 臂的双房
    // 形态）：再注入失败 → 轮 3 执行（计数+1）但 memory 候选随缓存 →
    // 显式 discard → 读回 Memory.k 仍为 2（失败轮候选未提交、丢弃不落盘）
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

/// B0-6 三件事的双房双玩家形态（片 3）：①被拒重试不销毁缓存（键集含
/// pending 外玩家 → bail 且缓存幸存）②TickInput 随失败轮缓存（重试复用
/// 记录值——signController datetime 早于重试起点）③两房意图一并提交。
/// （方案③停跑回充候选化的 embed 驱动归可选片 O3。）
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
    // p1：signer 贴 controller(1,1)（邻接门）；p2：野房采集；p3：注册但
    // 无对象无动作（own 房 controller 授视野）
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
