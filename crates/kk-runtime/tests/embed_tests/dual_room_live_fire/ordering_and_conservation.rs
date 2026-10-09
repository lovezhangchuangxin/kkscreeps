//! 双房执行顺序、资源守恒及生命周期闭环。
use super::*;

/// 反转玩家执行顺序，比较世界结果和各玩家 Memory。
/// source 只有 2 能量的场景用来区分执行顺序与意图处理顺序：始终应由 p1 先采到。
pub(crate) fn dual_execution_order_invariance_impl() -> anyhow::Result<()> {
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
pub(crate) fn dual_multi_tick_conservation_impl() -> anyhow::Result<()> {
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
    assert_eq!(e.get_game_time(), TICKS + 1, "时间恰推进 50（另含热身拍）");
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
    assert_eq!(
        b.get_game_time(),
        TICKS + 1,
        "B 世界时间恰推进 50（另含热身拍）"
    );
    b.teardown();
    Ok(())
}

/// 两房同轮孵化和建造，检查资源消耗、对象生命周期及跨房新 ID 的唯一性。
pub(crate) fn dual_lifecycle_spawn_and_construction_impl() -> anyhow::Result<()> {
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
