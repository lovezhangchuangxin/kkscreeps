//! 领域写故障、能力寿命与缓存玩家重试。
use super::*;

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
