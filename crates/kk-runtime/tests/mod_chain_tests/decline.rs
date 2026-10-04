//! 自有动作整链拒绝、局部恢复及故障优先级。
use super::*;

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
fn builtin_tick_rejects_decline_without_a_local_restore_boundary() -> anyhow::Result<()> {
    for binary in [false, true] {
        let slot = "builtin:link.tick";
        let source = package(&format!(
            "api.handlers.before('{slot}',{{id:'reject'}},()=>api.decline('full'));"
        ));
        let mut e = engine(&[("a", source)], binary)?;
        let initial_cooldown = 10;
        let id = link(&mut e, 20, 100, initial_cooldown)?;
        let error = e.tick(1).unwrap_err();
        assert!(error.to_string().contains("尚不支持正常拒绝"), "{error}");
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(cooldown(&e, &id), json!(initial_cooldown));
        assert!(e.needs_mod_recovery());
    }
    Ok(())
}
