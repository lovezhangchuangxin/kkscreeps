//! 群体策略的分页读面、批量恰一次、额外目标与结构增删恢复。
use super::*;
use harness::{enemy, events, fighter, hits, predictable_id};

#[test]
fn paginated_js_mass_formula_selects_far_targets_without_native_double_damage() -> anyhow::Result<()>
{
    let source = package(
        "api.handlers.before('builtin:creep.rangedMassAttack',{id:'formula'},ctx=>{
        const requests=[];let after='',pages=0;
        do{const page=ctx.combat.list(after,2);pages++;
            for(const target of page.objects){
                if(target.typeKey==='controller')throw Error('无耐久对象不可列出');
                if(target.owner==='p2'){
                    requests.push({id:target.id,amount:33});
                    target.body[0].hits=0;target.hits=0;
                    if(ctx.combat.get(target.id).hits!==100)throw Error('副本写回');
                }
            }after=page.next;
        }while(after!==null);
        ctx.combat.applyDamageBatch(requests);
        ctx.state={ids:requests.map(r=>r.id),pages};
    });",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let far = enemy(&mut e, "far", 30, 30, &["attack"])?;
        let a = fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let near = enemy(&mut e, "near", 11, 10, &["move"])?;
        let own = fighter(&mut e, "own", 10, 11, &["move"])?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[30,30],"structureType":"rampart","owner":"p1","hits":1000
            }),
        )?;
        e.run_player("p1", "Game.creeps.a.rangedMassAttack()")?;
        assert_eq!(
            hits(&e, &near),
            67,
            "before 消耗群体动作，native 不再追加 10"
        );
        assert_eq!(
            hits(&e, &far),
            67,
            "应用不限制原生射程，也不因 rampart 跳过"
        );
        assert_eq!(hits(&e, &own), 100, "目标筛选由 JS 决定");
        assert_eq!(hits(&e, &a), 100, "无隐含反击");
        assert_eq!(hits(&e, &rampart), 1000);
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"ids":[far,near],"pages":3})
        );
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 2);
        assert_eq!(attacks[0]["data"]["targetId"], far);
        assert_eq!(attacks[1]["data"]["targetId"], near);
        assert!(attacks.iter().all(|event| event["data"]["damage"] == 33
            && event["data"]["attackType"] == kk_core::constants::EVENT_ATTACK_TYPE_RANGED_MASS));
    }
    Ok(())
}

#[test]
fn declined_batch_restores_extra_targets_and_prior_heal_damage_queues() -> anyhow::Result<()> {
    let alt = predictable_id(2);
    let formula = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'hit'}},ctx=>ctx.combat.applyDamage('{alt}',40));
         api.handlers.replace('builtin:creep.heal',{{id:'heal'}},ctx=>ctx.combat.applyHeal('{alt}',7));
         api.handlers.replace('builtin:creep.rangedMassAttack',{{id:'batch'}},ctx=>{{
            const targets=ctx.combat.list('',256).objects.filter(o=>o.x>=30&&o.hits>0);
            ctx.combat.applyDamageBatch(targets.map(o=>({{id:o.id,amount:80}})));
            ctx.state={{dirty:true}};
         }});"
    ));
    let gate = package(
        "api.handlers.after('builtin:creep.rangedMassAttack',{id:'deny'},ctx=>{ctx.state={dirty:true};return api.decline('peace');});",
    );
    for binary in [false, true] {
        let mut e = engine(
            &[("alpha", formula.clone()), ("beta", gate.clone())],
            binary,
        )?;
        let far = enemy(&mut e, "far", 30, 30, &["move"])?;
        assert_eq!(far, alt);
        let far2 = enemy(&mut e, "far2", 31, 30, &["move"])?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[32,30],"structureType":"container","hits":20,"store":{"energy":5}}),
        )?;
        fighter(&mut e, "a0", 10, 10, &["attack"])?;
        fighter(&mut e, "h0", 12, 10, &["heal"])?;
        let mass = fighter(&mut e, "mass", 14, 10, &["ranged_attack", "carry", "move"])?;
        let near = enemy(&mut e, "near", 10, 11, &["move"])?;
        let store = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[14,11],"structureType":"container","store":{"energy":20}}),
        )?;
        e.run_player("p1", &format!(
            "Game.creeps.a0.attack(Game.getObjectById('{near}'));Game.creeps.h0.heal(Game.creeps.h0);
            Game.creeps.mass.transfer(Game.getObjectById('{store}'),'energy',5);
            Game.creeps.mass.rangedMassAttack();Game.creeps.mass.move(RIGHT);Memory.ok=true;"
        ))?;
        assert_eq!(
            hits(&e, &far),
            67,
            "只撤销批量 80，保留此前伤害 40 和治疗 7"
        );
        assert_eq!(hits(&e, &far2), 100);
        assert_eq!(hits(&e, &container), 20);
        assert_eq!(e.get_object(&container).unwrap()["store"]["energy"], 5);
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_RUINS)
                .is_empty()
        );
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 1);
        assert_eq!(events(&e, kk_core::constants::EVENT_HEAL).len(), 1);
        assert!(events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).is_empty());
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert_eq!(e.engine_mod_state("beta"), Value::Null);
        assert_eq!(e.get_object(&store).unwrap()["store"]["energy"], 25);
        assert_eq!(e.get_object(&mass).unwrap()["pos"]["x"], 15);
        let next = enemy(&mut e, "next", 20, 20, &["move"])?;
        assert_eq!(next, predictable_id(10), "废墟不能消耗 ID");
    }
    Ok(())
}

