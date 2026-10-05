//! 比较完整权威清单，覆盖账户、市场、扩展、Mod 自主对象与冷恢复执行掩码。
use super::*;
use kk_engine::settle::market::{MarketOrder, Transaction};
use kk_runner::{ModuleTable, SandboxConfig};

fn packages() -> anyhow::Result<BTreeMap<String, ModPackage>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../mods/official")
        .canonicalize()?;
    kk_mod_host::discover_packages(&root).map_err(anyhow::Error::from)
}
fn options() -> RegisterOptions {
    RegisterOptions {
        selection: kk_mod_host::ContentSelection {
            include: BTreeSet::from(["nursery".into(), "clockwork".into()]),
            exclude: BTreeSet::new(),
        },
        fault_disable_threshold: std::num::NonZeroU32::new(3),
        ..Default::default()
    }
}
fn prepare(
    binary: bool,
    packages: &BTreeMap<String, ModPackage>,
    options: &RegisterOptions,
) -> anyhow::Result<EmbedEngine> {
    let mut engine = EmbedEngine::new();
    engine.set_kkpv_channel(binary);
    engine.create_shard_with_mods(
        &json!({"players": ["p1", "p2"], "rooms": [
            {"name": "W1N1", "owner": "p1", "rcl": 6}, {"name": "W2N2", "owner": "p2", "rcl": 6}
        ]}),
        packages,
        options.clone(),
    )?;
    for room in ["W1N1", "W2N2"] {
        engine.place_mod_object(room, "nursery:emitter", &json!({"x": 10, "y": 10}))?;
        engine.place_mod_object(room, "clockwork:counter", &json!({"x": 12, "y": 10}))?;
    }
    for player in ["p1", "p2"] {
        let mut table = ModuleTable::new();
        table.insert_js("main", "exports.loop = () => { Memory.loops = (Memory.loops || 0) + 1; RawMemory.setActiveSegments([3]); RawMemory.segments[3] = String(Memory.loops); if (Game.time === 0) { Game.notify('saved notice'); RawMemory.setPublicSegments([3]); RawMemory.setDefaultPublicSegment(3); } return Memory.loops; };");
        table.insert_binary("asset.wasm", [0, 97, 115, 109]);
        engine.set_player_modules(player, table)?;
    }
    // 市场游标与所有字段即使场景没有成交也须参与完整状态比较。
    let id = engine.engine.market.next_order_id();
    engine.engine.market.insert_order(MarketOrder {
        id,
        user: "p1".into(),
        active: true,
        order_type: "sell".into(),
        resource_type: "energy".into(),
        price: 1000,
        amount: 0,
        remaining_amount: 10,
        total_amount: 10,
        room_name: Some("W1N1".into()),
        created: 0,
        created_timestamp: 0,
    });
    let id = engine.engine.market.alloc_transaction_id();
    engine.engine.market.record_transaction(Transaction {
        id,
        time: 0,
        sender: Some("p1".into()),
        recipient: Some("p2".into()),
        resource_type: "energy".into(),
        amount: 1,
        from: "W1N1".into(),
        to: "W2N2".into(),
        description: Some("saved".into()),
        order: None,
    });
    Ok(engine)
}
fn run(engine: &mut EmbedEngine, count: usize) -> anyhow::Result<()> {
    let codes = BTreeMap::from([
        ("p1".into(), "require('main').loop()".into()),
        ("p2".into(), "require('main').loop()".into()),
    ]);
    for _ in 0..count {
        let results = engine.run_players(&codes)?;
        assert!(results.values().all(|r| r.expr["ok"] == true));
    }
    Ok(())
}
fn freeze_cpu(engine: &mut EmbedEngine) {
    // 实际线程用量本就会改变 CPU 桶；停跑固定回充轮用于完整状态续跑等价验收。
    for player in engine.engine.players.values_mut() {
        player.cpu = kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, -1000.0);
    }
}

