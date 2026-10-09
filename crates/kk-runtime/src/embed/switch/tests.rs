//! 包切换的行为验收：增/减包、残留与内容变更拒绝、边界门、失败保旧、
//! 停用投影与故障过滤；冷恢复以新包集合身份通过属关键回归。
use super::*;
use kk_runner::ModuleTable;
use std::path::Path;

fn discover(root: &str) -> anyhow::Result<BTreeMap<String, ModPackage>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(root)
        .canonicalize()?;
    kk_mod_host::discover_packages(&root).map_err(anyhow::Error::from)
}

fn official() -> anyhow::Result<BTreeMap<String, ModPackage>> {
    discover("../../mods/official")
}

fn composition() -> anyhow::Result<BTreeMap<String, ModPackage>> {
    discover("../../compat/fixtures/mod-composition")
}

fn selection(ids: &[&str]) -> RegisterOptions {
    RegisterOptions {
        selection: kk_mod_host::ContentSelection {
            include: ids.iter().map(|id| (*id).into()).collect(),
            exclude: BTreeSet::new(),
        },
        fault_disable_threshold: std::num::NonZeroU32::new(3),
        ..Default::default()
    }
}

fn prepare(
    packages: &BTreeMap<String, ModPackage>,
    options: &RegisterOptions,
) -> anyhow::Result<EmbedEngine> {
    let mut engine = EmbedEngine::new();
    engine.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "owner": "p1", "rcl": 6}]}),
        packages,
        options.clone(),
    )?;
    let mut table = ModuleTable::new();
    table.insert_js(
        "main",
        "exports.loop = () => { Memory.loops = (Memory.loops || 0) + 1; };",
    );
    engine.set_player_modules("p1", table)?;
    Ok(engine)
}

fn run(engine: &mut EmbedEngine, count: usize) -> anyhow::Result<()> {
    let codes = BTreeMap::from([("p1".into(), "require('main').loop()".into())]);
    for _ in 0..count {
        let results = engine.run_players(&codes)?;
        assert!(results.values().all(|r| r.expr["ok"] == true));
    }
    Ok(())
}

/// 增包：身份换封（plan_hash/执行策略变化、包集合差集正确）、新包
/// 效果在切换后才出现、切换后检查点以新包集合冷恢复通过且旧选择集
/// 恢复被身份门拒绝。
#[test]
fn switch_adds_package_reseals_identity_and_survives_cold_restore() -> anyhow::Result<()> {
    let packages = official()?;
    let mut engine = prepare(&packages, &selection(&["nursery"]))?;
    engine.place_mod_object("W1N1", "nursery:emitter", &json!({"x": 10, "y": 10}))?;
    run(&mut engine, 1)?;
    let strategy_before = engine.mod_execution_strategy().expect("策略身份在位");

    let summary = engine.switch_mod_packages(&packages, &selection(&["nursery", "clockwork"]))?;
    assert_eq!(summary["added"], json!(["clockwork"]));
    assert_eq!(summary["removed"], json!([]));
    assert_ne!(
        summary["plan_hash_before"], summary["plan_hash_after"],
        "增包必产生新计划身份"
    );
    assert_ne!(
        summary["execution_strategy_before"], summary["execution_strategy_after"],
        "增包必产生新执行策略身份"
    );
    assert_eq!(summary["execution_strategy_before"], json!(strategy_before));

    // 新包效果只在切换后出现：clockwork 对象落位并写 state。
    engine.place_mod_object("W1N1", "clockwork:counter", &json!({"x": 12, "y": 10}))?;
    run(&mut engine, 2)?;
    let saved = engine.checkpoint(Value::Null)?;
    assert!(
        saved["engine"]["mod_states"]["clockwork"].is_object(),
        "切换后 clockwork 才开始运行并写 state"
    );

    // 冷恢复以新包集合通过；旧选择集被 options 身份门拒绝。
    let mut restored = EmbedEngine::new();
    restored.restore_checkpoint(
        saved.clone(),
        Some((&packages, &selection(&["nursery", "clockwork"]))),
    )?;
    run(&mut restored, 1)?;
    assert_eq!(restored.get_game_time(), 5);
    let mut stale = EmbedEngine::new();
    assert!(
        stale
            .restore_checkpoint(saved, Some((&packages, &selection(&["nursery"]))))
            .is_err(),
        "切换后的检查点携带新选择集身份，旧选择集必须拒绝"
    );
    Ok(())
}

