//! 真实 JS/native 整槽链：链序、候选同步、取消、失败回滚与能力寿命。
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use kk_mod_host::{RegisterOptions, discover_packages};
use kk_runtime::EmbedEngine;
use serde_json::{Value, json};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct PackageRoot(PathBuf);
impl Drop for PackageRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn engine(packages: &[(&str, String)], kkpv: bool) -> anyhow::Result<EmbedEngine> {
    engine_with_dependencies(packages, kkpv, &[])
}

fn engine_with_dependencies(
    packages: &[(&str, String)],
    kkpv: bool,
    dependencies: &[(&str, &str)],
) -> anyhow::Result<EmbedEngine> {
    engine_with_options(packages, kkpv, dependencies, RegisterOptions::default())
}

fn engine_with_options(
    packages: &[(&str, String)],
    kkpv: bool,
    dependencies: &[(&str, &str)],
    options: RegisterOptions,
) -> anyhow::Result<EmbedEngine> {
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-chain-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    for (id, source) in packages {
        let dir = root.0.join(id);
        std::fs::create_dir_all(&dir)?;
        let dependencies: serde_json::Map<String, Value> = dependencies
            .iter()
            .filter(|(dependent, _)| dependent == id)
            .map(|(_, dependency)| (dependency.to_string(), json!("^1.0.0")))
            .collect();
        std::fs::write(
            dir.join("mod.json"),
            json!({"id": id, "version": "1.0.0", "apiVersion": "^1.0.0", "dependencies": dependencies}).to_string(),
        )?;
        std::fs::write(dir.join("server.js"), source)?;
    }
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(kkpv);
    e.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        &packages,
        options,
    )?;
    Ok(e)
}

fn package(body: &str) -> String {
    format!("export default function register(api) {{ {body} }}")
}

fn link(e: &mut EmbedEngine, x: u8, energy: u32, cooldown: u32) -> anyhow::Result<String> {
    e.place_object("W1N1", "structure", &json!({"pos": [x, 20], "structureType": "link", "owner": "p1", "store": {"energy": energy}, "cooldown": cooldown}))
}

fn cooldown(e: &EmbedEngine, id: &str) -> Value {
    e.get_object(id).unwrap()["cooldown"].clone()
}

#[test]
fn settlement_fault_survives_recovery_and_clears_only_on_actual_settlement() -> anyhow::Result<()> {
    for binary in [false, true] {
        let outer = package(
            "api.handlers.wrap('builtin:link.transfer',{id:'outer'},(ctx,next)=>{try{next()}catch(e){} throw Error('outer');});",
        );
        let inner = package(
            "api.handlers.replace('builtin:link.transfer',{id:'inner'},ctx=>{ctx.state={dirty:true};throw Error('处理器 alpha:handler.outer: fake');});",
        );
        let mut e = engine(&[("alpha", outer), ("beta", inner)], binary)?;
        let source = link(&mut e, 20, 100, 0)?;
        let target = link(&mut e, 21, 0, 0)?;
        let code = format!(
            "Memory.dirty=true;Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25)"
        );
        assert!(e.run_player("p1", &code).is_err());
        let record = e.last_mod_settlement_fault().expect("保留槽故障");
        assert_eq!(record.tick, 0);
        assert_eq!(record.room, "W1N1");
        assert_eq!(record.target, source);
        let fault = record.error.fault.as_ref().unwrap();
        assert_eq!(fault.slot.as_deref(), Some("builtin:link.transfer"));
        assert_eq!(
            fault.binding.as_ref().unwrap().as_str(),
            "beta:handler.inner"
        );
        assert_eq!(fault.kind, kk_mod_host::ModFaultKind::Handler);
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 100);
        assert_eq!(e.engine_mod_state("beta"), Value::Null);
        e.fail_next_mod_rebuild_for_test()?;
        assert!(
            e.run_player("p1", "throw Error('不得重新运行玩家')")
                .is_err()
        );
        assert_eq!(e.last_mod_settlement_fault(), Some(record.clone()));
        e.rebuild_mod_realm()?;
        assert_eq!(e.last_mod_settlement_fault(), Some(record.clone()));
        // 未运行结算的拒绝保留上一条；实际结算的非 Mod 错误不能残留旧归因。
        e.fail_next_settle_for_test();
        assert!(e.run_player("p1", "").is_err());
        assert_eq!(e.last_mod_settlement_fault(), Some(record));
        e.fail_next_settle_stage_for_test(kk_engine::TickStage::Prepare);
        assert!(e.run_player("p1", "").is_err());
        assert!(e.last_mod_settlement_fault().is_none());
        e.discard_pending_round();
        e.tick(1)?;
        assert!(e.last_mod_settlement_fault().is_none());
        assert!(!e.needs_mod_recovery());
        assert!(e.last_mod_settlement_fault().is_none());
        assert_eq!(e.get_game_time(), 1);
    }
    Ok(())
}

