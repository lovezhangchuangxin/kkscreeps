//! 嵌入式 API 的集成测试（M0）：createShard → place → runPlayer → tick → 读状态。

use serde_json::json;

use kk_engine::EmbedEngine;

#[test]
fn embed_engine_suite() -> anyhow::Result<()> {
    // 两个场景必须串行（同进程多 isolate 的 LIFO 销毁纪律；并行会互相干扰销毁顺序）
    embed_engine_roundtrip_impl()?;
    embed_run_player_contract_impl()?;
    assert_eq!(kk_runner::lifo_violations(), 0, "LIFO 纪律零违规");
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
    assert_eq!(snap["type"], "creep");
    assert_eq!(snap["pos"]["x"], 25);
    assert_eq!(snap["owner"], "p1");

    // findInRoom
    assert_eq!(e.find_in_room("W1N1", 101).len(), 1, "FIND_CREEPS");

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

/// runPlayer 契约分类（第四轮评审修复的验证）：
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