/// 减包（无残留）：z-gate 规则覆盖随包移除回落内建默认，世界续跑；
/// 依赖闭包成员一并移除。
#[test]
fn switch_removes_package_without_residue_and_falls_back_rules() -> anyhow::Result<()> {
    let packages = composition()?;
    let mut engine = prepare(
        &packages,
        &selection(&["base", "z-gate", "a-audit", "dependent"]),
    )?;
    engine.place_mod_object("W1N1", "base:counter", &json!({"x": 10, "y": 10}))?;
    run(&mut engine, 1)?;
    assert_eq!(engine.engine.links().capacity, 700, "z-gate 覆盖生效中");

    let summary = engine.switch_mod_packages(&packages, &selection(&["base"]))?;
    assert_eq!(
        summary["removed"],
        json!(["a-audit", "dependent", "z-gate"])
    );
    assert_eq!(summary["added"], json!([]));
    assert_ne!(engine.engine.links().capacity, 700, "规则回落内建默认");

    // base:counter 独立对象中心不依赖被移除的链成员，继续结算。
    run(&mut engine, 2)?;
    assert_eq!(engine.get_game_time(), 4);
    let saved = engine.checkpoint(Value::Null)?;
    let mut restored = EmbedEngine::new();
    restored.restore_checkpoint(saved, Some((&packages, &selection(&["base"]))))?;
    Ok(())
}

/// 去包残留硬门：世界存在已移除类型的 Mod 对象时切换拒绝，旧分片
/// 完整且继续运行（不清理、不修补，指路内容级迁移）。
#[test]
fn switch_rejects_residue_and_content_change_keeping_old_shard() -> anyhow::Result<()> {
    let packages = official()?;
    let mut engine = prepare(&packages, &selection(&["nursery", "clockwork"]))?;
    engine.place_mod_object("W1N1", "nursery:emitter", &json!({"x": 10, "y": 10}))?;
    engine.place_mod_object("W1N1", "clockwork:counter", &json!({"x": 12, "y": 10}))?;
    run(&mut engine, 1)?;
    let before = engine.checkpoint(Value::Null)?;
    let strategy_before = engine.mod_execution_strategy().expect("策略身份在位");

    let residue = engine
        .switch_mod_packages(&packages, &selection(&["nursery"]))
        .expect_err("clockwork:counter 对象仍在世界，必须拒绝");
    assert!(
        residue.to_string().contains("残留"),
        "残留拒绝要指路清理/G23：{residue:#}"
    );
    assert_eq!(
        engine.checkpoint(Value::Null)?,
        before,
        "拒绝路径不动旧分片"
    );
    assert_eq!(
        engine.mod_execution_strategy().as_deref(),
        Some(strategy_before.as_str())
    );
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3, "旧分片继续运行");

    // 同包内容变更：交集包文件摘要漂移即拒绝。
    let mut drifted = official()?;
    drifted
        .get_mut("clockwork")
        .expect("包在")
        .files
        .insert(std::path::PathBuf::from("extra.js"), b"// drift".to_vec());
    let content = engine
        .switch_mod_packages(&drifted, &selection(&["nursery", "clockwork"]))
        .expect_err("同包内容变更必须拒绝");
    assert!(
        content.to_string().contains("内容级迁移"),
        "内容变更拒绝要指路 G23：{content:#}"
    );
    assert_eq!(engine.get_game_time(), 3, "内容拒绝同样不动旧分片");
    Ok(())
}

