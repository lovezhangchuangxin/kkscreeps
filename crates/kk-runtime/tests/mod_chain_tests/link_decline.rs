//! 内建传输的局部恢复：多对象、事件、namespace、组合拒绝与整轮重试。
use super::*;

fn transfer_events(e: &EmbedEngine) -> Vec<Value> {
    e.world_snapshot()["rooms"]["W1N1"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|event| event["event"] == kk_core::constants::EVENT_TRANSFER)
        .cloned()
        .collect()
}

#[test]
fn link_decline_at_each_chain_position_restores_objects_events_and_namespaces() -> anyhow::Result<()>
{
    for position in ["before", "wrap-before", "replace", "wrap-after", "after"] {
        for binary in [false, true] {
            for self_transfer in [false, true] {
                let alpha = package(
                    r#"
api.handlers.before('builtin:link.transfer',{id:'pre'},ctx=>{ctx.state={dirty:'before'};ctx.target.fields.cooldown=0;});
api.handlers.wrap('builtin:link.transfer',{id:'outer'},(ctx,next)=>{
  ctx.state={dirty:'wrap'};ctx.target.fields.cooldown=0;
  try{next()}catch(e){}
  ctx.state={dirty:'caught'};ctx.target.fields.cooldown=9;
});
api.handlers.after('builtin:link.transfer',{id:'post'},()=>{throw Error('拒绝后不能运行剩余 after');});
"#,
                );
                let beta = package(&format!(
                    r#"
const position='{position}';
api.handlers.before('builtin:link.transfer',{{id:'pre'}},ctx=>{{ctx.state={{dirty:'before'}};if(position==='before')return api.decline('blocked');}});
api.handlers.wrap('builtin:link.transfer',{{id:'inner'}},(ctx,next)=>{{
  ctx.state={{dirty:'wrap'}};
  if(position==='wrap-before')return api.decline('blocked');
  next();
  if(position!=='replace' && ctx.target.fields.store.energy!=={})throw Error('native 未执行');
  if(position==='wrap-after')return api.decline('blocked');
}});
if(position==='replace')api.handlers.replace('builtin:link.transfer',{{id:'center'}},()=>api.decline('blocked'));
api.handlers.after('builtin:link.transfer',{{id:'z-write'}},ctx=>{{ctx.state={{dirty:'after'}};ctx.target.fields.cooldown=12;}});
api.handlers.after('builtin:link.transfer',{{id:'post'}},()=>api.decline('blocked'));
"#,
                    if self_transfer { 99 } else { 75 }
                ));
                let mut e = engine(&[("alpha", alpha), ("beta", beta)], binary)?;
                let source = link(&mut e, 20, 100, 0)?;
                let target = if self_transfer {
                    source.clone()
                } else {
                    link(&mut e, 21, 775, 0)?
                };
                let old_source = e.get_object(&source).unwrap();
                let old_target = e.get_object(&target).unwrap();
                if self_transfer {
                    // 玩家 API 禁止自转移；可信外部意图仍会到达 native 的
                    // 自指分支，须验证只保存一份对象时仍能完整撤销。
                    e.settle_external(kk_engine::intent::IntentBatch::from_json(
                        "p1",
                        &json!({source.clone(): {"transfer": {"id":target,"amount":25,"resourceType":"energy"}}}),
                    ))?;
                } else {
                    let run = e.run_player("p1", &format!("Memory.committed=true;Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25)"))?;
                    assert_eq!(run.expr["value"], 0, "{position}");
                }
                assert_eq!(e.get_object(&source).unwrap(), old_source, "{position}");
                assert_eq!(e.get_object(&target).unwrap(), old_target, "{position}");
                assert!(transfer_events(&e).is_empty(), "{position}");
                assert_eq!(e.engine_mod_state("alpha"), Value::Null, "{position}");
                assert_eq!(e.engine_mod_state("beta"), Value::Null, "{position}");
                assert_eq!(
                    e.player_state("p1").unwrap().raw_memory,
                    if self_transfer {
                        ""
                    } else {
                        "{\"committed\":true}"
                    }
                );
                assert_eq!(e.get_game_time(), 1);
                assert!(!e.needs_mod_recovery());
                assert!(e.last_mod_settlement_fault().is_none());
            }
        }
    }
    Ok(())
}

