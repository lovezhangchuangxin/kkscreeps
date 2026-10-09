//! Creep 资源槽的链序、权威读面、实验室绑定恢复和同轮竞争。
use super::*;

const RULES: &str = "api.rules.override('builtin:store.container.capacity',37);api.rules.override('builtin:store.lab.energyCapacity',17);api.rules.override('builtin:store.lab.mineralCapacity',23);";

fn actor(e: &mut EmbedEngine, name: &str, x: u8, store: Value) -> anyhow::Result<String> {
    e.place_object("W1N1", "creep", &json!({"name":name,"owner":"p1","pos":[x,9],"body":["carry","move"],"store":store,"ticksToLive":100}))
}

fn warehouse(e: &mut EmbedEngine, kind: &str, store: Value) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":kind,"owner":"p1","pos":[10,10],"store":store}),
    )
}

fn stored(e: &EmbedEngine, id: &str, resource: &str) -> i64 {
    e.get_object(id).unwrap()["store"][resource]
        .as_i64()
        .unwrap_or(0)
}

fn events(e: &EmbedEngine) -> Vec<Value> {
    e.world_snapshot()["rooms"]["W1N1"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["event"] == kk_core::constants::EVENT_TRANSFER)
        .cloned()
        .collect()
}

#[test]
fn creep_resource_chains_keep_action_order_and_refresh_authoritative_args_and_store()
-> anyhow::Result<()> {
    let source = package(&format!(
        r#"
{RULES}
for(const action of ['transfer','withdraw']){{
  const slot='builtin:creep.'+action;
  api.handlers.before(slot,{{id:action+'-pre'}},ctx=>{{
    if(ctx.target.fields.name!=='hauler'||ctx.target.fields.owner!=='p1'||ctx.target.fields.storeCapacity!==50)throw Error('creep 读面错误');
    ctx.state=ctx.state||{{trace:[]}};ctx.state.trace.push(action+':before');
  }});
  api.handlers.wrap(slot,{{id:action+'-wrap'}},(ctx,next)=>{{
    const store=ctx.target.fields.store;const id=ctx.target.id;const target=ctx.args.id;const amount=ctx.args.amount;
    ctx.state.trace.push(action+':enter');
    ctx.target.id='000000000000000000000000';ctx.args.id=ctx.target.id;ctx.args.amount=999;ctx.args.resourceType='U';
    next();
    if(ctx.target.id!==id||ctx.args.id!==target||ctx.args.amount!==amount||ctx.args.resourceType!=='energy')throw Error('权威参数未刷新');
    if(store.energy!==(action==='transfer'?25:32))throw Error('native 或旧 store 引用未刷新');
    ctx.state.trace.push(action+':exit');
  }});
  api.handlers.after(slot,{{id:action+'-post'}},ctx=>ctx.state.trace.push(action+':after'));
}}
"#
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let hauler = actor(&mut e, "hauler", 10, json!({"energy":30}))?;
        let target = warehouse(&mut e, "container", json!({"energy":20}))?;
        let run = e.run_player("p1", &format!("const t=Game.getObjectById('{target}');[Game.creeps.hauler.transfer(t,RESOURCE_ENERGY,5),Game.creeps.hauler.withdraw(t,RESOURCE_ENERGY,7)]"))?;
        assert_eq!(run.expr["value"], json!([0, 0]));
        assert_eq!(stored(&e, &hauler, "energy"), 32);
        assert_eq!(stored(&e, &target, "energy"), 18);
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"trace":["transfer:before","transfer:enter","transfer:exit","transfer:after","withdraw:before","withdraw:enter","withdraw:exit","withdraw:after"]})
        );
        assert_eq!(
            events(&e),
            vec![
                json!({"event":kk_core::constants::EVENT_TRANSFER,"objectId":hauler,"data":{"targetId":target,"resourceType":"energy","amount":5}}),
                json!({"event":kk_core::constants::EVENT_TRANSFER,"objectId":target,"data":{"targetId":hauler,"resourceType":"energy","amount":7}}),
            ]
        );
    }
    Ok(())
}