/// 边界门与失败注入：开放轮次拒绝（可放弃后重试）；候选全验证后、
/// 切换前的注入失败保留旧分片（含 console 历史），第二次切换成功。
#[test]
fn switch_rejects_open_round_and_injected_failure_keeps_old_shard() -> anyhow::Result<()> {
    let packages = official()?;
    let mut engine = prepare(&packages, &selection(&["nursery"]))?;
    // 产生一条 console 历史：注入失败路径不得把它搬进报废的候选分片。
    let codes = BTreeMap::from([(
        "p1".into(),
        "console.log('before switch'); require('main').loop()".into(),
    )]);
    let results = engine.run_players(&codes)?;
    assert!(results.values().all(|r| r.expr["ok"] == true));
    let console_of = |engine: &EmbedEngine| -> anyhow::Result<Value> {
        Ok(engine.checkpoint(Value::Null)?["last_console"]["p1"].clone())
    };
    assert!(
        !console_of(&engine)?.as_array().is_some_and(Vec::is_empty),
        "前置：console 历史非空"
    );

    engine.fail_next_settle_for_test();
    let codes = BTreeMap::from([("p1".into(), "require('main').loop()".into())]);
    assert!(engine.run_players(&codes).is_err(), "结算失败留下待重试轮");
    let open = engine
        .switch_mod_packages(&packages, &selection(&["nursery", "clockwork"]))
        .expect_err("开放轮次必须拒绝");
    assert!(
        open.to_string().contains("switch_mod_packages 被拒"),
        "边界拒绝归因到切换入口：{open:#}"
    );
    engine.discard_pending_round();

    engine.fail_next_switch_for_test();
    let injected = engine
        .switch_mod_packages(&packages, &selection(&["nursery", "clockwork"]))
        .expect_err("注入失败必须保留旧分片");
    assert!(injected.to_string().contains("测试注入"));
    assert!(
        !console_of(&engine)?.as_array().is_some_and(Vec::is_empty),
        "注入失败不得搬走旧分片的 console 历史"
    );
    let codes = BTreeMap::from([(
        "p1".into(),
        "console.log('retry'); require('main').loop()".into(),
    )]);
    let results = engine.run_players(&codes)?;
    assert!(results.values().all(|r| r.expr["ok"] == true));
    assert_eq!(engine.get_game_time(), 3, "注入失败后旧计划继续结算");

    engine.switch_mod_packages(&packages, &selection(&["nursery", "clockwork"]))?;
    // last_console 记录的是最近一次完成结算的输出：在续跑覆盖之前核对
    // 成功切换把它带进了新分片。
    assert!(
        !console_of(&engine)?.as_array().is_some_and(Vec::is_empty),
        "成功切换保留 console 历史"
    );
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 4);
    Ok(())
}

