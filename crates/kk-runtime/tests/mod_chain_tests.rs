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
    let root = PackageRoot(std::env::temp_dir().join(format!(
        "kk-mod-chain-{}-{}",
        std::process::id(),
        NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
    )));
    for (id, source) in packages {
        let dir = root.0.join(id);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("mod.json"),
            json!({"id": id, "version": "1.0.0", "apiVersion": "^1.0.0"}).to_string(),
        )?;
        std::fs::write(dir.join("server.js"), source)?;
    }
    let packages = discover_packages(&root.0)?;
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(kkpv);
    e.create_shard_with_mods(
        &json!({"players": ["p1"], "rooms": [{"name": "W1N1", "rcl": 6, "owner": "p1"}]}),
        &packages,
        RegisterOptions::default(),
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
