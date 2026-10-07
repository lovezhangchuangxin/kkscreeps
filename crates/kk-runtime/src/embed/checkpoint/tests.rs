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
    let id = engine
        .engine
        .edit_market(|market| Ok(market.next_order_id()))
        .unwrap();
    engine
        .engine
        .edit_market(|market| {
            market.insert_order(MarketOrder {
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
            Ok(())
        })
        .unwrap();
    let id = engine
        .engine
        .edit_market(|market| Ok(market.alloc_transaction_id()))
        .unwrap();
    engine
        .engine
        .edit_market(|market| {
            market.record_transaction(Transaction {
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
            Ok(())
        })
        .unwrap();
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
    engine
        .engine
        .edit_players(|players| {
            for player in players.values_mut() {
                player.cpu = kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, -1000.0);
            }
            Ok(())
        })
        .unwrap();
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
        engine
            .engine
            .edit_players(|players| {
                players.get_mut("p1").unwrap().cpu =
                    kk_core::cpu::Bucket::new(20.0, 10000.0, 500.0, value);
                Ok(())
            })
            .unwrap();
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
            .edit_room("W1N1", |room| {
                room.extensions.insert(
                    ObjId::parse(&link).unwrap(),
                    BTreeMap::from([
                        ("cpfields:charge".into(), json!(99)),
                        ("cpfields:target".into(), json!(link)),
                    ]),
                );
                Ok(())
            })
            .unwrap();
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

/// v1 → v2 迁移用例：last_auto_disable 旧形态（trace 批次之前的真实
/// 记录——来源只在 trigger.error.fault.binding，证据为此前宽松反序列化
/// 的注释与测试历史）经注册迁移归一为结构化形态；v2 载荷内的不一致
/// mod_id 仍被解码端一致性校验拒绝。
#[test]
fn legacy_auto_disable_checkpoint_restores_structured_source() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    engine.disable_mod("clockwork")?;
    engine.rebuild_mod_realm()?;
    let mut saved = engine.checkpoint(Value::Null)?;
    // 合成显式 v1 载荷：版本 1、旧形态、无执行策略键（v1 先于该键）。
    let map = saved.as_object_mut().expect("检查点是对象");
    map.insert("checkpoint_format_version".into(), json!(1));
    map.remove("execution_strategy");
    saved["last_auto_disable"] = json!({
        "trigger": {"tick": 0, "room": "W1N1", "target": "counter",
            "error": {"context": "clockwork", "message": "fault", "fault": {
                "slot": "clockwork:counter.tick", "binding": "clockwork:counter.tick.process", "kind": "handler"
            }}},
        "consecutive_faults": 3, "disabled": ["clockwork"]
    });
    engine.restore_checkpoint(saved, Some((&packages, &options)))?;
    assert_eq!(engine.last_mod_auto_disable().unwrap().mod_id, "clockwork");
    // 迁移后再保存即当前格式：结构化 mod_id 显式在载荷中。
    let resaved = engine.checkpoint(Value::Null)?;
    assert_eq!(resaved["checkpoint_format_version"], json!(3));
    assert_eq!(resaved["last_auto_disable"]["mod_id"], json!("clockwork"));
    // v2 载荷内来源不一致仍拒（解码端一致性校验，非迁移修补）。
    let mut invalid = resaved.clone();
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

/// 版本路由与迁移失败语义：0/未来版本拒绝；迁移步骤失败（v1 载荷的
/// last_auto_disable 无法派生来源）整体拒绝且旧分片完整——「不自动
/// 修补不一致载荷」。
#[test]
fn format_version_routing_and_migration_failure_keep_the_old_shard() -> anyhow::Result<()> {
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    run(&mut engine, 2)?;
    let saved = engine.checkpoint(Value::Null)?;

    for version in [0u64, 4] {
        let mut bad = saved.clone();
        bad["checkpoint_format_version"] = json!(version);
        assert!(
            engine
                .restore_checkpoint(bad, Some((&packages, &options)))
                .is_err(),
            "版本 {version} 须拒绝"
        );
    }

    // 迁移失败：v1 载荷的 last_auto_disable 缺故障绑定来源。
    let mut broken = saved.clone();
    let map = broken.as_object_mut().expect("检查点是对象");
    map.insert("checkpoint_format_version".into(), json!(1));
    broken["last_auto_disable"] = json!({
        "trigger": {"tick": 0, "room": "W1N1", "target": "counter",
            "error": {"context": "clockwork", "message": "fault"}},
        "consecutive_faults": 3, "disabled": ["clockwork"]
    });
    assert!(
        engine
            .restore_checkpoint(broken, Some((&packages, &options)))
            .is_err(),
        "无法派生来源的载荷是畸形数据，不得修补"
    );
    assert_eq!(
        engine.checkpoint(Value::Null)?,
        saved,
        "迁移失败后旧分片保持完整"
    );
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3, "旧分片继续可运行");
    Ok(())
}

/// 迁移层直测：不经解码端宽容反序列化，证明归一确实发生在迁移里——
/// v1 载荷在解码前由 `v1_to_v2` 写入结构化 mod_id 并统一盖版本号；
/// 已是当前版本的载荷 `skip` 过全部步骤保持原样；版本字段的缺失、
/// 非法与超 u32 形态各有独立拒绝（不与缺键混同）。
#[test]
fn migrate_to_current_normalizes_v1_and_routes_versions() {
    let mut v1 = json!({
        "checkpoint_format_version": 1,
        "last_auto_disable": {
            "trigger": {"error": {"fault": {"binding": "clockwork:counter.tick.process"}}},
            "consecutive_faults": 3
        }
    });
    assert_eq!(migrate::migrate_to_current(&mut v1), Ok(1));
    assert_eq!(v1["checkpoint_format_version"], json!(3));
    assert_eq!(v1["last_auto_disable"]["mod_id"], json!("clockwork"));

    // 当前版本载荷：skip 语义下不重放任何步骤，载荷逐字保持。
    let mut v2 = json!({
        "checkpoint_format_version": 2,
        "last_auto_disable": {"mod_id": "kept", "note": "untouched"}
    });
    assert_eq!(migrate::migrate_to_current(&mut v2), Ok(2));
    assert_eq!(
        v2["last_auto_disable"].clone(),
        json!({"mod_id": "kept", "note": "untouched"})
    );

    // 迁移层自身的失败路径（区别于解码端同语义校验）。
    let mut underivable = json!({
        "checkpoint_format_version": 1,
        "last_auto_disable": {"trigger": {"error": {"message": "fault"}}}
    });
    assert!(
        migrate::migrate_to_current(&mut underivable)
            .unwrap_err()
            .contains("缺少故障绑定来源")
    );

    // 版本字段分诊：缺失、非整数、超 u32、0、未来版本各自独立拒绝。
    let cases: [(&str, Value, &str); 5] = [
        ("缺失", Value::Null, "缺少 checkpoint_format_version"),
        ("字符串", json!("2"), "必须是非负 u32 整数"),
        ("超 u32", json!(4_294_967_298u64), "必须是非负 u32 整数"),
        ("零", json!(0), "格式版本 0 无效"),
        ("未来", json!(4), "高于当前支持"),
    ];
    for (label, version, message) in cases {
        let mut payload = json!({"checkpoint_format_version": version, "time": 1});
        if version.is_null() {
            payload = json!({"time": 1});
        }
        let error = migrate::migrate_to_current(&mut payload).expect_err(label);
        assert!(error.contains(message), "{label}: {error}");
    }
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

mod schema;

#[test]
fn old_date_protocol_and_missing_v3_identity_reject_without_changing_world() -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({"players":[],"rooms":[]}))?;
    let saved = engine.checkpoint(Value::Null)?;
    let mut old = saved.clone();
    old["checkpoint_format_version"] = json!(2);
    old.as_object_mut().unwrap().remove("host_protocol");
    let error = engine.restore_checkpoint(old, None).unwrap_err();
    assert!(error.to_string().contains("旧时区语义"));
    assert_eq!(engine.checkpoint(Value::Null)?, saved);
    let mut missing = saved.clone();
    missing.as_object_mut().unwrap().remove("host_protocol");
    assert!(engine.restore_checkpoint(missing, None).is_err());
    assert_eq!(engine.checkpoint(Value::Null)?, saved);
    Ok(())
}
