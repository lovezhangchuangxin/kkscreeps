//! 战斗槽开放后保留既有世界语义：延迟伤害/死亡、近战反击、结构摧毁
//! 废墟、rampart 重定向、治疗、群体攻击与双玩家一致性。
use super::*;
use harness::{creep_at, events, hits, observe_pkg, predictable_id};

/// 延迟伤害与击杀边界：伤害在对象 tick 施加，致死创建墓碑并转移资源；
/// 拒绝后延迟队列条目恢复，目标存活、无墓碑、无事件残留。
#[test]
fn creep_combat_kill_boundary_restores_on_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.replace('builtin:creep.attack',{id:'no'},ctx=>api.decline('peace'));",
    );
    for (source, dead, label) in [
        (
            observe_pkg("builtin:creep.attack"),
            true,
            "控制组：击杀发生",
        ),
        (reject, false, "拒绝组：击杀被撤销"),
    ] {
        let mut e = engine(&[("alpha", source)], false)?;
        let _a1 = creep_at(
            &mut e,
            "W1N1",
            "a1",
            "p1",
            10,
            10,
            &["attack", "attack", "attack", "attack"],
        )?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
        )?;
        if dead {
            assert!(e.get_object(&e1).is_none(), "{label}: 目标死亡移除");
            let tombs = e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES);
            assert_eq!(tombs.len(), 1, "{label}");
            assert_eq!(
                events(&e, kk_core::constants::EVENT_ATTACK).len(),
                1,
                "{label}"
            );
        } else {
            assert_eq!(hits(&e, &e1), 100, "{label}: 延迟伤害被撤销");
            assert!(
                e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                    .is_empty(),
                "{label}"
            );
            assert!(
                events(&e, kk_core::constants::EVENT_ATTACK).is_empty(),
                "{label}"
            );
            assert_eq!(e.get_game_time(), 2, "{label}: 整轮照常发布");
        }
    }
    Ok(())
}

/// 近战反击与拒绝恢复：武装目标的反击伤害进入攻击者延迟队列；拒绝时
/// 双方队列条目与事件一并恢复，武装目标照常行动。
#[test]
fn creep_combat_attack_back_restores_both_sides_on_decline() -> anyhow::Result<()> {
    let reject =
        package("api.handlers.after('builtin:creep.attack',{id:'no'},()=>api.decline('peace'));");
    for (source, damaged, label) in [
        (
            observe_pkg("builtin:creep.attack"),
            true,
            "控制组：互伤落地",
        ),
        (reject, false, "拒绝组：双方恢复"),
    ] {
        let mut e = engine(&[("alpha", source)], false)?;
        let a1 = creep_at(&mut e, "W1N1", "a1", "p1", 10, 10, &["attack"])?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 11, 10, &["attack"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
        )?;
        let expect = if damaged { 70 } else { 100 };
        assert_eq!(hits(&e, &a1), expect, "{label}: 攻击者（反击面）");
        assert_eq!(hits(&e, &e1), expect, "{label}: 目标");
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK).len(),
            if damaged { 2 } else { 0 },
            "{label}: 主攻击与反击事件"
        );
    }
    Ok(())
}

/// 结构摧毁的局部恢复：摧毁产生废墟（含资源转存）；拒绝时废墟与对象
/// 移除一并撤销、结构按首触记录复原、摧毁事件截除。
#[test]
fn creep_combat_structure_destroy_restores_object_and_ruin_on_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.replace('builtin:creep.attack',{id:'no'},ctx=>api.decline('peace'));",
    );
    for (source, destroyed, label) in [
        (
            observe_pkg("builtin:creep.attack"),
            true,
            "控制组：摧毁与废墟",
        ),
        (reject, false, "拒绝组：结构复原"),
    ] {
        let mut e = engine(&[("alpha", source)], false)?;
        let _a1 = creep_at(&mut e, "W1N1", "a1", "p1", 10, 10, &["attack"])?;
        let tower = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":20,"store":{"energy":5}}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{tower}'))"),
        )?;
        let ruins = e.find_in_room("W1N1", kk_core::constants::FIND_RUINS);
        if destroyed {
            assert!(e.get_object(&tower).is_none(), "{label}: 结构移除");
            assert_eq!(ruins.len(), 1, "{label}");
            assert_eq!(ruins[0]["store"]["energy"], 5, "{label}: 资源转存废墟");
            assert_eq!(
                events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).len(),
                1,
                "{label}"
            );
        } else {
            assert_eq!(hits(&e, &tower), 20, "{label}: 首触记录复原");
            assert!(ruins.is_empty(), "{label}: 链内插入被撤销");
            assert!(
                events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).is_empty(),
                "{label}"
            );
            assert!(
                events(&e, kk_core::constants::EVENT_ATTACK).is_empty(),
                "{label}"
            );
            assert_eq!(e.get_game_time(), 2, "{label}");
        }
    }
    Ok(())
}

