//! 嵌入式 API 的集成测试（M0）：createShard → place → runPlayer → tick → 读状态。

use serde_json::json;

use kk_engine::EmbedEngine;

#[test]
fn embed_engine_suite() -> anyhow::Result<()> {
    // 场景必须串行（同进程多 isolate 的 LIFO 销毁纪律；并行会互相干扰销毁顺序）
    embed_engine_roundtrip_impl()?;
    embed_run_player_contract_impl()?;
    embed_create_shard_validation_impl()?;
    embed_real_settlement_impl()?;
    embed_run_players_shared_snapshot_impl()?;
    embed_snapshot_contract_impl()?;
    embed_intent_channel_impl()?;
    embed_per_viewer_and_caps_impl()?;
    embed_spawn_family_chain_impl()?;
    embed_resource_family_impl()?;
    embed_move_misc_family_impl()?;
    embed_mineral_direct_channel_impl()?;
    embed_gpt_review_clearance_impl()?;
    a5_memory_module_forms_impl()?;
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
    Ok(())
}

/// A4-i spawn 族全链（prelude spawnCreep/renewCreep → intentsList 清洗 →
/// settle/spawn.rs 执行器 → 投影回读）：孵化任务落库、能量扣费与涓流、
/// 到点出生、renewCreep 的 ageTime 递延。
fn embed_spawn_family_chain_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let spawn_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "spawn", "owner": "p1", "store": { "energy": 300 } }),
    )?;
    let spawn_json = serde_json::to_string(&spawn_id)?;

    // spawnCreep [WORK, WORK]：成本 200、needTime=3×2=6、spawnTime=0+6
    // （run_player 的结算即发生）——孵化任务 + 雏形落库；扣费 200 后
    // energyAvailable 100<300 → 免费涓流 +1 → 101
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById({spawn_json}).spawnCreep([WORK, WORK], 'child')"),
    )?;
    assert_eq!(run.expr["value"], 0, "spawnCreep OK");
    let snap = e.get_object(&spawn_id).unwrap();
    assert_eq!(snap["store"]["energy"], 101, "扣 200 + 涓流 1");
    assert_eq!(snap["spawning"]["name"], "child");
    assert_eq!(snap["spawning"]["needTime"], 6);
    assert_eq!(
        snap["spawning"]["remainingTime"], 5,
        "spawnTime(6) − time(1)"
    );
    assert_eq!(
        snap["storeCapacity"],
        serde_json::Value::Null,
        "spawn 无参容量 null（typed 投影 storeCapacityResource）"
    );

    // 同名拒（下一 tick：真实雏形已在 Game.creeps）
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById({spawn_json}).spawnCreep([WORK], 'child')"),
    )?;
    assert_eq!(run.expr["value"], -3, "ERR_NAME_EXISTS");
    // 该 tick spawn 仍在孵化 → BUSY
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById({spawn_json}).spawnCreep([WORK], 'child2')"),
    )?;
    assert_eq!(run.expr["value"], -4, "ERR_BUSY（真实孵化任务）");

    // CREEP-SPAWNING-001/002/004（孵化中雏形）：spawning=true、ticksToLive
    // undefined（官方雏形不带 ageTime，creeps.js:78 的 undefined 分支——
    // runExpr 通道把 undefined 归 null）、body 出生前可见
    let run = e.run_player(
        "p1",
        "(()=>{ const c = Game.creeps['child']; return [c ? c.spawning : 'none', c ? c.ticksToLive : 'none', c.body.length]; })()",
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], true, "孵化中 spawning=true");
    assert_eq!(
        v[1],
        serde_json::Value::Null,
        "孵化中 ticksToLive undefined"
    );
    assert_eq!(v[2], 2, "出生前 body 可见");

    // 推进到出生（time 5 即 spawnTime−1）：雏形移出 spawn 格、spawning=false
    e.tick(3)?;
    let creeps = e.find_in_room("W1N1", 101);
    assert_eq!(creeps.len(), 1, "child 出生");
    assert_eq!(creeps[0]["name"], "child");
    assert_eq!(creeps[0]["spawning"], false);
    assert_ne!(
        (&creeps[0]["pos"]["x"], &creeps[0]["pos"]["y"]),
        (&serde_json::json!(25), &serde_json::json!(25)),
        "已移出 spawn 格（TOP 方向出生 → (25,24)）"
    );
    let snap = e.get_object(&spawn_id).unwrap();
    assert_eq!(snap["spawning"], serde_json::Value::Null, "任务清空");

    // renewCreep 全链：短命 creep +600 tick（effect=floor(1.2·1500/3/1)）
    let old_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [26, 25], "owner": "p1", "body": ["move"], "name": "old", "ticksToLive": 100 }),
    )?;
    let old_json = serde_json::to_string(&old_id)?;
    assert_eq!(e.get_object(&old_id).unwrap()["ticksToLive"], 100);
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById({spawn_json}).renewCreep(Game.getObjectById({old_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0, "renewCreep OK");
    // ageTime(time4+100) + 600 = 704，time 推进到 5 → 剩余 699
    assert_eq!(
        e.get_object(&old_id).unwrap()["ticksToLive"],
        699,
        "+600 续命"
    );

    // recycleCreep 全链：creep 移除 + 携带与全额造价回收进墓碑（B3——官方
    // recycle-creep.js:23 的 dropRate=1.0，非 0.2 缺省）
    let rich_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [24, 25], "owner": "p1", "body": ["move"], "name": "rich", "store": { "energy": 40 } }),
    )?;
    let rich_json = serde_json::to_string(&rich_id)?;
    let run = e.run_player(
        "p1",
        &format!("Game.getObjectById({spawn_json}).recycleCreep(Game.getObjectById({rich_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0, "recycleCreep OK");
    assert!(e.get_object(&rich_id).is_none(), "回收目标移除");
    let tombstones = e.find_in_room("W1N1", 118);
    assert_eq!(tombstones.len(), 1, "回收死亡产墓碑");
    assert_eq!(tombstones[0]["pos"]["x"], 24, "墓碑位置 = 死亡格");
    assert_eq!(tombstones[0]["creepName"], "rich", "尸体信息（creepName）");
    // 契约面（vanilla TombstoneSnapshot）只有 kind/id/pos/creepName/deathTime/
    // store/ticksToDecay 七键——creepBody 等引擎面字段不进查询快照
    assert_eq!(
        tombstones[0].as_object().unwrap().len(),
        7,
        "查询契约键面（vanilla snapshots.ts:405-414）"
    );
    e.teardown();
    Ok(())
}

/// createShard 契约校验（screeps-ok adapter.ts:45-48）：`controller: false` 与
/// 显式控制器字段互斥、非法地形拒绝——报错且**不改状态**（旧分片继续可用）。
fn embed_create_shard_validation_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "w1" }),
    )?;
    e.run_player("p1", "1")?;
    let time_before = e.get_game_time();

    // 互斥：controller:false + 显式 rcl
    let bad = json!({
        "players": ["p1"],
        "rooms": [{ "name": "W2N2", "controller": false, "rcl": 3 }],
    });
    let err = e.create_shard(&bad).unwrap_err();
    assert!(
        err.to_string().contains("controller: false takes no rcl"),
        "契约错误文案: {err}"
    );
    // 报错且不改状态：旧分片完好
    assert_eq!(e.get_game_time(), time_before, "时间未变");
    assert_eq!(e.find_in_room("W1N1", 101).len(), 1, "旧分片对象仍在");
    let run = e.run_player("p1", "Game.creeps ? Object.keys(Game.creeps).length : -1")?;
    assert_eq!(run.expr["value"], 1, "旧分片继续可跑");

    // 非法地形（长度不足）同样拒绝且不改状态
    let bad_terrain = json!({
        "players": ["p1"],
        "rooms": [{ "name": "W3N3", "terrain": [0, 1, 2] }],
    });
    assert!(e.create_shard(&bad_terrain).is_err());
    assert!(e.get_controller_pos("W3N3").is_none(), "坏分片未落任何房间");
    assert_eq!(e.find_in_room("W1N1", 101).len(), 1, "旧分片不受影响");
    e.teardown();
    Ok(())
}

