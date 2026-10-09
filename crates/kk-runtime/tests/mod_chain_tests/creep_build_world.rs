//! 建造族槽开放后保留既有世界语义：建造完工变换、repair 的耐久/能量、
//! dismantle 的击毁废墟与超容掉落、rampart 重定向、无 Mod 路径零变化
//! 与双玩家一致性。
use super::*;
use harness::{creep_at, events, fighter, hits, observe_pkg, predictable_id};

/// repair 的耐久/能量恢复与事件截除。
#[test]
fn creep_repair_restores_hits_and_energy_on_decline() -> anyhow::Result<()> {
    let reject =
        package("api.handlers.after('builtin:creep.repair',{id:'no'},()=>api.decline('peace'));");
    for (pkg, repaired, label) in [
        (
            observe_pkg("builtin:creep.repair"),
            true,
            "控制组：修理落地",
        ),
        (reject, false, "拒绝组：双方恢复"),
    ] {
        let mut e = engine(&[("alpha", pkg)], false)?;
        let r1 = fighter(&mut e, "r1", 10, 10, &["work"])?;
        let wall = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"constructedWall","owner":"p1","hits":900}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.r1.repair(Game.getObjectById('{wall}'))"),
        )?;
        assert_eq!(
            hits(&e, &wall),
            if repaired { 1000 } else { 900 },
            "{label}: 耐久"
        );
        assert_eq!(
            e.get_object(&r1).unwrap()["store"]["energy"],
            if repaired { 29 } else { 30 },
            "{label}: 能量"
        );
        let repairs = events(&e, kk_core::constants::EVENT_REPAIR);
        assert_eq!(repairs.len(), if repaired { 1 } else { 0 }, "{label}");
        if repaired {
            assert_eq!(repairs[0]["data"]["amount"], json!(100));
            assert_eq!(repairs[0]["data"]["energySpent"], json!(1));
        }
    }
    Ok(())
}

/// repair 的 replace 改向：原目标耐久不变，改向目标获得修复量。
#[test]
fn creep_repair_replace_redirects_to_another_damaged_structure() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let redirect = package(&format!(
        "api.handlers.replace('builtin:creep.repair',{{id:'reroute'}},ctx=>{{ctx.build.request('{alt_id}');}});"
    ));
    let mut e = engine(&[("alpha", redirect)], false)?;
    let alt = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[12,10],"structureType":"constructedWall","owner":"p1","hits":900}),
    )?;
    assert_eq!(alt, alt_id, "改向目标 ID 预测破裂");
    let _r1 = fighter(&mut e, "r1", 10, 10, &["work"])?;
    let wall = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"constructedWall","owner":"p1","hits":800}),
    )?;
    e.run_player(
        "p1",
        &format!("Game.creeps.r1.repair(Game.getObjectById('{wall}'))"),
    )?;
    assert_eq!(hits(&e, &wall), 800, "原目标无修复");
    assert_eq!(hits(&e, &alt), 1000, "改向目标获得修复");
    let repairs = events(&e, kk_core::constants::EVENT_REPAIR);
    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0]["data"]["targetId"], json!(alt));
    Ok(())
}

/// dismantle 击毁：先回收再施伤——能量经超容掉落进入同格容器；拒绝时
/// 结构复原、废墟/掉落撤销、容器与事件恢复。5×WORK 拆解功率 250，
/// 目标 200 耐久被击毁，回收 1 能量。
#[test]
fn creep_dismantle_destroy_drop_and_restore_on_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.wrap('builtin:creep.dismantle',{id:'no'},(ctx,next)=>{next();return api.decline('peace');});",
    );
    for (pkg, destroyed, label) in [
        (
            observe_pkg("builtin:creep.dismantle"),
            true,
            "控制组：击毁与掉落",
        ),
        (reject, false, "拒绝组：结构复原"),
    ] {
        let mut e = engine(&[("alpha", pkg)], false)?;
        let _d1 = fighter(
            &mut e,
            "d1",
            10,
            10,
            &["work", "work", "work", "work", "work"],
        )?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,10],"structureType":"container","owner":"p1","hits":250000,"store":{"energy":10}}),
        )?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":200,"store":{"energy":5}}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.d1.dismantle(Game.getObjectById('{tower}'))"),
        )?;
        let ruins = e.find_in_room("W1N1", kk_core::constants::FIND_RUINS);
        let dropped = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
        if destroyed {
            assert!(e.get_object(&tower).is_none(), "{label}: 结构移除");
            assert_eq!(ruins.len(), 1, "{label}");
            assert_eq!(ruins[0]["store"]["energy"], 5, "{label}: 资源转存废墟");
            // 无 CARRY：既有 30 能量与回收 1 全部超容掉落，先入同格容器。
            assert_eq!(
                e.get_object(&container).unwrap()["store"]["energy"],
                41,
                "{label}: 掉落先入容器"
            );
            assert!(dropped.is_empty(), "{label}: 容器余量足够，无掉落堆");
            assert_eq!(
                events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).len(),
                1,
                "{label}"
            );
            let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(attacks.len(), 1, "{label}");
            assert_eq!(
                attacks[0]["data"]["attackType"],
                kk_core::constants::EVENT_ATTACK_TYPE_DISMANTLE,
                "{label}"
            );
            assert_eq!(attacks[0]["data"]["targetId"], json!(tower));
        } else {
            assert_eq!(hits(&e, &tower), 200, "{label}: 首触记录复原");
            assert!(ruins.is_empty(), "{label}: 废墟插入被撤销");
            assert!(dropped.is_empty(), "{label}");
            assert_eq!(
                e.get_object(&container).unwrap()["store"]["energy"],
                10,
                "{label}: 掉落面容器恢复"
            );
            assert!(
                events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).is_empty(),
                "{label}"
            );
            assert!(
                events(&e, kk_core::constants::EVENT_ATTACK).is_empty(),
                "{label}"
            );
        }
    }
    Ok(())
}

