//! 反击表只可在攻击前声明一次；转换重入与捕获不能掩盖能力故障。
use super::*;
use harness::{enemy, fighter, hits};

#[test]
fn invalid_retaliation_tables_and_late_or_duplicate_declarations_cannot_be_masked()
-> anyhow::Result<()> {
    for call in [
        "ctx.combat.setRetaliation(null)",
        "ctx.combat.setRetaliation([null])",
        "ctx.combat.setRetaliation([{id:123,amount:7}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:-1}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:.5}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:NaN}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:Infinity}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:1000000001}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:'7'}])",
        "ctx.combat.setRetaliation([{id:'kk1',amount:7}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:7},{id:'kkffff',amount:1}])",
        "ctx.combat.setRetaliation([{id:'kk3',amount:7},{id:'kk0000000000000003',amount:7}])",
        "ctx.combat.setRetaliation(Array.from({length:257},()=>({id:'kk3',amount:0})))",
        "ctx.combat.setRetaliation([]);ctx.combat.setRetaliation([])",
        "ctx.combat.cancel();ctx.combat.setRetaliation([])",
        "next();ctx.combat.setRetaliation([])",
        "ctx.combat.applyDamage('kk3',7);ctx.combat.setRetaliation([])",
        "ctx.combat.setRetaliation([{id:'kk3',get amount(){ctx.combat.setRetaliation([]);return 7}}])",
        "ctx.combat.setRetaliation([{id:'kk3',get amount(){ctx.combat.cancel();return 7}}])",
        "ctx.combat.setRetaliation([{id:'kk3',get amount(){try{ctx.combat.list('',0)}catch(e){}return 7}}])",
    ] {
        let source = package(&format!(
            "api.handlers.wrap('builtin:creep.attack',{{id:'bad'}},(ctx,next)=>{{try{{{call}}}catch(e){{}}ctx.state={{dirty:true}};return api.decline('hide');}});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.dirty=true;Game.creeps.a.attack(Game.getObjectById('{b}'))")
            )
            .is_err(),
            "{call}"
        );
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
    }
    Ok(())
}

#[test]
fn raw_retaliation_conversion_rechecks_capability_and_does_not_truncate_numbers()
-> anyhow::Result<()> {
    for (batch, ok) in [
        ("[{id:'kk3',amount:.5}]", false),
        ("[{id:'kk3',amount:NaN}]", false),
        ("Array.from({length:257},()=>({id:'kk3',amount:0}))", false),
        (
            "[{id:'kk3',get amount(){ctx.combat.cancel();return 7}}]",
            false,
        ),
        (
            "[{id:'kk3',get amount(){try{ctx.combat.list('',0)}catch(e){}return 7}}]",
            false,
        ),
        (
            "[{id:'kk3',get amount(){if(ctx.combat.list().objects.length!==2)throw Error('read');return 7}}]",
            true,
        ),
    ] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.attack',{{id:'raw'}},ctx=>{{try{{Deno.core.ops.op_mod_chain_combat_set_retaliation('1','alpha:handler.raw',{batch})}}catch(e){{}}if(!{ok})return api.decline('hide');}});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack"])?;
        let before = e.world_snapshot();
        let result = e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        );
        if ok {
            result?;
            assert_eq!(hits(&e, &a), 93);
        } else {
            assert!(result.is_err(), "{batch}");
            assert_eq!(e.world_snapshot(), before);
        }
    }
    Ok(())
}

#[test]
fn retaliation_is_attack_only_and_rejects_cross_room_or_spawning_sources() -> anyhow::Result<()> {
    for (slot, method) in [
        ("heal", "heal(Game.creeps.a)"),
        ("rangedHeal", "rangedHeal(Game.creeps.a)"),
        ("move", "move(RIGHT)"),
    ] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.{slot}',{{id:'wrong'}},ctx=>ctx.combat.setRetaliation([]));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["heal", "move"])?;
        let before = e.world_snapshot();
        assert!(
            e.run_player("p1", &format!("Game.creeps.a.{method}"))
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
    }
    for spawning in [false, true] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.attack',{{id:'wrong'}},ctx=>ctx.combat.setRetaliation([{{id:'kk{}',amount:7}}]));",
            if spawning { 6 } else { 3 }
        ));
        let mut e = engine_with_shard(
            &[("alpha", source)],
            false,
            &[],
            RegisterOptions::default(),
            &json!({"players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6},{"name":"W2N2"}]}),
        )?;
        let source_id = if spawning {
            e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[20,20],"structureType":"spawn","owner":"p1","store":{"energy":300}}),
            )?
        } else {
            harness::creep_at(&mut e, "W2N2", "foreign", "p2", 11, 10, &["attack"])?
        };
        assert_eq!(source_id, harness::predictable_id(3));
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
        if spawning {
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{source_id}').spawnCreep([WORK,WORK],'child')"),
            )?;
            assert_eq!(
                e.get_object(&harness::predictable_id(6)).unwrap()["spawning"],
                true
            );
        }
        let before = e.world_snapshot();
        assert!(
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))")
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
    }
    Ok(())
}

#[test]
fn saved_retaliation_table_setter_expires_at_handler_and_invocation_boundaries()
-> anyhow::Result<()> {
    for same in [false, true] {
        let source = if same {
            package(
                "let saved;api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{saved=ctx.combat.setRetaliation;});api.handlers.after('builtin:creep.attack',{id:'use'},()=>{try{saved([])}catch(e){}return api.decline('hide');});",
            )
        } else {
            package(
                "let saved;api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{if(!saved){saved=ctx.combat.setRetaliation;return;}try{saved([])}catch(e){}return api.decline('hide');});",
            )
        };
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
        let b = enemy(&mut e, "b", 11, 10, &["attack", "move"])?;
        let before = e.world_snapshot();
        let code = format!("Game.creeps.a.attack(Game.getObjectById('{b}'))");
        if same {
            assert!(e.run_player("p1", &code).is_err());
            assert_eq!(e.world_snapshot(), before);
        } else {
            e.run_player("p1", &code)?;
            assert_eq!(hits(&e, &a), 170);
            e.run_player("p1", &code)?;
            assert_eq!(hits(&e, &a), 170);
        }
    }
    Ok(())
}
