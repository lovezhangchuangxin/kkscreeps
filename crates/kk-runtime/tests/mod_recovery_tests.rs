//! Mod VM 全局污染隔离、固定启动快照与失败轮玩家候选复用。
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_engine::{TickStage, WritebackStep};
use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);
struct PackageRoot(PathBuf);
impl Drop for PackageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn shard() -> Value {
    json!({"players":["p1"],"rooms":[{"name":"W1N1","rcl":6,"owner":"p1"}]})
}

const SOURCE: &str = r#"
let cacheCalls = 0;
export default function register(api) {
  const step = api.config.step;
  api.handlers.wrap('builtin:link.tick', {id:'count'}, (ctx,next) => {
    next();
    ctx.state = {total:(ctx.state?.total || 0)+step, cacheCalls:++cacheCalls};
  });
}
"#;

fn engine(source: &str, binary: bool) -> anyhow::Result<(EmbedEngine, String)> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-recovery-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    let dir = root.0.join("recovery");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("mod.json"),
        json!({"id":"recovery","version":"1.0.0","apiVersion":"^1.0.0"}).to_string(),
    )?;
    std::fs::write(dir.join("server.js"), source)?;
    let mut packages = discover_packages(&root.0)?;
    let mut options = RegisterOptions::default();
    options.configs.insert("recovery".into(), json!({"step":3}));
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.set_sandbox_config_for_test(
        "p1",
        kk_runner::SandboxConfig {
            initial_bucket: 100.0,
            ..Default::default()
        },
    );
    e.create_shard_with_mods(&shard(), &packages, options.clone())?;
    // 调用方配置与包字节发生变化，随后删除目录；恢复必须仍用分片私有启动快照。
    packages.get_mut("recovery").unwrap().files.insert(
        PathBuf::from("server.js"),
        b"throw Error('changed package')".to_vec(),
    );
    options
        .configs
        .insert("recovery".into(), json!({"step":99}));
    let id = e.place_object("W1N1", "structure", &json!({"pos":[20,20],"structureType":"link","owner":"p1","store":{"energy":0},"cooldown":100}))?;
    Ok((e, id))
}

#[test]
fn late_settle_failures_reset_mod_globals_and_match_cold_start() -> anyhow::Result<()> {
    let stages =
        [TickStage::Market, TickStage::Validate, TickStage::Publish].map(|s| (Some(s), None));
    let steps = [
        WritebackStep::Market,
        WritebackStep::Cpu,
        WritebackStep::RawMemory,
        WritebackStep::Segments,
    ]
    .map(|s| (None, Some(s)));
    for binary in [false, true] {
        for (stage, step) in stages.into_iter().chain(steps) {
            let (mut cold, cold_id) = engine(SOURCE, binary)?;
            cold.run_player("p1", "0")?;
            let cold_state = cold.engine_mod_state("recovery");
            let cold_object = cold.get_object(&cold_id);
            let cold_time = cold.get_game_time();
            let cold_generation = cold.engine_generation();
            // JsRuntime 的 isolate 在同线程按进入栈使用，参考运行先结束再创建被测 VM。
            drop(cold);
            let (mut e, id) = engine(SOURCE, binary)?;
            let generation = e.engine_generation();
            let player = e.player_state_for_test("p1").unwrap().clone();
            if let Some(stage) = stage {
                e.fail_next_settle_stage_for_test(stage);
            }
            if let Some(step) = step {
                e.fail_next_writeback_for_test(step);
            }
            assert!(
                e.run_player("p1", "0").is_err(),
                "stage={stage:?}, step={step:?}"
            );
            assert!(e.needs_mod_recovery());
            assert_eq!(e.get_game_time(), 0);
            assert_eq!(e.engine_generation(), generation);
            assert_eq!(e.engine_mod_state("recovery"), Value::Null);
            assert_eq!(e.get_object(&id).unwrap()["cooldown"], 100);
            assert_eq!(e.player_state_for_test("p1"), Some(&player));
            // 不重读已经删除的目录，也不使用调用方后来修改的字节与配置。
            e.run_player("p1", "throw Error('cached retry only')")?;
            assert!(!e.needs_mod_recovery());
            assert_eq!(
                e.engine_mod_state("recovery"),
                json!({"total":3,"cacheCalls":1})
            );
            assert_eq!(e.engine_mod_state("recovery"), cold_state);
            assert_eq!(e.get_object(&id), cold_object);
            assert_eq!(e.get_game_time(), cold_time);
            assert_eq!(e.engine_generation(), cold_generation);
            e.tick(1)?;
            assert_eq!(
                e.engine_mod_state("recovery"),
                json!({"total":6,"cacheCalls":2}),
                "成功轮保留 VM 缓存"
            );
            e.fail_next_settle_stage_for_test(TickStage::Validate);
            assert!(e.tick(1).is_err());
            e.tick(1)?;
            assert_eq!(
                e.engine_mod_state("recovery"),
                json!({"total":9,"cacheCalls":1}),
                "重建从已提交持久状态继续，丢弃模块缓存"
            );
        }
    }
    Ok(())
}