/// 超容掉落溢出容器的恢复：容器满时余量合并为同格掉落堆，拒绝时堆
/// 插入与执行者能量一并撤销。
#[test]
fn creep_dismantle_overflow_drop_restores_pile_and_energy() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.after('builtin:creep.dismantle',{id:'no'},()=>api.decline('peace'));",
    );
    for (pkg, dropped_energy, label) in [
        (
            observe_pkg("builtin:creep.dismantle"),
            true,
            "控制组：掉落堆",
        ),
        (reject, false, "拒绝组：掉落与能量恢复"),
    ] {
        let mut e = engine(&[("alpha", pkg)], false)?;
        let d1 = fighter(
            &mut e,
            "d1",
            10,
            10,
            &["work", "work", "work", "work", "work"],
        )?;
        // 容器仅剩 0 容量：掉落全部合并为同格能量堆。
        let _full = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,10],"structureType":"container","owner":"p1","hits":250000,"store":{"energy":2000}}),
        )?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":200}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.d1.dismantle(Game.getObjectById('{tower}'))"),
        )?;
        let piles = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
        if dropped_energy {
            assert!(e.get_object(&tower).is_none(), "{label}");
            assert_eq!(piles.len(), 1, "{label}");
            assert_eq!(
                piles[0]["amount"], 30,
                "{label}: 既有 30 与回收 1（200/200）成堆后同 tick 衰减 ceil(31/1000)=1"
            );
        } else {
            assert_eq!(hits(&e, &tower), 200, "{label}");
            assert!(piles.is_empty(), "{label}: 掉落堆插入被撤销");
            assert_eq!(
                e.get_object(&d1).unwrap()["store"]["energy"],
                30,
                "{label}: 执行者能量恢复"
            );
        }
    }
    Ok(())
}

/// dismantle 经 rampart 重定向：目标格上的 rampart 承伤；拒绝时 rampart
/// 与目标的记录及事件全部恢复。
#[test]
fn creep_dismantle_rampart_redirect_restores_both_on_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.replace('builtin:creep.dismantle',{id:'no'},ctx=>api.decline('peace'));",
    );
    for (pkg, damaged, label) in [
        (
            observe_pkg("builtin:creep.dismantle"),
            true,
            "控制组：rampart 承伤",
        ),
        (reject, false, "拒绝组：双方复原"),
    ] {
        let mut e = engine(&[("alpha", pkg)], false)?;
        let _d1 = fighter(
            &mut e,
            "d1",
            10,
            10,
            &["work", "work", "work", "work", "work"],
        )?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":3000}),
        )?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"rampart","owner":"p1","hits":1000}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.d1.dismantle(Game.getObjectById('{tower}'))"),
        )?;
        assert_eq!(hits(&e, &tower), 3000, "{label}: 目标本体无伤");
        assert_eq!(
            hits(&e, &rampart),
            if damaged { 750 } else { 1000 },
            "{label}: rampart 承伤面"
        );
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), if damaged { 1 } else { 0 }, "{label}");
        if damaged {
            assert_eq!(attacks[0]["data"]["targetId"], json!(rampart));
        }
    }
    Ok(())
}