#[test]
fn complete_state_survives_cold_restart_and_continues_in_both_channels() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    for binary in [false, true] {
        let mut original = prepare(binary, &packages, &options)?;
        run(&mut original, 7)?;
        // 保留 schema/规则但停用 clockwork 执行，并显式恢复健康状态再保存。
        original.disable_mod("clockwork")?;
        original.rebuild_mod_realm()?;
        let saved = original.checkpoint(json!({"aliases": {"example": "persisted"}}))?;
        let roundtrip = serde_json::from_slice(&serde_json::to_vec(&saved)?)?;
        // 停跑轮有固定 CPU 回充，避免拿真实计时差异伪装恢复缺陷。
        freeze_cpu(&mut original);
        original.run_players(&BTreeMap::from([
            ("p1".into(), "0".into()),
            ("p2".into(), "0".into()),
        ]))?;
        let expected = original.checkpoint(Value::Null)?;
        drop(original);
        let mut restored = EmbedEngine::new();
        assert_eq!(
            restored.restore_checkpoint(roundtrip, Some((&packages, &options)))?,
            json!({"aliases": {"example": "persisted"}})
        );
        assert_eq!(
            restored.checkpoint(saved["metadata"].clone())?,
            saved,
            "恢复时完整状态须逐项相等"
        );
        freeze_cpu(&mut restored);
        restored.run_players(&BTreeMap::from([
            ("p1".into(), "0".into()),
            ("p2".into(), "0".into()),
        ]))?;
        assert_eq!(restored.checkpoint(Value::Null)?, expected);
        assert_eq!(
            restored.disabled_mods(),
            BTreeSet::from(["clockwork".into()])
        );
        let old_time = restored.get_game_time();
        let own = restored.checkpoint(Value::Null)?;
        restored.restore_checkpoint(own, Some((&packages, &options)))?;
        assert_eq!(
            restored.get_game_time(),
            old_time,
            "替换现行 Mod 分片遵守 V8 进入/退出栈顺序"
        );
    }
    Ok(())
}

#[test]
fn mismatch_and_invalid_payload_keep_the_running_mod_shard() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    run(&mut engine, 2)?;
    let saved = engine.checkpoint(Value::Null)?;
    assert!(engine.restore_checkpoint(saved.clone(), None).is_err());
    let mut changed = packages.clone();
    changed
        .get_mut("clockwork")
        .unwrap()
        .files
        .insert("server.js".into(), b"throw Error('changed');".to_vec());
    assert!(
        engine
            .restore_checkpoint(saved.clone(), Some((&changed, &options)))
            .is_err()
    );
    let mut drift = options.clone();
    drift.selection.include.remove("clockwork");
    assert!(
        engine
            .restore_checkpoint(saved.clone(), Some((&packages, &drift)))
            .is_err()
    );
    for change in ["plan", "cursor", "cpu", "room", "mask", "mod"] {
        let mut bad = saved.clone();
        match change {
            "plan" => bad["engine"]["plan_hash"] = json!("wrong"),
            "cursor" => bad["engine"]["world"]["next_obj_id"] = json!(1),
            "cpu" => bad["engine"]["players"]["p1"]["cpu"]["limit"] = json!(-1),
            "room" => bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"] = json!([0]),
            "mask" => bad["disabled_mods"] = json!(["missing"]),
            "mod" => bad["engine"]["mod_states"]["unexpected"] = json!({"dirty": true}),
            _ => unreachable!(),
        }
        assert!(
            engine
                .restore_checkpoint(bad, Some((&packages, &options)))
                .is_err(),
            "{change}"
        );
        assert_eq!(
            engine.checkpoint(Value::Null)?,
            saved,
            "拒绝后旧分片保持完整"
        );
    }
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3);
    Ok(())
}

#[test]
fn open_round_cannot_be_saved_or_replaced() -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({"players": ["p1"], "rooms": [{"name": "W1N1"}]}))?;
    let saved = engine.checkpoint(Value::Null)?;
    engine.fail_next_settle_for_test();
    assert!(engine.run_player("p1", "Memory.pending = true").is_err());
    assert!(engine.checkpoint(Value::Null).is_err());
    assert!(engine.restore_checkpoint(saved, None).is_err());
    engine.run_players(&BTreeMap::new())?;
    assert_eq!(
        engine.checkpoint(Value::Null)?["engine"]["players"]["p1"]["raw_memory"],
        "{\"pending\":true}"
    );
    Ok(())
}