/// typed world + 真结算经 embed 通道可见：source 重生回满、creep TTL 死亡产墓碑。
fn embed_real_settlement_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    // 归属 controller 房间的 source 容量 3000；放置 energy 100 → 未满挂 300 tick 钟
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "source", "energy": 100 }),
    )?;
    // 短命 creep：5 tick 后死亡，携带 50 energy 由墓碑承接
    e.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos": [20, 20], "owner": "p1", "body": ["move", "carry"],
            "store": { "energy": 50 }, "ticksToLive": 5,
        }),
    )?;

    // 首个 tick 不结算到位前：creep 存活
    e.tick(1)?;
    assert_eq!(e.find_in_room("W1N1", 101).len(), 1, "TTL 未到 creep 存活");

    // 越过 TTL：creep 死亡——B3（官方 _die.js）：产墓碑承接全部资源（M1
    // "资源直接掉地"灰区清偿）。body 2 件 ⇒ ticksToDecay = 2×5 = 10；TTL 死亡
    // 的造价回收 lifeRate = 0.2×1/1500 → floor 后 0，store 只含携带 50 energy；
    // 掉落只发生在墓碑衰减到点（此后每 tick 衰减 ceil(amount/ENERGY_DECAY)，
    // 官方 energy/tick.js）
    e.tick(4)?;
    assert_eq!(e.find_in_room("W1N1", 101).len(), 0, "TTL 到期 creep 死亡");
    assert!(
        e.find_in_room("W1N1", 106).is_empty(),
        "携带资源不再直接掉地（墓碑承接）"
    );
    let tombstones = e.find_in_room("W1N1", 118);
    assert_eq!(tombstones.len(), 1, "死亡产一枚墓碑（FIND_TOMBSTONES）");
    assert_eq!(tombstones[0]["store"]["energy"], 50, "墓碑承接携带 energy");
    assert_eq!(tombstones[0]["pos"]["x"], 20, "墓碑位置 = 死亡格");
    assert_eq!(
        tombstones[0]["ticksToDecay"], 9,
        "decayTime = 死亡 tick 4 + 2 部件 × 5 = 14；查询时 time 已推进到 5"
    );

    // 越过重生钟：source 回满 3000（放置 energy 100 + 300 tick 钟）
    e.tick(305)?;
    let sources = e.find_in_room("W1N1", 105);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["energy"], 3000, "重生钟到点回满");
    assert_eq!(sources[0]["energyCapacity"], 3000, "归属房间容量 3000");

    // 无名稳定名不因对象消亡而重名（序号源是单调 id 分配器，对象总数会回退）
    let id2 = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [22, 22], "owner": "p1", "body": ["move"], "ticksToLive": 2 }),
    )?;
    let name2 = e.get_object(&id2).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    e.tick(4)?; // 死亡（对象总数回退）
    let id3 = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [23, 23], "owner": "p1", "body": ["move"] }),
    )?;
    let name3 = e.get_object(&id3).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(name2, name3, "对象消亡后无名摆放不得重名");

    // runPlayer 的 +1 也是真实结算（玩家可见状态随结算推进）
    let run = e.run_player("p1", "Game.time")?;
    assert_eq!(
        run.expr["value"],
        e.get_game_time() - 1,
        "求值先于结算（上一 tick 视图）"
    );

    // setTerrain 守卫：首个玩家 tick 之后拒绝且世界不变（screeps-ok 契约；
    // DumbWorld 退役后此处是唯一测试锁定）
    let err = e.set_terrain("W1N1", vec![0; 2500]).unwrap_err();
    assert!(
        err.to_string().contains("第一个玩家 tick"),
        "守卫错误信息: {err}"
    );
    assert_eq!(
        e.find_in_room("W1N1", 105)[0]["energyCapacity"],
        3000,
        "世界不受失败的 setTerrain 影响"
    );
    e.teardown();
    Ok(())
}

/// runPlayers：全部玩家对同一世界快照求值（同一 Game.time），合计恰好 +1 tick。
fn embed_run_players_shared_snapshot_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    e.run_player("p1", "1")?; // time 0 → 1
    let t = e.get_game_time();
    let mut codes = std::collections::BTreeMap::new();
    codes.insert("p1".to_string(), "Game.time".to_string());
    codes.insert("p2".to_string(), "Game.time".to_string());
    let runs = e.run_players(&codes)?;
    assert_eq!(runs["p1"].expr["value"], t, "p1 看到结算前世界");
    assert_eq!(runs["p2"].expr["value"], t, "p2 与 p1 同一快照");
    assert_eq!(e.get_game_time(), t + 1, "合计恰好 +1 tick");
    e.teardown();
    Ok(())
}