#[test]
fn creep_decline_at_each_position_restores_lab_binding_and_both_namespace_states()
-> anyhow::Result<()> {
    for action in ["transfer", "withdraw"] {
        for position in ["before", "wrap-before", "replace", "wrap-after", "after"] {
            let alpha = package(&format!(
                r#"
{RULES}
const slot='builtin:creep.{action}';
api.handlers.before(slot,{{id:'pre'}},ctx=>{{ctx.state={{dirty:'before'}};}});
api.handlers.wrap(slot,{{id:'outer'}},(ctx,next)=>{{ctx.state={{dirty:'wrap'}};try{{next()}}catch(e){{}}ctx.state={{dirty:'caught'}};}});
api.handlers.after(slot,{{id:'post'}},()=>{{throw Error('拒绝后不得运行剩余 after');}});
"#
            ));
            let beta = package(&format!(
                r#"
const slot='builtin:creep.{action}';const position='{position}';
api.handlers.before(slot,{{id:'pre'}},ctx=>{{ctx.state={{dirty:'before'}};if(position==='before')return api.decline('blocked');}});
api.handlers.wrap(slot,{{id:'inner'}},(ctx,next)=>{{
  ctx.state={{dirty:'wrap'}};if(position==='wrap-before')return api.decline('blocked');
  next();if(ctx.target.fields.store.U!=={})throw Error('native 未执行');
  if(position==='wrap-after')return api.decline('blocked');
}});
if(position==='replace')api.handlers.replace(slot,{{id:'center'}},()=>api.decline('blocked'));
api.handlers.after(slot,{{id:'z-write'}},ctx=>{{ctx.state={{dirty:'after'}};}});
api.handlers.after(slot,{{id:'post'}},()=>api.decline('blocked'));
"#,
                if action == "transfer" { 20 } else { 10 }
            ));
            for binary in [false, true] {
                let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
                let hauler = actor(
                    &mut e,
                    "hauler",
                    10,
                    if action == "transfer" {
                        json!({"U":30})
                    } else {
                        json!({})
                    },
                )?;
                let lab = warehouse(
                    &mut e,
                    "lab",
                    if action == "transfer" {
                        json!({})
                    } else {
                        json!({"U":10})
                    },
                )?;
                let old_store = e.get_object(&hauler).unwrap()["store"].clone();
                let old_lab = e.get_object(&lab).unwrap();
                let run=e.run_player("p1",&format!("Memory.committed=true;Game.creeps.hauler.{action}(Game.getObjectById('{lab}'),'U',10)"))?;
                assert_eq!(run.expr["value"], 0, "{action}/{position}: {}", run.expr);
                assert_eq!(
                    e.get_object(&hauler).unwrap()["store"],
                    old_store,
                    "{action}/{position}"
                );
                assert_eq!(e.get_object(&lab).unwrap(), old_lab, "{action}/{position}");
                assert_eq!(e.engine_mod_state("alpha"), Value::Null);
                assert_eq!(e.engine_mod_state("beta"), Value::Null);
                assert!(events(&e).is_empty());
                assert_eq!(
                    e.player_state("p1").unwrap().raw_memory,
                    "{\"committed\":true}"
                );
                assert_eq!(e.get_game_time(), 2);
                assert!(!e.needs_mod_recovery());
                assert!(e.last_mod_settlement_fault().is_none());
            }
        }
    }
    Ok(())
}

