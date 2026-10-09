//! 自主生命周期的顺序、处理链、失败原子性和缓存重试。
use super::*;

fn ticking(body: &str) -> String {
    package(&format!(
        r#"
const counter = api.objects.define('chain:counter', {{
  version:1,playerClass:'Counter',visibility:'server-only',ownerField:'owner',positionFields:['x','y'],
  fields:{{owner:{{type:'string',maxBytes:128}},x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}},value:{{type:'i32',default:0}}}},
  tick(ctx) {{ {body} }}
}});
api.intents.define('chain:increment',{{scope:'object',target:counter,args:{{type:'record',fields:{{amount:{{type:'i32',min:1,max:10}}}}}},process(ctx){{ctx.target.fields.value+=ctx.args.amount;}}}});
"#
    ))
}

#[test]
fn stable_object_and_room_order_without_player_intents_in_both_channels() -> anyhow::Result<()> {
    for binary in [false, true] {
        let source = ticking(
            "if(ctx.args!==null)throw Error('tick args');ctx.target.fields.value++;ctx.state={order:[...(ctx.state?.order||[]),ctx.target.fields.x]};",
        );
        let mut e = engine_with_shard(
            &[("chain", source)],
            binary,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1"],"rooms":[{"name":"W2N2","owner":"p1"},{"name":"W1N1","owner":"p1"}]
            }),
        )?;
        let first = e.place_mod_object(
            "W1N1",
            "chain:counter",
            &json!({"owner":"p1","x":30,"y":12}),
        )?;
        let second = e.place_mod_object(
            "W1N1",
            "chain:counter",
            &json!({"owner":"p1","x":10,"y":12}),
        )?;
        let third = e.place_mod_object(
            "W2N2",
            "chain:counter",
            &json!({"owner":"p1","x":20,"y":12}),
        )?;
        e.tick(3)?;
        for id in [first, second, third] {
            assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 3);
        }
        assert_eq!(
            e.engine_mod_state("chain"),
            json!({"order":[30,10,20,30,10,20,30,10,20]})
        );
    }
    Ok(())
}

#[test]
fn actions_then_tick_and_all_chain_kinds_share_current_candidate_fields() -> anyhow::Result<()> {
    for binary in [false, true] {
        let own = ticking("ctx.target.fields.value+=10;");
        let audit = package(
            r#"
api.handlers.before('chain:counter.tick',{id:'pre'},ctx=>{ctx.target.fields.value++;ctx.state={trace:['before']};});
api.handlers.wrap('chain:counter.tick',{id:'wrap'},(ctx,next)=>{ctx.state.trace.push('wrap-in');next();ctx.state.trace.push('wrap-out');ctx.target.fields.value+=100;});
api.handlers.after('chain:counter.tick',{id:'after'},ctx=>{ctx.state.trace.push('after');ctx.target.fields.value+=1000;});
"#,
        );
        let mut e = engine(&[("chain", own.clone()), ("audit", audit)], binary)?;
        let id = counter(&mut e)?;
        increment(&mut e, &id)?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 1114);
        assert_eq!(
            e.engine_mod_state("audit"),
            json!({"trace":["before","wrap-in","wrap-out","after"]})
        );
        let object = e.get_object(&id).unwrap();
        let state = e.engine_mod_state("audit");
        e.disable_mod("chain")?;
        assert!(!e.disabled_mods().contains("audit"));
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap(), object);
        assert_eq!(
            e.engine_mod_state("audit"),
            state,
            "所属槽停用后其他包的钩子也跳过"
        );
        let replace = package(
            "api.handlers.replace('chain:counter.tick',{id:'replace'},ctx=>{ctx.target.fields.value=50;});",
        );
        let mut e = engine(&[("chain", own), ("replace", replace)], binary)?;
        let id = counter(&mut e)?;
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 50);
        e.disable_mod("replace")?;
        e.tick(1)?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 60);
    }
    Ok(())
}

