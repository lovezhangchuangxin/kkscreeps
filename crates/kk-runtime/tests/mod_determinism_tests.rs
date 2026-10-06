//! G22.b 确定性玩法时间/RNG 的运行时行为族：同种子逐位复现、异种子
//! 相异、检查点冷恢复续跑等值、失败重试同 tick 重抽同值、包切换保留
//! 种子流、teardown/setup 保留种子。夹具包 dice 见
//! compat/fixtures/mod-determinism（每 tick 抽值并写 ctx.state）。

use std::collections::BTreeMap;
use std::path::Path;

use kk_mod_host::{ModPackage, RegisterOptions, discover_packages};
use kk_runner::ModuleTable;
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

fn dice_packages() -> anyhow::Result<BTreeMap<String, ModPackage>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/fixtures/mod-determinism")
        .canonicalize()?;
    discover_packages(&root).map_err(anyhow::Error::from)
}

fn prepare(seed: u64, packages: &BTreeMap<String, ModPackage>) -> anyhow::Result<EmbedEngine> {
    let mut engine = EmbedEngine::with_world_seed(seed);
    engine.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        packages,
        RegisterOptions::default(),
    )?;
    let mut table = ModuleTable::new();
    table.insert_js("main", "exports.loop = () => {};");
    engine.set_player_modules("p1", table)?;
    engine.place_mod_object("W1N1", "dice:roller", &json!({"x": 10, "y": 10}))?;
    Ok(engine)
}

fn run_ticks(engine: &mut EmbedEngine, count: usize) -> anyhow::Result<()> {
    let codes = BTreeMap::from([("p1".into(), "require('main').loop()".into())]);
    for _ in 0..count {
        let results = engine.run_players(&codes)?;
        assert!(results.values().all(|r| r.expr["ok"] == true));
    }
    Ok(())
}

fn dice_state(engine: &EmbedEngine) -> Value {
    engine.engine_mod_state("dice")
}

/// 同种子同输入逐位相等；异种子抽值相异；结算期 Date = tick×3000。
/// 多个分片在同一测试线程按栈序使用：逐个构造→运行→取状态→销毁，
/// 不允许创建后回头使用更早的分片（V8 isolate 栈序门）。
#[test]
fn same_seed_bit_equal_and_different_seed_diverges() -> anyhow::Result<()> {
    let packages = dice_packages()?;
    let run_and_collect = |seed: u64| -> anyhow::Result<(Value, u64)> {
        let mut engine = prepare(seed, &packages)?;
        run_ticks(&mut engine, 4)?;
        let state = dice_state(&engine);
        let time = engine.get_game_time();
        Ok((state, time))
    };
    let (a, time_a) = run_and_collect(42)?;
    let (b, _) = run_and_collect(42)?;
    let (c, _) = run_and_collect(43)?;
    assert_eq!(a, b, "同种子同输入必须逐位相等");
    assert_ne!(a["draws"][0], c["draws"][0], "种子必须进入结算流");
    // 结算执行的是「当前 tick」：第 4 轮里玩家看到 Game.time=3、槽
    // ctx.tick=3，发布后世界时间才推进到 4。Date.now 与玩家可见时间
    // 同轴：当轮恒为 当前 Game.time×3000。
    assert_eq!(
        a["now"],
        json!((time_a - 1) * 3000),
        "结算期 Date.now = 当轮世界 tick×3000"
    );
    assert_eq!(
        a["newDate"],
        json!((time_a - 1) * 3000),
        "new Date() 与 Date.now() 同源"
    );
    assert_eq!(
        a["init"]["now"],
        json!(0),
        "装载期 Date 恒 0（不在玩法时间轴上）"
    );
    assert_eq!(
        a["init"]["temporalGone"],
        json!(true),
        "玩法 realm 不暴露 Temporal"
    );
    Ok(())
}

/// 检查点冷恢复续跑与直通运行等值：抽值按（种子、域、tick、序号）推导，
/// 恢复后继续的 tick 取同一流；init 抽值重装不变。
#[test]
fn cold_restore_continues_deterministically() -> anyhow::Result<()> {
    let packages = dice_packages()?;
    let mut straight = prepare(42, &packages)?;
    run_ticks(&mut straight, 6)?;

    let mut phased = prepare(42, &packages)?;
    run_ticks(&mut phased, 3)?;
    let saved = phased.checkpoint(json!({}))?;

    let mut restored = EmbedEngine::new();
    restored.restore_checkpoint(saved, Some((&packages, &RegisterOptions::default())))?;
    assert_eq!(
        restored.world_seed(),
        42,
        "恢复分片以载荷种子构造（realm 与世界同根）"
    );
    run_ticks(&mut restored, 3)?;

    assert_eq!(
        dice_state(&straight),
        dice_state(&restored),
        "冷恢复续跑的抽值序列必须与直通运行逐位相等"
    );
    Ok(())
}

/// 结算后期失败丢弃整轮候选后重试：同 tick 重抽同值，提交状态与干净
/// 运行等值（轮作用域计数重置是重试确定性的机制）。
#[test]
fn late_stage_failure_retry_redraws_identically() -> anyhow::Result<()> {
    let packages = dice_packages()?;
    let mut clean = prepare(42, &packages)?;
    run_ticks(&mut clean, 3)?;

    let mut retried = prepare(42, &packages)?;
    run_ticks(&mut retried, 2)?;
    retried.fail_next_settle_stage_for_test(kk_engine::TickStage::Validate);
    let codes = BTreeMap::from([("p1".into(), "require('main').loop()".into())]);
    let err = retried.run_players(&codes).unwrap_err();
    assert!(
        err.to_string()
            .contains(kk_engine::TickStage::Validate.label())
    );
    assert!(retried.has_pending_round());
    retried.run_players(&codes)?;

    assert_eq!(retried.get_game_time(), clean.get_game_time());
    assert_eq!(
        dice_state(&clean),
        dice_state(&retried),
        "失败轮的抽值不进提交状态；重试重抽同值"
    );
    Ok(())
}

/// 无开放轮次的包切换重建 realm：init 流重装不变、结算流仍按世界种子
/// 与 tick 推导——切换后继续运行与从头直通等值。
#[test]
fn switch_rebuild_keeps_seed_stream() -> anyhow::Result<()> {
    let packages = dice_packages()?;
    let mut straight = prepare(42, &packages)?;
    run_ticks(&mut straight, 4)?;

    let mut switched = prepare(42, &packages)?;
    run_ticks(&mut switched, 2)?;
    switched.switch_mod_packages(&packages, &RegisterOptions::default())?;
    run_ticks(&mut switched, 2)?;

    assert_eq!(switched.world_seed(), 42);
    assert_eq!(
        dice_state(&straight),
        dice_state(&switched),
        "切换重建后的抽值必须延续同一世界种子流"
    );
    Ok(())
}

/// teardown 与重建分片保留世界种子：种子是分片身份，不随世界清空更换。
#[test]
fn teardown_and_recreate_preserve_seed() -> anyhow::Result<()> {
    let packages = dice_packages()?;
    let mut engine = prepare(4242, &packages)?;
    run_ticks(&mut engine, 1)?;
    engine.teardown();
    assert_eq!(engine.world_seed(), 4242);
    engine.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        &packages,
        RegisterOptions::default(),
    )?;
    assert_eq!(engine.world_seed(), 4242, "重建分片不换随机协议根");
    Ok(())
}