fn embed_engine_roundtrip_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();

    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // 默认布局：(1,1) 有 controller
    assert_eq!(e.get_controller_pos("W1N1"), Some((1, 1)));

    // 摆放 + 快照
    let id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move", "work"], "store": { "energy": 30 } }),
    )?;
    let snap = e.get_object(&id).expect("对象存在");
    assert_eq!(snap["kind"], "creep", "查询面快照出契约 kind 字段");
    assert_eq!(snap["pos"]["x"], 25);
    assert_eq!(snap["owner"], "p1");

    // findInRoom
    assert_eq!(e.find_in_room("W1N1", 101).len(), 1, "FIND_CREEPS");
    // 104/108 与 prelude room.find 对齐（同一 FIND 语义两层一致）；
    // 104 官方条件 energy > 0（game.js:23），energy 缺省 3000
    assert_eq!(
        e.find_in_room("W1N1", 104).len(),
        0,
        "FIND_SOURCES_ACTIVE（本房间尚无 source）"
    );
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "source" }),
    )?;
    assert_eq!(
        e.find_in_room("W1N1", 104).len(),
        1,
        "FIND_SOURCES_ACTIVE 含有能量的 source（缺省 3000）"
    );
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 11], "structureType": "source", "energy": 0 }),
    )?;
    assert_eq!(
        e.find_in_room("W1N1", 104).len(),
        1,
        "energy:0 的 source 不算 active（官方 energy > 0）"
    );
    assert_eq!(e.find_in_room("W1N1", 105).len(), 2, "FIND_SOURCES 收全部");
    // FIND_MY_STRUCTURES 在视角中立面不可判定 → 空（A3c 修掉 MY_* 与
    // FIND_* 同臂的假绿面——screeps-ok find.ts 契约只支持 9 个中立常量；
    // 旧断言"仅 controller"是灰区登记的假绿）。归属语义由玩家面承载（下方
    // runPlayer 锁值：p1 的 MY_STRUCTURES = controller，source 不入）
    assert_eq!(
        e.find_in_room("W1N1", 108).len(),
        0,
        "FIND_MY_STRUCTURES 中立面无视角 → 空"
    );

    // runPlayer：真实沙箱求值，恰好 +1 tick
    let t0 = e.get_game_time();
    let run = e.run_player("p1", "Game.creeps ? Object.keys(Game.creeps).length : -1")?;
    assert!(run.executed);
    assert_eq!(run.expr["ok"], true, "求值成功: {:?}", run.expr);
    assert_eq!(run.expr["value"], 1, "玩家代码看见摆放的 creep");
    assert_eq!(e.get_game_time(), t0 + 1, "恰好推进 1 tick");

    // 玩家面 FIND 族 per-viewer 锁（A3c）：MY_STRUCTURES=controller（source
    // 不入——game.js:24 my===true 且 structureTypes 表无 source）、
    // HOSTILE_STRUCTURES 空、MY_CREEPS=本人 creep
    let run = e.run_player(
        "p1",
        "(()=>{ const r = Game.rooms['W1N1']; const st = r.find(FIND_MY_STRUCTURES).map(s=>s.structureType).sort(); \
         return [st.join(','), r.find(FIND_HOSTILE_STRUCTURES).length, r.find(FIND_MY_CREEPS).length, r.find(FIND_CREEPS).length]; })()",
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "controller", "MY_STRUCTURES 归属过滤 + source 不入");
    assert_eq!(v[1], 0, "HOSTILE_STRUCTURES 无敌对结构");
    assert_eq!(v[2], 1, "MY_CREEPS 本人");
    assert_eq!(v[3], 1, "FIND_CREEPS 全部（本房仅一只）");

    // Game.gcl 真值链（A3c）：upgrade 结算累点 PlayerState.gcl（settle/creep.rs
    // 的 player.gcl += boosted）→ 下一 tick world.gcl 投影 → prelude 正变换
    // （Game.gcl.progress）。行为探针：跨层链路而非单点锁值。upgradeController
    // 的 prelude API 面仍是桩（A4 族漏网，批次报告登记归 API 补漏批）——意图
    // 经 settle_external 直注（生产同形清洗路径），链路验证不受桩影响
    let upgrader_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [1, 2], "owner": "p1", "body": ["work", "carry"], "store": { "energy": 10 }, "name": "upgrader" }),
    )?;
    let run = e.run_player("p1", "Game.rooms['W1N1'].controller.id")?;
    let ctl_id = run.expr["value"]
        .as_str()
        .expect("controller id 字符串")
        .to_string();
    let batch = kk_engine::intent::IntentBatch::from_json(
        "p1",
        &json!({ upgrader_id.clone(): { "upgradeController": { "id": ctl_id } } }),
    );
    e.settle_external(batch)?;
    let run = e.run_player(
        "p1",
        "[Game.rooms['W1N1'].controller.progress, Game.gcl.level, Game.gcl.progress]",
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 1, "controller.progress 结算 +1");
    assert_eq!(v[1], 1, "1 点仍在 1 级");
    assert_eq!(v[2], 1, "upgrade +1 GCL 点 → Game.gcl.progress 跨层可见");

    // console 捕获
    let run = e.run_player("p1", "console.log('hello'); 42")?;
    assert_eq!(run.expr["value"], 42);
    assert_eq!(e.capture_console_logs("p1"), vec!["hello".to_string()]);

    // tick(n) 推进（五次 runPlayer + settle_external 各 +1 + tick(5) = t0+11）
    e.tick(5)?;
    assert_eq!(e.get_game_time(), t0 + 11);

    // 语法错误分类透传
    let run = e.run_player("p1", "var a = ;")?;
    assert_eq!(run.expr["kind"], "syntax");

    // 同一引擎二次 create_shard：完全重置（screeps-ok 套件每个测试都走的路径——
    // 旧沙箱回收、时间归零、handle 重建后 run_player 正常服务）
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W2N2", "rcl": 1, "owner": "p1" }],
    }))?;
    assert_eq!(e.get_game_time(), 0, "二次 createShard 时间重置");
    assert_eq!(e.get_controller_pos("W2N2"), Some((1, 1)), "新房间布局生效");
    assert!(e.get_controller_pos("W1N1").is_none(), "旧房间已清除");
    let run = e.run_player("p1", "Game.time")?;
    assert!(run.executed);
    assert_eq!(run.expr["value"], 0, "重置后的第一 tick：注入的世界 time=0");
    assert_eq!(e.get_game_time(), 1, "runPlayer 恰好 +1 tick");

    // createShard 的 PlayerSpec 对象形式（screeps-ok 契约 players: (string|PlayerSpec)[]）
    e.create_shard(&json!({
        "players": [{ "name": "p1", "gcl": { "level": 3 } }, "p3"],
        "rooms": [],
    }))?;
    let run = e.run_player("p1", "'obj-spec-ok'")?;
    assert!(run.executed, "对象形式 PlayerSpec 的玩家已注册且可执行");
    assert_eq!(run.expr["value"], "obj-spec-ok");
    // run_players 对未知 handle 显式报错（不再静默缺项）
    let mut codes = std::collections::BTreeMap::new();
    codes.insert("ghost".to_string(), "1".to_string());
    assert!(e.run_players(&codes).is_err(), "未知 handle 显式报错");

    // teardown 幂等
    e.teardown();
    e.teardown();
    Ok(())
}

/// runPlayer 契约分类：
/// ① Memory 不可序列化 → tick 末失败归 runtime 而非成功（adapter-spec:229）；
/// ② 已开始执行后硬中断 → runtime（executed=true）；
/// ③ 活对象返回值 → serialization（vanilla 的 constructor 检测口径）。
fn embed_run_player_contract_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // ① 写回失败不再是"成功 + console 报错"
    let run = e.run_player("p1", "Memory.big = 1n; 7")?;
    assert!(run.executed, "代码已执行");
    assert_eq!(run.expr["ok"], false, "写回失败不再是成功: {:?}", run.expr);
    assert_eq!(
        run.expr["kind"], "runtime",
        "tick 末序列化失败按契约归 runtime"
    );

    // 官方 _parsed 生命周期（评审 P1 纠偏后定案）：RawMemory 对象每 tick 重建
    // （runtime.js:102 是 _start 局部变量、make.js:160 每 tick 重调）——毒
    // _parsed 单 tick 生效；下一 tick 不访问 Memory 时无 _parsed → 写回跳过、
    // tick 成功，毒 raw 原样保留
    let run = e.run_player("p1", "1")?;
    assert_eq!(
        run.expr["value"], 1,
        "毒 _parsed 单 tick（下 tick 无保存、成功）"
    );

    // ② 死循环：已开始执行 → runtime（而非 executed:false 的基础设施错误）
    let run = e.run_player("p1", "while (true) {}")?;
    assert!(run.executed, "代码已开始执行（硬中断≠未执行）");
    assert_eq!(run.expr["kind"], "runtime", "硬中断按契约归 runtime");

    // ③ 活对象返回值
    let run = e.run_player("p1", "new RoomPosition(1,2,'W1N1')")?;
    assert_eq!(run.expr["kind"], "serialization", "活对象 → serialization");

    // ④ placeStructure 摆 spawn 可见（内部类别归一 + 无名输入生成稳定名称——
    //    上游 StructureSpec 没有 name 字段，无名是契约合法形态）
    let id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
    )?;
    let id_json = serde_json::to_string(&id)?;
    let code = format!(
        "[Object.keys(Game.spawns).length, \
         typeof Game.spawns[Object.keys(Game.spawns)[0]].name, \
         Game.rooms['W1N1'].find(FIND_MY_SPAWNS).length, \
         Game.getObjectById({id_json}) instanceof StructureSpawn]"
    );
    let run = e.run_player("p1", &code)?;
    let v = &run.expr["value"];
    assert_eq!(v[0], 1, "spawn 可见: {v:?}");
    assert_eq!(v[1], "string", "无名 spawn 生成了稳定名称: {v:?}");
    assert_eq!(v[2], 1, "FIND_MY_SPAWNS: {v:?}");
    assert_eq!(v[3], true, "instanceof StructureSpawn: {v:?}");
    e.teardown();
    Ok(())
}

