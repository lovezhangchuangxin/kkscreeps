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

    // teardown 幂等
    e.teardown();
    e.teardown();
    Ok(())
}
