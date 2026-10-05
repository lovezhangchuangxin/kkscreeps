//! 受击数值链替换与多包组合，来源/治疗读面及默认回退。
use super::*;
use harness::{events, fighter, hits};

pub(super) fn armored(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({
            "name":"b","owner":"p2","pos":[10,11],"body":["tough","move"],
            "boosts":{"0":"GHO2"},"ticksToLive":100
        }),
    )
}

#[test]
fn native_ranged_mass_tower_js_batch_and_explicit_retaliation_reach_incoming() -> anyhow::Result<()>
{
    for mode in ["ranged", "mass", "tower", "batch", "counter"] {
        let extra = match mode {
            "batch" => {
                "api.handlers.replace('builtin:creep.rangedMassAttack',{id:'batch'},ctx=>ctx.combat.applyDamageBatch(ctx.combat.list().objects.filter(o=>o.owner==='p2').map(o=>({id:o.id,amount:40}))));"
            }
            "counter" => {
                "api.handlers.before('builtin:creep.attack',{id:'counter'},ctx=>ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]));"
            }
            _ => "",
        };
        let source = package(&format!(
            "{extra}
            api.handlers.replace('builtin:creep.receiveDamage',{{id:'receive'}},ctx=>{{
                if(ctx.args.sourceCount!==1||ctx.args.unattributedDamage!==0)throw Error('来源');
                if(ctx.args.sources[0].amount!==ctx.args.rawDamage)throw Error('量');
                if(!ctx.combat.get(ctx.args.sources[0].sourceId))throw Error('来源读面');
                ctx.incoming.setDamage(1);ctx.state={{calls:(ctx.state?.calls||0)+1}};
            }});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &["attack", "ranged_attack"])?;
            let b = armored(&mut e)?;
            let code = match mode {
                "ranged" => format!("Game.creeps.a.rangedAttack(Game.getObjectById('{b}'))"),
                "mass" | "batch" => "Game.creeps.a.rangedMassAttack()".into(),
                "tower" => {
                    let tower=e.place_object("W1N1","structure",&json!({"pos":[11,10],"structureType":"tower","owner":"p1","hits":3000,"store":{"energy":100}}))?;
                    format!("Game.getObjectById('{tower}').attack(Game.getObjectById('{b}'))")
                }
                _ => format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            };
            e.run_player("p1", &code)?;
            assert_eq!(hits(&e, &b), 199, "{mode}");
            assert_eq!(hits(&e, &a), if mode == "counter" { 199 } else { 200 });
            assert_eq!(
                e.engine_mod_state("alpha"),
                json!({"calls":if mode=="counter" {2} else {1}})
            );
            if mode == "counter" {
                assert_eq!(
                    events(&e, kk_core::constants::EVENT_ATTACK)[0]["data"]["attackType"],
                    kk_core::constants::EVENT_ATTACK_TYPE_HIT_BACK
                );
            }
        }
    }
    Ok(())
}

#[test]
fn incoming_can_replace_tough_without_reapplying_native_reduction() -> anyhow::Result<()> {
    let source = package("api.handlers.replace('builtin:creep.receiveDamage',{id:'raw'},ctx=>{
        if(ctx.args.rawDamage!==30||ctx.args.defaultDamage!==15||ctx.incoming.getDamage()!==15)throw Error('输入');
        if(ctx.args.sources.length!==1||ctx.args.sourceCount!==1||ctx.args.sourcesTruncated||ctx.args.unattributedDamage!==0)throw Error('来源');
        const source=ctx.combat.get(ctx.args.sources[0].sourceId);
        if(source.owner!=='p1'||ctx.args.sources[0].amount!==30)throw Error('攻击者');
        ctx.incoming.setDamage(ctx.args.rawDamage);
        ctx.state={calls:(ctx.state?.calls||0)+1};
    });");
    for binary in [false, true] {
        let mut e = engine(&[("raw", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(hits(&e, &b), 170);
        assert_eq!(e.get_object(&b).unwrap()["body"][0]["hits"], 70);
        assert_eq!(e.engine_mod_state("raw"), json!({"calls":1}));
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK)[0]["data"]["damage"],
            30
        );
    }
    Ok(())
}

#[test]
fn separate_armor_and_shield_packages_compose_through_native_and_wrap_next() -> anyhow::Result<()> {
    for replace in [false, true] {
        let armor = if replace {
            "api.handlers.replace('builtin:creep.receiveDamage',{id:'armor'},ctx=>{ctx.incoming.setDamage(Math.ceil(ctx.args.rawDamage/3));ctx.state={calls:1};});"
        } else {
            "api.handlers.before('builtin:creep.receiveDamage',{id:'armor'},ctx=>{ctx.incoming.setDamage(Math.ceil(ctx.incoming.getDamage()/2));ctx.state={calls:1};});"
        };
        let shield=package("api.handlers.wrap('builtin:creep.receiveDamage',{id:'shield'},(ctx,next)=>{
            next();ctx.incoming.setDamage(Math.max(0,ctx.incoming.getDamage()-4));ctx.state={calls:1};
        });");
        for binary in [false, true] {
            let mut e = engine(
                &[("armor", package(armor)), ("shield", shield.clone())],
                binary,
            )?;
            fighter(&mut e, "a", 10, 10, &["attack"])?;
            let b = armored(&mut e)?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &b), if replace { 194 } else { 196 });
            assert_eq!(e.engine_mod_state("armor"), json!({"calls":1}));
            assert_eq!(e.engine_mod_state("shield"), json!({"calls":1}));
        }
    }
    Ok(())
}

