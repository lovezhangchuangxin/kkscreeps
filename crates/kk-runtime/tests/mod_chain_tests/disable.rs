//! 停用处理器、Mod 自有槽及能力范围。
use super::*;

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
        assert_eq!(e.get_game_time(), 1);
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
        assert_eq!(e.get_game_time(), 1);
        let retried = e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(retried.expr["value"], "cached");
        assert_eq!(e.get_game_time(), 2);
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
        assert_eq!(e.get_game_time(), 3);
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
        assert_eq!(e.get_game_time(), 2);
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
    assert_eq!(e.get_game_time(), 1);
    let fault = e.last_mod_settlement_fault().unwrap().error.fault.unwrap();
    assert_eq!(fault.kind, kk_mod_host::ModFaultKind::Capability);
    assert!(fault.binding.is_none());
    Ok(())
}
