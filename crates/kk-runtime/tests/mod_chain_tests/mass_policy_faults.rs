//! 批量 SDK 预检、转换重入、能力门与故障优先。
use super::*;
use harness::{enemy, fighter, hits, predictable_id};

#[test]
fn malformed_batches_duplicates_and_double_consumption_cannot_be_masked() -> anyhow::Result<()> {
    let target = predictable_id(3);
    for call in [
        "ctx.combat.applyDamageBatch(null)",
        "ctx.combat.applyDamageBatch({})",
        "ctx.combat.applyDamageBatch([null])",
        "ctx.combat.applyDamageBatch(new Array(1))",
        "ctx.combat.applyDamageBatch([{id:123,amount:1}])",
        "ctx.combat.applyDamageBatch([{id:'bad-id',amount:1}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:-1}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:0.5}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:NaN}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:Infinity}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:1000000001}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:'1'}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:1n}])",
        "ctx.combat.applyDamageBatch([{id:target,amount:1},{id:'kk3',amount:1}])",
        "ctx.combat.applyDamageBatch(Array.from({length:257},()=>({id:target,amount:0})))",
        "ctx.combat.applyDamageBatch(new Proxy([],{get:(o,k)=>k==='length'?Infinity:Reflect.get(o,k)}))",
        "ctx.combat.applyDamageBatch([{id:target,amount:23}]);ctx.combat.applyDamageBatch([])",
        "ctx.combat.cancel();ctx.combat.applyDamageBatch([])",
        "next();ctx.combat.applyDamageBatch([])",
    ] {
        let source = package(&format!(
            "api.handlers.wrap('builtin:creep.rangedMassAttack',{{id:'bad'}},(ctx,next)=>{{
                const target='{target}';try{{{call}}}catch(e){{}}ctx.state={{dirty:true}};return api.decline('hide');
            }});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        assert_eq!(enemy(&mut e, "b", 11, 10, &["move"])?, target);
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        assert!(
            e.run_player("p1", "Memory.dirty=true;Game.creeps.a.rangedMassAttack()")
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
fn batch_conversion_getters_cannot_bypass_consumption_or_hide_faults() -> anyhow::Result<()> {
    for getter in [
        "ctx.combat.cancel();return 23",
        "ctx.combat.applyDamageBatch([]);return 23",
        "throw Error('getter-failed')",
        "try{ctx.combat.list('',0)}catch(e){}return 23",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.rangedMassAttack',{{id:'get'}},ctx=>{{
                const target=ctx.combat.list().objects.find(o=>o.owner==='p2');
                try{{ctx.combat.applyDamageBatch([{{id:target.id,get amount(){{{getter}}}}}])}}catch(e){{}}
                return api.decline('hide');
            }});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        enemy(&mut e, "b", 11, 10, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.a.rangedMassAttack()")
                .is_err(),
            "{getter}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    Ok(())
}

#[test]
fn batch_is_mass_only_and_rejects_late_non_health_or_foreign_targets() -> anyhow::Result<()> {
    for (slot, method) in [
        ("attack", "attack(target)"),
        ("rangedAttack", "rangedAttack(target)"),
        ("heal", "heal(Game.creeps.a)"),
        ("rangedHeal", "rangedHeal(Game.creeps.a)"),
        ("move", "move(RIGHT)"),
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.{slot}',{{id:'wrong'}},ctx=>ctx.combat.applyDamageBatch([]));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(
            &mut e,
            "a",
            10,
            10,
            &["attack", "ranged_attack", "heal", "move"],
        )?;
        enemy(&mut e, "b", 11, 10, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "const target=Game.getObjectById('kk0000000000000003');Game.creeps.a.{method}"
                )
            )
            .is_err(),
            "{slot}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    for bad in ["kk1", "kkffff", "kk3"] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.rangedMassAttack',{{id:'late'}},ctx=>{{
            const local=ctx.combat.list().objects.find(o=>o.owner==='p2');
            ctx.combat.applyDamageBatch([{{id:local.id,amount:23}},{{id:'{bad}',amount:1}}]);
        }});"
        ));
        let mut e = engine_with_shard(
            &[("alpha", source)],
            false,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6},{"name":"W2N2"}]
            }),
        )?;
        assert_eq!(
            harness::creep_at(&mut e, "W2N2", "foreign", "p2", 10, 10, &["move"])?,
            predictable_id(3)
        );
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        enemy(&mut e, "b", 11, 10, &["move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.a.rangedMassAttack()")
                .is_err(),
            "{bad}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    Ok(())
}