#[test]
fn every_decline_position_falls_back_to_default_and_keeps_attacks_heals_and_movement()
-> anyhow::Result<()> {
    for gate in [
        "api.handlers.before(S,{id:'deny'},ctx=>{ctx.incoming.setDamage(0);ctx.state={dirty:true};return api.decline('peace');});",
        "api.handlers.wrap(S,{id:'deny'},(ctx,next)=>{ctx.incoming.setDamage(0);ctx.state={dirty:true};return api.decline('peace');});",
        "api.handlers.replace(S,{id:'deny'},ctx=>{ctx.incoming.setDamage(0);ctx.state={dirty:true};return api.decline('peace');});",
        "api.handlers.wrap(S,{id:'deny'},(ctx,next)=>{next();ctx.incoming.setDamage(0);ctx.state={dirty:true};return api.decline('peace');});",
        "api.handlers.after(S,{id:'deny'},ctx=>{ctx.incoming.setDamage(0);ctx.state={dirty:true};return api.decline('peace');});",
    ] {
        let source=package(&format!("const S='builtin:creep.receiveDamage';
            api.handlers.replace('builtin:creep.attack',{{id:'hit'}},ctx=>{{ctx.combat.applyDamage(ctx.args.id,150);ctx.state={{actions:(ctx.state?.actions||0)+1}};}});
            api.handlers.replace('builtin:creep.heal',{{id:'heal'}},ctx=>{{ctx.combat.applyHeal(ctx.args.id,12);ctx.state={{actions:(ctx.state?.actions||0)+1}};}});
            api.handlers.before(S,{{id:'read'}},ctx=>{{if(ctx.args.pendingHeal!==12||ctx.args.rawDamage!==150||ctx.args.defaultDamage!==75)throw Error('队列');}});
            {gate}"));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let a = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
            fighter(&mut e, "doctor", 11, 11, &["heal"])?;
            let b = armored(&mut e)?;
            e.run_player("p1",&format!("const b=Game.getObjectById('{b}');Game.creeps.a.attack(b);Game.creeps.a.move(RIGHT);Game.creeps.doctor.heal(b);"))?;
            assert_eq!(hits(&e, &b), 137);
            assert_eq!(harness::pos_of(&e, &a), (11, 10));
            assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
            assert_eq!(events(&e, kk_core::constants::EVENT_HEAL).len(), 1);
            assert_eq!(e.engine_mod_state("alpha"), json!({"actions":2}));
        }
    }
    Ok(())
}

#[test]
fn zero_damage_is_immunity_and_large_selected_damage_is_final_before_death() -> anyhow::Result<()> {
    for (amount, alive) in [(0, true), (kk_mod_api::incoming::MAX_DAMAGE, false)] {
        let source = package(&format!(
            "api.handlers.replace('builtin:creep.receiveDamage',{{id:'set'}},ctx=>ctx.incoming.setDamage({amount}));"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            fighter(&mut e, "a", 10, 10, &["attack"])?;
            let b = armored(&mut e)?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(e.get_object(&b).is_some(), alive);
            if alive {
                assert_eq!(hits(&e, &b), 200);
            }
            assert_eq!(
                e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                    .len(),
                usize::from(!alive)
            );
            assert_eq!(
                events(&e, kk_core::constants::EVENT_ATTACK)[0]["data"]["damage"],
                30
            );
        }
    }
    Ok(())
}

#[test]
fn bounded_sources_include_all_counts_and_immunity_applies_once_per_recipient() -> anyhow::Result<()>
{
    let source=package("api.handlers.replace('builtin:creep.receiveDamage',{id:'immune'},ctx=>{
        if(ctx.args.sourceCount!==260||ctx.args.sources.length!==256||!ctx.args.sourcesTruncated||ctx.args.unattributedDamage!==0)throw Error('有界来源');
        if(ctx.args.rawDamage!==7800||ctx.args.defaultDamage!==7700)throw Error('完整聚合');
        ctx.args.sources[0].amount=0;ctx.args.defaultDamage=0;
        if(ctx.incoming.getDamage()!==7700)throw Error('输入副本改写候选');
        ctx.incoming.setDamage(0);ctx.state={calls:(ctx.state?.calls||0)+1};
    });");
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        for n in 0..260 {
            fighter(&mut e, &format!("a{n}"), 10, 10, &["attack"])?;
        }
        let b = armored(&mut e)?;
        e.run_player("p1",&format!("const target=Game.getObjectById('{b}');for(const c of Object.values(Game.creeps))c.attack(target);"))?;
        assert_eq!(hits(&e, &b), 200);
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 260);
        assert_eq!(e.engine_mod_state("alpha"), json!({"calls":1}));
    }
    Ok(())
}

#[test]
fn incoming_shield_state_and_body_commit_once_after_late_retry() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.after('builtin:creep.receiveDamage',{id:'shield'},ctx=>{
        const pool=ctx.state?.pool??20;const absorbed=Math.min(pool,ctx.incoming.getDamage());
        ctx.incoming.setDamage(ctx.incoming.getDamage()-absorbed);
        ctx.state={pool:pool-absorbed,calls:(ctx.state?.calls||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("shield", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e)?;
        let code = format!(
            "Memory.runs=(Memory.runs||0)+1;Game.creeps.a.attack(Game.getObjectById('{b}'))"
        );
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.engine_mod_state("shield"), Value::Null);
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(hits(&e, &b), 200);
        assert_eq!(e.engine_mod_state("shield"), json!({"pool":5,"calls":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
        e.run_player("p1", &code)?;
        assert_eq!(hits(&e, &b), 190);
        assert_eq!(e.engine_mod_state("shield"), json!({"pool":0,"calls":2}));
    }
    Ok(())
}