fn competing(
    binary: bool,
    action: &str,
    reject: &str,
    outer_fault: bool,
) -> anyhow::Result<(EmbedEngine, Vec<String>, String)> {
    let alpha = package(&format!(
        r#"
{RULES}
api.handlers.before('builtin:creep.{action}',{{id:'pre'}},ctx=>{{ctx.state={{count:(ctx.state?.count||0)+1}};}});
api.handlers.wrap('builtin:creep.{action}',{{id:'outer'}},(ctx,next)=>{{
  try{{next()}}catch(e){{}}
  if({outer_fault}&&ctx.target.fields.name==='rejected')throw Error('拒绝后异常');
}});
"#
    ));
    let beta = package(&format!(
        r#"
let calls=0;
api.handlers.before('builtin:creep.{action}',{{id:'pre'}},ctx=>{{ctx.state={{count:(ctx.state?.count||0)+1,calls:++calls}};}});
api.handlers.wrap('builtin:creep.{action}',{{id:'inner'}},(ctx,next)=>{{if(ctx.target.fields.name==='rejected'){{{reject}}}else next();}});
"#
    ));
    let mut e = engine(&[("alpha", alpha), ("beta", beta)], binary)?;
    let initial = if action == "transfer" {
        json!({"energy":30})
    } else {
        json!({})
    };
    let first = actor(&mut e, "first", 9, initial.clone())?;
    let rejected = actor(&mut e, "rejected", 10, initial.clone())?;
    let last = actor(&mut e, "last", 11, initial)?;
    let target = warehouse(
        &mut e,
        "container",
        if action == "transfer" {
            json!({"H":7})
        } else {
            json!({"energy":30,"H":7})
        },
    )?;
    let code = format!(
        "globalThis.executions=(globalThis.executions||0)+1;Memory.executions=globalThis.executions;const t=Game.getObjectById('{target}');['first','rejected','last'].map(n=>Game.creeps[n].{action}(t,RESOURCE_ENERGY,20))"
    );
    Ok((e, vec![first, rejected, last, target], code))
}

fn assert_competing(e: &EmbedEngine, ids: &[String], action: &str) {
    let expected = if action == "transfer" {
        [10, 30, 20, 30]
    } else {
        [20, 0, 10, 0]
    };
    for (id, energy) in ids.iter().zip(expected) {
        assert_eq!(stored(e, id, "energy"), energy, "{action}");
    }
    assert_eq!(stored(e, &ids[3], "H"), 7);
    let events = events(e);
    assert_eq!(events.len(), 2);
    for (i, amount) in [(0, 20), (1, 10)] {
        let actor = &ids[if i == 0 { 0 } else { 2 }];
        let (source, target) = if action == "transfer" {
            (actor, &ids[3])
        } else {
            (&ids[3], actor)
        };
        assert_eq!(events[i]["objectId"], *source);
        assert_eq!(events[i]["data"]["targetId"], *target);
        assert_eq!(events[i]["data"]["amount"], amount);
    }
    assert_eq!(e.engine_mod_state("alpha"), json!({"count":2}));
    assert_eq!(e.engine_mod_state("beta"), json!({"count":2,"calls":3}));
    assert_eq!(
        e.player_state("p1").unwrap().raw_memory,
        "{\"executions\":1}"
    );
    assert_eq!(e.get_game_time(), 2);
    assert!(!e.needs_mod_recovery());
    assert!(e.last_mod_settlement_fault().is_none());
}

#[test]
fn creep_decline_preserves_shared_target_competition_and_cached_writeback_retry()
-> anyhow::Result<()> {
    for action in ["transfer", "withdraw"] {
        for binary in [false, true] {
            let (mut e, ids, code) = competing(
                binary,
                action,
                "next();return api.decline('blocked');",
                false,
            )?;
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
            assert!(e.run_player("p1", &code).is_err());
            assert_eq!(e.world_snapshot(), before);
            assert_eq!(e.player_state("p1"), Some(&player));
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            assert_eq!(e.engine_mod_state("beta"), Value::Null);
            assert!(e.needs_mod_recovery());
            assert!(e.last_mod_settlement_fault().is_none());
            assert_eq!(
                e.run_player("p1", "throw Error('不得重新运行玩家')")?.expr["value"],
                json!([0, 0, 0])
            );
            assert_competing(&e, &ids, action);
            assert_eq!(
                e.run_player("p1", "[Memory.executions,globalThis.executions]")?
                    .expr["value"],
                json!([1, 1])
            );
        }
    }
    Ok(())
}

