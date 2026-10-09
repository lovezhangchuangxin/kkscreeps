//! 独立反击表可组合 native/request/JS 伤害；反击只登记一次且可恢复。
use super::*;
use harness::{enemy, events, fighter, hits, predictable_id};

#[test]
fn explicit_melee_retaliation_replaces_native_safe_mode_suppression() -> anyhow::Result<()> {
    for explicit in [false, true] {
        let statement = if explicit {
            "ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]);"
        } else {
            ""
        };
        let source = package(&format!(
            "api.handlers.before('builtin:creep.attack',{{id:'policy'}},ctx=>{{{statement}}});"
        ));
        let mut e = engine_with_shard(
            &[("alpha", source)],
            false,
            &[],
            RegisterOptions::default(),
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6,"safeMode":100}]}),
        )?;
        let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack", "move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(hits(&e, &a), if explicit { 193 } else { 200 });
        assert_eq!(hits(&e, &b), 170);
    }
    Ok(())
}

#[test]
fn native_ranged_and_mass_attacks_can_counter_with_the_attacker_on_a_rampart() -> anyhow::Result<()>
{
    for mass in [false, true] {
        let slot = if mass {
            "rangedMassAttack"
        } else {
            "rangedAttack"
        };
        let source = package(&format!(
            "api.handlers.before('builtin:creep.{slot}',{{id:'policy'}},ctx=>ctx.combat.setRetaliation([{{id:'kk3',amount:7}},{{id:'kk4',amount:11}}]));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["ranged_attack", "move"])?;
        let b = enemy(&mut e, "b", 11, 10, &["move"])?;
        let c = enemy(&mut e, "c", 12, 10, &["move"])?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,10],"structureType":"rampart","owner":"p1","hits":1000}),
        )?;
        let call = if mass {
            "rangedMassAttack()".to_string()
        } else {
            format!("rangedAttack(Game.getObjectById('{b}'))")
        };
        e.run_player("p1", &format!("Game.creeps.a.{call}"))?;
        assert_eq!(hits(&e, &a), if mass { 182 } else { 193 });
        assert_eq!(hits(&e, &b), 90);
        assert_eq!(hits(&e, &c), if mass { 96 } else { 100 });
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK).len(),
            if mass { 4 } else { 2 }
        );
    }
    Ok(())
}

#[test]
fn native_melee_retaliation_can_be_disabled_or_replaced_without_changing_primary_damage()
-> anyhow::Result<()> {
    for binary in [false, true] {
        for (statement, back) in [
            ("", 30),
            ("ctx.combat.setRetaliation([]);", 0),
            ("ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]);", 7),
        ] {
            let source = package(&format!(
                "api.handlers.before('builtin:creep.attack',{{id:'policy'}},ctx=>{{{statement}}});"
            ));
            let mut e = engine(&[("alpha", source)], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
            let b = enemy(&mut e, "b", 11, 10, &["attack", "move"])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &a), 200 - back);
            assert_eq!(hits(&e, &b), 170);
            let ev = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(ev.len(), if back > 0 { 2 } else { 1 });
            if back > 0 {
                assert_eq!(ev[0]["objectId"], b);
                assert_eq!(ev[0]["data"]["targetId"], a);
                assert_eq!(ev[0]["data"]["damage"], back);
                assert_eq!(
                    ev[0]["data"]["attackType"],
                    kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
                );
            }
        }
    }
    Ok(())
}

#[test]
fn retaliation_uses_actual_rampart_target_and_structures_can_counter_when_destroyed()
-> anyhow::Result<()> {
    for counter_rampart in [false, true] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.attack',{{id:'policy'}},ctx=>ctx.combat.setRetaliation([{{id:'kk{}',amount:7}}]));",
            if counter_rampart { 4 } else { 3 }
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack"])?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[11,10],"structureType":"rampart","owner":"p2","hits":5}),
        )?;
        assert_eq!(rampart, predictable_id(4));
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert!(e.get_object(&rampart).is_none());
        assert_eq!(hits(&e, &b), 100);
        assert_eq!(hits(&e, &a), if counter_rampart { 193 } else { 200 });
        let ev = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(ev.len(), if counter_rampart { 2 } else { 1 });
        if counter_rampart {
            assert_eq!(ev[0]["objectId"], rampart);
        }
    }
    Ok(())
}

#[test]
fn custom_single_and_mass_damage_accept_retaliation_without_recursive_attacks() -> anyhow::Result<()>
{
    for binary in [false, true] {
        for (slot, call, mass) in [
            ("attack", "attack(target)", false),
            ("rangedAttack", "rangedAttack(target)", false),
            ("rangedMassAttack", "rangedMassAttack()", true),
        ] {
            let apply = if mass {
                "ctx.combat.applyDamageBatch([{id:'kk3',amount:13},{id:'kk4',amount:13}]);"
            } else {
                "ctx.combat.applyDamage('kk3',13);"
            };
            let source = package(&format!(
                "api.handlers.replace('builtin:creep.{slot}',{{id:'policy'}},ctx=>{{ctx.combat.setRetaliation([{{id:'kk3',amount:7}},{{id:'kk4',amount:11}}]);{apply}}});"
            ));
            let mut e = engine(&[("alpha", source)], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &["attack", "ranged_attack", "move"])?;
            let b = enemy(&mut e, "b", 11, 10, &["attack"])?;
            let c = enemy(&mut e, "c", 30, 30, &["attack"])?;
            e.run_player(
                "p1",
                &format!("const target=Game.getObjectById('{b}');Game.creeps.a.{call}"),
            )?;
            assert_eq!(hits(&e, &a), if mass { 282 } else { 293 });
            assert_eq!(hits(&e, &b), 87);
            assert_eq!(hits(&e, &c), if mass { 87 } else { 100 });
            let ev = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(ev.len(), if mass { 4 } else { 2 });
            assert_eq!(ev[0]["objectId"], b);
            assert_eq!(ev[1]["objectId"], a);
            if mass {
                assert_eq!(ev[2]["objectId"], c);
            }
        }
    }
    Ok(())
}

