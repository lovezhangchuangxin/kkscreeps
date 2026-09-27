//! 嵌入式 API 的集成测试（M0）：createShard → place → runPlayer → tick → 读状态。

use serde_json::json;

use kk_engine::EmbedEngine;

#[test]
fn embed_engine_roundtrip() -> anyhow::Result<()> {
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
