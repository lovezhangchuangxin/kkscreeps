//! 从父测试模块拆出的领域验收，复用原夹具。
use super::*;

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
            .edit_players(|players| {
                let p1 = players.get_mut("p1").unwrap();
                p1.resources.insert("pixel".into(), 5);
                // power 域（M2H.a）：塞入一条记录使注册表子树进键树钉死。
                p1.power_creeps.insert(
                    7,
                    kk_engine::settle::PowerCreepRecord {
                        name: "PC".into(),
                        class_name: "operator".into(),
                        level: 1,
                        hits_max: 2000,
                        store_capacity: 200,
                        powers: [(1, 1)].into_iter().collect(),
                        spawn_cooldown_time: None,
                        delete_time: None,
                    },
                );
                p1.power = 16000;
                Ok(())
            })
            .unwrap();
        engine
            .engine
            .edit_world(|world| {
                world.edit_room("W1N1", |room| {
                    let id = *room.objects.keys().next().unwrap();
                    room.effects.insert(
                        id,
                        vec![kk_world::EffectEntry {
                            effect: Some(3),
                            power: 3,
                            level: 1,
                            end_time: 100,
                        }],
                    );
                    Ok(())
                })
            })
            .unwrap();
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
        .edit_room("W1N1", |room| {
            room.extensions.insert(
                ObjId::parse(&link).unwrap(),
                BTreeMap::from([("cpfields:charge".into(), json!(99))]),
            );
            Ok(())
        })
        .unwrap();
    for _ in 0..2 {
        assert!(engine.tick(1).is_err());
        engine.discard_pending_round();
        engine.rebuild_mod_realm()?;
    }
    collect_schema_keys(&engine.checkpoint(Value::Null)?, String::new(), tree);
    Ok(())
}

/// 私有 NPC 记忆同时采集 null 与完整路径，避免可空分支漏出键树门禁。
fn schema_tree_keeper_memory(tree: &mut BTreeSet<String>) -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({"players":["p1"],"rooms":[{"name":"W1N1"}]}))?;
    let keeper = engine.place_object(
        "W1N1",
        "creep",
        &json!({
            "pos":[10,10],"name":"keeper","owner":kk_world::domain::keeper::OWNER,"body":["move"]
        }),
    )?;
    engine.tick(1)?;
    collect_schema_keys(&engine.checkpoint(Value::Null)?, String::new(), tree);
    // source 无独立放置类别，须走 structure 通道才会成为原生 Source 变体。
    engine.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"source","pos":[15,10]}),
    )?;
    engine.tick(1)?;
    let saved = engine.checkpoint(Value::Null)?;
    assert!(
        saved["engine"]["world"]["rooms"]["W1N1"]["keeper_memory"]
            [ObjId::parse(&keeper).unwrap().0.to_string()]["move_cache"]
            .is_object()
    );
    collect_schema_keys(&saved, String::new(), tree);
    Ok(())
}

fn schema_tree_invader_memory(tree: &mut BTreeSet<String>) -> anyhow::Result<()> {
    let mut engine = EmbedEngine::new();
    engine.create_shard(&json!({"players":["p1"],"rooms":[{"name":"W1N1"}]}))?;
    let id = engine.place_object(
        "W1N1",
        "creep",
        &json!({"pos":[10,10],"owner":"Invader","name":"invader","body":["attack","move"]}),
    )?;
    engine.place_object(
        "W1N1",
        "creep",
        &json!({"pos":[18,10],"owner":"p1","name":"target","body":["move"]}),
    )?;
    engine.tick(1)?;
    let saved = engine.checkpoint(Value::Null)?;
    let key = ObjId::parse(&id).unwrap().0.to_string();
    assert!(
        saved["engine"]["world"]["rooms"]["W1N1"]["invader_memory"][&key]["move_cache"].is_object()
    );
    collect_schema_keys(&saved, String::new(), tree);
    let mut empty = saved;
    empty["engine"]["world"]["rooms"]["W1N1"]["invader_memory"][&key] =
        json!({"move_cache":null,"last_move":null});
    engine.restore_checkpoint(empty, None)?;
    collect_schema_keys(&engine.checkpoint(Value::Null)?, String::new(), tree);
    Ok(())
}