/// 查询面快照契约（screeps-ok common.ts + vanilla snapshots.ts 对照）：
/// kind 映射、getter 剩余值/null 形态、正量 store、storeCapacity number、
/// flag 不进查询面。embed 查询面（snapshot.rs）与 prelude 世界投影
/// （world_json.rs）是两个消费面——本用例锁契约面的形状。
fn embed_snapshot_contract_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;

    // creep：kind/storeCapacity（CARRY×2×50=100）/0 值资源不出现/ticksToLive 剩余值
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [5, 5], "owner": "p1", "body": ["carry", "carry"], "ticksToLive": 100 }),
    )?;
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["kind"], "creep");
    assert_eq!(snap["storeCapacity"], 100, "CARRY×2 × CARRY_CAPACITY(50)");
    assert_eq!(
        snap["store"].as_object().unwrap().len(),
        0,
        "0 值 energy 不出现在 store"
    );
    assert_eq!(snap["ticksToLive"], 100);

    // spawn：storeCapacity 读 null（官方带 storeCapacityResource，无参
    // getCapacity() 为 null——store.js:28-33 + adapter-spec 明文）、spawning 无任务读 null
    let spawn_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [6, 6], "structureType": "spawn", "owner": "p1", "store": { "energy": 50 } }),
    )?;
    let snap = e.get_object(&spawn_id).unwrap();
    assert_eq!(snap["kind"], "structure");
    assert_eq!(snap["structureType"], "spawn");
    assert_eq!(snap["storeCapacity"], serde_json::Value::Null);
    assert_eq!(
        snap["spawning"],
        serde_json::Value::Null,
        "无孵化任务读 null"
    );
    assert_eq!(snap["store"]["energy"], 50);

    // extension：同 spawn——带 storeCapacityResource（build.js:131-135），读 null
    let ext_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [7, 6], "structureType": "extension", "owner": "p1" }),
    )?;
    let snap = e.get_object(&ext_id).unwrap();
    assert_eq!(
        snap["storeCapacity"],
        serde_json::Value::Null,
        "extension 无参 getCapacity() 读 null"
    );

    // source：独立 kind；满能量未挂钟读 null，放置即不满则预挂钟读剩余数
    // （place.rs 归一化预挂钟是 M1 既定语义——放置 energy<capacity 即挂 300 钟）
    let source_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [10, 10], "structureType": "source" }),
    )?;
    let snap = e.get_object(&source_id).unwrap();
    assert_eq!(snap["kind"], "source");
    assert_eq!(
        snap["ticksToRegeneration"],
        serde_json::Value::Null,
        "满能量未挂钟读 null"
    );
    let drained_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [11, 10], "structureType": "source", "energy": 100 }),
    )?;
    let snap = e.get_object(&drained_id).unwrap();
    assert_eq!(
        snap["ticksToRegeneration"], 300,
        "放置即不满：预挂钟读剩余数（M1 place 语义）"
    );

    // controller：8 级 progressTotal 读 null、isPowerEnabled 布尔、reservation/sign 占位 null
    let ctl = e.find_in_room("W1N1", 107)[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = e.get_object(&ctl).unwrap();
    assert_eq!(snap["structureType"], "controller");
    assert_eq!(snap["isPowerEnabled"], false);
    assert_eq!(snap["reservation"], serde_json::Value::Null);
    assert_eq!(snap["sign"], serde_json::Value::Null);

    // site：kind 'site'；resource：kind 'resource'
    let site_id = e.place_object(
        "W1N1",
        "constructionSite",
        &json!({ "pos": [15, 15], "structureType": "road" }),
    )?;
    let snap = e.get_object(&site_id).unwrap();
    assert_eq!(snap["kind"], "site");
    assert_eq!(snap["structureType"], "road");
    assert_eq!(snap["progress"], 0);

    let res_id = e.place_object(
        "W1N1",
        "resource",
        &json!({ "pos": [16, 16], "amount": 40 }),
    )?;
    let snap = e.get_object(&res_id).unwrap();
    assert_eq!(snap["kind"], "resource");
    assert_eq!(snap["resourceType"], "energy");
    assert_eq!(snap["amount"], 40);

    // mineral 经 structure 通道放置（适配器 placeMineral 真实路径）：kind 判
    // structureType 而非透传 type（契约要求 kind:'mineral'）
    let mineral_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [17, 16], "structureType": "mineral", "mineralType": "X", "mineralAmount": 2000 }),
    )?;
    let snap = e.get_object(&mineral_id).unwrap();
    assert_eq!(snap["kind"], "mineral", "structure 通道的 mineral 判 kind");
    assert_eq!(e.find_in_room("W1N1", 116)[0]["kind"], "mineral");

    // 跨面锁定：prelude 玩家面的 FIND_STRUCTURES 同步排除 source/mineral
    // （与查询面 matches_find 两层一致——官方 game.js:229-251 structureTypes 表）
    let run = e.run_player(
        "p1",
        "Game.rooms['W1N1'].find(FIND_STRUCTURES).map(s => s.structureType).sort().join(',')",
    )?;
    assert_eq!(
        run.expr["value"],
        "controller,extension,spawn", // mineral/source 均被排除
        "prelude find(FIND_STRUCTURES) 不含 source: {:?}",
        run.expr
    );

    // flag 不在查询面契约内（vanilla snapshotObject 无 flag 分支）
    let flag_id = e.place_object("W1N1", "flag", &json!({ "pos": [3, 3], "name": "f1" }))?;
    assert!(
        e.get_object(&flag_id).is_none(),
        "flag 的 getObject 读 null"
    );
    assert!(
        e.find_in_room("W1N1", 110).is_empty(),
        "FIND_FLAGS 不在查询面中性选择器内"
    );

    // 玩家面不受查询面影响：prelude 仍能看见 flag 与 Game.flags
    let run = e.run_player(
        "p1",
        "[Game.flags ? Object.keys(Game.flags).length : -1, \
          Game.rooms['W1N1'].find(FIND_FLAGS).length]",
    )?;
    assert_eq!(
        run.expr["value"][0], 1,
        "prelude 玩家面 flag 可见: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"][1], 1,
        "prelude find(FIND_FLAGS) 不受查询面影响"
    );
    e.teardown();
    Ok(())
}