#[test]
fn nested_js_wraps_flush_refresh_and_run_after_once() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.before('builtin:link.tick', {id:'pre'}, ctx => { ctx.state = {trace:['before'], count:0}; });
api.handlers.wrap('builtin:link.tick', {id:'a-outer'}, (ctx,next) => {
  const state = ctx.state; const fields = ctx.target.fields;
  state.trace.push('outer-enter'); state.count++;
  fields.cooldown = 8;
  next();
  if (fields.cooldown !== 5 || state.count !== 3) throw Error('外层旧引用未刷新');
  state.trace.push('outer-exit'); state.count++; fields.cooldown = 10;
});
api.handlers.wrap('builtin:link.tick', {id:'b-inner'}, (ctx,next) => {
  const fields = ctx.target.fields;
  if (ctx.state.count !== 1 || fields.cooldown !== 8) throw Error('外层写入未先提交候选');
  ctx.state.trace.push('inner-enter'); ctx.state.count++; fields.cooldown = 6;
  next();
  if (fields.cooldown !== 5) throw Error('native 修改未刷新');
  ctx.state.trace.push('inner-exit'); ctx.state.count++;
});
api.handlers.after('builtin:link.tick', {id:'post'}, ctx => {
  if (ctx.target.fields.cooldown !== 10 || ctx.state.count !== 4) throw Error('after 时点错误');
  ctx.state.trace.push('after'); ctx.state.count++;
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        for _ in 0..2 {
            e.tick(1)?;
            assert_eq!(cooldown(&e, &id), json!(10));
            assert_eq!(
                e.engine_mod_state("chain"),
                json!({"trace":["before","outer-enter","inner-enter","inner-exit","outer-exit","after"],"count":5})
            );
        }
    }
    Ok(())
}

#[test]
fn link_transfer_uses_authoritative_args_and_refreshes_nested_store_reference() -> anyhow::Result<()>
{
    let source = package(
        r#"
api.handlers.wrap('builtin:link.transfer', {id:'audit'}, (ctx,next) => {
  const store = ctx.target.fields.store;
  const amount = ctx.args.amount;
  ctx.args.amount = 999; ctx.target.id = '000000000000000000000000';
  next();
  if (store.energy !== 100 - amount) throw Error('native 使用了伪造参数或未刷新 store');
  ctx.state = {amount, energy:store.energy};
});
"#,
    );
    let mut e = engine(&[("chain", source)], false)?;
    let source = link(&mut e, 20, 100, 0)?;
    let target = link(&mut e, 21, 0, 0)?;
    let run = e.run_player(
        "p1",
        &format!(
            "Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25)"
        ),
    )?;
    assert_eq!(run.expr["value"], json!(0));
    assert_eq!(
        e.engine_mod_state("chain"),
        json!({"amount":25,"energy":75})
    );
    assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], json!(24));
    Ok(())
}

#[test]
fn js_replace_is_wrapped_and_does_not_run_native_center() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.wrap('builtin:link.tick', {id:'audit'}, (ctx,next) => {
  next(); if (ctx.target.fields.cooldown !== 3) throw Error('replace 中心未执行');
  ctx.state.trace.push('wrap-exit');
});
api.handlers.replace('builtin:link.tick', {id:'replacement'}, ctx => {
  ctx.target.fields.cooldown = 3; ctx.state = {trace:['replace']};
});
api.handlers.after('builtin:link.tick', {id:'post'}, ctx => { ctx.state.trace.push('after'); });
"#,
    );
    let mut e = engine(&[("chain", source)], false)?;
    let id = link(&mut e, 20, 0, 100)?;
    e.tick(1)?;
    assert_eq!(cooldown(&e, &id), json!(3));
    assert_eq!(
        e.engine_mod_state("chain")["trace"],
        json!(["replace", "wrap-exit", "after"])
    );
    Ok(())
}

const COUNTER: &str = r#"
const counter = api.objects.define('chain:counter', {
  version:1, playerClass:'Counter', visibility:'server-only', ownerField:'owner', positionFields:['x','y'],
  fields:{owner:{type:'string',maxBytes:128},x:{type:'u32',min:0,max:49},y:{type:'u32',min:0,max:49},value:{type:'i32',default:0}}
});
api.intents.define('chain:increment', {
  scope:'object', target:counter, args:{type:'record',fields:{amount:{type:'i32',min:1,max:10}}},
  process(ctx) { ctx.target.fields.value += ctx.args.amount; ctx.state.trace.push('process'); }
});
"#;

fn counter(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_mod_object(
        "W1N1",
        "chain:counter",
        &json!({"owner":"p1","x":10,"y":12}),
    )
}
fn increment(e: &mut EmbedEngine, id: &str) -> anyhow::Result<()> {
    e.run_player(
        "p1",
        &format!(
            "__kkEmitExtensionIntent('chain:increment','{id}',{{amount:3}});Memory.chainAttempt=1"
        ),
    )?;
    Ok(())
}

