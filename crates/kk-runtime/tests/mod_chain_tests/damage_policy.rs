//! JS 策略伤害：数值和默认策略分离、局部拒绝、整轮故障与恢复。
use super::*;
use harness::{enemy, events, fighter, hits, predictable_id};

#[test]
fn combat_views_are_detached_and_support_js_armor_formulas() -> anyhow::Result<()> {
    let scaled = include_str!("../../../../compat/fixtures/mod-damage-policy/scaled/server.js");
    for binary in [false, true] {
        let mut e = engine(&[("scaled-damage", scaled.into())], binary)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["tough", "attack"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(
            hits(&e, &b),
            185,
            "JS 读取目标 TOUGH，伤害为 ceil(100*0.3/2)"
        );
    }
    let source = package("api.handlers.before('builtin:creep.attack',{id:'read'},ctx=>{
        const target=ctx.combat.get(ctx.args.id);
        if(target.typeKey!=='creep'||target.owner!=='p2'||target.hitsMax!==100)throw Error('目标读面');
        target.hits=0;target.body[0].hits=0;
        if(ctx.combat.get(ctx.args.id).body[0].hits!==100)throw Error('副本写回了候选');
        if(ctx.combat.get('kk00000000000000ff')!==null)throw Error('缺失目标');
        if(ctx.combat.get('kk0000000000000001')!==null)throw Error('控制器没有战斗读面');
        ctx.combat.applyDamage(ctx.args.id,20);
        if(ctx.combat.get(ctx.args.id).hits!==100)throw Error('creep 伤害应延迟落地');
    });");
    let mut e = engine(&[("alpha", source)], false)?;
    fighter(&mut e, "a", 10, 10, &["attack"])?;
    let b = enemy(&mut e, "b", 10, 11, &["move"])?;
    e.run_player(
        "p1",
        &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
    )?;
    assert_eq!(hits(&e, &b), 80);
    Ok(())
}

#[test]
fn captured_policy_capability_cannot_write_in_a_later_invocation() -> anyhow::Result<()> {
    let source = package(
        "let saved;
        api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{
            if(!saved){saved=ctx.combat.applyDamage;return;}
            try{saved(ctx.args.id,40)}catch(e){}
            return api.decline('hide');
        });",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    fighter(&mut e, "a", 10, 10, &["attack"])?;
    let b = enemy(&mut e, "b", 10, 11, &["move"])?;
    let code = format!("Game.creeps.a.attack(Game.getObjectById('{b}'))");
    e.run_player("p1", &code)?;
    e.run_player("p1", &code)?;
    assert_eq!(e.get_game_time(), 2);
    assert_eq!(hits(&e, &b), 70, "旧 token 不可访问新调用的候选");
    assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
    Ok(())
}

#[test]
fn ended_handler_capability_faults_the_same_invocation_even_when_caught() -> anyhow::Result<()> {
    let source = package("let saved;
        api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{saved=ctx.combat.applyDamage;});
        api.handlers.after('builtin:creep.attack',{id:'use'},ctx=>{
            try{saved(ctx.args.id,40)}catch(e){}return api.decline('hide');
        });");
    let mut e = engine(&[("alpha", source)], false)?;
    fighter(&mut e, "a", 10, 10, &["attack"])?;
    let b = enemy(&mut e, "b", 10, 11, &["move"])?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    Ok(())
}

#[test]
fn js_formula_sets_damage_without_native_power_or_retaliation() -> anyhow::Result<()> {
    for (slot, method, part) in [
        ("attack", "attack", "attack"),
        ("rangedAttack", "rangedAttack", "ranged_attack"),
    ] {
        let source = package(&format!(
            "api.handlers.before('builtin:creep.{slot}',{{id:'formula'}},ctx=>{{
                if(ctx.target.fields.hitsMax!==100)throw Error('耐久上限读面');
                const active=ctx.target.fields.body.filter(p=>p.type==='{part}'&&p.hits>0).length;
                ctx.combat.applyDamage(ctx.args.id,active*45);
            }});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &[part])?;
            let b = enemy(&mut e, "b", 10, 11, &["attack"])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.{method}(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &b), 55, "{slot}/{binary}: JS 公式生效");
            assert_eq!(hits(&e, &a), 100, "策略应用不隐含原生反击");
            let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(attacks.len(), 1, "before 消耗动作，native 不重复施加");
            assert_eq!(attacks[0]["data"]["damage"], 45);
        }
    }
    Ok(())
}