/// 钉死检查点 schema 键树：夹具取值非空的字段增删会让本测试红，须同步
/// docs/architecture/state-inventory.md 与清单并当批补齐保存恢复、版本
/// 策略。覆盖边界（数组只下钻首元素、Value 载荷按当时形状、不透明叶子）
/// 见该文档"覆盖边界"一节。
const PINNED_CHECKPOINT_KEYS: &[&str] = &[
    "checkpoint_format_version",
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
    "engine.players.*.power",
    "engine.players.*.power_creeps.*.class_name",
    "engine.players.*.power_creeps.*.delete_time",
    "engine.players.*.power_creeps.*.hits_max",
    "engine.players.*.power_creeps.*.level",
    "engine.players.*.power_creeps.*.name",
    "engine.players.*.power_creeps.*.powers.*",
    "engine.players.*.power_creeps.*.spawn_cooldown_time",
    "engine.players.*.power_creeps.*.store_capacity",
    "engine.players.*.public_ids[]",
    "engine.players.*.raw_memory",
    "engine.players.*.resources.*",
    "engine.players.*.rooms[]",
    "engine.players.*.segments.*",
    "engine.world.generation",
    "engine.world.next_obj_id",
    "engine.world.player_ticks",
    "engine.world.rooms.*.effects.*[]",
    "engine.world.rooms.*.effects.*[].effect",
    "engine.world.rooms.*.effects.*[].power",
    "engine.world.rooms.*.effects.*[].level",
    "engine.world.rooms.*.effects.*[].end_time",
    "engine.world.rooms.*.extensions.*",
    "engine.world.rooms.*.invader_memory.*.last_move",
    "engine.world.rooms.*.invader_memory.*.move_cache",
    "engine.world.rooms.*.invader_memory.*.move_cache.destination[]",
    "engine.world.rooms.*.invader_memory.*.move_cache.time",
    "engine.world.rooms.*.invader_memory.*.move_cache.path[]",
    "engine.world.rooms.*.invader_memory.*.move_cache.path[][]",
    "engine.world.rooms.*.keeper_memory.*.source_id",
    "engine.world.rooms.*.keeper_memory.*.move_cache",
    "engine.world.rooms.*.keeper_memory.*.move_cache.destination[]",
    "engine.world.rooms.*.keeper_memory.*.move_cache.path[]",
    "engine.world.rooms.*.keeper_memory.*.move_cache.path[][]",
    "engine.world.rooms.*.keeper_memory.*.move_cache.time",
    "engine.world.rooms.*.snapshot[]",
    "engine.world.seed",
    "engine.world.terrain_epoch",
    "engine.world.time",
    "engine_version",
    "execution_strategy",
    "fault_streak",
    "fault_streak.consecutive_faults",
    "fault_streak.mod_id",
    "host_protocol",
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
    // 无 mod 包分片的 mods 为 null 叶子（与可空 binary 的分支计键同理）。
    "mods",
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
    "workers.*.modules.*.binary",
    "workers.*.modules.*.binary[]",
    "workers.*.modules.*.js",
];

#[test]
fn checkpoint_schema_keys_are_pinned() -> anyhow::Result<()> {
    let mut tree = BTreeSet::new();
    schema_tree_official_packages(&mut tree)?;
    schema_tree_fault_and_extension_package(&mut tree)?;
    schema_tree_keeper_memory(&mut tree)?;
    schema_tree_invader_memory(&mut tree)?;
    assert_eq!(
        PINNED_CHECKPOINT_KEYS.len(),
        130,
        "钉死键总数；可空 binary 的 null 与数组分支、无包分片的 mods null 分支分别计键，增删时同步清单"
    );
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

/// 执行策略身份与格式版本的分支钉死（G23.a 后口径，仅本地开发无存量
/// 用户）：v2 Mod 检查点缺 execution_strategy 即拒；显式 v1 载荷（带
/// checkpoint_format_version:1、无该键）经迁移后按 legacy 宽容恢复；
/// 缺格式版本即拒；原生检查点携带该键即拒。
#[test]
fn strategy_key_compatibility_branches_are_pinned() -> anyhow::Result<()> {
    // (a1) v2 载荷仅删 execution_strategy → 硬门拒绝。
    let packages = packages()?;
    let options = options();
    let mut engine = prepare(false, &packages, &options)?;
    run(&mut engine, 1)?;
    let saved = engine.checkpoint(Value::Null)?;
    assert!(
        saved["execution_strategy"].is_string(),
        "当前保存路径必带该键"
    );
    let mut v2_missing = saved.clone();
    v2_missing
        .as_object_mut()
        .expect("检查点是对象")
        .remove("execution_strategy");
    assert!(
        engine
            .restore_checkpoint(v2_missing, Some((&packages, &options)))
            .is_err(),
        "v2 起 Mod 检查点必须携带执行策略身份"
    );

    // (a2) 显式 v1（版本 1、无执行策略键）→ 迁移后宽容恢复且续跑。
    let mut v1_payload = saved.clone();
    let map = v1_payload.as_object_mut().expect("检查点是对象");
    map.remove("execution_strategy");
    map.insert("checkpoint_format_version".into(), json!(1));
    engine.restore_checkpoint(v1_payload, Some((&packages, &options)))?;
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3, "v1 宽容语义恢复后正常续跑");

    // (a3) 缺格式版本 → 畸形载荷拒绝（无缺键猜测）。
    let mut no_version = saved.clone();
    no_version
        .as_object_mut()
        .expect("检查点是对象")
        .remove("checkpoint_format_version");
    assert!(
        engine
            .restore_checkpoint(no_version, Some((&packages, &options)))
            .is_err(),
        "缺 checkpoint_format_version 即拒"
    );
    assert_eq!(
        engine.checkpoint(Value::Null)?["checkpoint_format_version"],
        json!(6),
        "当前保存恒为格式 6"
    );

    // (b) 原生检查点注入该键 → 拒绝且旧分片保持原生形态。
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