/// M2A-A2 intent 通道端到端（tick 契约场景 1）：真实沙箱的 move/harvest 经
/// 清洗、结算，下一轮玩家视图可见；假沙箱（ScriptedSandbox）与真沙箱走同一条
/// 结算入口（Engine::tick 与 tick_with_intents 汇合于 settle_room），相同
/// intent 序列产出相同世界转移。
fn embed_intent_channel_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move", "work", "carry"], "name": "w1" }),
    )?;
    let source_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [24, 25], "structureType": "source" }),
    )?;
    let src_json = serde_json::to_string(&source_id)?;

    // 真沙箱 move(BOTTOM)：预检过返回 OK，下一 tick 视图位置 (25,26)
    let run = e.run_player("p1", "Game.creeps['w1'].move(5)")?;
    assert_eq!(run.expr["value"], 0, "预检通过返回 OK");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["pos"]["x"], 25, "x 不变");
    assert_eq!(snap["pos"]["y"], 26, "BOTTOM 落格 (25,26)");

    // 真沙箱 harvest（相邻 source）：WORK×HARVEST_POWER(2) 进 store、source 扣减
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].harvest(Game.getObjectById({src_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0);
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 2, "1×WORK × 2");
    let src_snap = e.get_object(&source_id).unwrap();
    assert_eq!(src_snap["energy"], 2998, "3000 − 2");
    e.teardown();

    // ---- 同一入口等价：ScriptedSandbox 相同 intent 序列 → 相同世界转移 ----
    let mut world = kk_world::World::new(1);
    let mut terrain = kk_core::Terrain::plains();
    terrain.normalize();
    world.register_room("W1N1", terrain);
    world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 1,
                y: 1,
                level: 3,
                safe_mode: 0,
                safe_mode_available: 1,
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
        .unwrap();
    let part = |k: kk_world::types::BodyKind| kk_schema::room::BodyPart {
        kind: k.code(),
        hits: 100,
        boost: None,
    };
    let fake_creep = world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Creep(kk_schema::room::CreepObj {
                id: 0,
                x: 25,
                y: 25,
                fatigue: 0,
                age_time: 1500,
                hits: 200,
                spawning: false,
                action_log: 0,
                name: "w1".into(),
                owner: "p1".into(),
                body: vec![
                    part(kk_world::types::BodyKind::Move),
                    part(kk_world::types::BodyKind::Work),
                    part(kk_world::types::BodyKind::Carry),
                ],
                store: Default::default(),
                store_extra: Vec::new(),
            }),
        )
        .unwrap();
    let fake_source = world
        .insert(
            "W1N1",
            kk_schema::room::RoomObject::Source(kk_schema::room::SourceObj {
                id: 0,
                x: 24,
                y: 25,
                energy: 3000,
                energy_capacity: 3000,
                next_regeneration_time: None,
            }),
        )
        .unwrap();
    let mut sandbox = kk_engine::ScriptedSandbox::new();
    let mut move_batch = kk_engine::intent::IntentBatch::default();
    move_batch.add_object_intent(
        "p1",
        fake_creep,
        kk_engine::intent::Action::Move,
        kk_engine::intent::IntentData {
            direction: Some(5),
            ..kk_engine::intent::IntentData::bare()
        },
    );
    let mut harvest_batch = kk_engine::intent::IntentBatch::default();
    harvest_batch.add_object_intent(
        "p1",
        fake_creep,
        kk_engine::intent::Action::Harvest,
        kk_engine::intent::IntentData::target(fake_source),
    );
    sandbox.at(0, move_batch);
    sandbox.at(1, harvest_batch);
    let mut engine = kk_engine::Engine::new(world);
    engine.sandboxes.insert("p1".into(), Box::new(sandbox));
    engine.tick().map_err(|err| anyhow::anyhow!("{err}"))?;
    engine.tick().map_err(|err| anyhow::anyhow!("{err}"))?;
    let (x, y, energy, src_energy) = match engine.world.get(fake_creep).unwrap().1 {
        kk_schema::room::RoomObject::Creep(c) => match engine.world.get(fake_source).unwrap().1 {
            kk_schema::room::RoomObject::Source(s) => {
                (c.x, c.y, c.store.energy as i64, s.energy as i64)
            }
            _ => panic!(),
        },
        _ => panic!(),
    };
    assert_eq!(
        (x, y, energy, src_energy),
        (25, 26, 2, 2998),
        "假沙箱与真沙箱同一结算入口：世界转移一致"
    );
    Ok(())
}

/// 外部复审 P1-2/P1-3 修复的回归锁：per-viewer 视野裁切（不可见房/对象/集合）
/// 与工地总量帽（伪造批量 room intent 也被 100 帽挡住）。
fn embed_per_viewer_and_caps_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N2", "rcl": 3, "owner": "p2" },
        ],
    }))?;
    let mine = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p1", "body": ["move"], "name": "mine" }),
    )?;
    let secret = e.place_object(
        "W2N2",
        "creep",
        &json!({ "pos": [25, 25], "owner": "p2", "body": ["move"], "name": "secret" }),
    )?;
    let secret_json = serde_json::to_string(&secret)?;

    // P1-3：p1 的视图——看不见 W2N2、看不见 secret、Game.creeps 只收自家
    let run = e.run_player(
        "p1",
        &format!(
            "[Object.keys(Game.rooms).sort().join(','), Object.keys(Game.creeps).join(','), String(Game.getObjectById({secret_json}) == null)]"
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "W1N1", "敌方房间不进 Game.rooms: {v:?}");
    assert_eq!(v[1], "mine", "Game.creeps 只收本人: {v:?}");
    // getObjectById 对不可见对象返回 null（官方同构；== null 同时覆盖 undefined）
    assert_eq!(v[2], "true", "敌方对象 getObjectById 得 null: {v:?}");

    // p2 的视图对偶
    let run = e.run_player("p2", "Object.keys(Game.rooms).join(',')")?;
    assert_eq!(run.expr["value"], "W2N2", "p2 只见自家房");

    // 我方对象的 my 判定
    let mine_json = serde_json::to_string(&mine)?;
    let run = e.run_player("p1", &format!("Game.getObjectById({mine_json}).my"))?;
    assert_eq!(run.expr["value"], true);

    // P1-2：批量 room intent（engine 直驱，等价伪造面）被总量帽挡住——
    // 120 条 createConstructionSite 只落 100
    let mut room_intents = Vec::new();
    for i in 0..120 {
        room_intents.push(json!({
            "roomName": "W1N1", "x": (i % 40) + 5, "y": (i / 40) * 10 + 5,
            "structureType": "road"
        }));
    }
    let list = json!({ "room": { "createConstructionSite": room_intents } });
    let batch = kk_engine::intent::IntentBatch::from_json("p1", &list);
    let sites_before = e.find_in_room("W1N1", 111).len();
    e.settle_external(batch)?;
    let sites_after = e.find_in_room("W1N1", 111).len();
    assert_eq!(
        sites_after - sites_before,
        100,
        "MAX_CONSTRUCTION_SITES 总量帽：120 条只落 100"
    );

    // 自家工地全集（官方 data.js:103 按 user 全量拉 + game.js:366-374）：工地
    // 不授房视野（data.js:113-116），但 Game.constructionSites 收全部自家工地
    // 含不可见房——两个语义正交（对抗评审 P2 的回归锁：world 破例导出 +
    // buildGame visibleRooms 排除工地）
    let own_site_in_hidden_room = e.place_object(
        "W2N2",
        "constructionSite",
        &json!({ "pos": [15, 15], "owner": "p1", "structureType": "road" }),
    )?;
    let site_json = serde_json::to_string(&own_site_in_hidden_room)?;
    let run = e.run_player(
        "p1",
        &format!(
            "[Object.keys(Game.rooms).sort().join(','), \
             Object.keys(Game.constructionSites).join(','), \
             String(Game.getObjectById({site_json}) !== null), \
             String(Game.rooms['W2N2'])]"
        ),
    )?;
    let v = &run.expr["value"];
    assert_eq!(v[0], "W1N1", "工地不授房视野：W2N2 不进 Game.rooms: {v:?}");
    let sites_keys = v[1].as_str().unwrap_or_default();
    assert!(
        sites_keys
            .split(',')
            .any(|k| k == site_json.trim_matches('"')),
        "自家工地全集进 Game.constructionSites（含不可见房，此前帽测试的 100 工地 + 新 1）: {v:?}"
    );
    assert_eq!(v[2], "true", "不可见房自家工地 getObjectById 可达: {v:?}");
    assert_eq!(
        v[3], "undefined",
        "工地所在不可见房 Game.rooms 条目 undefined: {v:?}"
    );
    Ok(())
}