#[test]
fn js_policy_can_choose_far_target_and_bypass_rampart_redirection() -> anyhow::Result<()> {
    let alt = predictable_id(2);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'policy'}},ctx=>ctx.combat.applyDamage('{alt}',40));"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let far = enemy(&mut e, "far", 30, 30, &["attack"])?;
        assert_eq!(far, alt);
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[30,30],"structureType":"rampart","owner":"p1","hits":1000
            }),
        )?;
        let a = fighter(&mut e, "a", 10, 10, &["attack"])?;
        let near = enemy(&mut e, "near", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{near}'))"),
        )?;
        assert_eq!(hits(&e, &far), 60, "距离不是伤害表示不变量");
        assert_eq!(hits(&e, &rampart), 1000, "策略已明确指定承伤对象");
        assert_eq!(hits(&e, &near), 100);
        assert_eq!(hits(&e, &a), 100);
    }
    Ok(())
}

#[test]
fn decline_restores_policy_target_queue_but_keeps_prior_action() -> anyhow::Result<()> {
    let alt = predictable_id(2);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'policy'}},ctx=>{{
            ctx.combat.applyDamage('{alt}',ctx.target.fields.name==='a0'?7:80);
            ctx.state={{count:(ctx.state?.count||0)+1}};
        }});
        api.handlers.after('builtin:creep.attack',{{id:'deny'}},ctx=>{{
            if(ctx.target.fields.name==='a1')return api.decline('peace');
        }});"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let far = enemy(&mut e, "far", 30, 30, &["move"])?;
        assert_eq!(far, alt);
        fighter(&mut e, "a0", 10, 10, &["attack"])?;
        fighter(&mut e, "a1", 11, 10, &["attack"])?;
        let near = enemy(&mut e, "near", 10, 11, &["move"])?;
        e.run_player("p1", &format!(
            "const target=Game.getObjectById('{near}');Game.creeps.a0.attack(target);Game.creeps.a1.attack(target);"
        ))?;
        assert_eq!(hits(&e, &far), 93, "首触日志恢复此前动作的待伤害");
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
        assert_eq!(e.engine_mod_state("alpha"), json!({"count":1}));
        assert_eq!(e.get_game_time(), 1);
    }
    Ok(())
}

#[test]
fn declined_policy_destruction_restores_structure_inventory_indexes_and_id() -> anyhow::Result<()> {
    let alt = predictable_id(2);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'policy'}},ctx=>ctx.combat.applyDamage('{alt}',80));
        api.handlers.after('builtin:creep.attack',{{id:'deny'}},()=>api.decline('peace'));"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let target = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[30,30],"structureType":"container","hits":20,"store":{"energy":5}
            }),
        )?;
        assert_eq!(target, alt);
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let near = enemy(&mut e, "near", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{near}'))"),
        )?;
        assert_eq!(hits(&e, &target), 20);
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 5);
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_RUINS)
                .is_empty()
        );
        assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
        assert!(events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).is_empty());
        e.run_player(
            "p1",
            "Memory.ids=Game.rooms.W1N1.lookForAt(LOOK_STRUCTURES,30,30).map(o=>o.id);",
        )?;
        let memory: Value = serde_json::from_str(&e.player_state("p1").unwrap().raw_memory)?;
        assert_eq!(memory["ids"], json!([target]));
        let next = enemy(&mut e, "next", 20, 20, &["move"])?;
        assert_eq!(next, predictable_id(5), "拒绝不能消耗废墟的对象 ID");
    }
    Ok(())
}