#[test]
fn native_request_redirect_and_zero_damage_use_the_declared_table() -> anyhow::Result<()> {
    for zero in [false, true] {
        let apply = if zero {
            "ctx.combat.applyDamage('kk4',0)"
        } else {
            "ctx.combat.request('kk4')"
        };
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.attack',{{id:'policy'}},ctx=>{{ctx.combat.setRetaliation([{{id:'kk3',amount:11}},{{id:'kk4',amount:7}}]);{apply};}});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack"])?;
        let c = enemy(&mut e, "c", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(hits(&e, &a), 93);
        assert_eq!(hits(&e, &b), 100);
        assert_eq!(hits(&e, &c), if zero { 100 } else { 70 });
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 2);
    }
    Ok(())
}

#[test]
fn declined_retaliation_restores_prior_actor_queues_structure_events_and_ids() -> anyhow::Result<()>
{
    let source=package("api.handlers.replace('builtin:creep.attack',{id:'policy'},ctx=>{
        if(ctx.target.fields.name==='prior'){ctx.combat.applyDamage('kk4',10);return;}
        ctx.combat.setRetaliation([{id:'kk6',amount:50}]);ctx.combat.applyDamage('kk6',80);ctx.state={dirty:true};
    });api.handlers.replace('builtin:creep.heal',{id:'heal'},ctx=>ctx.combat.applyHeal('kk4',3));
    api.handlers.after('builtin:creep.attack',{id:'deny'},ctx=>{if(ctx.target.fields.name==='a')return api.decline('peace');});");
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "prior", 12, 11, &["attack"])?;
        fighter(&mut e, "healer", 12, 10, &["heal"])?;
        let a = fighter(&mut e, "a", 10, 10, &["attack", "carry", "move"])?;
        assert_eq!(a, predictable_id(4));
        let b = enemy(&mut e, "b", 11, 11, &["move"])?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[30,30],"structureType":"container","hits":20,"store":{"energy":5}}),
        )?;
        let store = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[10,11],"structureType":"container","store":{"energy":20}}),
        )?;
        assert_eq!(container, predictable_id(6));
        // 前两个执行者先提交双队列，再验证本动作拒绝只撤销反击。
        e.run_player("p1",&format!("Game.creeps.prior.attack(Game.getObjectById('{b}'));Game.creeps.healer.heal(Game.creeps.healer);Game.creeps.a.transfer(Game.getObjectById('{store}'),'energy',5);Game.creeps.a.attack(Game.getObjectById('{b}'));Game.creeps.a.move(RIGHT);"))?;
        assert_eq!(hits(&e, &a), 293);
        assert_eq!(hits(&e, &container), 20);
        assert_eq!(e.get_object(&store).unwrap()["store"]["energy"], 25);
        assert_eq!(e.get_object(&a).unwrap()["pos"]["x"], 11);
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_RUINS)
                .is_empty()
        );
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert_eq!(enemy(&mut e, "next", 20, 20, &["move"])?, predictable_id(8));
    }
    Ok(())
}

#[test]
fn lethal_retaliation_writeback_retry_publishes_death_and_state_once() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.attack',{id:'policy'},ctx=>{ctx.combat.setRetaliation([{id:ctx.args.id,amount:200}]);ctx.state={applied:(ctx.state?.applied||0)+1};});",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let a = fighter(&mut e, "a", 10, 10, &["attack", "carry"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack", "move"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;Game.creeps.a.attack(Game.getObjectById('{b}'))"
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        e.run_player("p1", "throw Error('must use cache')")?;
        assert!(e.get_object(&a).is_none());
        assert_eq!(hits(&e, &b), 170);
        assert_eq!(
            e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                .len(),
            1
        );
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 2);
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}

#[test]
fn policy_is_discarded_after_each_chain_and_native_misses_cannot_counter() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.attack',{id:'policy'},ctx=>{if(ctx.tick===1)ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]);});",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
    let b = enemy(&mut e, "b", 11, 10, &["attack", "move"])?;
    let code = format!("Game.creeps.a.attack(Game.getObjectById('{b}'))");
    e.run_player("p1", &code)?;
    assert_eq!(hits(&e, &a), 193);
    e.run_player("p1", &code)?;
    assert_eq!(hits(&e, &a), 163);
    let source = package(
        "api.handlers.before('builtin:creep.rangedAttack',{id:'policy'},ctx=>ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]));",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let a = fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
    let b = enemy(&mut e, "b", 30, 30, &["attack"])?;
    e.run_player(
        "p1",
        &format!("Game.creeps.a.rangedAttack(Game.getObjectById('{b}'))"),
    )?;
    assert_eq!(hits(&e, &a), 100);
    assert_eq!(hits(&e, &b), 100);
    assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
    Ok(())
}