/// A4-ii 资源族四方法端到端（prelude 换真 ↔ 执行器已有结算的接线）：
/// transfer/withdraw/drop/pickup 的能量流经真实投影（含 extension 容量按
/// rcl 折算的 prelude/执行器双源一致）。
fn embed_resource_family_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    }))?;
    let creep_id = e.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos": [25, 25], "owner": "p1", "body": ["carry", "carry", "move"], "name": "w1",
            "store": { "energy": 80 },
        }),
    )?;
    let ext_id = e.place_object(
        "W1N1",
        "structure",
        &json!({
            "pos": [26, 24], "structureType": "extension", "owner": "p1",
            "store": { "energy": 0 },
        }),
    )?;
    let ext_json = serde_json::to_string(&ext_id)?;

    // transfer：缺省钳到 rcl3 extension 容量 50（prelude 折算）＝ 执行器
    // extension_cap(rcl) 钳制（settle/creep.rs）——双源一致的能量转移
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].transfer(Game.getObjectById({ext_json}), RESOURCE_ENERGY)"),
    )?;
    assert_eq!(run.expr["value"], 0, "transfer 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 30, "80 − 50（缺省钳到目标容量）");
    let ext = e.get_object(&ext_id).unwrap();
    assert_eq!(ext["store"]["energy"], 50, "extension 0 → 50");

    // withdraw：缺省 min(空容量 70, 存量 50) = 50 全量取回
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].withdraw(Game.getObjectById({ext_json}), RESOURCE_ENERGY)"),
    )?;
    assert_eq!(run.expr["value"], 0, "withdraw 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 80, "30 + 50");
    let ext = e.get_object(&ext_id).unwrap();
    // snapshot 契约：空 store 折叠为 {}（snap_store），0 值键缺省
    assert_eq!(
        ext["store"]["energy"].as_i64().unwrap_or(0),
        0,
        "extension 50 → 0"
    );

    // drop 30：同格生成 resource 堆
    let run = e.run_player("p1", "Game.creeps['w1'].drop(RESOURCE_ENERGY, 30)")?;
    assert_eq!(run.expr["value"], 0, "drop 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 50, "80 − 30");
    let dropped = e.find_in_room("W1N1", 106);
    assert_eq!(dropped.len(), 1, "drop 落地一个 resource");
    // 掉落当 tick 即经 energy 衰减（objects/tick：ceil(30/1000)=1）→ 29
    assert_eq!(dropped[0]["amount"], 29, "掉落 30 − 当 tick 衰减 1");
    assert_eq!(dropped[0]["pos"]["x"], 25, "掉落位置 = creep 格");
    let pile_id = dropped[0]["id"].as_str().unwrap().to_string();

    // pickup：整堆拾取（余量 50 ≥ 30）
    let pile_json = serde_json::to_string(&pile_id)?;
    let run = e.run_player(
        "p1",
        &format!("Game.creeps['w1'].pickup(Game.getObjectById({pile_json}))"),
    )?;
    assert_eq!(run.expr["value"], 0, "pickup 预检通过");
    let snap = e.get_object(&creep_id).unwrap();
    assert_eq!(snap["store"]["energy"], 79, "50 + 29（衰减后余量）");
    assert!(e.get_object(&pile_id).is_none(), "堆清零移除");
    assert!(
        e.find_in_room("W1N1", 106).is_empty(),
        "房间无残留 resource"
    );
    e.teardown();
    Ok(())
}

/// A4-iv 移动杂项族闭环（prelude API → intentsList 清洗 → settle）：
/// pull+move(puller) 配对位移（被拉者无 MOVE 照样动、疲劳归链拉者）；
/// suicide 当 tick 死亡并掉落；say 免费且不影响位移。
fn embed_move_misc_family_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;
    // 拉者 [move,work]@(10,10)：被拉者 [work]@(11,10)（无 MOVE、无 fatigue）
    let _puller = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [10, 10], "owner": "p1", "body": ["move", "work"], "name": "puller" }),
    )?;
    let _pulled = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [11, 10], "owner": "p1", "body": ["work"], "name": "pulled" }),
    )?;
    // 自杀者携带 40 energy
    let _martyr = e.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos": [20, 20], "owner": "p1", "body": ["move", "carry"],
            "name": "martyr", "store": { "energy": 40 }
        }),
    )?;

    let run = e.run_player(
        "p1",
        r#"
        Game.creeps['puller'].pull(Game.creeps['pulled']);
        Game.creeps['pulled'].move(Game.creeps['puller']);
        Game.creeps['puller'].move(LEFT);
        Game.creeps['martyr'].suicide();
        Game.creeps['martyr'].say('bye', true);
        Game.creeps['puller'].say('hi');
    "#,
    )?;
    assert!(run.executed, "玩家代码应真实执行");

    // 拉者 (10,10)→(9,10)；被拉者 (11,10)→(10,10)（补拉者让位格——无 MOVE
    // 经 pull 配对移动，官方 canMove 的 _pulled 分支）
    let by_name = |e: &EmbedEngine, name: &str| {
        e.find_in_room("W1N1", 101)
            .into_iter()
            .find(|o| o["name"] == name)
            .unwrap_or_else(|| panic!("{name} 应存活"))
            .clone()
    };
    let puller = by_name(&e, "puller");
    assert_eq!(
        (puller["pos"]["x"].clone(), puller["pos"]["y"].clone()),
        (json!(9), json!(10))
    );
    let pulled = by_name(&e, "pulled");
    assert_eq!(
        (pulled["pos"]["x"].clone(), pulled["pos"]["y"].clone()),
        (json!(10), json!(10))
    );
    // 疲劳归链：拉者自身 1 work×2 + 被拉者 1 work×2 归链 = 4，1 MOVE 恢复 -2
    assert_eq!(puller["fatigue"], json!(2), "被拉者的移动疲劳记在拉者头上");
    assert_eq!(pulled["fatigue"], json!(0), "被拉者无疲劳");

    // suicide：当 tick消亡——B3 墓碑承接（官方 _die.js）：携带 40 + 造价回收
    // min(125,50×0.2)×2 = 20（dropRate 0.2、TTL 满寿 lifeRate=0.2）⇒ store 60；
    // 掉落发生在墓碑衰减到点（2 部件 ⇒ 10 tick）
    assert!(
        e.find_in_room("W1N1", 101)
            .into_iter()
            .all(|o| o["name"] != "martyr"),
        "suicide 当 tick 死亡"
    );
    let martyrs_tomb: Vec<_> = e
        .find_in_room("W1N1", 118)
        .into_iter()
        .filter(|o| o["pos"]["x"] == 20 && o["pos"]["y"] == 20)
        .collect();
    assert_eq!(martyrs_tomb.len(), 1, "suicide 产墓碑");
    assert_eq!(
        martyrs_tomb[0]["store"]["energy"],
        json!(60),
        "携带 40 + 造价回收 20"
    );
    assert!(e.find_in_room("W1N1", 106).is_empty(), "不直接掉地");
    e.teardown();
    Ok(())
}

