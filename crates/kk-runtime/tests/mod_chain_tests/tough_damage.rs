//! 真实玩家 VM 下，TOUGH 对原生/JS 主伤害和反击使用同一延迟结算边界。
use super::*;
use harness::{events, fighter, hits};

fn armored(e: &mut EmbedEngine, name: &str, owner: &str, pos: [u8; 2]) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({
            "name":name,"owner":owner,"pos":pos,
            "body":["tough","attack","move"],"boosts":{"0":"GHO2"},"ticksToLive":100
        }),
    )
}

#[test]
fn tough_reduces_native_and_js_primary_and_retaliation_damage() -> anyhow::Result<()> {
    for binary in [false, true] {
        for custom in [false, true] {
            let source = if custom {
                package(
                    "api.handlers.replace('builtin:creep.attack',{id:'formula'},ctx=>{
                    ctx.combat.setRetaliation([{id:ctx.args.id,amount:7}]);
                    ctx.combat.applyDamage(ctx.args.id,45);
                });",
                )
            } else {
                harness::observe_pkg("builtin:creep.attack")
            };
            let mut e = engine(&[("alpha", source)], binary)?;
            let a = armored(&mut e, "a", "p1", [10, 10])?;
            let b = armored(&mut e, "b", "p2", [10, 11])?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(hits(&e, &a), if custom { 297 } else { 285 });
            assert_eq!(hits(&e, &b), if custom { 278 } else { 285 });
            let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(attacks.len(), 2);
            assert_eq!(attacks[0]["data"]["damage"], if custom { 7 } else { 30 });
            assert_eq!(attacks[1]["data"]["damage"], if custom { 45 } else { 30 });
            assert_eq!(
                e.get_object(&b).unwrap()["body"][0]["hits"],
                if custom { 78 } else { 85 }
            );
        }
    }
    Ok(())
}

#[test]
fn js_batch_shares_creep_defense_but_structures_take_full_damage() -> anyhow::Result<()> {
    let source = package("api.handlers.replace('builtin:creep.rangedMassAttack',{id:'formula'},ctx=>{
        ctx.combat.applyDamageBatch(ctx.combat.list().objects.filter(o=>o.owner==='p2').map(o=>({id:o.id,amount:40})));
    });");
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let b = armored(&mut e, "b", "p2", [10, 11])?;
        let wall = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[11,11],"structureType":"rampart","owner":"p2","hits":150
            }),
        )?;
        e.run_player("p1", "Game.creeps.a.rangedMassAttack();")?;
        assert_eq!(hits(&e, &b), 280);
        assert_eq!(hits(&e, &wall), 110);
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 2);
        assert!(attacks.iter().all(|event| event["data"]["damage"] == 40));
    }
    Ok(())
}

#[test]
fn declined_damage_restores_raw_queue_before_aggregate_rounding() -> anyhow::Result<()> {
    for binary in [false, true] {
        for decline in [false, true] {
            let gate = if decline {
                "api.handlers.after('builtin:creep.attack',{id:'deny'},ctx=>{
                    if(ctx.target.fields.name==='a1')return api.decline('peace');
                });"
            } else {
                ""
            };
            let source = package(&format!(
                "api.handlers.replace('builtin:creep.attack',{{id:'formula'}},ctx=>{{
                ctx.combat.applyDamage(ctx.args.id,1);ctx.state={{count:(ctx.state?.count||0)+1}};
            }});{gate}"
            ));
            let mut e = engine(&[("alpha", source)], binary)?;
            fighter(&mut e, "a0", 10, 10, &["attack"])?;
            fighter(&mut e, "a1", 11, 10, &["attack"])?;
            let b = armored(&mut e, "b", "p2", [10, 11])?;
            e.run_player("p1", &format!("const t=Game.getObjectById('{b}');Game.creeps.a0.attack(t);Game.creeps.a1.attack(t);"))?;
            assert_eq!(
                hits(&e, &b),
                if decline { 300 } else { 299 },
                "两次伤害合并再取整"
            );
            assert_eq!(
                events(&e, kk_core::constants::EVENT_ATTACK).len(),
                if decline { 1 } else { 2 }
            );
            assert_eq!(
                e.engine_mod_state("alpha"),
                json!({"count":if decline {1} else {2}})
            );
        }
    }
    Ok(())
}

#[test]
fn late_retry_reapplies_tough_from_committed_body_once() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:creep.attack',{id:'formula'},ctx=>{
        ctx.combat.applyDamage(ctx.args.id,150);ctx.state={count:(ctx.state?.count||0)+1};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = armored(&mut e, "b", "p2", [10, 11])?;
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
        e.run_player("p1", "throw Error('must use cached player results')")?;
        assert_eq!(hits(&e, &b), 225);
        assert_eq!(e.get_object(&b).unwrap()["body"][0]["hits"], 25);
        assert_eq!(e.engine_mod_state("alpha"), json!({"count":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
    }
    Ok(())
}

#[test]
fn tower_uses_the_same_tough_reduction_without_implicit_retaliation() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(&[("alpha", package(""))], binary)?;
        let b = e.place_object(
            "W1N1",
            "creep",
            &json!({
                "name":"b","owner":"p2","pos":[10,11],
                "body":["tough","attack","move","move","move","move"],
                "boosts":{"0":"GHO2"},"ticksToLive":100
            }),
        )?;
        e.place_object("W1N1", "structure", &json!({
            "pos":[10,10],"structureType":"tower","owner":"p1","hits":3000,"store":{"energy":100}
        }))?;
        e.run_player("p1", &format!("Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_TOWER).attack(Game.getObjectById('{b}'));"))?;
        assert_eq!(hits(&e, &b), 100, "600 原始伤害减免 100，目标存活");
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 1);
        assert_eq!(attacks[0]["data"]["damage"], 600);
        assert_eq!(
            e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                .len(),
            0
        );
    }
    Ok(())
}
