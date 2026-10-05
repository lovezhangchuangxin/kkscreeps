//! JS 治疗策略的功率、延迟应用、局部恢复与故障/缓存重试边界。
use super::*;
use harness::{creep_at, enemy, events, fighter, hits, predictable_id};

#[test]
fn js_heal_formula_consumes_before_native_and_keeps_deferred_views() -> anyhow::Result<()> {
    for (slot, heal_type) in [
        ("heal", kk_core::constants::EVENT_HEAL_TYPE_MELEE),
        ("rangedHeal", kk_core::constants::EVENT_HEAL_TYPE_RANGED),
    ] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.{slot}',{{id:'formula'}},ctx=>{{
                if(ctx.target.fields.body[0].type!=='heal')throw Error('执行者部件');
                const target=ctx.combat.get(ctx.args.id);
                if(target.hits!==170||target.hitsMax!==200)throw Error('治疗前读面');
                ctx.combat.applyHeal(target.id,23);
                if(ctx.combat.get(target.id).hits!==170)throw Error('治疗须延迟');
                ctx.state={{applied:1}};
            }});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &["heal", "attack"])?;
            let b = enemy(&mut e, "b", 10, 11, &["move", "move"])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.{slot}(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &b), 193, "{slot}/{binary}: 不叠加 native 12/4");
            let heals = events(&e, kk_core::constants::EVENT_HEAL);
            assert_eq!(heals.len(), 1);
            assert_eq!(heals[0]["objectId"], a);
            assert_eq!(heals[0]["data"]["amount"], 23);
            assert_eq!(heals[0]["data"]["healType"], heal_type);
            assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        }
    }
    Ok(())
}

#[test]
fn healing_restores_body_and_clamps_final_hits_including_self_heal() -> anyhow::Result<()> {
    for (amount, expected) in [(0, 80), (45, 125), (kk_mod_api::combat::MAX_HEAL, 200)] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.heal',{{id:'formula'}},ctx=>ctx.combat.applyHeal(ctx.args.id,{amount}));"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            fighter(
                &mut e,
                "a",
                10,
                10,
                &["heal", "attack", "attack", "attack", "attack"],
            )?;
            let b = enemy(&mut e, "b", 10, 11, &["move", "move"])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.heal(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &b), expected);
            let target = e.get_object(&b).unwrap();
            assert_eq!(target["body"][0]["hits"], (expected - 100).max(0));
            assert_eq!(target["body"][1]["hits"], expected.min(100));
            assert_eq!(
                events(&e, kk_core::constants::EVENT_HEAL)[0]["data"]["amount"],
                amount
            );
        }
    }
    let source = package(
        "api.handlers.replace('builtin:creep.heal',{id:'self'},ctx=>ctx.combat.applyHeal(ctx.target.id,23));",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let a = fighter(&mut e, "a", 10, 10, &["heal", "attack"])?;
    let b = enemy(&mut e, "b", 10, 11, &["attack"])?;
    e.run_player(
        "p1",
        &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
    )?;
    assert_eq!(hits(&e, &a), 170, "原生反击预伤害");
    e.run_player("p1", "Game.creeps.a.heal(Game.creeps.a)")?;
    assert_eq!(hits(&e, &a), 193);
    Ok(())
}

#[test]
fn full_health_healing_offsets_same_tick_damage_before_death() -> anyhow::Result<()> {
    for (heal, alive) in [(70, true), (0, false)] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.heal',{{id:'heal'}},ctx=>ctx.combat.applyHeal(ctx.args.id,{heal}));
             api.handlers.replace('builtin:creep.attack',{{id:'hit'}},ctx=>ctx.combat.applyDamage(ctx.args.id,130));"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            fighter(&mut e, "a", 10, 10, &["heal"])?;
            fighter(&mut e, "striker", 11, 10, &["attack"])?;
            let b = enemy(&mut e, "b", 10, 11, &["move"])?;
            e.run_player("p1", &format!(
                "const b=Game.getObjectById('{b}');Game.creeps.a.heal(b);Game.creeps.striker.attack(b);"
            ))?;
            assert_eq!(e.get_object(&b).is_some(), alive);
            if alive {
                assert_eq!(hits(&e, &b), 40, "不能在登记时按满血截断治疗");
            }
            assert_eq!(
                e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                    .len(),
                usize::from(!alive)
            );
            assert_eq!(events(&e, kk_core::constants::EVENT_HEAL).len(), 1);
        }
    }
    Ok(())
}