#[test]
fn declined_mass_destruction_restores_spawn_child_indexes_and_ids() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:creep.rangedMassAttack',{id:'destroy'},ctx=>{
        ctx.combat.applyDamageBatch([{id:'kk3',amount:80},{id:'kk4',amount:80}]);
    });api.handlers.after('builtin:creep.rangedMassAttack',{id:'deny'},()=>api.decline('peace'));",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let spawn = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[30,30],"structureType":"spawn","owner":"p1","hits":20,"store":{"energy":300}
            }),
        )?;
        let container = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "pos":[31,30],"structureType":"container","hits":20,"store":{"energy":5}
            }),
        )?;
        assert_eq!(spawn, predictable_id(3));
        assert_eq!(container, predictable_id(4));
        assert_eq!(
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{spawn}').spawnCreep([WORK,WORK],'child')")
            )?
            .expr["value"],
            0
        );
        let child = predictable_id(5);
        assert_eq!(e.get_object(&child).unwrap()["spawning"], true);
        e.run_player("p1", "Game.creeps.a.rangedMassAttack()")?;
        assert_eq!(hits(&e, &spawn), 20);
        assert_eq!(hits(&e, &container), 20);
        assert_eq!(
            e.get_object(&spawn).unwrap()["store"]["energy"],
            102,
            "正常孵化涓流保留"
        );
        assert_eq!(e.get_object(&child).unwrap()["spawning"], true);
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_RUINS)
                .is_empty()
        );
        assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
        e.run_player("p1", "Memory.ids=Game.rooms.W1N1.lookForAt(LOOK_STRUCTURES,30,30).map(o=>o.id);Memory.child=Game.creeps.child.spawning;")?;
        let memory: Value = serde_json::from_str(&e.player_state("p1").unwrap().raw_memory)?;
        assert_eq!(memory["ids"], json!([spawn]));
        assert_eq!(memory["child"], true);
        assert_eq!(enemy(&mut e, "next", 20, 20, &["move"])?, predictable_id(6));
    }
    Ok(())
}

#[test]
fn mass_writeback_failure_retries_all_targets_and_state_exactly_once() -> anyhow::Result<()> {
    let source = package("api.handlers.replace('builtin:creep.rangedMassAttack',{id:'batch'},ctx=>{
        ctx.combat.applyDamageBatch(ctx.combat.list().objects.filter(o=>o.owner==='p2').map(o=>({id:o.id,amount:23})));
        ctx.state={applied:(ctx.state?.applied||0)+1};
    });");
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["move"])?;
        let c = enemy(&mut e, "c", 30, 30, &["move"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                "Memory.runs=(Memory.runs||0)+1;Game.creeps.a.rangedMassAttack()"
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        e.run_player("p1", "throw Error('cached only')")?;
        assert_eq!(hits(&e, &b), 77);
        assert_eq!(hits(&e, &c), 77);
        assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 2);
        assert_eq!(e.engine_mod_state("alpha"), json!({"applied":1}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"runs\":1}");
    }
    Ok(())
}

#[test]
fn empty_js_batch_consumes_action_and_does_not_fall_back_to_native() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.before('builtin:creep.rangedMassAttack',{id:'empty'},ctx=>ctx.combat.applyDamageBatch([]));",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        fighter(&mut e, "a", 10, 10, &["ranged_attack"])?;
        let b = enemy(&mut e, "b", 11, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.a.rangedMassAttack()")?;
        assert_eq!(hits(&e, &b), 100);
        assert!(events(&e, kk_core::constants::EVENT_ATTACK).is_empty());
    }
    Ok(())
}