#[test]
fn mod_process_chain_shares_current_candidate_and_preserves_after_order() -> anyhow::Result<()> {
    let source = package(&format!(
        r#"{COUNTER}
api.handlers.before('chain:increment.slot',{{id:'pre'}},ctx=>{{ctx.state={{trace:['before']}};}});
api.handlers.wrap('chain:increment.slot',{{id:'a-outer'}},(ctx,next)=>{{
  const fields=ctx.target.fields; const state=ctx.state;
  fields.value=2; state.trace.push('outer-enter'); next();
  if(fields.value!==6) throw Error('Mod 字段旧引用未刷新');
  fields.value++; state.trace.push('outer-exit');
}});
api.handlers.wrap('chain:increment.slot',{{id:'b-inner'}},(ctx,next)=>{{
  ctx.state.trace.push('inner-enter'); ctx.target.fields.value++; next(); ctx.state.trace.push('inner-exit');
}});
api.handlers.after('chain:increment.slot',{{id:'post'}},ctx=>{{ctx.state.trace.push('after');}});
"#
    ));
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = counter(&mut e)?;
        increment(&mut e, &id)?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], json!(7));
        assert_eq!(
            e.engine_mod_state("chain")["trace"],
            json!([
                "before",
                "outer-enter",
                "inner-enter",
                "process",
                "inner-exit",
                "outer-exit",
                "after"
            ])
        );
    }
    Ok(())
}

#[test]
fn outer_or_inner_cancellation_skips_center_and_after_still_runs_once() -> anyhow::Result<()> {
    for outer_calls in [false, true] {
        let source = package(&format!(
            r#"
api.handlers.before('builtin:link.tick',{{id:'pre'}},ctx=>{{ctx.state={{trace:['before']}};}});
api.handlers.wrap('builtin:link.tick',{{id:'a-outer'}},(ctx,next)=>{{
  ctx.state.trace.push('outer'); {}
}});
api.handlers.wrap('builtin:link.tick',{{id:'b-inner'}},ctx=>{{ctx.state.trace.push('inner-cancel');}});
api.handlers.after('builtin:link.tick',{{id:'post'}},ctx=>{{ctx.state.trace.push('after');}});
"#,
            if outer_calls { "next();" } else { "" }
        ));
        let mut e = engine(&[("chain", source)], false)?;
        let id = link(&mut e, 20, 0, 100)?;
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), json!(100));
        assert_eq!(
            e.engine_mod_state("chain")["trace"],
            if outer_calls {
                json!(["before", "outer", "inner-cancel", "after"])
            } else {
                json!(["before", "outer", "after"])
            }
        );
    }
    Ok(())
}

#[test]
fn caught_next_faults_and_thenables_abort_native_writes_and_retry() -> anyhow::Result<()> {
    for body in [
        "next(); try { next(); } catch(e) {}",
        "try { next(); } catch(e) {}",
        "next(); throw Error('outer-after-next');",
        "next(); return {then(){}};",
        "try { ctx.target.fields.cooldown = -1; next(); } catch(e) {}",
    ] {
        let inner = if body == "try { next(); } catch(e) {}" {
            "api.handlers.wrap('builtin:link.tick',{id:'b-inner'},(ctx,next)=>{next();throw Error('inner-after-next');});"
        } else {
            ""
        };
        let source = package(&format!(
            r#"
api.handlers.before('builtin:link.tick',{{id:'pre'}},ctx=>{{ctx.state={{attempt:1}};}});
api.handlers.wrap('builtin:link.tick',{{id:'a-outer'}},(ctx,next)=>{{{body}}});
{inner}
api.handlers.after('builtin:link.tick',{{id:'post'}},ctx=>{{ctx.target.fields.cooldown=999;}});
"#
        ));
        let mut e = engine(&[("chain", source)], false)?;
        let id = link(&mut e, 20, 0, 100)?;
        let generation = e.engine_generation();
        for _ in 0..2 {
            let error = e.tick(1).expect_err("捕获错误不能使有故障的候选成功");
            assert!(!error.to_string().is_empty());
            assert_eq!(e.get_game_time(), 0);
            assert_eq!(e.engine_generation(), generation);
            assert_eq!(cooldown(&e, &id), json!(100));
            assert_eq!(e.engine_mod_state("chain"), Value::Null);
        }
    }
    Ok(())
}

#[test]
fn stale_next_cannot_consume_a_later_calls_native_capability() -> anyhow::Result<()> {
    let source = r#"
let saved;
export default function register(api) {
  api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
    if (saved) { try { saved(); } catch(e) {} }
    saved=next; next();
  });
}
"#;
    let mut e = engine(&[("chain", source.into())], false)?;
    let id = link(&mut e, 20, 0, 100)?;
    e.tick(2)?;
    assert_eq!(cooldown(&e, &id), json!(98));
    Ok(())
}

#[test]
fn different_mod_namespaces_share_target_without_overwriting_each_others_state()
-> anyhow::Result<()> {
    let a = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  ctx.state={count:(ctx.state?.count||0)+1}; ctx.target.fields.cooldown=8;
  next(); if(ctx.state.count!==1) throw Error('命名空间状态串写');
  if(ctx.target.fields.cooldown!==5) throw Error('跨 Mod 目标未刷新');
});
"#,
    );
    let b = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  ctx.state={count:(ctx.state?.count||0)+3}; ctx.target.fields.cooldown=6; next();
});
"#,
    );
    let mut e = engine(&[("a", a), ("b", b)], false)?;
    let id = link(&mut e, 20, 0, 100)?;
    e.tick(1)?;
    assert_eq!(cooldown(&e, &id), json!(5));
    assert_eq!(e.engine_mod_state("a"), json!({"count":1}));
    assert_eq!(e.engine_mod_state("b"), json!({"count":3}));
    Ok(())
}

