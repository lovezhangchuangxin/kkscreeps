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
    // created_timestamp 取当前墙钟：订单生命周期按墙钟判过期，置 0 会在
    // 首轮结算被清退，schema 门禁与冷恢复比较就覆盖不到订单表。该值随
    // 运行变化，不得据此引入检查点字节级比较。
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
        created_timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
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
    for change in ["plan", "cursor", "cpu", "room", "mask", "mod", "strategy"] {
        let mut bad = saved.clone();
        match change {
            "plan" => bad["engine"]["plan_hash"] = json!("wrong"),
            "cursor" => bad["engine"]["world"]["next_obj_id"] = json!(1),
            "cpu" => bad["engine"]["players"]["p1"]["cpu"]["limit"] = json!(-1),
            "room" => bad["engine"]["world"]["rooms"]["W1N1"]["snapshot"] = json!([0]),
            "mask" => bad["disabled_mods"] = json!(["missing"]),
            "mod" => bad["engine"]["mod_states"]["unexpected"] = json!({"dirty": true}),
            "strategy" => bad["execution_strategy"] = json!("0".repeat(64)),
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

#[test]
fn trace_enabled_after_assembly_survives_restore_without_old_entries() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    for binary in [false, true] {
        let mut engine = prepare(binary, &packages, &options)?;
        engine.enable_mod_trace();
        engine.tick(1)?;
        assert!(!engine.mod_trace_snapshot().unwrap().0.is_empty());
        // 停用使恢复装配需要重建 realm；该内部重建不能留下 tick=0 的伪运行事件。
        engine.disable_mod("clockwork")?;
        engine.rebuild_mod_realm()?;
        let saved = engine.checkpoint(Value::Null)?;
        let mut invalid = saved.clone();
        invalid["engine_version"] = json!("unknown");
        let before = engine.mod_trace_snapshot();
        assert!(
            engine
                .restore_checkpoint(invalid, Some((&packages, &options)))
                .is_err()
        );
        assert_eq!(engine.mod_trace_snapshot(), before);
        engine.restore_checkpoint(saved, Some((&packages, &options)))?;
        assert_eq!(engine.mod_trace_snapshot(), Some((Vec::new(), 0)));
        let resumed_tick = engine.get_game_time();
        engine.tick(1)?;
        let (entries, _) = engine.mod_trace_snapshot().unwrap();
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|entry| entry.tick == resumed_tick));
        engine.teardown();
        engine.create_shard_with_mods(
            &json!({"players":[],"rooms":[]}),
            &packages,
            options.clone(),
        )?;
        assert!(
            engine.mod_trace_snapshot().is_none(),
            "teardown 清除开启意愿"
        );
    }
    Ok(())
}

#[test]
fn legacy_auto_disable_checkpoint_restores_structured_source() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    engine.disable_mod("clockwork")?;
    engine.rebuild_mod_realm()?;
    let mut saved = engine.checkpoint(Value::Null)?;
    // trace 批次之前的真实记录形态：来源只在 trigger.error.fault.binding 中。
    saved["last_auto_disable"] = json!({
        "trigger": {"tick": 0, "room": "W1N1", "target": "counter",
            "error": {"context": "clockwork", "message": "fault", "fault": {
                "slot": "clockwork:counter.tick", "binding": "clockwork:counter.tick.process", "kind": "handler"
            }}},
        "consecutive_faults": 3, "disabled": ["clockwork"]
    });
    engine.restore_checkpoint(saved, Some((&packages, &options)))?;
    assert_eq!(engine.last_mod_auto_disable().unwrap().mod_id, "clockwork");
    let mut invalid = engine.checkpoint(Value::Null)?;
    invalid["last_auto_disable"]["mod_id"] = json!("nursery");
    let before = engine.checkpoint(Value::Null)?;
    assert!(
        engine
            .restore_checkpoint(invalid, Some((&packages, &options)))
            .is_err()
    );
    assert_eq!(engine.checkpoint(Value::Null)?, before);
    Ok(())
}

/// 键为数据的映射表（玩家/房间/包/订单/段等）：折叠为首个条目的
/// "*"，仍下钻内部 schema；调用方或 Mod 自管内容（metadata、manifest、
/// options、扩展字段载荷、Mod namespace state）视为不透明叶子。
const SCHEMA_DYNAMIC_MAPS: &[&str] = &[
    "engine.world.rooms",
    "engine.world.rooms.*.extensions",
    "engine.players",
    "engine.players.*.segments",
    "engine.players.*.resources",
    "engine.market.orders",
    "workers",
    "workers.*.modules",
    "workers.*.constant_overrides",
    "mods.packages",
    "last_console",
];
const SCHEMA_OPAQUE: &[&str] = &[
    "metadata",
    "mods.options",
    "mods.packages.*.manifest",
    "mods.packages.*.files_sha256",
    "engine.world.rooms.*.extensions.*",
    "engine.mod_states",
];