#[test]
fn declined_healing_restores_far_target_but_preserves_prior_queues_and_actions()
-> anyhow::Result<()> {
    let alt = predictable_id(2);
    let formula = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'hit'}},ctx=>ctx.combat.applyDamage('{alt}',40));
         api.handlers.replace('builtin:creep.heal',{{id:'heal'}},ctx=>{{
            ctx.combat.applyHeal('{alt}',ctx.target.fields.name==='h0'?7:80);
            ctx.state={{count:(ctx.state?.count||0)+1}};
         }});"
    ));
    let gate = package(
        "api.handlers.after('builtin:creep.heal',{id:'deny'},ctx=>{
        if(ctx.target.fields.name==='h1'){ctx.state={dirty:true};return api.decline('peace');}
    });",
    );
    for binary in [false, true] {
        let mut e = engine(
            &[("alpha", formula.clone()), ("beta", gate.clone())],
            binary,
        )?;
        let far = enemy(&mut e, "far", 30, 30, &["move"])?;
        assert_eq!(far, alt);
        fighter(&mut e, "a0", 10, 10, &["attack"])?;
        fighter(&mut e, "h0", 12, 10, &["heal"])?;
        let h1 = fighter(&mut e, "h1", 14, 10, &["heal", "carry", "move"])?;
        let near = enemy(&mut e, "near", 10, 11, &["move"])?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[14,11],"structureType":"container","store":{"energy":20}
            }),
        )?;
        e.run_player(
            "p1",
            &format!(
                "const o=Game.getObjectById;Game.creeps.a0.attack(o('{near}'));
            Game.creeps.h0.heal(Game.creeps.h0);
            Game.creeps.h1.transfer(o('{container}'),'energy',5);
            Game.creeps.h1.heal(Game.creeps.h1);Game.creeps.h1.move(RIGHT);Memory.ok=true;"
            ),
        )?;
        assert_eq!(
            hits(&e, &far),
            67,
            "只撤销 h1 的 80，保留既有伤害 40/治疗 7"
        );
        assert_eq!(hits(&e, &near), 100);
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 25);
        assert_eq!(e.get_object(&h1).unwrap()["pos"]["x"], 15);
        assert_eq!(events(&e, kk_core::constants::EVENT_HEAL).len(), 1);
        assert_eq!(e.engine_mod_state("alpha"), json!({"count":1}));
        assert_eq!(e.engine_mod_state("beta"), Value::Null);
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"ok\":true}");
        assert!(!e.needs_mod_recovery());
    }
    Ok(())
}

#[test]
fn invalid_healing_or_repeated_consumption_cannot_be_hidden() -> anyhow::Result<()> {
    for call in [
        "ctx.combat.applyHeal(ctx.args.id,-1)",
        "ctx.combat.applyHeal(ctx.args.id,0.5)",
        "ctx.combat.applyHeal(ctx.args.id,NaN)",
        "ctx.combat.applyHeal(ctx.args.id,Infinity)",
        "ctx.combat.applyHeal(ctx.args.id,1000000001)",
        "ctx.combat.applyHeal(ctx.args.id,'23')",
        "ctx.combat.applyHeal(123,23)",
        "ctx.combat.applyHeal('bad-id',23)",
        "ctx.combat.applyHeal('kk00000000000000ff',23)",
        "ctx.combat.applyHeal(ctx.args.id,23);ctx.combat.applyHeal(ctx.args.id,23)",
        "ctx.combat.applyHeal(ctx.args.id,23);ctx.combat.request(ctx.args.id)",
        "ctx.combat.cancel();ctx.combat.applyHeal(ctx.args.id,23)",
        "next();ctx.combat.applyHeal(ctx.args.id,23)",
    ] {
        let source = package(&format!(
            "api.handlers.wrap('builtin:creep.heal',{{id:'policy'}},(ctx,next)=>{{
                try{{{call}}}catch(e){{}}ctx.state={{dirty:true}};return api.decline('hide');
            }});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["heal"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.dirty=true;Game.creeps.a.heal(Game.getObjectById('{a}'))")
            )
            .is_err(),
            "{call}"
        );
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert!(e.needs_mod_recovery());
    }
    Ok(())
}