#[test]
fn raw_batch_ops_preserve_fractional_validation_and_conversion_reentrancy() -> anyhow::Result<()> {
    for (batch, succeeds) in [
        ("[{id:'kk3',amount:0.5}]", false),
        ("[{id:'kk3',amount:NaN}]", false),
        ("[{id:'kk3',amount:Infinity}]", false),
        ("[{id:'kk3',amount:1000000001}]", false),
        ("Array.from({length:257},()=>({id:'kk3',amount:0}))", false),
        ("[{id:'kk3',amount:23,unknown:true}]", true),
        (
            "[{id:'kk3',get amount(){ctx.combat.cancel();return 23}}]",
            false,
        ),
        (
            "[{id:'kk3',get amount(){try{ctx.combat.list('',0)}catch(e){}return 23}}]",
            false,
        ),
        (
            "[{id:'kk3',get amount(){const page=ctx.combat.list();if(page.objects.length!==2)throw Error('nested-read');return 23}}]",
            true,
        ),
    ] {
        let source=package(&format!("api.handlers.replace('builtin:creep.rangedMassAttack',{{id:'raw'}},ctx=>{{
            try{{Deno.core.ops.op_mod_chain_combat_apply_damage_batch('1','alpha:handler.raw',{batch})}}catch(e){{}}
            if(!{succeeds})return api.decline('hide');
        }});"));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["move"])?;
        let before = e.world_snapshot();
        let result = e.run_player("p1", "Game.creeps.a.rangedMassAttack()");
        if succeeds {
            result?;
            assert_eq!(hits(&e, &b), 77);
        } else {
            assert!(result.is_err(), "{batch}");
            assert_eq!(e.world_snapshot(), before);
        }
    }
    Ok(())
}

#[test]
fn invalid_combat_pages_and_non_combat_reading_fault_the_chain() -> anyhow::Result<()> {
    for call in [
        "ctx.combat.list('',0)",
        "ctx.combat.list('',257)",
        "ctx.combat.list('',1.5)",
        "ctx.combat.list('',NaN)",
        "ctx.combat.list('',Infinity)",
        "ctx.combat.list('', '2')",
        "ctx.combat.list('bad-id',2)",
        "ctx.combat.list(null,2)",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.rangedMassAttack',{{id:'list'}},ctx=>{{try{{{call}}}catch(e){{}}return api.decline('hide');}});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", "Game.creeps.a.rangedMassAttack()")
                .is_err(),
            "{call}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    let source =
        package("api.handlers.before('builtin:creep.move',{id:'list'},ctx=>ctx.combat.list());");
    let mut e = engine(&[("alpha", source)], false)?;
    fighter(&mut e, "a", 10, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", "Game.creeps.a.move(RIGHT)").is_err());
    assert_eq!(e.world_snapshot(), before);
    Ok(())
}

#[test]
fn saved_batch_capabilities_expire_without_touching_next_invocation() -> anyhow::Result<()> {
    for same_invocation in [true, false] {
        let source = if same_invocation {
            package("let saved;api.handlers.before('builtin:creep.rangedMassAttack',{id:'save'},ctx=>{saved=ctx.combat.applyDamageBatch;});
                api.handlers.after('builtin:creep.rangedMassAttack',{id:'use'},ctx=>{try{saved([])}catch(e){}return api.decline('hide');});")
        } else {
            package(
                "let saved;api.handlers.before('builtin:creep.rangedMassAttack',{id:'save'},ctx=>{
                if(!saved){saved=ctx.combat.applyDamageBatch;return;}
                try{saved([{id:'kk3',amount:23}])}catch(e){}return api.decline('hide');
            });",
            )
        };
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["move"])?;
        if same_invocation {
            let before = e.world_snapshot();
            assert!(
                e.run_player("p1", "Game.creeps.a.rangedMassAttack()")
                    .is_err()
            );
            assert_eq!(e.world_snapshot(), before);
        } else {
            e.run_player("p1", "Game.creeps.a.rangedMassAttack()")?;
            assert_eq!(hits(&e, &b), 90);
            e.run_player("p1", "Game.creeps.a.rangedMassAttack()")?;
            assert_eq!(hits(&e, &b), 90);
        }
    }
    Ok(())
}