/// B4 合入冒烟锁值：embed place mineral/extractor 直通道（裸 kind——原落
/// Generic 透传致 FIND 失配，B4 评审 3 P3-1 登记项清偿）与玩家面
/// FIND_MINERALS/FIND_STRUCTURES 命中。并入主套件（embed 单测试不变量——
/// 独立 #[test] 会留 LIFO 违规，A5-lite P2-I 同款教训）。
fn embed_mineral_direct_channel_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 6, "owner": "p1" }],
    }))
    .unwrap();
    let id = e
        .place_object(
            "W1N1",
            "mineral",
            &serde_json::json!({ "pos": [10, 10], "mineralType": "U", "density": 3 }),
        )
        .unwrap();
    let snap = e.get_object(&id).expect("getObject");
    assert_eq!(snap["kind"], "mineral");
    assert_eq!(snap["mineralType"], "U");
    assert_eq!(
        e.find_in_room("W1N1", 116).len(),
        1,
        "FIND_MINERALS 契约面命中"
    );
    let ex = e
        .place_object(
            "W1N1",
            "extractor",
            &serde_json::json!({ "pos": [10, 10], "owner": "p1" }),
        )
        .unwrap();
    assert_eq!(e.get_object(&ex).unwrap()["structureType"], "extractor");
    let structures: Vec<String> = e
        .find_in_room("W1N1", 107)
        .iter()
        .filter_map(|v| v["structureType"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        structures,
        vec!["controller".to_string(), "extractor".to_string()],
        "extractor 进 FIND_STRUCTURES（mineral 不进）"
    );
    Ok(())
}