#[test]
fn shared_object_and_array_references_split_without_losing_inner_updates() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.before('builtin:link.tick',{id:'pre'},ctx=>{
  ctx.state={a:{n:1},b:{n:1},c:[{n:1}],d:[{n:1}]};
});
api.handlers.wrap('builtin:link.tick',{id:'a-outer'},(ctx,next)=>{
  const object=ctx.state.a, array=ctx.state.c;
  ctx.state.b=object; ctx.state.d=array;
  next();
  if(object.n!==2 || array[0].n!==3) throw Error('首次复用的旧引用未刷新');
  if(ctx.state.a===ctx.state.b || ctx.state.c===ctx.state.d) throw Error('候选独立路径仍共用容器');
});
api.handlers.wrap('builtin:link.tick',{id:'b-inner'},(ctx,next)=>{
  ctx.state.a.n=2; ctx.state.c[0].n=3; next();
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        for tick in 1..=2 {
            e.tick(1)?;
            assert_eq!(cooldown(&e, &id), json!(100 - tick));
            assert_eq!(
                e.engine_mod_state("chain"),
                json!({"a":{"n":2},"b":{"n":1},"c":[{"n":3}],"d":[{"n":1}]})
            );
        }
    }
    Ok(())
}

#[test]
fn refreshing_new_data_keys_does_not_mutate_inherited_objects() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'a-outer'},(ctx,next)=>{
  if(({}).chainPolluted!==undefined) throw Error('上轮污染了对象原型');
  const inherited={n:1};
  INITIALIZE_STATE
  next();
  if(({}).chainPolluted!==undefined || prototype.chainPolluted!==undefined || inherited.n!==1) throw Error('刷新修改了继承对象');
  if(prototype!==Object.prototype && prototype.inherited!==inherited) throw Error('刷新删除了原型成员');
  if(!Object.hasOwn(ctx.state,'__proto__') || !Object.hasOwn(ctx.state,'inherited')) throw Error('新增键没有落为自有属性');
  ctx.state.observedPollution=false;
});
api.handlers.wrap('builtin:link.tick',{id:'b-inner'},(ctx,next)=>{
  ctx.state=JSON.parse('{"__proto__":{"chainPolluted":123},"inherited":{"n":2}}');
  next();
});
"#,
    );
    for initializer in [
        "const prototype=Object.prototype; ctx.state={};",
        "const prototype={inherited}; ctx.state=Object.create(prototype);",
    ] {
        let source = source.replace("INITIALIZE_STATE", initializer);
        for binary in [false, true] {
            let mut e = engine(&[("chain", source.clone())], binary)?;
            link(&mut e, 20, 0, 100)?;
            for _ in 0..2 {
                e.tick(1)?;
                assert_eq!(
                    e.engine_mod_state("chain"),
                    json!({"__proto__":{"chainPolluted":123},"inherited":{"n":2},"observedPollution":false})
                );
            }
        }
    }
    Ok(())
}

#[test]
fn next_preserves_local_context_helpers_without_serializing_them() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  const local={ctx};
  const fields=ctx.target.fields;
  ctx.helper=()=>42; ctx.local=local;
  ctx.target.helper=()=>43; ctx.target.local=local;
  next();
  if(ctx.local!==local || ctx.target.local!==local || local.ctx!==ctx) throw Error('本地上下文引用被删除');
  if(fields.cooldown!==99) throw Error('保留本地成员时遗漏权威字段刷新');
  ctx.state={n:ctx.helper(),targetN:ctx.target.helper()};
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), json!(99));
        assert_eq!(e.engine_mod_state("chain"), json!({"n":42,"targetN":43}));
    }
    Ok(())
}

#[test]
fn aliases_between_state_and_target_do_not_overwrite_native_fields() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  ctx.state=ctx.target.fields;
  next();
  if(ctx.state===ctx.target.fields) throw Error('state 与目标候选路径未拆开');
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), json!(99));
        assert_eq!(e.engine_mod_state("chain")["cooldown"], json!(100));
    }
    Ok(())
}

#[test]
fn namespace_writeback_preserves_null_scalars_and_nested_json() -> anyhow::Result<()> {
    let values = r#"[null,true,17.5,"文本",[1,{"nested":[false,null]}],{"__proto__":{"n":2},"data":["x",3]}]"#;
    let source = format!(
        r#"
const values=JSON.parse({encoded});
let index=0;
export default function register(api) {{
  api.handlers.before('builtin:link.tick',{{id:'pre'}},ctx=>{{ctx.state=values[index++];}});
  api.handlers.after('builtin:link.tick',{{id:'post'}},ctx=>{{
    if(JSON.stringify(ctx.state)!==JSON.stringify(values[index-1])) throw Error('namespace 状态改变');
  }});
}}
"#,
        encoded = json!(values),
    );
    let expected: Vec<Value> = serde_json::from_str(values)?;
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        for (index, state) in expected.iter().enumerate() {
            e.tick(1)?;
            assert_eq!(&e.engine_mod_state("chain"), state);
            assert_eq!(cooldown(&e, &id), json!(99 - index));
        }
    }
    Ok(())
}

