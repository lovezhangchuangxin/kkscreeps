//! 资源领域能力的替换、损耗、竞争、多对象恢复与完整候选重试。
use super::*;

pub(super) fn actor(e: &mut EmbedEngine, name: &str, energy: i64) -> anyhow::Result<String> {
    e.place_object("W1N1", "creep", &json!({"name":name,"owner":"p1","pos":[10,9],"body":["carry","move"],"store":{"energy":energy},"ticksToLive":100}))
}

pub(super) fn store(
    e: &mut EmbedEngine,
    kind: &str,
    x: u8,
    contents: Value,
) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":kind,"owner":"p1","pos":[x,10],"store":contents}),
    )
}

pub(super) fn amount(e: &EmbedEngine, id: &str, resource: &str) -> i64 {
    e.get_object(id).unwrap()["store"][resource]
        .as_i64()
        .unwrap_or(0)
}

pub(super) fn events(e: &EmbedEngine) -> Vec<Value> {
    e.world_snapshot()["rooms"]["W1N1"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["event"] == kk_core::constants::EVENT_TRANSFER)
        .cloned()
        .collect()
}

#[test]
fn replacement_transfers_with_loss_and_refreshes_held_store_references() -> anyhow::Result<()> {
    let source = package(
        r#"
api.rules.override('builtin:store.container.capacity',37);
for (const action of ['transfer','withdraw']) {
  api.handlers.replace('builtin:creep.'+action,{id:action},ctx=>{
    const from=action==='transfer'?ctx.target.id:ctx.args.id;
    const to=action==='transfer'?ctx.args.id:ctx.target.id;
    const resource=ctx.args.resourceType;
    const read=ctx.resources.read(from,resource);
    const capacity=ctx.resources.read(to,resource);
    const debit=Math.min(8,ctx.args.amount,read.amount,capacity.free+2);
    const held=ctx.target.fields.store;
    ctx.state=ctx.state||{receipts:[],after:[]};
    ctx.state.receipts.push(ctx.resources.transfer(from,to,resource,debit,2));
    if(held.energy!==(action==='transfer'?22:28))throw Error('资源写后旧引用未刷新:'+held.energy+':'+ctx.target.fields.store.energy+':'+action);
    if(ctx.resources.read(to,resource).amount!==(action==='transfer'?26:28))throw Error('候选读没有看到转账');
  });
  api.handlers.after('builtin:creep.'+action,{id:action+'-after'},ctx=>ctx.state.after.push(ctx.target.fields.store.energy));
}
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let creep = actor(&mut e, "hauler", 30)?;
        let target = store(&mut e, "container", 10, json!({"energy":20}))?;
        let run = e.run_player("p1", &format!("const t=Game.getObjectById('{target}');[Game.creeps.hauler.transfer(t,'energy',10),Game.creeps.hauler.withdraw(t,'energy',10)]"))?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        assert_eq!(amount(&e, &creep, "energy"), 28);
        assert_eq!(amount(&e, &target, "energy"), 18);
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"receipts":[{"debited":8,"credited":6,"lost":2},{"debited":8,"credited":6,"lost":2}],"after":[22,28]})
        );
        assert_eq!(
            events(&e),
            vec![
                json!({"event":kk_core::constants::EVENT_TRANSFER,"objectId":creep,"data":{"targetId":target,"resourceType":"energy","amount":6,"debited":8,"loss":2}}),
                json!({"event":kk_core::constants::EVENT_TRANSFER,"objectId":target,"data":{"targetId":creep,"resourceType":"energy","amount":6,"debited":8,"loss":2}})
            ]
        );
    }
    Ok(())
}