#[test]
fn second_room_fault_rolls_back_objects_state_and_memory_then_retries_cached_players()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let own = ticking("ctx.target.fields.value++;ctx.state={count:(ctx.state?.count||0)+1};");
        let bad = package(
            "api.handlers.after('chain:counter.tick',{id:'fault'},ctx=>{if(ctx.target.fields.x===20)throw Error('second room');});",
        );
        let mut e = engine_with_shard(
            &[("chain", own), ("bad", bad)],
            binary,
            &[],
            RegisterOptions::default(),
            &json!({
                "players":["p1"],"rooms":[{"name":"W1N1","owner":"p1"},{"name":"W2N2","owner":"p1"}]
            }),
        )?;
        let first = counter(&mut e)?;
        let second = e.place_mod_object(
            "W2N2",
            "chain:counter",
            &json!({"owner":"p1","x":20,"y":12}),
        )?;
        let generation = e.engine_generation();
        assert!(
            e.run_player(
                "p1",
                "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;'cached'"
            )
            .is_err()
        );
        assert_eq!(e.get_game_time(), 1);
        assert_eq!(e.engine_generation(), generation);
        for id in [&first, &second] {
            assert_eq!(e.get_object(id).unwrap()["fields"]["value"], 0);
        }
        assert_eq!(e.engine_mod_state("chain"), Value::Null);
        let fault = e.last_mod_settlement_fault().unwrap();
        assert_eq!(fault.room, "W2N2");
        assert_eq!(fault.target, second);
        assert_eq!(
            fault.error.fault.unwrap().slot.as_deref(),
            Some("chain:counter.tick")
        );
        e.disable_mod("bad")?;
        let result = e.run_player("p1", "throw Error('cannot rerun')")?;
        assert_eq!(result.expr["value"], "cached");
        assert_eq!(e.engine_mod_state("chain"), json!({"count":2}));
        assert_eq!(e.get_object(&first).unwrap()["fields"]["value"], 1);
        let result = e.run_player(
            "p1",
            "if(globalThis.n!==1||Memory.n!==1)throw Error('reran');true",
        )?;
        assert_eq!(result.expr["value"], true);
    }
    Ok(())
}

#[test]
fn late_writeback_failure_does_not_publish_tick_and_retry_does_not_double_apply()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let own = ticking("ctx.target.fields.value++;ctx.state={count:(ctx.state?.count||0)+1};");
        let mut e = engine(&[("chain", own)], binary)?;
        let id = counter(&mut e)?;
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(e.run_player("p1", "Memory.n=1;'cached'").is_err());
        assert_eq!(e.get_game_time(), 1);
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 0);
        assert_eq!(e.engine_mod_state("chain"), Value::Null);
        assert_eq!(
            e.run_player("p1", "throw Error('reran')")?.expr["value"],
            "cached"
        );
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 1);
        assert_eq!(e.engine_mod_state("chain"), json!({"count":1}));
        let object = e.get_object(&id).unwrap();
        let state = e.engine_mod_state("chain");
        e.disable_mod("chain")?;
        e.tick(2)?;
        assert_eq!(e.get_object(&id).unwrap(), object);
        assert_eq!(e.engine_mod_state("chain"), state);
    }
    Ok(())
}

#[test]
fn decline_invalid_fields_async_native_and_resource_access_are_tick_faults() -> anyhow::Result<()> {
    for body in [
        "return api.decline('no');",
        "try { Deno.core.ops.op_mod_chain_decline('1','chain:counter.tick.process','no'); } catch(e) {}",
        "ctx.target.fields.x=50;",
        "return Promise.resolve();",
        "while(true){}",
        "try { Deno.core.ops.op_mod_chain_native('1'); } catch(e) {}",
        "try { ctx.resources.read(ctx.target.id,'energy'); } catch(e) {}",
    ] {
        for binary in [false, true] {
            let own = ticking(&format!(
                "ctx.target.fields.value++;ctx.state={{dirty:true}};{body}"
            ));
            let mut e = engine(&[("chain", own)], binary)?;
            let id = counter(&mut e)?;
            let before = e.get_object(&id).unwrap();
            assert!(e.tick(1).is_err(), "{body}");
            assert_eq!(e.get_game_time(), 1);
            assert_eq!(e.get_object(&id).unwrap(), before);
            assert_eq!(e.engine_mod_state("chain"), Value::Null);
            assert!(e.last_mod_settlement_fault().is_some(), "{body}");
        }
    }
    Ok(())
}

#[test]
fn nonfunction_tick_fails_registration() {
    let own = ticking("").replace("tick(ctx) {  }", "tick: 7");
    assert!(engine(&[("chain", own)], false).is_err());
}