/// 无 Mod 引擎与 observe 链的终态一致（JSON/KKPV 双通道）：建造进度、
/// repair 耐久与 dismantle 击毁经整槽宿主执行后与原生直调相同。
#[test]
fn creep_build_family_native_and_observed_settle_agree() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut snapshots = Vec::new();
        // 空注册 Mod 不挂任何处理器，三槽保持 Native 直调，即原生路径对照。
        let empty_pkg = package("");
        for pkg in [empty_pkg, observe_pkg("builtin:creep.build")] {
            let mut e = engine(&[("alpha", pkg)], binary)?;
            let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
            )?;
            let _r1 = fighter(&mut e, "r1", 10, 12, &["work"])?;
            let wall = e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[11,12],"structureType":"constructedWall","owner":"p1","hits":900}),
            )?;
            let _d1 = fighter(
                &mut e,
                "d1",
                10,
                14,
                &["work", "work", "work", "work", "work"],
            )?;
            let tower = e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[11,14],"structureType":"tower","owner":"p1","hits":200}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "const o=Game.getObjectById;[Game.creeps.b1.build(o('{site}')),\
                     Game.creeps.r1.repair(o('{wall}')),Game.creeps.d1.dismantle(o('{tower}'))]"
                ),
            )?;
            assert_eq!(e.get_object(&site).unwrap()["progress"], 5);
            assert_eq!(hits(&e, &wall), 1000);
            assert!(e.get_object(&tower).is_none());
            assert_eq!(
                e.find_in_room("W1N1", kk_core::constants::FIND_RUINS).len(),
                1
            );
            snapshots.push(e.world_snapshot());
        }
        assert_eq!(
            snapshots[0], snapshots[1],
            "{binary}: 无 Mod 与 observe 链终态一致"
        );
    }
    Ok(())
}

/// 双玩家双房间：拒绝只作用于门控执行者；串行与并发派发结果一致。
#[test]
fn creep_build_family_dual_player_rooms_and_dispatch_modes_agree() -> anyhow::Result<()> {
    let shard = json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 6, "owner": "p1" },
            { "name": "W0N1", "rcl": 4, "owner": "p2" },
        ]
    });
    let source = package(
        r#"
api.handlers.before('builtin:creep.build',{id:'gate-build'},ctx=>{
  if(ctx.target.fields.owner==='p2')return api.decline('not-you');
  ctx.state={builds:(ctx.state?.builds||0)+1};
});
api.handlers.before('builtin:creep.dismantle',{id:'gate-dismantle'},ctx=>{
  if(ctx.target.fields.owner==='p2')return api.decline('not-you');
  ctx.state={builds:(ctx.state?.builds||0)+1};
});
"#,
    );
    let mut snapshots = Vec::new();
    for concurrency in [1usize, 2] {
        let mut e = engine_with_shard(
            &[("alpha", source.clone())],
            false,
            &[],
            RegisterOptions::default(),
            &shard,
        )?;
        let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
        let site1 = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
        )?;
        let _d2 = creep_at(
            &mut e,
            "W0N1",
            "d2",
            "p2",
            20,
            20,
            &["work", "work", "work", "work", "work"],
        )?;
        let tower2 = e.place_object(
            "W0N1",
            "structure",
            &json!({"pos":[21,20],"structureType":"tower","owner":"p2","hits":200}),
        )?;
        // 门控拒绝 p2 执行者：p1 建造落地、p2 拆解被拒。
        let mut codes = std::collections::BTreeMap::new();
        codes.insert(
            "p1".to_string(),
            format!("Memory.p=1;Game.creeps.b1.build(Game.getObjectById('{site1}'))"),
        );
        codes.insert(
            "p2".to_string(),
            format!("Memory.p=2;Game.creeps.d2.dismantle(Game.getObjectById('{tower2}'))"),
        );
        e.set_player_concurrency(concurrency)?;
        let runs = e.run_players(&codes)?;
        assert_eq!(runs["p1"].expr["value"], json!(0));
        assert_eq!(runs["p2"].expr["value"], json!(0));
        assert_eq!(e.get_object(&site1).unwrap()["progress"], 5, "p1 建造保留");
        assert_eq!(hits(&e, &tower2), 200, "p2 拆解被拒绝");
        assert_eq!(
            e.player_state("p1").unwrap().raw_memory,
            "{\"p\":1}",
            "两名玩家整轮照常发布"
        );
        assert_eq!(e.player_state("p2").unwrap().raw_memory, "{\"p\":2}");
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"builds":1}),
            "仅未被拒的 p1 执行者累计"
        );
        snapshots.push(e.world_snapshot());
    }
    assert_eq!(snapshots[0], snapshots[1], "串行与并发派发结果一致");
    Ok(())
}

