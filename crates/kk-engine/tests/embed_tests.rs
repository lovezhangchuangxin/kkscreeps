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
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
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

/// typed world + 真结算经 embed 通道可见：source 重生回满、creep TTL 死亡掉能量。
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
    // 短命 creep：5 tick 后死亡，携带 50 energy 掉地
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

    // 越过 TTL：creep 死亡，50 energy 落在同格（死亡 tick 立即检查——
    // 掉落 energy 此后每 tick 衰减 ceil(amount/ENERGY_DECAY)，官方 energy/tick.js）
    e.tick(4)?;
    assert_eq!(e.find_in_room("W1N1", 101).len(), 0, "TTL 到期 creep 死亡");
    let dropped = e.find_in_room("W1N1", 106);
    assert_eq!(dropped.len(), 1, "死亡掉落一个 resource");
    assert_eq!(dropped[0]["amount"], 50, "掉落数量 = 携带 energy");
    assert_eq!(dropped[0]["pos"]["x"], 20, "掉落位置 = 死亡格");

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
    // FIND_MY_STRUCTURES 不含 source/mineral（官方 game.js:229-251 structureTypes
    // 表；与 prelude room.find 两层一致）——房间仅默认 controller 入选
    assert_eq!(
        e.find_in_room("W1N1", 108).len(),
        1,
        "FIND_MY_STRUCTURES 仅 controller（source 不入）"
    );

    // runPlayer：真实沙箱求值，恰好 +1 tick
    let t0 = e.get_game_time();
    let run = e.run_player("p1", "Game.creeps ? Object.keys(Game.creeps).length : -1")?;
    assert!(run.executed);
    assert_eq!(run.expr["ok"], true, "求值成功: {:?}", run.expr);
    assert_eq!(run.expr["value"], 1, "玩家代码看见摆放的 creep");
    assert_eq!(e.get_game_time(), t0 + 1, "恰好推进 1 tick");

    // console 捕获
    let run = e.run_player("p1", "console.log('hello'); 42")?;
    assert_eq!(run.expr["value"], 42);
    assert_eq!(e.capture_console_logs("p1"), vec!["hello".to_string()]);

    // tick(n) 推进
    e.tick(5)?;
    assert_eq!(e.get_game_time(), t0 + 7);

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

    // 正常路径不受影响（含上一 tick 的坏 Memory 已被保留、不炸后续）
    let run = e.run_player("p1", "1")?;
    assert_eq!(run.expr["value"], 1);

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