fn collect_schema_keys(value: &Value, path: String, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if SCHEMA_OPAQUE.contains(&path.as_str()) {
                out.insert(path);
                return;
            }
            let entries: Vec<(&String, &Value)> = if SCHEMA_DYNAMIC_MAPS.contains(&path.as_str()) {
                map.iter().take(1).collect()
            } else {
                map.iter().collect()
            };
            for (key, child) in entries {
                let child_path = if SCHEMA_DYNAMIC_MAPS.contains(&path.as_str()) {
                    format!("{path}.*")
                } else if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                collect_schema_keys(child, child_path, out);
            }
        }
        Value::Array(items) => {
            let leaf = format!("{path}[]");
            out.insert(leaf.clone());
            if let Some(first) = items.first() {
                collect_schema_keys(first, leaf, out);
            }
        }
        _ => {
            out.insert(path);
        }
    }
}

/// 官方包全量场景：双通道各采一份（载荷与通道无关，双采样防通道相关
/// 字段混入）；p1 注入一条跨 shard 资源余额，钉住 resources 字段存在性。
fn schema_tree_official_packages(tree: &mut BTreeSet<String>) -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    for binary in [false, true] {
        let mut engine = prepare(binary, &packages, &options)?;
        run(&mut engine, 1)?;
        engine
            .engine
            .players
            .get_mut("p1")
            .unwrap()
            .resources
            .insert("pixel".into(), 5);
        engine.disable_mod("clockwork")?;
        engine.rebuild_mod_realm()?;
        collect_schema_keys(
            &engine.checkpoint(json!({"opaque": true}))?,
            String::new(),
            tree,
        );
    }
    Ok(())
}

/// 故障自动停用场景：mounts 声明包的 before 处理器连续两轮故障跨过
/// 阈值 2，fault_streak/last_fault/last_auto_disable 与停用集均取非空值。
/// 这三个结构是持久化链上没有 deny_unknown_fields 的少数（自动停用记录
/// 还是宽松自定义反序列化），schema 只能靠本场景取值钉死。
fn schema_tree_fault_and_extension_package(tree: &mut BTreeSet<String>) -> anyhow::Result<()> {
    let package = custom_package(
        r#"
export default function register(api) {
  api.mounts.define({key: 'builtin:link', version: 1}, 'charge', {type: 'u32', min: 0, max: 100, visibility: 'server-only'});
  api.handlers.before('builtin:link.tick', {id: 'fault'}, () => { throw Error('schema gate fault'); });
}
"#,
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
        options,
    )?;
    let link = engine.place_object(
        "W1N1",
        "structure",
        &json!({"pos": [10, 10], "structureType": "link", "owner": "p1"}),
    )?;
    // 已声明 mount 的扩展字段取非空值，钉住 extensions 字段存在性。
    engine
        .engine
        .world
        .room_mut("W1N1")
        .unwrap()
        .extensions
        .insert(
            ObjId::parse(&link).unwrap(),
            BTreeMap::from([("cpfields:charge".into(), json!(99))]),
        );
    for _ in 0..2 {
        assert!(engine.tick(1).is_err());
        engine.discard_pending_round();
        engine.rebuild_mod_realm()?;
    }
    collect_schema_keys(&engine.checkpoint(Value::Null)?, String::new(), tree);
    Ok(())
}

