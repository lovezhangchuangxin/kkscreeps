//! lab/power/nuke 与既有候选、玩家投影和受击槽的交界回归。
use super::*;

fn plain_engine(binary: bool, rcl: u8) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":10000}],"rooms":[{"name":"W1N1","owner":"p1","rcl":rcl}]}))?;
    Ok(e)
}

fn lab(e: &mut EmbedEngine, pos: [u8; 2], store: Value) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({
            "structureType":"lab", "owner":"p1", "pos":pos, "store":store
        }),
    )
}

/// 官方 labs/intents.js 先解除强化，再执行反应；两次预检 OK 不代表两条均落地。
#[test]
fn lab_unboost_precedes_reaction_and_blocks_its_consumption() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = plain_engine(binary, 6)?;
        let a = lab(&mut e, [20, 20], json!({"UH":5,"energy":100}))?;
        let b = lab(&mut e, [21, 20], json!({"U":5}))?;
        let c = lab(&mut e, [20, 21], json!({"H":5}))?;
        let d = e.place_object(
            "W1N1",
            "creep",
            &json!({
                "owner":"p1", "name":"boosted", "pos":[19,20],
                "body":["attack","move"], "boosts":{"0":"UH"}
            }),
        )?;
        let run = e.run_player("p1", &format!(
            "const a=Game.getObjectById('{a}'); [a.runReaction(Game.getObjectById('{b}'),Game.getObjectById('{c}')),a.unboostCreep(Game.getObjectById('{d}'))]"
        ))?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        assert!(e.get_object(&d).unwrap()["body"][0].get("boost").is_none());
        assert_eq!(e.get_object(&a).unwrap()["store"]["UH"], 5);
        assert_eq!(e.get_object(&b).unwrap()["store"]["U"], 5);
        assert_eq!(e.get_object(&c).unwrap()["store"]["H"], 5);
        let drops = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
        assert_eq!(drops.len(), 1);
        // 意图阶段退回 15，掉落堆参加当轮 tick 后衰减为 14。
        assert_eq!(drops[0]["amount"], 14);
    }
    Ok(())
}

/// boost 必须先消耗矿物，reverse 不能抢走最后一份强化资源。
#[test]
fn lab_boost_consumes_before_reverse() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = plain_engine(binary, 6)?;
        let a = lab(&mut e, [20, 20], json!({"UH":30,"energy":100}))?;
        let b = lab(&mut e, [21, 20], json!({}))?;
        let c = lab(&mut e, [20, 21], json!({}))?;
        let d = harness::fighter(&mut e, "unboosted", 19, 20, &["attack"])?;
        let run = e.run_player("p1", &format!(
            "const a=Game.getObjectById('{a}'); [a.reverseReaction(Game.getObjectById('{b}'),Game.getObjectById('{c}')),a.boostCreep(Game.getObjectById('{d}'))]"
        ))?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        assert_eq!(e.get_object(&d).unwrap()["body"][0]["boost"], "UH");
        assert!(e.get_object(&a).unwrap()["store"].get("UH").is_none());
        assert!(e.get_object(&b).unwrap()["mineralType"].is_null());
    }
    Ok(())
}

/// 借用玩家原型不会绕过全局处理器的类型与所有权门。
#[test]
fn power_suicide_does_not_delete_generic_structure() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = plain_engine(binary, 6)?;
        let id = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "structureType":"factory","owner":"p1","pos":[20,20],"store":{"energy":20}
            }),
        )?;
        let run = e.run_player(
            "p1",
            &format!("PowerCreep.prototype.suicide.call(Game.getObjectById('{id}'))"),
        )?;
        assert_eq!(run.expr["value"], 0);
        assert_eq!(e.get_object(&id).unwrap()["store"]["energy"], 20);
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_TOMBSTONES)
                .is_empty()
        );
    }
    Ok(())
}

/// shard 由生成态派生，冷恢复不丢失；死亡回未生成态后清除。
#[test]
fn power_shard_tracks_spawn_and_death_across_checkpoint() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = plain_engine(binary, 8)?;
        let ps = e.place_object(
            "W1N1",
            "structure",
            &json!({
                "structureType":"powerSpawn","owner":"p1","pos":[20,20]
            }),
        )?;
        assert_eq!(
            e.run_player("p1", "PowerCreep.create('operator',POWER_CLASS.OPERATOR)")?
                .expr["value"],
            0
        );
        assert_eq!(
            e.run_player("p1", "Game.powerCreeps.operator.shard === undefined")?
                .expr["value"],
            true
        );
        assert_eq!(
            e.run_player(
                "p1",
                &format!("Game.powerCreeps.operator.spawn(Game.getObjectById('{ps}'))")
            )?
            .expr["value"],
            0
        );
        let checkpoint = e.checkpoint(Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(checkpoint, None)?;
        assert_eq!(
            restored
                .run_player("p1", "Game.powerCreeps.operator.shard === Game.shard.name")?
                .expr["value"],
            true
        );
        restored.run_player("p1", "Game.powerCreeps.operator.suicide()")?;
        assert_eq!(
            restored
                .run_player("p1", "Game.powerCreeps.operator.shard === undefined")?
                .expr["value"],
            true
        );
    }
    Ok(())
}