#[test]
fn fixed_context_envelope_keeps_key_order_and_refreshes_authoritative_metadata()
-> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.wrap('builtin:link.tick',{id:'audit'},(ctx,next)=>{
  if(Object.keys(ctx).join(',')!=='args,room,state,target,tick') throw Error('上下文键序改变');
  if(Object.keys(ctx.target).join(',')!=='fields,id') throw Error('目标键序改变');
  if(Object.getPrototypeOf(ctx)!==null || Object.getPrototypeOf(ctx.target)!==null) throw Error('封套原型改变');
  const tick=ctx.tick, room=ctx.room, id=ctx.target.id, target=ctx.target;
  if(typeof tick!=='number' || typeof id!=='string' || !id || room!=='W1N1' || ctx.args!==null) throw Error('封套成员改变');
  ctx.tick=-1; ctx.room='fake'; ctx.target.id='fake'; ctx.args={local:1};
  next();
  if(ctx.tick!==tick || ctx.room!==room || ctx.target.id!==id || ctx.args!==null || ctx.target!==target) throw Error('权威成员未刷新');
  if(Object.keys(ctx).join(',')!=='args,room,state,target,tick' || Object.keys(ctx.target).join(',')!=='fields,id') throw Error('刷新键序改变');
  ctx.state={verified:true};
});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("chain", source.clone())], binary)?;
        let id = link(&mut e, 20, 0, 100)?;
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), json!(99));
        assert_eq!(e.engine_mod_state("chain"), json!({"verified":true}));
    }
    Ok(())
}

#[test]
fn caught_write_conversion_fault_rolls_back_getter_native_effects_and_retry() -> anyhow::Result<()>
{
    for payload in [
        "{get state(){Deno.core.ops.op_mod_chain_native('1');throw Error('conversion-failure');}}",
        "{state:{n:1},get extra(){Deno.core.ops.op_mod_chain_native('1');return Symbol('bad');}}",
        "{state:{n:1},target:{fields:{cooldown:5},get extra(){Deno.core.ops.op_mod_chain_native('1');return Symbol('bad');}}}",
        "{get state(){Deno.core.ops.op_mod_chain_native('1');return Symbol('bad');}}",
    ] {
        let source = package(&format!(
            r#"
api.handlers.before('builtin:link.tick',{{id:'audit'}},ctx=>{{
  try {{ Deno.core.ops.op_mod_chain_write('1','chain:handler.audit',{payload}); }} catch(e) {{}}
}});
api.handlers.after('builtin:link.tick',{{id:'after'}},ctx=>{{ctx.target.fields.cooldown=999;}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("chain", source.clone())], binary)?;
            let id = link(&mut e, 20, 0, 100)?;
            let generation = e.engine_generation();
            for _ in 0..2 {
                let error = e
                    .tick(1)
                    .expect_err("吞掉转换异常不能提交 getter 的 native 副作用");
                assert!(error.to_string().contains("转换失败"), "{error}");
                assert_eq!(e.get_game_time(), 0);
                assert_eq!(e.engine_generation(), generation);
                assert_eq!(cooldown(&e, &id), json!(100));
                assert_eq!(e.engine_mod_state("chain"), Value::Null);
            }
        }
    }
    Ok(())
}

// 三个同对象动作依稳定键序执行；中间动作拒绝不能丢失前一个动作的结果。
fn decline_package(rejection: &str) -> String {
    format!(
        r#"
let calls = 0;
export default function register(api) {{
  const counter = api.objects.define('a:counter', {{version:1,playerClass:'Counter',visibility:'server-only',ownerField:'owner',positionFields:['x','y'],fields:{{
    owner:{{type:'string',maxBytes:128}},x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}},value:{{type:'i32',default:0}}
  }}}});
  for (const name of ['a-accept','b-test','c-accept']) {{
    api.intents.define('a:'+name,{{scope:'object',target:counter,args:{{type:'record',fields:{{}}}},process(ctx){{
      calls++;
      ctx.target.fields.value++;
      ctx.state={{count:(ctx.state?.count||0)+1,calls}};
      if(name==='b-test') {{ {rejection} }}
    }}}});
  }}
  api.handlers.before('a:b-test.slot',{{id:'pre'}},ctx=>{{ctx.target.fields.x=21;ctx.state={{count:100}};}});
  api.handlers.wrap('a:b-test.slot',{{id:'outer'}},(ctx,next)=>{{
    const fields=ctx.target.fields;
    try {{ next(); }} catch(e) {{}}
    if (fields.value!==7) throw Error('next 后字段未同步');
    ctx.state={{count:999}};
  }});
  api.handlers.after('a:b-test.slot',{{id:'after'}},ctx=>{{ctx.state={{count:888}};}});
}}
"#
    )
}