/// 钉死检查点 schema 键树：夹具取值非空的字段增删会让本测试红，须同步
/// docs/architecture/state-inventory.md 与清单并当批补齐保存恢复、版本
/// 策略。覆盖边界（数组只下钻首元素、Value 载荷按当时形状、不透明叶子）
/// 见该文档"覆盖边界"一节。
const PINNED_CHECKPOINT_KEYS: &[&str] = &[
    "concurrency",
    "disabled_mods[]",
    "engine.last_events[]",
    "engine.market.next_order_seq",
    "engine.market.next_transaction_id",
    "engine.market.transactions[]",
    "engine.market.transactions[].amount",
    "engine.market.transactions[].description",
    "engine.market.transactions[].from",
    "engine.market.transactions[].id",
    "engine.market.transactions[].order",
    "engine.market.transactions[].recipient",
    "engine.market.transactions[].resource_type",
    "engine.market.transactions[].sender",
    "engine.market.transactions[].time",
    "engine.market.transactions[].to",
    "engine.market.orders.*.active",
    "engine.market.orders.*.amount",
    "engine.market.orders.*.created",
    "engine.market.orders.*.created_timestamp",
    "engine.market.orders.*.id",
    "engine.market.orders.*.order_type",
    "engine.market.orders.*.price",
    "engine.market.orders.*.remaining_amount",
    "engine.market.orders.*.resource_type",
    "engine.market.orders.*.room_name",
    "engine.market.orders.*.total_amount",
    "engine.market.orders.*.user",
    "engine.mod_states",
    "engine.next_id",
    "engine.plan_hash",
    "engine.players.*.active_request[]",
    "engine.players.*.cpu.bucket",
    "engine.players.*.cpu.cap",
    "engine.players.*.cpu.limit",
    "engine.players.*.cpu.max_per_tick",
    "engine.players.*.cpu.tick_limit",
    "engine.players.*.default_public",
    "engine.players.*.foreign_request",
    "engine.players.*.gcl",
    "engine.players.*.money",
    "engine.players.*.notifications[]",
    "engine.players.*.notifications[].group_interval",
    "engine.players.*.notifications[].message",
    "engine.players.*.notifications[].tick",
    "engine.players.*.public_ids[]",
    "engine.players.*.raw_memory",
    "engine.players.*.resources.*",
    "engine.players.*.rooms[]",
    "engine.players.*.segments.*",
    "engine.world.generation",
    "engine.world.next_obj_id",
    "engine.world.player_ticks",
    "engine.world.rooms.*.extensions.*",
    "engine.world.rooms.*.snapshot[]",
    "engine.world.seed",
    "engine.world.terrain_epoch",
    "engine.world.time",
    "engine_version",
    "execution_strategy",
    "fault_streak",
    "fault_streak.consecutive_faults",
    "fault_streak.mod_id",
    "kkpv",
    "last_auto_disable",
    "last_auto_disable.consecutive_faults",
    "last_auto_disable.disabled[]",
    "last_auto_disable.mod_id",
    "last_auto_disable.trigger.error.context",
    "last_auto_disable.trigger.error.fault.binding",
    "last_auto_disable.trigger.error.fault.kind",
    "last_auto_disable.trigger.error.fault.slot",
    "last_auto_disable.trigger.error.message",
    "last_auto_disable.trigger.room",
    "last_auto_disable.trigger.target",
    "last_auto_disable.trigger.tick",
    "last_console.*[]",
    "last_fault",
    "last_fault.error.context",
    "last_fault.error.fault.binding",
    "last_fault.error.fault.kind",
    "last_fault.error.fault.slot",
    "last_fault.error.message",
    "last_fault.room",
    "last_fault.target",
    "last_fault.tick",
    "metadata",
    "mods.options",
    "mods.packages.*.files_sha256",
    "mods.packages.*.manifest",
    "room_schema_version",
    "workers.*.bucket_cap",
    "workers.*.constant_overrides.*",
    "workers.*.cpu_limit",
    "workers.*.heap_limit_bytes",
    "workers.*.initial_bucket",
    "workers.*.install_wall_limit",
    "workers.*.max_per_tick",
    "workers.*.modules.*.binary[]",
    "workers.*.modules.*.js",
];

#[test]
fn checkpoint_schema_keys_are_pinned() -> anyhow::Result<()> {
    let mut tree = BTreeSet::new();
    schema_tree_official_packages(&mut tree)?;
    schema_tree_fault_and_extension_package(&mut tree)?;
    let expected: BTreeSet<String> = PINNED_CHECKPOINT_KEYS
        .iter()
        .map(|key| (*key).to_string())
        .collect();
    let added: Vec<&String> = tree.difference(&expected).collect();
    let missing: Vec<&String> = expected.difference(&tree).collect();
    assert!(
        added.is_empty() && missing.is_empty(),
        "检查点 schema 键树与钉死清单不一致（新增字段须当批补齐保存恢复、\
         版本策略并同步 state-inventory 文档与清单）：\n新增: {added:?}\n缺失: {missing:?}"
    );
    Ok(())
}

/// 执行策略身份的两个兼容分支钉死：(a) 缺键的检查点跳过核对仍可恢复
///（engine_version 精确门下只可能是同版本开发期产物；G23.a 决定是否
/// 升格硬门前，该语义由本测试锁定）；(b) 原生检查点携带该键即拒。
#[test]
fn strategy_key_compatibility_branches_are_pinned() -> anyhow::Result<()> {
    // (a) 删除键 → 恢复成功且续跑。
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    run(&mut engine, 1)?;
    let mut saved = engine.checkpoint(Value::Null)?;
    assert!(
        saved["execution_strategy"].is_string(),
        "当前保存路径必带该键"
    );
    saved
        .as_object_mut()
        .expect("检查点是对象")
        .remove("execution_strategy");
    engine.restore_checkpoint(saved, Some((&packages, &options)))?;
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 2, "缺键跳过后正常续跑");

    // (b) 原生检查点注入该键 → 拒绝且旧分片完整。
    let mut native = EmbedEngine::new();
    native.create_shard(&json!({"players": ["p1"], "rooms": []}))?;
    let mut poisoned = native.checkpoint(Value::Null)?;
    assert!(
        poisoned["execution_strategy"].is_null(),
        "无 Mod 分片为 null"
    );
    poisoned["execution_strategy"] = json!("0".repeat(64));
    assert!(native.restore_checkpoint(poisoned, None).is_err());
    let intact = native.checkpoint(Value::Null)?;
    assert!(
        intact["execution_strategy"].is_null(),
        "拒绝后旧分片保持原生形态"
    );
    Ok(())
}