/// 对象 tick 产生的托管伤害必须执行槽，异常也必须撤销整轮候选。
#[test]
fn nuke_runs_hosted_incoming_and_rolls_back_on_fault() -> anyhow::Result<()> {
    for fault in [false, true] {
        let handler = if fault {
            "throw Error('nuke-fault');"
        } else {
            "ctx.incoming.setDamage(100);"
        };
        let source = package(&format!(
            "api.objects.define('guard:cell',{{version:1,playerClass:'GuardCell',visibility:'server-only',positionFields:['x','y'],hitsMax:300,fields:{{x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}}}}}});
            api.handlers.replace('builtin:mod.receiveDamage',{{id:'armor'}},ctx=>{{
              if(ctx.args.source.attackType!==6)throw Error('nuke attack type');
              ctx.state={{calls:(ctx.state?.calls||0)+1}};{handler}
            }});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("guard", source.clone())], binary)?;
            let target = e.place_mod_object("W1N1", "guard:cell", &json!({"x":20,"y":20}))?;
            let nuke = e.place_object(
                "W1N1",
                "nuke",
                &json!({"pos":[20,20],"timeToLand":1,"launchRoomName":"W2N1"}),
            )?;
            let time = e.get_game_time();
            let result = e.tick(1);
            if fault {
                assert!(result.is_err());
                assert_eq!(e.get_game_time(), time);
                assert_eq!(e.get_object(&target).unwrap()["fields"]["hits"], 300);
                assert_eq!(e.engine_mod_state("guard"), Value::Null);
                assert!(e.get_object(&nuke).is_some());
            } else {
                result?;
                assert_eq!(e.get_object(&target).unwrap()["fields"]["hits"], 200);
                assert_eq!(e.engine_mod_state("guard"), json!({"calls":1}));
                e.tick(1)?;
                assert!(e.get_object(&nuke).is_none());
                assert_eq!(e.get_object(&target).unwrap()["fields"]["hits"], 200);
            }
        }
    }
    Ok(())
}

/// 托管移动输入须与原生所有权/让位判定一致，powerCreep 不依赖 MOVE 部件。
#[test]
fn power_movement_hosted_inputs_preserve_owner_and_yielding() -> anyhow::Result<()> {
    let source = package(
        r#"
      api.handlers.before('builtin:room.movePrecheck',{id:'precheck'},ctx=>{
        for(const r of ctx.args.requests){
          if(r.owner!=='p1'||r.nativeStaticBlocked)throw Error('precheck owner/rampart');
        }
        ctx.state={...(ctx.state||{}),precheck:true};
      });
      api.handlers.before('builtin:room.moveContest',{id:'contest'},ctx=>{
        for(const c of ctx.args.contests)for(const r of c.contenders){
          if(r.owner!=='p1')throw Error('contest owner');
        }
        ctx.state={...(ctx.state||{}),contest:true};
      });
      api.handlers.before('builtin:creep.moveAdmission',{id:'admission'},ctx=>{
        for(const m of ctx.args.movers)if(m.owner!=='p1')throw Error('admission owner');
        ctx.state={...(ctx.state||{}),admission:true};
      });
      api.handlers.before('builtin:room.moveObstacle',{id:'obstacle'},ctx=>{
        for(const m of ctx.args.movers){
          if(m.owner!=='p1'||m.nativeBlocked)throw Error('obstacle owner/rampart');
        }
        if(!ctx.args.movers.some(m=>m.objects.some(o=>o.kind==='powerCreep'&&o.owner==='p1'&&o.yielding)))throw Error('power yielding');
        ctx.state={...(ctx.state||{}),obstacle:true};
      });
    "#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("move", source.clone())], binary)?;
        let pc = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"operator","owner":"p1","pos":[10,10]}),
        )?;
        let other = harness::creep_at(&mut e, "W1N1", "other", "p1", 10, 11, &["move"])?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"structureType":"rampart","owner":"p1","pos":[10,11],"hits":1000}),
        )?;
        // 第三名请求者制造竞争；互换方仍以原生第一键胜出。
        harness::creep_at(&mut e, "W1N1", "contender", "p1", 9, 11, &["move"])?;
        e.run_player("p1","Game.powerCreeps.operator.move(BOTTOM);Game.creeps.other.move(TOP);Game.creeps.contender.move(RIGHT)")?;
        assert_eq!(harness::pos_in(&e, "W1N1", &pc), (10, 11));
        assert_eq!(harness::pos_in(&e, "W1N1", &other), (10, 10));
        assert_eq!(
            e.engine_mod_state("move"),
            json!({"precheck":true,"contest":true,"admission":true,"obstacle":true})
        );
    }
    Ok(())
}

/// 官方 nukes/pretick.js 在爆炸意图阶段之前取消整房新孵化，避免白扣能量。
#[test]
fn nuke_landing_cancels_new_spawn_without_consuming_energy() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = plain_engine(binary, 6)?;
        let spawn = e.place_object(
            "W1N1",
            "spawn",
            &json!({"name":"safe","owner":"p1","pos":[10,10],"store":{"energy":300}}),
        )?;
        e.place_object(
            "W1N1",
            "nuke",
            &json!({"pos":[40,40],"timeToLand":1,"launchRoomName":"W2N1"}),
        )?;
        assert_eq!(
            e.run_player("p1", "Game.spawns.safe.spawnCreep([MOVE],'newborn')")?
                .expr["value"],
            0
        );
        assert_eq!(e.get_object(&spawn).unwrap()["store"]["energy"], 300);
        assert!(e.get_object(&spawn).unwrap()["spawning"].is_null());
        assert!(
            e.find_in_room("W1N1", kk_core::constants::FIND_CREEPS)
                .is_empty()
        );
        // 核弹已经落地、下一拍仅移除；孵化不应继续被禁。
        assert_eq!(
            e.run_player("p1", "Game.spawns.safe.spawnCreep([MOVE],'newborn')")?
                .expr["value"],
            0
        );
        assert_eq!(e.get_object(&spawn).unwrap()["store"]["energy"], 251);
        assert_eq!(
            e.find_in_room("W1N1", kk_core::constants::FIND_CREEPS)
                .len(),
            1
        );
    }
    Ok(())
}