#[test]
fn competing_exact_transfers_do_not_overfill_or_charge_loss_on_shortage() -> anyhow::Result<()> {
    let source = package(
        r#"
api.rules.override('builtin:store.container.capacity',37);
api.handlers.replace('builtin:creep.transfer',{id:'replace'},ctx=>{
  const target=ctx.args.id;
  ctx.state=ctx.state||{receipts:[]};
  const requested=ctx.target.fields.name==='first'?10:ctx.target.fields.name==='second'?10:5;
  ctx.state.receipts.push(ctx.resources.transfer(ctx.target.id,target,'energy',requested,1));
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let first = actor(&mut e, "first", 20)?;
        let second = actor(&mut e, "second", 20)?;
        let third = actor(&mut e, "third", 2)?;
        let target = store(&mut e, "container", 10, json!({"energy":28}))?;
        e.run_player("p1",&format!("const t=Game.getObjectById('{target}');[Game.creeps.first.transfer(t,'energy',1),Game.creeps.second.transfer(t,'energy',1),Game.creeps.third.transfer(t,'energy',1)]"))?;
        assert_eq!(
            [
                amount(&e, &first, "energy"),
                amount(&e, &second, "energy"),
                amount(&e, &third, "energy"),
                amount(&e, &target, "energy")
            ],
            [10, 20, 2, 37]
        );
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"receipts":[{"debited":10,"credited":9,"lost":1},{"debited":0,"credited":0,"lost":0},{"debited":0,"credited":0,"lost":0}]})
        );
        assert_eq!(events(&e).len(), 1);
    }
    Ok(())
}

#[test]
fn declined_multi_object_writes_restore_labs_and_keep_prior_and_later_actions() -> anyhow::Result<()>
{
    let lab_id = kk_world::ObjId(2).as_str();
    let bank_id = kk_world::ObjId(3).as_str();
    let alpha = package(
        r#"
api.rules.override('builtin:store.container.capacity',37);
api.rules.override('builtin:store.lab.energyCapacity',17);
api.rules.override('builtin:store.lab.mineralCapacity',23);
api.handlers.before('builtin:creep.transfer',{id:'pre'},ctx=>{ctx.state={count:(ctx.state?.count||0)+1};});
api.handlers.wrap('builtin:creep.transfer',{id:'outer'},(ctx,next)=>{next();ctx.state.count+=10;});
"#,
    );
    let beta = package(&format!(
        r#"
api.handlers.wrap('builtin:creep.transfer',{{id:'route'}},(ctx,next)=>{{
  next();ctx.state={{count:(ctx.state?.count||0)+1}};
  if(ctx.target.fields.name==='reject'){{
    if(ctx.resources.transfer('{lab_id}',ctx.target.id,'U',10).debited!==10)throw Error('取空失败');
    if(ctx.resources.transfer(ctx.target.id,'{bank_id}','U',10).credited!==10)throw Error('转存失败');
    return api.decline('route-blocked');
  }}
}});
"#
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
        let lab = store(&mut e, "lab", 30, json!({"U":10}))?;
        let bank = store(&mut e, "lab", 31, json!({}))?;
        assert_eq!((&lab, &bank), (&lab_id, &bank_id));
        let old_lab = e.get_object(&lab).unwrap();
        let old_bank = e.get_object(&bank).unwrap();
        let first = actor(&mut e, "first", 20)?;
        let reject = actor(&mut e, "reject", 20)?;
        let last = actor(&mut e, "last", 20)?;
        let target = store(&mut e, "container", 10, json!({}))?;
        e.run_player("p1",&format!("const t=Game.getObjectById('{target}');Memory.saved=true;[Game.creeps.first.transfer(t,'energy',10),Game.creeps.reject.transfer(t,'energy',10),Game.creeps.last.transfer(t,'energy',10)]"))?;
        assert_eq!(
            [
                amount(&e, &first, "energy"),
                amount(&e, &reject, "energy"),
                amount(&e, &last, "energy"),
                amount(&e, &target, "energy")
            ],
            [10, 20, 10, 20]
        );
        assert_eq!(amount(&e, &reject, "U"), 0);
        assert_eq!(e.get_object(&lab).unwrap(), old_lab);
        assert_eq!(e.get_object(&bank).unwrap(), old_bank);
        assert_eq!(e.engine_mod_state("alpha"), json!({"count":22}));
        assert_eq!(e.engine_mod_state("beta"), json!({"count":2}));
        assert_eq!(events(&e).len(), 2);
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"saved\":true}");
        assert!(e.last_mod_settlement_fault().is_none());
    }
    Ok(())
}

