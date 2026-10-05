//! 受击候选的表示域、能力范围、故障留存与停用后的缓存回退。
use super::incoming_policy::armored;

#[test]
fn raw_incoming_op_preserves_float_validation_and_selected_safe_integer() -> anyhow::Result<()> {
    for (value, valid) in [
        ("0.5", false),
        ("NaN", false),
        ("Infinity", false),
        ("-1", false),
        ("9007199254740992", false),
        ("3", true),
    ] {
        let source=package(&format!("api.handlers.replace('builtin:creep.receiveDamage',{{id:'raw'}},ctx=>{{
            try{{Deno.core.ops.op_mod_chain_incoming_set_damage('1','alpha:handler.raw',{value})}}catch(e){{}}
            if(!{valid})return api.decline('hide');
        }});"));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        let before = e.world_snapshot();
        let result = e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        );
        assert_eq!(result.is_ok(), valid, "{value}: {result:?}");
        if valid {
            assert_eq!(hits(&e, &b), 197);
        } else {
            assert_eq!(e.world_snapshot(), before);
        }
    }
    Ok(())
}

#[test]
fn incoming_target_fields_remain_read_only_on_successful_handler_writeback() -> anyhow::Result<()> {
    for mutation in [
        "ctx.target.fields.hits=0",
        "ctx.target.fields.body[0].hits=0",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.receiveDamage',{{id:'bad'}},ctx=>{{ctx.incoming.setDamage(0);{mutation};}});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
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
use super::*;
use harness::{fighter, hits};

#[test]
fn incoming_invalid_values_and_world_writes_cannot_be_hidden_by_decline() -> anyhow::Result<()> {
    for call in [
        "ctx.incoming.setDamage(-1)",
        "ctx.incoming.setDamage(0.5)",
        "ctx.incoming.setDamage(NaN)",
        "ctx.incoming.setDamage(Infinity)",
        "ctx.incoming.setDamage(Number.MAX_SAFE_INTEGER+1)",
        "ctx.incoming.setDamage('1')",
        "ctx.combat.applyDamage(ctx.target.id,1)",
        "ctx.combat.applyHeal(ctx.target.id,1)",
        "ctx.combat.setRetaliation([])",
        "ctx.movement.request(3)",
        "ctx.resources.transfer(ctx.target.id,ctx.target.id,'energy',0,0)",
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.receiveDamage',{{id:'bad'}},ctx=>{{
            ctx.incoming.setDamage(0);try{{{call}}}catch(e){{}}return api.decline('hide');
        }});"
        ));
        let mut e = engine(&[("bad", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        let before = e.world_snapshot();
        assert!(
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))")
            )
            .is_err(),
            "{call}"
        );
        assert_eq!(e.world_snapshot(), before, "{call}");
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
    }
    Ok(())
}

#[test]
fn incoming_abilities_are_unavailable_in_attack_and_old_tokens_do_not_write_new_calls()
-> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.attack',{id:'wrong'},ctx=>{
        try{ctx.incoming.setDamage(0)}catch(e){}return api.decline('hide');
    });",
    );
    let mut e = engine(&[("bad", source)], false)?;
    fighter(&mut e, "a", 10, 10, &["attack"])?;
    let b = armored(&mut e)?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);

    for same_call in [false, true] {
        let source = if same_call {
            package("let saved;const S='builtin:creep.receiveDamage';
                api.handlers.before(S,{id:'save'},ctx=>{saved=ctx.incoming.setDamage;});
                api.handlers.after(S,{id:'use'},ctx=>{try{saved(0)}catch(e){}return api.decline('hide');});")
        } else {
            package(
                "let saved;api.handlers.before('builtin:creep.receiveDamage',{id:'save'},ctx=>{
                if(!saved){saved=ctx.incoming.setDamage;return;}
                try{saved(0)}catch(e){}return api.decline('expired');
            });",
            )
        };
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        let code = format!("Game.creeps.a.attack(Game.getObjectById('{b}'))");
        if same_call {
            let before = e.world_snapshot();
            assert!(e.run_player("p1", &code).is_err());
            assert_eq!(e.world_snapshot(), before);
        } else {
            e.run_player("p1", &code)?;
            e.run_player("p1", &code)?;
            assert_eq!(
                hits(&e, &b),
                170,
                "旧能力不可把新调用改成免疫；拒绝仍按默认受击"
            );
            assert_eq!(e.get_game_time(), 2);
        }
    }
    Ok(())
}

#[test]
fn incoming_fault_disable_keeps_cached_players_and_healthy_default_damage() -> anyhow::Result<()> {
    let bad = package(
        "api.handlers.replace('builtin:creep.receiveDamage',{id:'fault'},ctx=>{
        ctx.incoming.setDamage(0);ctx.state={dirty:true};throw Error('fault');
    });",
    );
    let healthy = package(
        "api.handlers.after('builtin:creep.receiveDamage',{id:'audit'},ctx=>{
        ctx.state={damage:ctx.incoming.getDamage(),calls:(ctx.state?.calls||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(
            &[("bad", bad.clone()), ("healthy", healthy.clone())],
            binary,
        )?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        let before = e.world_snapshot();
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
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
        assert_eq!(e.engine_mod_state("healthy"), Value::Null);
        assert!(
            e.last_mod_settlement_fault()
                .unwrap()
                .error
                .to_string()
                .contains("fault")
        );
        e.disable_mod("bad")?;
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(hits(&e, &b), 185);
        assert_eq!(
            e.engine_mod_state("healthy"),
            json!({"damage":15,"calls":1})
        );
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}