/// 攻击经 rampart 重定向：目标格上的 rampart 承伤；拒绝时 rampart 与
/// 目标的记录及事件全部恢复。
#[test]
fn creep_combat_rampart_redirect_restores_both_on_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.wrap('builtin:creep.attack',{id:'no'},(ctx,next)=>{next();return api.decline('peace');});",
    );
    for (source, damaged, label) in [
        (
            observe_pkg("builtin:creep.attack"),
            true,
            "控制组：rampart 承伤",
        ),
        (reject, false, "拒绝组：双方复原"),
    ] {
        let mut e = engine(&[("alpha", source)], false)?;
        let _a1 = creep_at(&mut e, "W1N1", "a1", "p1", 10, 10, &["attack"])?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 10, 11, &["move"])?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,11],"structureType":"rampart","owner":"p1","hits":1000}),
        )?;
        let rampart_before = hits(&e, &rampart);
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 100, "{label}: 目标本体无伤");
        assert_eq!(
            hits(&e, &rampart),
            if damaged {
                rampart_before - 30
            } else {
                rampart_before
            },
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

/// 近远程治疗：治疗量入延迟队列并限幅于最大生命；拒绝撤销治疗与事件；
/// replace 可改向治疗另一受伤目标。
#[test]
fn creep_heal_slots_apply_deferred_heal_and_restore_on_decline() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let reject_for = |slot: &str| {
        package(&format!(
            "api.handlers.after('{slot}',{{id:'no'}},()=>api.decline('no-heal'));"
        ))
    };
    let redirect = package(&format!(
        "api.handlers.replace('builtin:creep.heal',{{id:'reroute'}},ctx=>{{ctx.combat.request('{alt_id}');}});"
    ));
    for (slot, action, power) in [
        ("builtin:creep.heal", "heal", 12),
        ("builtin:creep.rangedHeal", "rangedHeal", 4),
    ] {
        // 先以控制 tick 预伤害（creep hits 不可放置，须走真实伤害路径）。
        let pre = package(&format!(
            "api.handlers.before('{slot}',{{id:'observe'}},ctx=>{{ctx.state={{seen:1}};}});"
        ));
        let mut e = engine(&[("alpha", pre)], false)?;
        let _h1 = creep_at(&mut e, "W1N1", "h1", "p1", 10, 10, &["heal", "attack"])?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 10, 11, &["move", "move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.h1.attack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 170, "预伤害");
        // 治疗目标与攻击目标同一 creep（heal 允许治疗敌方）。
        e.run_player(
            "p1",
            &format!("Game.creeps.h1.{action}(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 170 + power, "治疗落地");
        let heals = events(&e, kk_core::constants::EVENT_HEAL);
        assert_eq!(heals.len(), 1);
        assert_eq!(heals[0]["data"]["amount"], json!(power));

        // 拒绝：治疗与事件撤销。
        let mut e2engine = engine(&[("beta", reject_for(slot))], false)?;
        let _h1 = creep_at(
            &mut e2engine,
            "W1N1",
            "h1",
            "p1",
            10,
            10,
            &["heal", "attack"],
        )?;
        let t1 = creep_at(&mut e2engine, "W1N1", "e1", "p2", 10, 11, &["move", "move"])?;
        e2engine.run_player(
            "p1",
            &format!("Game.creeps.h1.attack(Game.getObjectById('{t1}'))"),
        )?;
        e2engine.run_player(
            "p1",
            &format!("Game.creeps.h1.{action}(Game.getObjectById('{t1}'))"),
        )?;
        assert_eq!(hits(&e2engine, &t1), 170, "拒绝：治疗被撤销");
        assert!(
            events(&e2engine, kk_core::constants::EVENT_HEAL).is_empty(),
            "拒绝：事件截除"
        );

        // 改向（仅近程治疗覆盖邻格目标）：治疗改投另一受伤 creep。
        if slot == "builtin:creep.heal" {
            let mut e3 = engine(&[("gamma", redirect.clone())], false)?;
            let t2 = creep_at(&mut e3, "W1N1", "e2", "p2", 9, 10, &["move", "move"])?;
            assert_eq!(t2, alt_id, "改向目标 ID 预测破裂");
            let _h1 = creep_at(&mut e3, "W1N1", "h1", "p1", 10, 10, &["heal", "attack"])?;
            let t1 = creep_at(&mut e3, "W1N1", "e1", "p2", 10, 11, &["move", "move"])?;
            e3.run_player(
                "p1",
                &format!("Game.creeps.h1.attack(Game.getObjectById('{t1}'))"),
            )?;
            e3.run_player(
                "p1",
                &format!("Game.creeps.h1.attack(Game.getObjectById('{t2}'))"),
            )?;
            assert_eq!(hits(&e3, &t1), 170, "第一击预伤害");
            assert_eq!(hits(&e3, &t2), 170, "第二击预伤害");
            e3.run_player(
                "p1",
                &format!("Game.creeps.h1.heal(Game.getObjectById('{t1}'))"),
            )?;
            assert_eq!(hits(&e3, &t1), 170, "原目标未获治疗");
            assert_eq!(hits(&e3, &t2), 170 + 12, "改向目标获得治疗");
        }
    }
    Ok(())
}

/// 群体攻击：距离倍率 1/0.1 保持、己方免伤；拒绝全部恢复；wrap 内
/// cancel 抑制 native 中心；mass 槽没有目标改向能力（故障）。
#[test]
fn creep_ranged_mass_attack_rates_cancel_and_decline() -> anyhow::Result<()> {
    let reject = package(
        "api.handlers.after('builtin:creep.rangedMassAttack',{id:'no'},()=>api.decline('peace'));",
    );
    let cancel = package(
        "api.handlers.wrap('builtin:creep.rangedMassAttack',{id:'hold'},(ctx,next)=>{ctx.combat.cancel();next();ctx.state={held:true};});",
    );
    let fault = package(
        "api.handlers.replace('builtin:creep.rangedMassAttack',{id:'bad'},ctx=>{try{ctx.combat.request(ctx.target.id)}catch(e){}return api.decline('peace');});",
    );
    for (source, outcome, label) in [
        (
            observe_pkg("builtin:creep.attack"),
            "hit",
            "控制组：倍率落地",
        ),
        (reject, "decline", "拒绝组：全部恢复"),
        (cancel, "cancel", "取消组：native 抑制"),
    ] {
        let mut e = engine(&[("alpha", source)], false)?;
        let _m1 = creep_at(
            &mut e,
            "W1N1",
            "m1",
            "p1",
            10,
            10,
            &["ranged_attack", "ranged_attack"],
        )?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 11, 10, &["move"])?;
        let e2 = creep_at(&mut e, "W1N1", "e2", "p2", 13, 10, &["move"])?;
        let own = creep_at(&mut e, "W1N1", "own", "p1", 10, 11, &["move"])?;
        e.run_player("p1", "Game.creeps.m1.rangedMassAttack()")?;
        match outcome {
            "hit" => {
                assert_eq!(hits(&e, &e1), 80, "{label}: 距离 1 全额 20");
                assert_eq!(hits(&e, &e2), 98, "{label}: 距离 3 倍率 0.1 取整 2");
            }
            _ => {
                assert_eq!(hits(&e, &e1), 100, "{label}");
                assert_eq!(hits(&e, &e2), 100, "{label}");
                assert!(
                    events(&e, kk_core::constants::EVENT_ATTACK).is_empty(),
                    "{label}"
                );
            }
        }
        assert_eq!(hits(&e, &own), 100, "{label}: 己方免伤");
        if outcome == "cancel" {
            assert_eq!(e.engine_mod_state("alpha"), json!({"held":true}));
            assert_eq!(e.get_game_time(), 2, "{label}: 取消仍成功提交");
        }
    }
    // mass 槽目标改向为故障，不能被 decline 掩盖。
    let mut e = engine(&[("alpha", fault)], false)?;
    let _m1 = creep_at(&mut e, "W1N1", "m1", "p1", 10, 10, &["ranged_attack"])?;
    let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 11, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(
        e.run_player("p1", "Game.creeps.m1.rangedMassAttack()")
            .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    assert_eq!(hits(&e, &e1), 100);
    Ok(())
}

/// 双玩家双房间：拒绝只作用于门控执行者；串行与并发派发结果一致。
#[test]
fn creep_combat_dual_player_rooms_and_dispatch_modes_agree() -> anyhow::Result<()> {
    let shard = json!({
        "players": ["p1", "p2"],
        "rooms": [
            { "name": "W1N1", "rcl": 6, "owner": "p1" },
            { "name": "W0N1", "rcl": 4, "owner": "p2" },
        ]
    });
    let source = package(
        r#"
api.handlers.before('builtin:creep.attack',{id:'gate'},ctx=>{
  if(ctx.target.fields.owner==='p2')return api.decline('not-you');
  ctx.state={hits:(ctx.state?.hits||0)+1};
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
        let _a = creep_at(&mut e, "W1N1", "a", "p1", 10, 10, &["attack"])?;
        let x = creep_at(&mut e, "W1N1", "x", "p2", 10, 11, &["move"])?;
        let _c = creep_at(&mut e, "W0N1", "c", "p2", 20, 20, &["attack"])?;
        let y = creep_at(&mut e, "W0N1", "y", "p1", 20, 21, &["move"])?;
        // 门控拒绝 p2 执行者：p1 攻击落地、p2 攻击被拒。
        let mut codes = std::collections::BTreeMap::new();
        codes.insert(
            "p1".to_string(),
            format!("Memory.p=1;Game.creeps.a.attack(Game.getObjectById('{x}'))"),
        );
        codes.insert(
            "p2".to_string(),
            format!("Memory.p=2;Game.creeps.c.attack(Game.getObjectById('{y}'))"),
        );
        e.set_player_concurrency(concurrency)?;
        let runs = e.run_players(&codes)?;
        assert_eq!(runs["p1"].expr["value"], json!(0));
        assert_eq!(runs["p2"].expr["value"], json!(0));
        assert_eq!(hits(&e, &x), 70, "p1 攻击保留");
        assert_eq!(hits(&e, &y), 100, "p2 攻击被拒绝");
        assert_eq!(
            e.player_state("p1").unwrap().raw_memory,
            "{\"p\":1}",
            "两名玩家整轮照常发布"
        );
        assert_eq!(e.player_state("p2").unwrap().raw_memory, "{\"p\":2}");
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"hits":1}),
            "仅未被拒的 p1 执行者累计"
        );
        snapshots.push(e.world_snapshot());
    }
    assert_eq!(snapshots[0], snapshots[1], "串行与并发派发结果一致");
    Ok(())
}

/// 远程攻击槽的 Hosted 链拒绝与改向：距离 2 的原生射程保持；拒绝撤销
/// 延迟伤害与事件；replace 改向另一射程内目标（RangedAttack 能力臂）。
#[test]
fn creep_ranged_attack_hosted_decline_and_redirect() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let reject = package(
        "api.handlers.wrap('builtin:creep.rangedAttack',{id:'no'},(ctx,next)=>{next();return api.decline('peace');});",
    );
    let redirect = package(&format!(
        "api.handlers.replace('builtin:creep.rangedAttack',{{id:'reroute'}},ctx=>{{ctx.combat.request('{alt_id}');}});"
    ));
    // 拒绝：距离 2 的远程伤害被撤销。
    {
        let mut e = engine(&[("alpha", reject)], false)?;
        let _r1 = creep_at(&mut e, "W1N1", "r1", "p1", 10, 10, &["ranged_attack"])?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 12, 10, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.r1.rangedAttack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 100, "拒绝：延迟伤害撤销");
        assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
    }
    // 改向：原目标无伤，射程 3 内的另一目标承伤 10。
    {
        let mut e = engine(&[("alpha", redirect)], false)?;
        let e3 = creep_at(&mut e, "W1N1", "e3", "p2", 10, 13, &["move"])?;
        assert_eq!(e3, alt_id, "改向目标 ID 预测破裂");
        let _r1 = creep_at(&mut e, "W1N1", "r1", "p1", 10, 10, &["ranged_attack"])?;
        let e1 = creep_at(&mut e, "W1N1", "e1", "p2", 12, 10, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.r1.rangedAttack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 100, "改向：原目标无伤");
        assert_eq!(hits(&e, &e3), 90, "改向：新目标承伤");
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 1);
        assert_eq!(attacks[0]["data"]["targetId"], json!(e3));
    }
    Ok(())
}