#[test]
fn native_player_globals_reset_but_modules_memory_segments_and_cpu_survive() -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.set_sandbox_config_for_test(
        "p1",
        SandboxConfig {
            initial_bucket: 200.0,
            ..Default::default()
        },
    );
    engine.create_shard(&json!({"players": ["p1"], "rooms": []}))?;
    let mut table = ModuleTable::new();
    table.insert_js("value", "module.exports = 9;");
    engine.set_player_modules("p1", table)?;
    engine.run_player("p1", "globalThis.cache = 17; Memory.saved = require('value'); RawMemory.setActiveSegments([4]); RawMemory.segments[4] = 'saved';")?;
    let saved = engine.checkpoint(Value::Null)?;
    let bucket = engine.player_state("p1").unwrap().cpu.bucket();
    drop(engine);
    let mut restored = EmbedEngine::new();
    restored.restore_checkpoint(saved, None)?;
    let result = restored.run_player("p1", "[typeof globalThis.cache, Memory.saved, require('value'), RawMemory.segments[4], Game.cpu.bucket]")?;
    assert_eq!(result.expr["value"][0], "undefined");
    assert_eq!(result.expr["value"][1], 9);
    assert_eq!(result.expr["value"][2], 9);
    assert_eq!(result.expr["value"][3], "saved");
    assert_eq!(result.expr["value"][4].as_f64(), Some(bucket));
    Ok(())
}

#[test]
fn finite_floating_state_is_bit_exact_after_disk_json_roundtrip() -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({"players": ["p1"], "rooms": []}))?;
    let mut bits = 7_u64;
    for _ in 0..1000 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        let value = f64::from_bits(bits);
        if !value.is_finite() || !(0.0..=10000.0).contains(&value) {
            continue;
        }
        engine.engine.players.get_mut("p1").unwrap().cpu =
            kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, value);
        let saved = engine.checkpoint(Value::Null)?;
        let back: Value = serde_json::from_slice(&serde_json::to_vec(&saved)?)?;
        assert_eq!(
            back["engine"]["players"]["p1"]["cpu"]["bucket"]
                .as_f64()
                .unwrap()
                .to_bits(),
            value.to_bits()
        );
    }
    Ok(())
}

#[test]
fn restoring_below_another_active_mod_shard_is_rejected_without_retiring_it() -> anyhow::Result<()>
{
    let packages = packages()?;
    let options = options();
    let mut older = prepare(false, &packages, &options)?;
    let saved = older.checkpoint(Value::Null)?;
    let newer = prepare(false, &packages, &options)?;
    assert!(
        older
            .restore_checkpoint(saved.clone(), Some((&packages, &options)))
            .unwrap_err()
            .to_string()
            .contains("其他活跃 Mod 分片")
    );
    assert_eq!(older.checkpoint(Value::Null)?, saved);
    drop(newer);
    run(&mut older, 1)?;
    Ok(())
}

fn custom_package(source: &str) -> anyhow::Result<ModPackage> {
    Ok(ModPackage {
        root: std::env::temp_dir().join("kk-checkpoint-fields"),
        manifest: serde_json::from_value(
            json!({"id": "cpfields", "version": "1.0.0", "apiVersion": "^1.0.0"}),
        )?,
        files: BTreeMap::from([("server.js".into(), source.as_bytes().to_vec())]),
    })
}