#[test]
fn successful_resource_writes_release_and_rebind_lab_mineral_slots() -> anyhow::Result<()> {
    let source = package(
        r#"
api.rules.override('builtin:store.lab.energyCapacity',17);
api.rules.override('builtin:store.lab.mineralCapacity',23);
api.handlers.replace('builtin:creep.transfer',{id:'switch'},ctx=>{
  ctx.state={old:ctx.resources.read(ctx.args.id,'U')};
  ctx.resources.transfer(ctx.args.id,ctx.target.id,'U',10);
  ctx.state.empty=ctx.resources.read(ctx.args.id,'K');
  ctx.resources.transfer(ctx.target.id,ctx.args.id,'K',5);
  ctx.state.bound=ctx.resources.read(ctx.args.id,'K');
  ctx.state.other=ctx.resources.read(ctx.args.id,'U');
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let creep=e.place_object("W1N1","creep",&json!({"name":"hauler","owner":"p1","pos":[10,9],"body":["carry","move"],"store":{"energy":1,"K":5}}))?;
        let lab = store(&mut e, "lab", 10, json!({"U":10}))?;
        e.run_player(
            "p1",
            &format!("Game.creeps.hauler.transfer(Game.getObjectById('{lab}'),'energy',1)"),
        )?;
        assert_eq!(
            [
                amount(&e, &creep, "U"),
                amount(&e, &creep, "K"),
                amount(&e, &creep, "energy"),
                amount(&e, &lab, "U"),
                amount(&e, &lab, "K")
            ],
            [10, 0, 1, 0, 5]
        );
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"old":{"amount":10,"capacity":23,"free":13},"empty":{"amount":0,"capacity":23,"free":23},"bound":{"amount":5,"capacity":23,"free":18},"other":{"amount":0,"capacity":0,"free":0}})
        );
        assert_eq!(events(&e).len(), 2);
    }
    Ok(())
}

#[test]
fn multi_object_resource_candidates_retry_without_running_players_again() -> anyhow::Result<()> {
    let bank_id = kk_world::ObjId(2).as_str();
    let source = package(&format!(
        r#"
api.handlers.replace('builtin:creep.transfer',{{id:'route'}},ctx=>{{
  ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',5);
  ctx.resources.transfer(ctx.args.id,'{bank_id}','energy',3,1);
  ctx.state={{count:(ctx.state?.count||0)+1}};
}});
"#
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let bank = store(&mut e, "container", 30, json!({}))?;
        assert_eq!(bank, bank_id);
        let creep = actor(&mut e, "hauler", 30)?;
        let target = store(&mut e, "container", 10, json!({}))?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!(
            "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;Game.creeps.hauler.transfer(Game.getObjectById('{target}'),'energy',5);globalThis.n"
        );
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert_eq!(
            e.run_player("p1", "throw Error('不得重跑')")?.expr["value"],
            1
        );
        assert_eq!(
            [
                amount(&e, &creep, "energy"),
                amount(&e, &target, "energy"),
                amount(&e, &bank, "energy")
            ],
            [25, 2, 2]
        );
        assert_eq!(e.engine_mod_state("alpha"), json!({"count":1}));
        assert_eq!(events(&e).len(), 2);
        assert_eq!(
            e.run_player("p1", "[globalThis.n,Memory.n]")?.expr["value"],
            json!([1, 1])
        );
    }
    Ok(())
}

#[test]
fn link_replace_can_use_resource_api_and_keep_cooldown_write_semantics() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.replace('builtin:link.transfer',{id:'replace'},ctx=>{
  const held=ctx.target.fields.store;
  ctx.state=ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',10,3);
  if(held.energy!==90)throw Error('link 库存没有刷新');
  ctx.target.fields.cooldown=5;
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let from = link(&mut e, 10, 100, 0)?;
        let to = link(&mut e, 11, 0, 0)?;
        e.run_player(
            "p1",
            &format!("Game.getObjectById('{from}').transferEnergy(Game.getObjectById('{to}'),10)"),
        )?;
        assert_eq!(
            [amount(&e, &from, "energy"), amount(&e, &to, "energy")],
            [90, 7]
        );
        assert_eq!(cooldown(&e, &from), 4);
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"debited":10,"credited":7,"lost":3})
        );
    }
    Ok(())
}