/// GPT 外部复审（74c16d5..d5de6ad）五条 P1 的清偿行为探针：P1-1 可见房/
/// P1-2 tombstone·ruin 提取/P1-3 冷却预检/P1-5 raw 事件日志不可污染
/// （P1-4 destroy 无事件由 settle 测试 destroy_structure_intent_paths 锁定）。
fn embed_gpt_review_clearance_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1", "controller": true }],
    }))?;
    // P1-1：无 controller 房只放己方墓碑——官方 data.js:103-116 按用户对象
    // 归集（只排除 flag/工地），房必须在 Game.rooms
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [
            { "name": "W1N1", "rcl": 3, "owner": "p1" },
            { "name": "W2N2", "controller": false },
        ],
    }))?;
    let tomb = e.place_object(
        "W2N2",
        "tombstone",
        &serde_json::json!({
            "pos": [10, 10], "store": { "energy": 40 }, "ticksToDecay": 50,
            "creepName": "dead", "creepBody": ["carry"], "user": "p1",
        }),
    )?;
    let run = e.run_player(
        "p1",
        "Game.rooms['W2N2'] ? Game.rooms['W2N2'].name : 'MISSING'",
    )?;
    assert_eq!(
        run.expr["value"], "W2N2",
        "P1-1：仅剩己方墓碑的房必须在 Game.rooms"
    );

    // P1-2：withdraw 从墓碑提取（官方 creeps.js:507-515 接受 Tombstone 目标）
    let c = e.place_object(
        "W2N2",
        "creep",
        &serde_json::json!({
            "pos": [10, 11], "owner": "p1", "body": ["carry"], "name": "w1",
            "store": { "energy": 0 },
        }),
    )?;
    let run = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById({:?}).withdraw(Game.getObjectById({:?}), RESOURCE_ENERGY, 10)",
            c, tomb
        ),
    )?;
    assert_eq!(run.expr["value"], 0, "P1-2：withdraw 墓碑返回 OK");
    let run = e.run_player("p1", &format!("Game.getObjectById({:?}).store.energy", c))?;
    assert_eq!(run.expr["value"], 10, "P1-2：提取 10 落袋（原静默丢弃）");
    let snap = e.get_object(&tomb).unwrap();
    assert_eq!(snap["store"]["energy"], 30, "P1-2：墓碑扣 10");

    // P1-3：extractor cooldown 预检（官方 structures.js:268 getter + creeps.js:383
    // 非 0 冷却 → ERR_TIRED=-6）
    e.create_shard(&serde_json::json!({
        "players": ["p1"],
        "rooms": [{ "name": "W3N3", "rcl": 6, "owner": "p1" }],
    }))?;
    let m = e.place_object(
        "W3N3",
        "mineral",
        &serde_json::json!({
            "pos": [10, 10], "mineralType": "U", "density": 3,
        }),
    )?;
    e.place_object(
        "W3N3",
        "extractor",
        &serde_json::json!({
            "pos": [10, 10], "owner": "p1", "cooldown": 5,
        }),
    )?;
    let c3 = e.place_object(
        "W3N3",
        "creep",
        &serde_json::json!({
            "pos": [10, 11], "owner": "p1", "body": ["work", "carry"], "name": "m1",
        }),
    )?;
    let run = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById({:?}).harvest(Game.getObjectById({:?}))",
            c3, m
        ),
    )?;
    assert_eq!(
        run.expr["value"], -11,
        "P1-3：冷却期 harvest → ERR_TIRED=-11（原假 OK）"
    );
    let snap = e.get_object(&m).unwrap();
    assert_eq!(snap["mineralAmount"], 70_000, "P1-3：矿量不动");

    // P1-5：修改非 raw 解析数组不得污染 raw 字符串（官方 rooms.js:573-583
    // raw 返回持久化原始串）
    let run = e.run_player(
        "p1",
        "(()=>{ const log = Game.rooms['W3N3'].getEventLog(); \
         if (log.length > 0) { log[0].data = { hacked: true }; } \
         return Game.rooms['W3N3'].getEventLog(true); })()",
    )?;
    let raw = run.expr["value"].as_str().unwrap_or("");
    assert!(
        !raw.contains("hacked"),
        "P1-5：raw 不得被解析数组改写污染: {raw}"
    );
    Ok(())
}
/// A5-core 片 1 行为探针：Memory/RawMemory/模块系统/Game 可写性的官方形态
/// （对照 screeps-ok UNDOC-MEMHACK 族与官方 game.js/runtime.js 逐条）。
fn a5_memory_module_forms_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 1, "owner": "p1" }],
    }))?;

    // ① Memory 描述符生命周期（MEMHACK-001/012）：tick 起 getter 无 setter 可
    // 配置；首访翻转为值属性
    let run = e.run_player(
        "p1",
        r#"(() => {
        const d0 = Object.getOwnPropertyDescriptor(globalThis, 'Memory');
        void Memory;
        const d1 = Object.getOwnPropertyDescriptor(globalThis, 'Memory');
        return {
            startGetter: typeof d0.get === 'function' && d0.set === undefined && d0.configurable,
            afterValue: 'value' in d1 && d1.get === undefined && d1.configurable && d1.enumerable,
        };
    })()"#,
    )?;
    assert_eq!(
        run.expr["value"]["startGetter"], true,
        "tick 起 getter 形态: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["afterValue"], true,
        "首访固化值属性: {:?}",
        run.expr
    );

    // ② RawMemory._parsed 身份与 set 失效（MEMHACK-004/007 尾拍）：首访后
    // _parsed === Memory 引用；set 即删 _parsed → 写回跳过、下一 tick 从新 raw 重解析
    let run = e.run_player(
        "p1",
        r#"(() => { Memory.keep = 1; return RawMemory._parsed === Memory; })()"#,
    )?;
    assert_eq!(
        run.expr["value"], true,
        "_parsed 与 Memory 同引用: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"(() => { void Memory; Memory.mut = 'x'; RawMemory.set('{"fromSet":true}'); return 'ok'; })()"#)?;
    assert_eq!(run.expr["value"], "ok");
    let run = e.run_player(
        "p1",
        r#"({ raw: RawMemory.get(), mut: Memory.mut, fromSet: Memory.fromSet })"#,
    )?;
    assert_eq!(
        run.expr["value"]["raw"], r#"{"fromSet":true}"#,
        "set 后写回未覆写: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["mut"],
        serde_json::Value::Null,
        "陈旧 mutation 未落 raw"
    );
    assert_eq!(
        run.expr["value"]["fromSet"], true,
        "下一 tick 从新 raw 重解析"
    );

    // ③ 解析失败 → Memory null、raw 保留、写回跳过（官方 game.js:479-487）
    let run = e.run_player("p1", r#"RawMemory.set('{broken'); 'set-ok'"#)?;
    assert_eq!(run.expr["value"], "set-ok");
    let run = e.run_player("p1", r#"({ m: Memory === null, raw: RawMemory.get() })"#)?;
    assert_eq!(
        run.expr["value"]["m"], true,
        "解析失败 Memory=null: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["raw"], "{broken",
        "毒 raw 原样保留（不写回）"
    );

    // ④ RawMemory.set 双门消息形态（官方 runtime.js:110-114）
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set(5); 'no-throw' } catch (e) { e.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Raw memory value is not a string",
        "非字符串门: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set('x'.repeat(2*1024*1024+1)); 'no-throw' } catch (e) { e.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Raw memory length exceeded 2 MB limit",
        "2MB 门: {:?}",
        run.expr
    );

    // ⑤ delete+assign 绕过解析（MEMHACK-003）与 Game 可写性（官方平赋值）
    let run = e.run_player(
        "p1",
        r#"(() => {
        delete globalThis.Memory;
        globalThis.Memory = { injected: true };
        const before = Memory.injected;
        Game = { hacked: true };
        return { injected: before, gameHacked: Game.hacked === true };
    })()"#,
    )?;
    assert_eq!(
        run.expr["value"]["injected"], true,
        "delete+assign 绕过: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["gameHacked"], true,
        "Game 平赋值可写: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"({ g: Game.hacked === undefined, t: Game.time })"#)?;
    assert_eq!(
        run.expr["value"]["g"], true,
        "下一 tick Game 重装: {:?}",
        run.expr
    );

    // ⑥ spawn.memory 官方形态（structures.js:865-885，本批新增）
    let spawn_id = e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [20, 20], "structureType": "spawn", "owner": "p1" }),
    )?;
    let spawn_json = serde_json::to_string(&spawn_id)?;
    let run = e.run_player(
        "p1",
        &format!(
            r#"(() => {{
        RawMemory.set('{{}}'); // ③ 毒化后的 raw 在此洗净（同 tick 重解析生效）
        const s = Game.getObjectById({spawn_json});
        s.memory.job = 'harvest';
        return {{ job: s.memory.job, inMem: Memory.spawns[s.name].job }};
    }})()"#
        ),
    )?;
    assert_eq!(
        run.expr["value"]["job"], "harvest",
        "spawn.memory 读写: {:?}",
        run.expr
    );
    assert_eq!(
        run.expr["value"]["inMem"], "harvest",
        "spawn.memory 落 Memory.spawns: {:?}",
        run.expr
    );

    // ⑦ 盲区补锁（评审 3 清单）：RawMemory 每 tick 新建（对象身份）；require
    // 负向（.js 补全/ ../ 均不存在→Unknown module 原名）；非对象 JSON 原样
    // 保留（Memory === '5'）；顶层 __proto__=null（hasOwnProperty 不可用）；
    // String 包装对象过 _.isString 门
    let run = e.run_player("p1", r#"globalThis.__rm = RawMemory; 'captured'"#)?;
    assert_eq!(run.expr["value"], "captured");
    let run = e.run_player(
        "p1",
        r#"({ fresh: RawMemory !== globalThis.__rm, raw: RawMemory.get() })"#,
    )?;
    assert_eq!(
        run.expr["value"]["fresh"], true,
        "RawMemory 每 tick 新建: {:?}",
        run.expr
    );
    assert!(
        run.expr["value"]["raw"]
            .as_str()
            .is_some_and(|r| r.contains("spawns")),
        "raw 字符串本体跨 tick 持久（⑥ 的正常写回已洗净 ③ 的毒 raw）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { require('nope.js'); 'no-throw' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Unknown module 'nope.js'",
        "无 .js 补全（官方 :553）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { require('../x'); 'no-throw' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "Unknown module '../x'",
        "无 ../ 解析: {:?}",
        run.expr
    );
    let run = e.run_player("p1", r#"RawMemory.set('"5"'); Memory === '5'"#)?;
    assert_eq!(
        run.expr["value"], true,
        "合法 JSON 非对象值原样保留: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"RawMemory.set('{"a":1}'); void Memory.a; try { Memory.hasOwnProperty('a'); 'callable' } catch (err) { 'throws' }"#,
    )?;
    assert_eq!(
        run.expr["value"], "throws",
        "顶层 __proto__=null（hasOwnProperty 不可用）: {:?}",
        run.expr
    );
    let run = e.run_player(
        "p1",
        r#"try { RawMemory.set(new String('x')); 'passed' } catch (err) { err.message }"#,
    )?;
    assert_eq!(
        run.expr["value"], "passed",
        "String 包装对象过 _.isString 门: {:?}",
        run.expr
    );
    Ok(())
}