/// 停用投影与故障过滤：停用集合跨切换保持（对仍安装的包）；指向已
/// 移除包的故障诊断三域记录丢弃。
#[test]
fn switch_projects_disabled_set_and_filters_fault_diagnostics() -> anyhow::Result<()> {
    let packages = official()?;
    let mut engine = prepare(&packages, &selection(&["nursery", "clockwork"]))?;
    // 停用后 clockwork 从未执行：无对象、无 state，可被干净移除。
    engine.disable_mod("clockwork")?;
    engine.rebuild_mod_realm()?;
    engine.place_mod_object("W1N1", "nursery:emitter", &json!({"x": 10, "y": 10}))?;
    run(&mut engine, 1)?;
    // 直接注入诊断记录：切换路径不允许通过真实故障制造（那会留下待
    // 恢复状态，切换入口先被边界门拒绝）。
    {
        let vm = engine.mod_realm.as_ref().expect("VM 在位");
        let mut vm = vm.borrow_mut();
        vm.fault_streak = Some(crate::ModFaultStreak {
            mod_id: "clockwork".into(),
            consecutive_faults: 2,
        });
        vm.last_fault = Some(crate::ModSettlementFault {
            tick: 1,
            room: "W1N1".into(),
            target: "counter".into(),
            error: kk_mod_host::ModHostError::new("clockwork", "staged"),
        });
        vm.last_auto_disable = Some(crate::ModAutoDisable {
            mod_id: "clockwork".into(),
            trigger: crate::ModSettlementFault {
                tick: 1,
                room: "W1N1".into(),
                target: "counter".into(),
                error: kk_mod_host::ModHostError::new("clockwork", "staged"),
            },
            consecutive_faults: 3,
            disabled: BTreeSet::from(["clockwork".into()]),
        });
    }

    engine.switch_mod_packages(&packages, &selection(&["nursery"]))?;
    assert!(
        engine.disabled_mods().is_empty(),
        "唯一停用包被移除，投影后停用集为空"
    );
    {
        let vm = engine.mod_realm.as_ref().expect("新 VM 在位");
        let vm = vm.borrow();
        assert!(vm.fault_streak.is_none(), "指向已移除包的故障计数丢弃");
        assert!(vm.last_fault.is_none(), "指向已移除包的最近故障丢弃");
        assert!(
            vm.last_auto_disable.is_none(),
            "指向已移除包的自动停用决定丢弃"
        );
    }
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3);
    Ok(())
}

/// 仍安装包的停用跨切换存活：投影保留并重放，新计划下继续按停用掩码
/// 执行；同时增包不受停用投影影响。
#[test]
fn switch_keeps_disabled_set_for_installed_packages() -> anyhow::Result<()> {
    let packages = official()?;
    let mut engine = prepare(&packages, &selection(&["nursery", "clockwork"]))?;
    engine.disable_mod("clockwork")?;
    engine.rebuild_mod_realm()?;
    engine.place_mod_object("W1N1", "nursery:emitter", &json!({"x": 10, "y": 10}))?;
    run(&mut engine, 1)?;

    let summary =
        engine.switch_mod_packages(&packages, &selection(&["nursery", "clockwork", "panel"]))?;
    assert_eq!(summary["added"], json!(["panel"]));
    assert_eq!(
        engine.disabled_mods(),
        BTreeSet::from(["clockwork".to_string()]),
        "仍安装的停用包跨切换保持停用"
    );
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3);
    Ok(())
}

/// 投影闭包增长拒绝：新计划给已停用包新增依赖者时，投影集不再满足
/// 闭包——显式拒绝（不静默扩大停用集），旧分片继续运行。
#[test]
fn switch_rejects_projection_closure_growth() -> anyhow::Result<()> {
    let packages = composition()?;
    // dependent → a-audit（manifest 依赖）：初始不含 dependent，停用
    // a-audit 的闭包就是 {a-audit}；切换补装 dependent 后闭包增长。
    let mut engine = prepare(&packages, &selection(&["base", "z-gate", "a-audit"]))?;
    engine.place_mod_object("W1N1", "base:counter", &json!({"x": 10, "y": 10}))?;
    engine.disable_mod("a-audit")?;
    engine.rebuild_mod_realm()?;
    run(&mut engine, 1)?;

    let rejected = engine
        .switch_mod_packages(
            &packages,
            &selection(&["base", "z-gate", "a-audit", "dependent"]),
        )
        .expect_err("新增依赖者使闭包增长，投影必须拒绝");
    assert!(
        rejected.to_string().contains("依赖闭包"),
        "闭包失配要有归因：{rejected:#}"
    );
    assert_eq!(
        engine.disabled_mods(),
        BTreeSet::from(["a-audit".to_string()]),
        "拒绝路径不动旧分片的停用集"
    );
    run(&mut engine, 1)?;
    assert_eq!(engine.get_game_time(), 3, "旧分片继续运行");
    Ok(())
}