#[test]
fn two_players_compete_for_one_candidate_store_in_stable_order() -> anyhow::Result<()> {
    let source = package(
        r#"
api.rules.override('builtin:store.container.capacity',37);
api.handlers.replace('builtin:creep.transfer',{id:'replace'},ctx=>{
  ctx.state=ctx.state||{receipts:[]};
  ctx.state.receipts.push({owner:ctx.target.fields.owner,...ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',10,1)});
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine_with_shard(
            &[("alpha", source.clone())],
            binary,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1","p2"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]
            }),
        )?;
        e.set_player_concurrency(2)?;
        let mut ids = Vec::new();
        for player in ["p1", "p2"] {
            ids.push(e.place_object("W1N1","creep",&json!({"name":player,"owner":player,"pos":[10,9],"body":["carry","move"],"store":{"energy":20}}))?);
        }
        let target = store(&mut e, "container", 10, json!({"energy":28}))?;
        let code = format!(
            "Memory.runs=(Memory.runs||0)+1;Object.values(Game.creeps)[0].transfer(Game.getObjectById('{target}'),'energy',1)"
        );
        let codes =
            std::collections::BTreeMap::from([("p1".into(), code.clone()), ("p2".into(), code)]);
        let runs = e.run_players(&codes)?;
        assert_eq!(runs["p1"].expr["value"], 0);
        assert_eq!(runs["p2"].expr["value"], 0);
        assert_eq!(
            [
                amount(&e, &ids[0], "energy"),
                amount(&e, &ids[1], "energy"),
                amount(&e, &target, "energy")
            ],
            [10, 20, 37]
        );
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"receipts":[{"owner":"p1","debited":10,"credited":9,"lost":1},{"owner":"p2","debited":0,"credited":0,"lost":0}]})
        );
    }
    Ok(())
}

#[test]
fn resource_decline_at_every_chain_position_restores_extra_objects_and_namespaces()
-> anyhow::Result<()> {
    let bank_id = kk_world::ObjId(2).as_str();
    for (kind, body) in [
        ("before", "route(ctx);return api.decline('blocked');"),
        ("wrap", "route(ctx);return api.decline('blocked');"),
        ("replace", "route(ctx);return api.decline('blocked');"),
        ("wrap", "next();route(ctx);return api.decline('blocked');"),
        ("after", "route(ctx);return api.decline('blocked');"),
    ] {
        let alpha = package(
            "api.handlers.before('builtin:creep.transfer',{id:'pre'},ctx=>{ctx.state={dirty:true}});",
        );
        let beta = package(&format!(
            r#"
function route(ctx){{ctx.state={{dirty:true}};ctx.resources.transfer(ctx.target.id,'{bank_id}','energy',5,2);}}
api.handlers.{kind}('builtin:creep.transfer',{{id:'reject'}},(ctx,next)=>{{{body}}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let bank = store(&mut e, "lab", 30, json!({}))?;
            assert_eq!(bank, bank_id);
            let creep = actor(&mut e, "hauler", 20)?;
            let target = store(&mut e, "container", 10, json!({}))?;
            e.run_player("p1",&format!("Memory.saved=true;Game.creeps.hauler.transfer(Game.getObjectById('{target}'),'energy',5)"))?;
            assert_eq!(
                [
                    amount(&e, &creep, "energy"),
                    amount(&e, &target, "energy"),
                    amount(&e, &bank, "energy")
                ],
                [20, 0, 0]
            );
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            assert_eq!(e.engine_mod_state("beta"), Value::Null);
            assert!(events(&e).is_empty());
            assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"saved\":true}");
            assert!(e.last_mod_settlement_fault().is_none());
        }
    }
    Ok(())
}
