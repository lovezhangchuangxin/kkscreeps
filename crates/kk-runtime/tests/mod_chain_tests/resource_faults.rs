//! 资源 op 故障不能被捕获或正常拒绝掩盖；能力不跨处理器或房间。
use super::resource_domain::{actor, amount, store};
use super::*;

#[test]
fn invalid_resource_operations_poison_candidate_even_when_caught_then_declined()
-> anyhow::Result<()> {
    for expression in [
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',NaN)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',-1)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',1.5)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',Number.MAX_SAFE_INTEGER+1)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',1,2)",
        "ctx.resources.transfer(ctx.target.id,ctx.target.id,'energy',1)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'unknown-resource',1)",
        "ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',{valueOf(){throw Error('不可转换')}})",
        "ctx.resources.read('kk000000000000ffff','energy')",
        "ctx.resources.read({toString(){throw Error('不可转换')}},'energy')",
    ] {
        let source = package(&format!(
            r#"
api.handlers.replace('builtin:creep.transfer',{{id:'bad'}},ctx=>{{
  ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',1);
  try{{{expression}}}catch(e){{}}
  return api.decline('masked');
}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let creep = actor(&mut e, "hauler", 20)?;
            let target = store(&mut e, "container", 10, json!({}))?;
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            let code = format!(
                "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;Game.creeps.hauler.transfer(Game.getObjectById('{target}'),'energy',5);globalThis.n"
            );
            assert!(e.run_player("p1", &code).is_err(), "{expression}");
            assert_eq!(e.world_snapshot(), before, "{expression}");
            assert_eq!(e.player_state("p1"), Some(&player));
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            let fault = e.last_mod_settlement_fault().unwrap().error.fault.unwrap();
            assert_eq!(fault.binding.unwrap().namespace(), "alpha");
            e.disable_mod("alpha")?;
            assert_eq!(
                e.run_player("p1", "throw Error('不得重跑')")?.expr["value"],
                1
            );
            assert_eq!(
                [amount(&e, &creep, "energy"), amount(&e, &target, "energy")],
                [15, 5]
            );
        }
    }
    Ok(())
}

#[test]
fn resource_capability_cannot_be_reused_after_its_handler_returns() -> anyhow::Result<()> {
    for retained in ["read", "transfer"] {
        let source = format!(
            r#"
let saved;
export default function register(api){{
  api.handlers.before('builtin:creep.transfer',{{id:'save'}},ctx=>{{saved=ctx.resources.{retained};}});
  api.handlers.after('builtin:creep.transfer',{{id:'use'}},ctx=>{{
    try{{saved({{}})}}catch(e){{}}
    return api.decline('masked');
  }});
}}
"#
        );
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            actor(&mut e, "hauler", 20)?;
            let target = store(&mut e, "container", 10, json!({}))?;
            let before = e.world_snapshot();
            let error = e
                .run_player(
                    "p1",
                    &format!(
                        "Game.creeps.hauler.transfer(Game.getObjectById('{target}'),'energy',5)"
                    ),
                )
                .unwrap_err();
            assert!(error.to_string().contains("所属处理器"), "{error}");
            assert_eq!(e.world_snapshot(), before);
        }
    }
    Ok(())
}

#[test]
fn unrelated_slots_and_other_rooms_have_no_resource_write_capability() -> anyhow::Result<()> {
    for body in [
        "api.handlers.before('builtin:link.tick',{id:'bad'},ctx=>ctx.resources.read(ctx.target.id,'energy'));".to_string(),
        "api.handlers.replace('builtin:creep.transfer',{id:'bad'},ctx=>ctx.resources.transfer(ctx.target.id,'kk0000000000000003','energy',1));".to_string(),
    ] {
        for binary in [false,true] {
            let mut e=engine_with_shard(&[("alpha",package(&body))],binary,&[],RegisterOptions::default(),&json!({
                "players":["p1"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6},{"name":"W2N1","controller":false}]
            }))?;
            if body.contains("link.tick") {
                link(&mut e,10,20,0)?;
            } else {
                // 创建额外房间及对象，固定并核对 ID 以验证全局 ID 不能跨房访问。
                actor(&mut e,"hauler",20)?;
                let other=e.place_object("W2N1","structure",&json!({"structureType":"container","pos":[10,10],"store":{}}))?;
                assert_eq!(other,"kk0000000000000003");
                store(&mut e,"container",10,json!({}))?;
            }
            let before=e.world_snapshot();
            let code=if body.contains("link.tick") { "0".into() } else {
                "Game.creeps.hauler.transfer(Game.getObjectById('kk0000000000000004'),'energy',1)".to_string()
            };
            assert!(e.run_player("p1",&code).is_err());
            assert_eq!(e.world_snapshot(),before);
        }
    }
    Ok(())
}

#[test]
fn resource_journal_limit_counts_distinct_endpoints_and_rolls_back_overflow() -> anyhow::Result<()>
{
    for count in [255, 256] {
        let source = package(&format!(
            r#"
api.handlers.replace('builtin:creep.transfer',{{id:'many'}},ctx=>{{
  ctx.resources.transfer(ctx.target.id,ctx.args.id,'energy',1);
  for(let n=3;n<3+{count};n++){{
    ctx.resources.transfer(ctx.target.id,'kk'+n.toString(16).padStart(16,'0'),'energy',0);
  }}
  ctx.state={{count:{count}}};
}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let creep = actor(&mut e, "hauler", 20)?;
            let mut target = String::new();
            for n in 0..count {
                let id = store(&mut e, "container", 10, json!({}))?;
                if n == 0 {
                    target = id;
                }
            }
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            let result = e.run_player("p1", &format!(
                "Memory.executed=true;Game.creeps.hauler.transfer(Game.getObjectById('{target}'),'energy',5)"
            ));
            if count == 255 {
                result?;
                assert_eq!(
                    [amount(&e, &creep, "energy"), amount(&e, &target, "energy")],
                    [19, 1]
                );
                assert_eq!(e.engine_mod_state("alpha"), json!({"count":255}));
            } else {
                let error = result.unwrap_err();
                assert!(error.to_string().contains("256 个对象"), "{error}");
                assert_eq!(e.world_snapshot(), before);
                assert_eq!(e.player_state("p1"), Some(&player));
                assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            }
        }
    }
    Ok(())
}