#[test]
fn declined_intent_rolls_back_only_its_fields_and_all_namespace_states() -> anyhow::Result<()> {
    let source = decline_package("return api.decline('full');");
    let b = "api.handlers.before('a:b-test.slot',{id:'pre'},ctx=>{ctx.target.fields.value+=5;ctx.target.fields.y=23;ctx.state={count:500};});";
    // wrap 捕获内层拒绝时不会刷新；不得把捕获后的代码错误误当成拒绝。
    let source = source.replace(
        "if (fields.value!==7) throw Error('next 后字段未同步');",
        "",
    );
    for (binary, initialized) in [(false, false), (true, false), (false, true), (true, true)] {
        let b = if initialized {
            package(&format!(
                "{b} api.handlers.before('a:a-accept.slot',{{id:'initialize'}},ctx=>{{ctx.state={{count:(ctx.state?.count||0)+1}};}});"
            ))
        } else {
            package(b)
        };
        let mut e = engine(&[("a", source.clone()), ("b", b.clone())], binary)?;
        let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
        let result = e.run_player("p1", &format!("Memory.committed=true;for(const key of ['a:a-accept','a:b-test','a:c-accept']) __kkEmitExtensionIntent(key,'{id}',{{}}); 'ok'"))?;
        assert_eq!(result.expr["value"], json!("ok"));
        assert_eq!(e.get_game_time(), 1);
        assert!(!e.needs_mod_recovery());
        assert!(e.last_mod_settlement_fault().is_none());
        let object = e.get_object(&id).unwrap();
        assert_eq!(
            object["fields"],
            json!({"owner":"p1","x":10,"y":12,"value":2})
        );
        assert_eq!(object["pos"], json!({"x":10,"y":12,"roomName":"W1N1"}));
        assert_eq!(
            e.engine_mod_state("a"),
            json!({"count":2,"calls":3}),
            "正常拒绝不重建 VM，全局调用计数保留"
        );
        assert_eq!(
            e.engine_mod_state("b"),
            if initialized {
                json!({"count":1})
            } else {
                Value::Null
            },
            "拒绝须保留动作前的 namespace state，也不能新增空条目"
        );
        // Memory 与 CPU 候选照常发布；下一轮正常动作读取前一轮已提交结果。
        let next = e.run_player("p1", &format!("if(!Memory.committed) throw Error('Memory 未发布');__kkEmitExtensionIntent('a:a-accept','{id}',{{}})"))?;
        assert!(next.expr.get("error").is_none(), "{:?}", next.expr);
        assert_eq!(e.engine_mod_state("a"), json!({"count":3,"calls":4}));
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], json!(3));
    }
    Ok(())
}

#[test]
fn decline_at_each_chain_position_discards_the_entire_action() -> anyhow::Result<()> {
    for position in [
        "before",
        "wrap-before",
        "process",
        "replace",
        "wrap-after",
        "after",
    ] {
        let mut source = decline_package(if position == "process" {
            "return api.decline('full');"
        } else {
            ""
        });
        source = source.replace(
            "if (fields.value!==7) throw Error('next 后字段未同步');",
            "if(fields.value!==1) throw Error('next 后字段未同步');",
        );
        match position {
            "replace" => source = source.replace("api.handlers.before('a:b-test.slot'", "api.handlers.replace('a:b-test.slot',{id:'replacement'},ctx=>{ctx.state={count:123};ctx.target.fields.value=9;return api.decline('full')});api.handlers.before('a:b-test.slot'"),
            "before" => {
                source = source.replace(
                    "ctx.state={count:100};",
                    "ctx.state={count:100};return api.decline('full');",
                )
            }
            "wrap-before" => {
                source = source.replace(
                    "const fields=ctx.target.fields;",
                    "return api.decline('full');const fields=ctx.target.fields;",
                )
            }
            "wrap-after" => {
                source = source.replace(
                    "ctx.state={count:999};",
                    "ctx.state={count:999};return api.decline('full');",
                )
            }
            "after" => {
                source = source.replace(
                    "ctx.state={count:888};",
                    "ctx.state={count:888};return api.decline('full');",
                )
            }
            _ => {}
        }
        // 内层正常拒绝被 wrap 捕获时不刷新旧引用，也不需要 Mod 主动退出。
        if matches!(position, "process" | "replace") {
            source = source.replace("if(fields.value!==1) throw Error('next 后字段未同步');", "");
        }
        for binary in [false, true] {
            let mut e = engine(&[("a", source.clone())], binary)?;
            let id =
                e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
            e.run_player(
                "p1",
                &format!("__kkEmitExtensionIntent('a:b-test','{id}',{{}})"),
            )?;
            assert_eq!(e.get_game_time(), 1, "{position}");
            assert!(!e.needs_mod_recovery(), "{position}");
            assert_eq!(e.engine_mod_state("a"), Value::Null, "{position}");
            assert_eq!(
                e.get_object(&id).unwrap()["fields"]["value"],
                json!(0),
                "{position}"
            );
            assert_eq!(
                e.get_object(&id).unwrap()["pos"]["x"],
                json!(10),
                "{position}"
            );
        }
    }
    Ok(())
}