/// repair 的 hitsMax 钳制与非整百分能耗：缺口 50 时 effect 钳到 50、
/// cost=ceil(50/100)=1；这正是 combat.rs 中 missing 钳制与 ceil 分支。
#[test]
fn creep_repair_clamps_to_hits_max_with_partial_energy_cost() -> anyhow::Result<()> {
    let mut e = engine(&[("alpha", observe_pkg("builtin:creep.repair"))], false)?;
    let r1 = fighter(&mut e, "r1", 10, 10, &["work"])?;
    let wall = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"constructedWall","owner":"p1",
                "hits":300000000i64 - 50}),
    )?;
    e.run_player(
        "p1",
        &format!("Game.creeps.r1.repair(Game.getObjectById('{wall}'))"),
    )?;
    assert_eq!(hits(&e, &wall), 300000000, "钳到 hitsMax");
    assert_eq!(
        e.get_object(&r1).unwrap()["store"]["energy"],
        29,
        "ceil(50/100)=1"
    );
    let repairs = events(&e, kk_core::constants::EVENT_REPAIR);
    assert_eq!(repairs[0]["data"]["amount"], json!(50));
    assert_eq!(repairs[0]["data"]["energySpent"], json!(1));
    Ok(())
}

/// 宿主放置进度超限工地时保留有符号运算：remaining 为负，build 减少
/// 进度并返还能量，一次完工且能量 30+5=35（不能改成饱和减法的钉子）。
#[test]
fn creep_build_over_progress_site_completes_and_refunds_energy() -> anyhow::Result<()> {
    for (pkg, label, completed) in [
        (observe_pkg("builtin:creep.build"), "控制组", true),
        (
            package(
                "api.handlers.after('builtin:creep.build',{id:'no'},()=>api.decline('peace'));",
            ),
            "拒绝组",
            false,
        ),
    ] {
        let mut e = engine(&[("alpha", pkg)], false)?;
        let b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"road","owner":"p1","progress":305}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
        )?;
        let roads = e
            .find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
            .into_iter()
            .filter(|s| s["structureType"] == "road")
            .count();
        assert_eq!(roads, if completed { 1 } else { 0 }, "{label}");
        assert_eq!(
            e.get_object(&site).is_none(),
            completed,
            "{label}: 超限工地一次完工"
        );
        assert_eq!(
            e.get_object(&b1).unwrap()["store"]["energy"],
            if completed { 35 } else { 30 },
            "{label}: 负进度返还能量"
        );
    }
    Ok(())
}

/// dismantle 改向后拒绝：改向目标不在引擎预计算受影响集内，只能由
/// 运行时首触日志恢复——replace 改向 + after 拒绝，断言原目标、改向
/// 目标、掉落面（容器）与事件全部复原。
#[test]
fn creep_dismantle_redirect_then_decline_restores_via_runtime_journal() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let source = package(&format!(
        r#"
const isTarget=ctx=>ctx.target.fields.name==='d1';
api.handlers.replace('builtin:creep.dismantle',{{id:'reroute'}},ctx=>{{
  if(isTarget(ctx))ctx.build.request('{alt_id}');
  else if(ctx.args.id!==null)ctx.build.request(ctx.args.id);
}});
api.handlers.after('builtin:creep.dismantle',{{id:'no'}},ctx=>{{
  if(isTarget(ctx))return api.decline('peace');
}});
"#
    ));
    let mut e = engine(&[("alpha", source)], false)?;
    let alt = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[9,10],"structureType":"tower","owner":"p1","hits":200}),
    )?;
    assert_eq!(alt, alt_id, "改向目标 ID 预测破裂");
    let d1 = fighter(
        &mut e,
        "d1",
        10,
        10,
        &["work", "work", "work", "work", "work"],
    )?;
    let container = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[10,10],"structureType":"container","owner":"p1","hits":250000,"store":{"energy":10}}),
    )?;
    let tower = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":200}),
    )?;
    e.run_player(
        "p1",
        &format!("Game.creeps.d1.dismantle(Game.getObjectById('{tower}'))"),
    )?;
    assert_eq!(hits(&e, &tower), 200, "原目标无伤");
    assert_eq!(hits(&e, &alt), 200, "改向目标由首触日志复原");
    assert!(
        e.find_in_room("W1N1", kk_core::constants::FIND_RUINS)
            .is_empty(),
        "废墟插入被撤销"
    );
    assert!(
        e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES)
            .is_empty(),
        "掉落堆插入被撤销"
    );
    assert_eq!(
        e.get_object(&container).unwrap()["store"]["energy"],
        10,
        "掉落面容器复原"
    );
    assert_eq!(
        e.get_object(&d1).unwrap()["store"]["energy"],
        30,
        "执行者能量复原"
    );
    assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
    assert_eq!(e.get_game_time(), 2, "整轮照常发布");
    assert!(!e.needs_mod_recovery());
    Ok(())
}