fn competition(
    binary: bool,
    reject: &str,
    outer_fault: bool,
) -> anyhow::Result<(EmbedEngine, Vec<String>, String)> {
    let alpha = package(
        r#"
api.rules.override('builtin:structure.quota.link.rcl6',6);
api.handlers.before('builtin:link.transfer',{id:'pre'},ctx=>{ctx.state={count:(ctx.state?.count||0)+1};ctx.target.fields.cooldown=0;});
api.handlers.wrap('builtin:link.transfer',{id:'outer'},(ctx,next)=>{
  try{next()}catch(e){}
  // outer-fault
  ctx.target.fields.cooldown=9;
});
"#,
    );
    let alpha = alpha.replace(
        "// outer-fault",
        if outer_fault {
            "if(ctx.args.amount===26)throw Error('拒绝后异常');"
        } else {
            ""
        },
    );
    let beta = package(&format!(
        r#"
let calls=0;
api.handlers.before('builtin:link.transfer',{{id:'pre'}},ctx=>{{ctx.state={{count:(ctx.state?.count||0)+1,calls:++calls}};}});
api.handlers.wrap('builtin:link.transfer',{{id:'inner'}},(ctx,next)=>{{
  if(ctx.args.amount===26){{ {reject} }}else next();
}});
"#
    ));
    let mut e = engine(&[("alpha", alpha), ("beta", beta)], binary)?;
    // 对象按 ID 派发；中间动作与前后动作共用已接近满仓的接收方。
    let first = link(&mut e, 20, 100, 0)?;
    let rejected = link(&mut e, 21, 100, 0)?;
    let last = link(&mut e, 22, 100, 0)?;
    let target = link(&mut e, 23, 760, 0)?;
    let code = format!(
        "globalThis.executions=(globalThis.executions||0)+1;Memory.executions=globalThis.executions;const t=Game.getObjectById('{target}');[Game.getObjectById('{first}').transferEnergy(t,25),Game.getObjectById('{rejected}').transferEnergy(t,26),Game.getObjectById('{last}').transferEnergy(t,27)]"
    );
    Ok((e, vec![first, rejected, last, target], code))
}

fn assert_competition(e: &EmbedEngine, ids: &[String]) {
    for (id, energy, cooldown) in [
        (&ids[0], 75, 8),
        (&ids[1], 100, 0),
        (&ids[2], 84, 8),
        (&ids[3], 799, 0),
    ] {
        let object = e.get_object(id).unwrap();
        assert_eq!(object["store"]["energy"], energy);
        assert_eq!(object["cooldown"], cooldown);
    }
    let events = transfer_events(e);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["objectId"], ids[0]);
    assert_eq!(events[0]["data"]["targetId"], ids[3]);
    assert_eq!(events[0]["data"]["amount"], 25);
    assert_eq!(events[1]["objectId"], ids[2]);
    assert_eq!(events[1]["data"]["targetId"], ids[3]);
    assert_eq!(events[1]["data"]["amount"], 16);
    assert_eq!(e.engine_mod_state("alpha"), json!({"count":2}));
    assert_eq!(e.engine_mod_state("beta"), json!({"count":2,"calls":3}));
    assert_eq!(e.get_game_time(), 1);
    assert_eq!(
        e.player_state("p1").unwrap().raw_memory,
        "{\"executions\":1}"
    );
    assert!(!e.needs_mod_recovery());
    assert!(e.last_mod_settlement_fault().is_none());
}

#[test]
fn link_decline_preserves_prior_shared_target_writes_and_runs_later_actions() -> anyhow::Result<()>
{
    for binary in [false, true] {
        let (mut e, ids, code) =
            competition(binary, "next();return api.decline('blocked');", false)?;
        assert_eq!(e.run_player("p1", &code)?.expr["value"], json!([0, 0, 0]));
        assert_competition(&e, &ids);
    }
    Ok(())
}

#[test]
fn link_decline_cannot_hide_a_fault_and_fault_rolls_back_prior_actions() -> anyhow::Result<()> {
    for (reject, outer_fault, message) in [
        ("next();return api.decline('blocked');", true, "拒绝后异常"),
        (
            "ctx.target.fields.store.energy++;try{next()}catch(e){}return api.decline('blocked');",
            false,
            "其余字段只读",
        ),
    ] {
        for binary in [false, true] {
            let (mut e, _, code) = competition(binary, reject, outer_fault)?;
            let before = e.world_snapshot();
            let error = e.run_player("p1", &code).unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(e.world_snapshot(), before);
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            assert_eq!(e.engine_mod_state("beta"), Value::Null);
            assert_eq!(e.player_state("p1").unwrap().raw_memory, "");
            assert!(e.needs_mod_recovery());
            assert!(e.last_mod_settlement_fault().is_some());
        }
    }
    Ok(())
}

#[test]
fn link_decline_survives_late_writeback_failure_and_cached_retry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let (mut e, ids, code) =
            competition(binary, "next();return api.decline('blocked');", false)?;
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
        let retry = e.run_player("p1", "throw Error('不得重新运行玩家')")?;
        assert_eq!(retry.expr["value"], json!([0, 0, 0]));
        assert_competition(&e, &ids);
        let next = e.run_player("p1", "[Memory.executions,globalThis.executions]")?;
        assert_eq!(next.expr["value"], json!([1, 1]));
    }
    Ok(())
}

#[test]
fn link_wrap_cancellation_still_commits_its_writes_and_runs_after() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine(
            &[(
                "alpha",
                package(
                    "api.handlers.wrap('builtin:link.transfer',{id:'cancel'},ctx=>{ctx.state={cancelled:true};ctx.target.fields.cooldown=9;});api.handlers.after('builtin:link.transfer',{id:'post'},ctx=>{ctx.state.after=true;});",
                ),
            )],
            binary,
        )?;
        let source = link(&mut e, 20, 100, 0)?;
        let target = link(&mut e, 21, 0, 0)?;
        let run = e.run_player(
            "p1",
            &format!(
                "Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25)"
            ),
        )?;
        assert_eq!(run.expr["value"], 0);
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 100);
        assert_eq!(cooldown(&e, &source), 8);
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 0);
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"cancelled":true,"after":true})
        );
        assert!(transfer_events(&e).is_empty());
    }
    Ok(())
}