#[test]
fn decline_cannot_downgrade_a_later_exception_or_an_earlier_conversion_fault() -> anyhow::Result<()>
{
    for source in [
        decline_package("return api.decline('full');").replace(
            "if (fields.value!==7) throw Error('next 后字段未同步');",
            "throw Error('拒绝后的异常');",
        ),
        decline_package(
            "try { Deno.core.ops.op_mod_chain_write('2','a:b-test.process',{state:{bad:Symbol('bad')}}); }catch(e){} return api.decline('full');",
        ),
    ] {
        for binary in [false, true] {
            let mut e = engine(&[("a", source.clone())], binary)?;
            let id =
                e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
            let old = e.get_object(&id).unwrap();
            let error = e.run_player("p1", &format!("Memory.committed=true;for(const key of ['a:a-accept','a:b-test']) __kkEmitExtensionIntent(key,'{id}',{{}})" )).unwrap_err();
            assert!(
                error.to_string().contains("异常") || error.to_string().contains("转换失败"),
                "{error}"
            );
            assert_eq!(e.get_game_time(), 0);
            assert_eq!(e.get_object(&id).unwrap(), old);
            assert_eq!(e.engine_mod_state("a"), Value::Null);
            assert!(e.needs_mod_recovery());
        }
    }
    Ok(())
}

#[test]
fn builtin_slots_reject_decline_until_their_native_writes_can_be_restored() -> anyhow::Result<()> {
    for slot in ["builtin:link.tick", "builtin:link.transfer"] {
        let source = package(&format!(
            "api.handlers.before('{slot}',{{id:'reject'}},()=>api.decline('full'));"
        ));
        let mut e = engine(&[("a", source)], false)?;
        let initial_cooldown = if slot.ends_with("tick") { 10 } else { 0 };
        let id = link(&mut e, 20, 100, initial_cooldown)?;
        let error = if slot.ends_with("tick") {
            e.tick(1).unwrap_err()
        } else {
            let target = link(&mut e, 21, 0, 0)?;
            e.run_player(
                "p1",
                &format!(
                    "Game.getObjectById('{id}').transferEnergy(Game.getObjectById('{target}'),25)"
                ),
            )
            .unwrap_err()
        };
        assert!(error.to_string().contains("尚不支持正常拒绝"), "{error}");
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(cooldown(&e, &id), json!(initial_cooldown));
        assert!(e.needs_mod_recovery());
    }
    Ok(())
}

#[test]
fn disabling_faulting_replace_retries_cached_players_with_native_and_healthy_hooks()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let bad = package(
            "api.handlers.replace('builtin:link.transfer',{id:'bad'},ctx=>{ctx.state={dirty:true};throw Error('bad');});",
        );
        let healthy = package(
            "api.handlers.wrap('builtin:link.transfer',{id:'audit'},(ctx,next)=>{next();ctx.state={energy:ctx.target.fields.store.energy};});",
        );
        let mut e = engine(&[("bad", bad), ("healthy", healthy)], binary)?;
        let source = link(&mut e, 20, 100, 0)?;
        let target = link(&mut e, 21, 0, 0)?;
        let generation = e.engine_generation();
        let code = format!(
            "globalThis.executions=(globalThis.executions||0)+1;Memory.executions=globalThis.executions;Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25);'cached'"
        );
        assert!(e.run_player("p1", &code).is_err());
        let fault = e.last_mod_settlement_fault().unwrap();
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.engine_generation(), generation);
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 100);
        assert_eq!(e.engine_mod_state("healthy"), Value::Null);
        assert!(e.disable_mod("missing").is_err());
        assert!(e.disabled_mods().is_empty());
        assert_eq!(
            e.disable_mod("bad")?,
            std::collections::BTreeSet::from(["bad".into()])
        );
        e.fail_next_mod_rebuild_for_test()?;
        assert!(e.run_player("p1", "throw Error('不得重跑')").is_err());
        assert_eq!(e.last_mod_settlement_fault(), Some(fault));
        assert_eq!(e.get_game_time(), 0);
        let retried = e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(retried.expr["value"], "cached");
        assert_eq!(e.get_game_time(), 1);
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 75);
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 24);
        assert_eq!(e.engine_mod_state("healthy"), json!({"energy":75}));
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
        assert!(!e.needs_mod_recovery());
        assert!(e.disable_mod("bad")?.is_empty());
        assert!(!e.needs_mod_recovery(), "重复停用不能重置健康 realm");
        let next = e.run_player("p1", "if(globalThis.executions!==1||Memory.executions!==1)throw Error('玩家重跑或 Memory 未提交');'ok'")?;
        assert_eq!(next.expr["value"], "ok");
    }
    Ok(())
}

#[test]
fn disabling_all_hook_kinds_preserves_healthy_chain() -> anyhow::Result<()> {
    for binary in [false, true] {
        let bad = package(
            "for(const kind of ['before','wrap','replace','after'])api.handlers[kind]('builtin:link.tick',{id:kind},()=>{throw Error('停用钩子不得执行')});",
        );
        let healthy = package(
            "api.handlers.wrap('builtin:link.tick',{id:'wrap'},(ctx,next)=>{next();ctx.state={cooldown:ctx.target.fields.cooldown};});",
        );
        let mut e = engine(&[("bad", bad), ("healthy", healthy)], binary)?;
        let id = link(&mut e, 20, 0, 10)?;
        e.disable_mod("bad")?;
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), 9);
        assert_eq!(e.engine_mod_state("healthy"), json!({"cooldown":9}));
        assert_eq!(e.engine_mod_state("bad"), Value::Null);
    }
    Ok(())
}