#[test]
fn healing_rejects_other_slots_structures_and_cross_room_targets() -> anyhow::Result<()> {
    for (slot, code) in [
        ("attack", "Game.creeps.a.attack(target)"),
        ("rangedAttack", "Game.creeps.a.rangedAttack(target)"),
        ("rangedMassAttack", "Game.creeps.a.rangedMassAttack()"),
        ("move", "Game.creeps.a.move(RIGHT)"),
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.{slot}',{{id:'bad'}},ctx=>ctx.combat.applyHeal(ctx.target.id,23));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack", "ranged_attack", "move"])?;
        let target = enemy(&mut e, "b", 10, 11, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player(
                "p1",
                &format!("const target=Game.getObjectById('{target}');{code}")
            )
            .is_err(),
            "{slot}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    let source = package(
        "api.handlers.replace('builtin:creep.heal',{id:'bad'},ctx=>ctx.combat.applyHeal('kk0000000000000003',23));",
    );
    for foreign_room in [false, true] {
        let mut e = engine_with_shard(
            &[("alpha", source.clone())],
            false,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6},{"name":"W2N2"}]
            }),
        )?;
        let foreign = if foreign_room {
            creep_at(&mut e, "W2N2", "foreign", "p2", 10, 10, &["move"])?
        } else {
            e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[30,30],"structureType":"container","hits":20}),
            )?
        };
        assert_eq!(foreign, predictable_id(3));
        fighter(&mut e, "a", 10, 10, &["heal"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.a.heal(Game.creeps.a)")
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
    }
    Ok(())
}

#[test]
fn healing_failure_retries_cached_player_and_mod_state_exactly_once() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:creep.heal',{id:'policy'},ctx=>{
        ctx.combat.applyHeal(ctx.args.id,23);ctx.state={applied:(ctx.state?.applied||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["heal", "attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;Game.creeps.a.heal(Game.getObjectById('{b}'))"
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        e.run_player("p1", "throw Error('cached retry only')")?;
        assert_eq!(hits(&e, &b), 93);
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
        assert_eq!(events(&e, kk_core::constants::EVENT_HEAL).len(), 1);
    }
    Ok(())
}

#[test]
fn spawning_target_can_be_declined_but_cannot_receive_policy_healing() -> anyhow::Result<()> {
    let child_id = predictable_id(4);
    for guarded in [true, false] {
        let body = if guarded {
            "return api.decline('not-born');"
        } else {
            "try{ctx.combat.applyHeal(target.id,23)}catch(e){}return api.decline('hide');"
        };
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.heal',{{id:'birth'}},ctx=>{{
                const target=ctx.combat.get('{child_id}');
                if(!target?.spawning)throw Error('须读到孵化中目标');{body}
            }});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let spawn = e.place_object(
                "W1N1",
                "structure",
                &json!({
                    "pos":[20,20],"structureType":"spawn","owner":"p1","store":{"energy":300}
                }),
            )?;
            fighter(&mut e, "a", 10, 10, &["heal"])?;
            let run = e.run_player(
                "p1",
                &format!("Game.getObjectById('{spawn}').spawnCreep([WORK,WORK],'child')"),
            )?;
            assert_eq!(run.expr["value"], 0);
            assert_eq!(e.get_object(&child_id).unwrap()["spawning"], true);
            let before = e.world_snapshot();
            let result = e.run_player("p1", "Game.creeps.a.heal(Game.creeps.a)");
            if guarded {
                result?;
                assert_eq!(hits(&e, &child_id), 200);
                assert!(events(&e, kk_core::constants::EVENT_HEAL).is_empty());
            } else {
                assert!(result.is_err());
                assert_eq!(e.world_snapshot(), before);
            }
        }
    }
    Ok(())
}

#[test]
fn ended_heal_capability_faults_current_chain_but_old_token_cannot_touch_next() -> anyhow::Result<()>
{
    for same_invocation in [true, false] {
        let source = if same_invocation {
            package("let saved;
                api.handlers.before('builtin:creep.heal',{id:'save'},ctx=>{saved=ctx.combat.applyHeal;});
                api.handlers.after('builtin:creep.heal',{id:'use'},ctx=>{
                    try{saved(ctx.args.id,23)}catch(e){}return api.decline('hide');
                });")
        } else {
            package(
                "let saved;
                api.handlers.before('builtin:creep.heal',{id:'save'},ctx=>{
                    if(!saved){saved=ctx.combat.applyHeal;return;}
                    try{saved(ctx.args.id,23)}catch(e){}return api.decline('hide');
                });",
            )
        };
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["heal", "attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        let code = format!("Game.creeps.a.heal(Game.getObjectById('{b}'))");
        if same_invocation {
            let before = e.world_snapshot();
            assert!(e.run_player("p1", &code).is_err());
            assert_eq!(e.world_snapshot(), before);
        } else {
            e.run_player("p1", &code)?;
            assert_eq!(hits(&e, &b), 82, "第一次仍走原生治疗");
            e.run_player("p1", &code)?;
            assert_eq!(hits(&e, &b), 82, "旧 token 不能访问或写入新候选");
            assert!(events(&e, kk_core::constants::EVENT_HEAL).is_empty());
        }
    }
    Ok(())
}