#[test]
fn private_extensions_declared_resources_and_effective_rules_roundtrip() -> anyhow::Result<()> {
    let package = custom_package(
        r#"
export default function register(api) {
  api.mounts.define({key: 'builtin:link', version: 1}, 'charge', {type: 'u32', min: 0, max: 100, visibility: 'server-only'});
  api.mounts.define({key: 'builtin:link', version: 1}, 'target', {type: 'ref', target: 'builtin:link', version: 1});
  api.resources.define('cpfields:crystal');
  api.rules.override('builtin:link.capacity', 321);
}
"#,
    )?;
    let packages = BTreeMap::from([("cpfields".into(), package)]);
    let options = RegisterOptions::default();
    for binary in [false, true] {
        let mut engine = EmbedEngine::new();
        engine.set_kkpv_channel(binary);
        engine.create_shard_with_mods(
            &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}]}),
            &packages,
            options.clone(),
        )?;
        let link = engine.place_object("W1N1", "structure", &json!({"pos": [10, 10], "owner": "p1", "structureType": "link", "store": {"energy": 31}}))?;
        engine.place_object("W1N1", "structure", &json!({"pos": [12, 10], "owner": "p1", "structureType": "storage", "store": {"cpfields:crystal": 27}}))?;
        engine
            .engine
            .world
            .room_mut("W1N1")
            .unwrap()
            .extensions
            .insert(
                ObjId::parse(&link).unwrap(),
                BTreeMap::from([
                    ("cpfields:charge".into(), json!(99)),
                    ("cpfields:target".into(), json!(link)),
                ]),
            );
        let saved = engine.checkpoint(Value::Null)?;
        let mut bad = saved.clone();
        bad["engine"]["world"]["rooms"]["W1N1"]["extensions"]
            [ObjId::parse(&link).unwrap().0.to_string()]["cpfields:charge"] = json!(101);
        assert!(
            engine
                .restore_checkpoint(bad, Some((&packages, &options)))
                .is_err()
        );
        assert_eq!(engine.checkpoint(Value::Null)?, saved);
        drop(engine);
        let mut restored = EmbedEngine::new();
        restored.restore_checkpoint(
            serde_json::from_slice(&serde_json::to_vec(&saved)?)?,
            Some((&packages, &options)),
        )?;
        assert_eq!(restored.checkpoint(Value::Null)?, saved);
        let result = restored.run_player("p1", "[LINK_CAPACITY, RESOURCES_ALL.includes('cpfields:crystal'), Game.rooms.W1N1.find(FIND_STRUCTURES).find(o => o.structureType === 'storage').store['cpfields:crystal']]")?;
        assert_eq!(result.expr["value"], json!([321, true, 27]));
    }
    Ok(())
}

#[test]
fn automatic_disable_fault_counter_continues_across_a_cold_restart() -> anyhow::Result<()> {
    let package = custom_package(
        "export default function register(api) { api.handlers.before('builtin:link.tick', {id: 'fault'}, () => { throw Error('checkpoint fault'); }); }",
    )?;
    let packages = BTreeMap::from([("cpfields".into(), package)]);
    let options = RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(2),
        ..Default::default()
    };
    let mut engine = EmbedEngine::new();
    engine.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}]}),
        &packages,
        options.clone(),
    )?;
    engine.place_object(
        "W1N1",
        "structure",
        &json!({"pos": [10, 10], "structureType": "link", "owner": "p1"}),
    )?;
    assert!(engine.tick(1).is_err());
    engine.discard_pending_round();
    engine.rebuild_mod_realm()?;
    assert_eq!(engine.mod_fault_streak().unwrap().consecutive_faults, 1);
    let saved = engine.checkpoint(Value::Null)?;
    drop(engine);
    let mut restored = EmbedEngine::new();
    restored.restore_checkpoint(
        serde_json::from_slice(&serde_json::to_vec(&saved)?)?,
        Some((&packages, &options)),
    )?;
    assert_eq!(restored.checkpoint(Value::Null)?, saved);
    assert!(restored.tick(1).is_err());
    assert_eq!(
        restored.disabled_mods(),
        BTreeSet::from(["cpfields".into()])
    );
    assert_eq!(
        restored.last_mod_auto_disable().unwrap().consecutive_faults,
        2
    );
    restored.tick(1)?;
    assert_eq!(restored.get_game_time(), 1);
    Ok(())
}

#[test]
fn checkpoint_restore_cannot_retire_a_mod_vm_below_a_native_sandbox() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    let saved = engine.checkpoint(Value::Null)?;
    let sandbox = kk_runner::Sandbox::new(SandboxConfig::default())?;
    assert!(
        engine
            .restore_checkpoint(saved.clone(), Some((&packages, &options)))
            .unwrap_err()
            .to_string()
            .contains("玩家 VM")
    );
    assert_eq!(engine.checkpoint(Value::Null)?, saved);
    drop(sandbox);
    engine.restore_checkpoint(saved, Some((&packages, &options)))?;
    run(&mut engine, 1)?;
    Ok(())
}