#[test]
fn invalid_damage_or_repeated_execution_cannot_be_hidden_by_decline() -> anyhow::Result<()> {
    for call in [
        "ctx.combat.applyDamage(ctx.args.id,-1)",
        "ctx.combat.applyDamage(ctx.args.id,0.5)",
        "ctx.combat.applyDamage(ctx.args.id,NaN)",
        "ctx.combat.applyDamage(ctx.args.id,Infinity)",
        "ctx.combat.applyDamage(ctx.args.id,1000000001)",
        "ctx.combat.applyDamage(ctx.args.id,'40')",
        "ctx.combat.applyDamage('not-an-id',40)",
        "ctx.combat.applyDamage('kk00000000000000ff',40)",
        "ctx.combat.applyDamage(ctx.args.id,40);ctx.combat.applyDamage(ctx.args.id,40)",
        "ctx.combat.applyDamage(ctx.args.id,40);ctx.combat.request(ctx.args.id)",
        "ctx.combat.cancel();ctx.combat.applyDamage(ctx.args.id,40)",
        "next();ctx.combat.applyDamage(ctx.args.id,40)",
    ] {
        let source = package(&format!(
            "api.handlers.wrap('builtin:creep.attack',{{id:'policy'}},(ctx,next)=>{{
                try{{{call}}}catch(e){{}}ctx.state={{dirty:true}};return api.decline('hide');
            }});"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
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
        assert_eq!(e.player_state("p1"), Some(&player), "{call}");
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert!(e.needs_mod_recovery());
    }
    Ok(())
}

#[test]
fn policy_damage_is_scoped_to_attack_slots_and_same_room() -> anyhow::Result<()> {
    for (slot, code) in [
        ("heal", "Game.creeps.a.heal(target)"),
        ("rangedMassAttack", "Game.creeps.a.rangedMassAttack()"),
        ("move", "Game.creeps.a.move(RIGHT)"),
    ] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.{slot}',{{id:'bad'}},ctx=>ctx.combat.applyDamage(ctx.target.id,40));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        let a = fighter(
            &mut e,
            "a",
            10,
            10,
            &["attack", "heal", "ranged_attack", "move"],
        )?;
        let before = e.world_snapshot();
        assert!(
            e.run_player(
                "p1",
                &format!("const target=Game.getObjectById('{a}');{code}")
            )
            .is_err(),
            "{slot}"
        );
        assert_eq!(e.world_snapshot(), before);
    }
    let foreign = predictable_id(3);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'foreign'}},ctx=>ctx.combat.applyDamage('{foreign}',40));"
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
    let placed = harness::creep_at(&mut e, "W2N2", "foreign", "p2", 10, 10, &["move"])?;
    assert_eq!(placed, foreign);
    fighter(&mut e, "a", 10, 10, &["attack"])?;
    let near = enemy(&mut e, "near", 10, 11, &["move"])?;
    let before = e.world_snapshot();
    let error = e
        .run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{near}'))"),
        )
        .unwrap_err();
    assert!(error.to_string().contains("不存在于本房"), "{error}");
    assert_eq!(e.world_snapshot(), before);
    Ok(())
}

#[test]
fn damage_bounds_and_late_writeback_retry_preserve_exactly_once_effects() -> anyhow::Result<()> {
    for (amount, alive) in [(0, true), (kk_mod_api::combat::MAX_DAMAGE, false)] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.attack',{{id:'policy'}},ctx=>ctx.combat.applyDamage(ctx.args.id,{amount}));"
        ));
        let mut e = engine(&[("alpha", source)], false)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(e.get_object(&b).is_some(), alive);
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK)[0]["data"]["damage"],
            amount
        );
    }
    let source = package(
        "api.handlers.replace('builtin:creep.attack',{id:'policy'},ctx=>{ctx.combat.applyDamage(ctx.args.id,75);ctx.state={hits:(ctx.state?.hits||0)+1};});",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = enemy(&mut e, "b", 10, 11, &["move"])?;
        let before = e.world_snapshot();
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
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        e.run_player("p1", "throw Error('cached retry only')")?;
        assert_eq!(hits(&e, &b), 25);
        assert_eq!(e.engine_mod_state("alpha"), json!({"hits":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
    }
    Ok(())
}
