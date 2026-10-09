//! 宿主场景设置不暗中推进时间，也不能改变待重试的玩家结果。
use super::*;

pub(super) fn sim_setup_preserves_tick_and_failure_candidates_impl() -> anyhow::Result<()> {
    let mut e = EmbedEngine::new();
    e.create_shard(&json!({"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1"}]}))?;
    let mut modules = kk_runner::ModuleTable::new();
    modules.insert_js(
        "main",
        "exports.loop = () => { Memory.count += require('helper').delta; };",
    );
    modules.insert_js("helper", "exports.delta = 2;");
    assert!(e.set_player_modules("unknown", modules.clone()).is_err());
    assert!(e.set_player_memory("unknown", &json!({})).is_err());
    assert!(e.set_player_memory("p1", &json!(null)).is_err());
    e.set_player_modules("p1", modules)?;
    e.set_player_memory("p1", &json!({"count": 4}))?;
    assert_eq!(e.get_game_time(), 1, "创建含热身拍，设置操作不再推进");
    assert_eq!(e.world_snapshot()["time"], 1);
    let code = "require('main').loop(); Memory.count";
    assert_eq!(e.run_player("p1", code)?.expr["value"], 6);
    e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
    assert!(e.run_player("p1", code).is_err());
    let mut replacement = kk_runner::ModuleTable::new();
    replacement.insert_js("main", "exports.loop = () => { Memory.count = 1000; };");
    assert!(e.set_player_modules("p1", replacement).is_err());
    assert!(e.set_player_memory("p1", &json!({"count": 1000})).is_err());
    assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"count\":6}");
    assert_eq!(e.get_game_time(), 2);
    assert_eq!(e.run_player("p1", "'ignored'")?.expr["value"], 8);
    assert_eq!(e.run_player("p1", code)?.expr["value"], 10);

    // UTF-8 字节大于上限但 UTF-16 码元在范围内的输入必须接受。
    let unicode = json!({"text": "界".repeat(1_000_000)});
    e.set_player_memory("p1", &unicode)?;
    assert!(e.player_state("p1").unwrap().raw_memory.len() > 2 * 1024 * 1024);
    assert!(
        e.set_player_memory("p1", &json!({"text": "a".repeat(2 * 1024 * 1024)}))
            .is_err()
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&e.player_state("p1").unwrap().raw_memory)?,
        unicode
    );
    assert_eq!(e.get_game_time(), 4);
    e.teardown();
    Ok(())
}