#[test]
fn recovery_failure_preserves_pending_players_and_never_redispatches() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, _) = engine(SOURCE, binary)?;
        let creep = e.place_object(
            "W1N1",
            "creep",
            &json!({"pos":[21,21],"owner":"p1","name":"mover","body":["move"]}),
        )?;
        let before = e.player_state_for_test("p1").unwrap().clone();
        let generation = e.engine_generation();
        let code = "Game.creeps.mover.move(RIGHT); globalThis.n=(globalThis.n||0)+1; Memory.saved=globalThis.n; RawMemory.segments[5]='candidate'; RawMemory.setActiveSegments([5]); console.log('once'); globalThis.n";
        e.fail_next_writeback_for_test(WritebackStep::Segments);
        assert!(e.run_player("p1", code).is_err());
        assert!(e.has_pending_round());
        assert!(e.needs_mod_recovery());
        assert_eq!(e.player_state_for_test("p1"), Some(&before));
        assert_eq!(e.get_object(&creep).unwrap()["pos"]["x"], 21);
        e.fail_next_mod_rebuild_for_test()?;
        assert!(
            e.run_player("p1", "throw Error('must not execute')")
                .unwrap_err()
                .to_string()
                .contains("恢复失败")
        );
        assert!(e.has_pending_round());
        assert!(e.needs_mod_recovery());
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.engine_generation(), generation);
        assert_eq!(e.player_state_for_test("p1"), Some(&before));
        assert!(e.capture_console_logs("p1").is_empty());
        // 错误的玩家请求也不得消耗缓存或触发恢复；空请求重试整个缓存轮。
        assert!(e.run_player("ghost", "0").is_err());
        assert!(e.needs_mod_recovery());
        let runs = e.run_players(&BTreeMap::new())?;
        assert_eq!(runs["p1"].expr["value"], 1);
        assert!(!e.has_pending_round());
        assert!(!e.needs_mod_recovery());
        assert_eq!(
            e.get_object(&creep).unwrap()["pos"]["x"],
            22,
            "缓存玩家意图恰一次生效"
        );
        assert_eq!(
            e.engine_mod_state("recovery"),
            json!({"total":3,"cacheCalls":1})
        );
        let committed = e.player_state_for_test("p1").unwrap();
        assert_eq!(committed.raw_memory, r#"{"saved":1}"#);
        assert_eq!(committed.segments[&5], "candidate");
        let bucket = committed.cpu.bucket();
        let next = e.run_player(
            "p1",
            "[globalThis.n,Memory.saved,RawMemory.segments[5],Game.cpu.bucket]",
        )?;
        assert_eq!(next.expr["value"][0], 1);
        assert_eq!(next.expr["value"][1], 1);
        assert_eq!(next.expr["value"][2], "candidate");
        assert_eq!(next.expr["value"][3].as_f64(), Some(bucket));
    }
    Ok(())
}

#[test]
fn discarded_round_cannot_bypass_mod_recovery_gate() -> anyhow::Result<()> {
    let (mut e, id) = engine(SOURCE, false)?;
    e.fail_next_settle_stage_for_test(TickStage::Validate);
    assert!(e.settle_external(Default::default()).is_err());
    e.discard_pending_round();
    assert!(!e.has_pending_round());
    assert!(e.needs_mod_recovery());
    assert!(
        e.place_market_order(
            &json!({"owner":"p1","type":"sell","resourceType":"energy","price":1,"totalAmount":1})
        )
        .unwrap_err()
        .to_string()
        .contains("Mod VM 待恢复")
    );
    assert!(e.set_terrain("W1N1", vec![0; 2500]).is_err());
    e.fail_next_mod_rebuild_for_test()?;
    assert!(e.run_player("p1", "globalThis.unwanted=1").is_err());
    assert_eq!(e.get_game_time(), 0);
    e.rebuild_mod_realm()?;
    let run = e.run_player("p1", "globalThis.unwanted||0")?;
    assert_eq!(run.expr["value"], 0, "恢复失败时不能执行新玩家轮");
    assert_eq!(e.get_object(&id).unwrap()["cooldown"], 99);
    e.fail_next_settle_stage_for_test(TickStage::Validate);
    assert!(e.tick(1).is_err());
    e.create_shard(&shard())?;
    assert!(!e.needs_mod_recovery());
    e.tick(1)?;
    e.teardown();
    assert!(!e.needs_mod_recovery());
    Ok(())
}

#[test]
fn mod_handler_fault_is_retried_in_fresh_vm_and_never_published() -> anyhow::Result<()> {
    let source = r#"
let n=0;
export default function register(api) {
  api.handlers.wrap('builtin:link.tick', {id:'throw'}, (ctx,next) => {
    next(); ctx.state={dirty:++n};
    if(n===1) throw Error('first call fails');
  });
}
"#;
    let (mut e, id) = engine(source, false)?;
    for _ in 0..3 {
        assert!(
            e.tick(1)
                .unwrap_err()
                .to_string()
                .contains("first call fails")
        );
        assert!(e.needs_mod_recovery());
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.engine_mod_state("recovery"), Value::Null);
        assert_eq!(e.get_object(&id).unwrap()["cooldown"], 100);
    }
    Ok(())
}