#[test]
fn declined_transfer_keeps_prior_drop_and_later_withdraw_and_movement() -> anyhow::Result<()> {
    let source = package(&format!(
        "{RULES}api.handlers.after('builtin:creep.transfer',{{id:'reject'}},()=>api.decline('blocked'));"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let hauler = actor(&mut e, "hauler", 10, json!({"energy":30}))?;
        let target = warehouse(&mut e, "container", json!({"energy":20}))?;
        let run=e.run_player("p1",&format!("const h=Game.creeps.hauler,t=Game.getObjectById('{target}');[h.drop(RESOURCE_ENERGY,5),h.transfer(t,RESOURCE_ENERGY,10),h.withdraw(t,RESOURCE_ENERGY,7),h.move(TOP)]"))?;
        assert_eq!(run.expr["value"], json!([0, 0, 0, 0]));
        assert_eq!(stored(&e, &hauler, "energy"), 32);
        assert_eq!(stored(&e, &target, "energy"), 13);
        assert_eq!(e.get_object(&hauler).unwrap()["pos"]["y"], 8);
        let drops = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
        assert_eq!(drops.len(), 1);
        assert_eq!(drops[0]["amount"], 4, "先前 drop 保留，当轮衰减 1");
        assert_eq!(events(&e).len(), 1);
        assert_eq!(events(&e)[0]["objectId"], target);
        assert_eq!(events(&e)[0]["data"]["targetId"], hauler);
    }
    Ok(())
}

#[test]
fn creep_decline_cannot_hide_readonly_write_fault_or_later_exception() -> anyhow::Result<()> {
    for action in ["transfer", "withdraw"] {
        for (reject, outer_fault, message) in [
            ("next();return api.decline('blocked');", true, "拒绝后异常"),
            (
                "ctx.target.fields.store.energy++;try{next()}catch(e){}return api.decline('blocked');",
                false,
                "当前只读",
            ),
        ] {
            for binary in [false, true] {
                let (mut e, ids, code) = competing(binary, action, reject, outer_fault)?;
                let before = e.world_snapshot();
                let player = e.player_state("p1").unwrap().clone();
                let error = e.run_player("p1", &code).unwrap_err();
                assert!(error.to_string().contains(message), "{error}");
                assert_eq!(e.world_snapshot(), before);
                assert_eq!(e.player_state("p1"), Some(&player));
                assert_eq!(e.engine_mod_state("alpha"), Value::Null);
                assert_eq!(e.engine_mod_state("beta"), Value::Null);
                assert!(e.needs_mod_recovery());
                assert!(e.last_mod_settlement_fault().is_some());
                // 停用后恢复原 native，缓存玩家代码不重跑；所有三次调用
                // 都完成（末次可因库存/容量不足而没有事件）。
                e.disable_mod(if outer_fault { "alpha" } else { "beta" })?;
                if outer_fault {
                    e.disable_mod("beta")?;
                }
                assert_eq!(
                    e.run_player("p1", "throw Error('不得重跑')")?.expr["value"],
                    json!([0, 0, 0])
                );
                let rejected = e.get_object(&ids[1]).unwrap();
                assert_eq!(
                    rejected["store"]["energy"].as_i64().unwrap_or(0),
                    if action == "transfer" { 20 } else { 10 }
                );
                assert_eq!(
                    e.player_state("p1").unwrap().raw_memory,
                    "{\"executions\":1}"
                );
                assert!(!e.needs_mod_recovery());
            }
        }
    }
    Ok(())
}

#[test]
fn creep_replace_and_wrap_cancellation_commit_state_and_run_after() -> anyhow::Result<()> {
    for action in ["transfer", "withdraw"] {
        for kind in ["replace", "wrap"] {
            let source = package(&format!(
                "{RULES}api.handlers.{kind}('builtin:creep.{action}',{{id:'cancel'}},ctx=>{{ctx.state={{cancelled:true}};}});api.handlers.after('builtin:creep.{action}',{{id:'post'}},ctx=>{{ctx.state.after=true;}});"
            ));
            for binary in [false, true] {
                let mut e = engine(&[("alpha", source.clone())], binary)?;
                let hauler = actor(&mut e, "hauler", 10, json!({"energy":20}))?;
                let target = warehouse(&mut e, "container", json!({"energy":20}))?;
                let run=e.run_player("p1",&format!("Game.creeps.hauler.{action}(Game.getObjectById('{target}'),RESOURCE_ENERGY,5)"))?;
                assert_eq!(run.expr["value"], 0);
                assert_eq!(stored(&e, &hauler, "energy"), 20);
                assert_eq!(stored(&e, &target, "energy"), 20);
                assert_eq!(
                    e.engine_mod_state("alpha"),
                    json!({"cancelled":true,"after":true})
                );
                assert!(events(&e).is_empty());
            }
        }
    }
    Ok(())
}