#[test]
fn disabling_own_slot_drops_hooks_and_actions_but_preserves_objects_state_and_dependencies()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let own = decline_package("").replace(
            "if (fields.value!==7) throw Error('next 后字段未同步');",
            "",
        );
        let dependent = package(
            "api.handlers.before('builtin:link.tick',{id:'dependent'},()=>{throw Error('依赖者不得执行')});",
        );
        let transitive = package(
            "api.handlers.after('builtin:link.tick',{id:'transitive'},()=>{throw Error('传递依赖者不得执行')});",
        );
        let healthy = package(
            "api.handlers.before('a:b-test.slot',{id:'own'},()=>{throw Error('所属槽停用后健康钩子也不得执行')});api.handlers.after('builtin:link.tick',{id:'healthy'},ctx=>{ctx.state={count:(ctx.state?.count||0)+1};});",
        );
        let mut e = engine_with_dependencies(
            &[
                ("a", own),
                ("dependent", dependent),
                ("transitive", transitive),
                ("healthy", healthy),
            ],
            binary,
            &[("dependent", "a"), ("transitive", "dependent")],
        )?;
        let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
        e.run_player(
            "p1",
            &format!("__kkEmitExtensionIntent('a:a-accept','{id}',{{}})"),
        )?;
        let before = e.get_object(&id).unwrap();
        let state = e.engine_mod_state("a");
        let link_id = link(&mut e, 20, 0, 10)?;
        let added = e.disable_mod("a")?;
        assert_eq!(
            added,
            std::collections::BTreeSet::from(["a".into(), "dependent".into(), "transitive".into()])
        );
        let result = e.run_player("p1", &format!("Memory.disabled=true;for(const key of ['a:a-accept','a:b-test','a:c-accept'])__kkEmitExtensionIntent(key,'{id}',{{}});'ok'"))?;
        assert_eq!(result.expr["value"], "ok");
        assert_eq!(e.get_object(&id).unwrap(), before);
        assert_eq!(e.engine_mod_state("a"), state);
        assert_eq!(e.engine_mod_state("healthy"), json!({"count":1}));
        assert_eq!(cooldown(&e, &link_id), 9);
        assert_eq!(e.get_game_time(), 2);
        assert!(e.last_mod_settlement_fault().is_none());
        // 目录仍有效：旧对象继续投影，原 schema 仍能用于新对象。
        e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":11,"y":12}))?;
        let result = e.run_player("p1", "Memory.disabled")?;
        assert_eq!(result.expr["value"], true);
    }
    Ok(())
}

#[test]
fn disabled_replace_on_enabled_own_slot_restores_original_process() -> anyhow::Result<()> {
    for binary in [false, true] {
        let own = decline_package("").replace(
            "if (fields.value!==7) throw Error('next 后字段未同步');",
            "",
        );
        let bad = package(
            "api.handlers.replace('a:a-accept.slot',{id:'replacement'},()=>{throw Error('bad')});",
        );
        let mut e = engine(&[("a", own), ("bad", bad)], binary)?;
        let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
        assert!(
            e.run_player(
                "p1",
                &format!("__kkEmitExtensionIntent('a:a-accept','{id}',{{}})")
            )
            .is_err()
        );
        e.disable_mod("bad")?;
        e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(e.get_object(&id).unwrap()["fields"]["value"], 1);
        assert_eq!(e.engine_mod_state("a"), json!({"count":1,"calls":1}));
        assert_eq!(e.get_game_time(), 1);
    }
    Ok(())
}

#[test]
fn disabled_bindings_are_removed_from_active_candidate_capabilities() -> anyhow::Result<()> {
    let bad = package("api.handlers.before('builtin:link.tick',{id:'removed'},()=>{});");
    let healthy = package(
        "api.handlers.before('builtin:link.tick',{id:'probe'},()=>{try{Deno.core.ops.op_mod_chain_read('1','bad:handler.removed')}catch(e){}});",
    );
    let mut e = engine(&[("bad", bad), ("healthy", healthy)], false)?;
    let id = link(&mut e, 20, 0, 10)?;
    e.disable_mod("bad")?;
    assert!(e.tick(1).is_err(), "被捕获的非法能力故障仍应阻止发布");
    assert_eq!(cooldown(&e, &id), 10);
    assert_eq!(e.get_game_time(), 0);
    let fault = e.last_mod_settlement_fault().unwrap().error.fault.unwrap();
    assert_eq!(fault.kind, kk_mod_host::ModFaultKind::Capability);
    assert!(fault.binding.is_none());
    Ok(())
}

#[path = "mod_chain_tests/auto_disable.rs"]
mod auto_disable;
#[path = "mod_chain_tests/store_rules.rs"]
mod store_rules;
