//! 自动停用的结算边界、连续来源计数与缓存玩家重试。
use super::*;

fn auto_disable_options(threshold: u32) -> RegisterOptions {
    RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(threshold),
        ..Default::default()
    }
}

#[test]
fn automatic_disable_retains_streak_across_recovery_and_retries_cached_players()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let bad = package(
            "api.handlers.replace('builtin:link.transfer',{id:'bad'},()=>{throw Error('bad')});",
        );
        let healthy = package(
            "api.handlers.wrap('builtin:link.transfer',{id:'healthy'},(ctx,next)=>{next();if(ctx.args.amount===2)throw Error('healthy-fault');ctx.state={energy:ctx.target.fields.store.energy};});",
        );
        let dependent = package(
            "api.handlers.before('builtin:link.transfer',{id:'dependent'},ctx=>{ctx.state={dirty:true};});",
        );
        let mut e = engine_with_options(
            &[("bad", bad), ("healthy", healthy), ("dependent", dependent)],
            binary,
            &[("dependent", "bad")],
            auto_disable_options(2),
        )?;
        let source = link(&mut e, 20, 100, 0)?;
        let target = link(&mut e, 21, 0, 0)?;
        let player = e.player_state_for_test("p1").unwrap().clone();
        let before_bucket = e.player_bucket_for_test("p1")?;
        let generation = e.engine_generation();
        let code = format!(
            "globalThis.n=(globalThis.n||0)+1;Memory.n=globalThis.n;const start=Game.cpu.getUsed();while(Game.cpu.getUsed()-start<35){{}};Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),25);globalThis.n"
        );
        assert!(
            e.run_player("p1", &code)
                .unwrap_err()
                .to_string()
                .contains("bad")
        );
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
        assert!(e.last_mod_auto_disable().is_none());
        e.fail_next_mod_rebuild_for_test()?;
        assert!(e.run_player("p1", "throw Error('不得重跑')").is_err());
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
        e.fail_next_settle_for_test();
        assert!(e.run_player("p1", "throw Error('不得重跑')").is_err());
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
        assert!(e.disabled_mods().is_empty());
        let error = e.run_player("p1", "throw Error('不得重跑')").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("已自动停用 Mod [bad, dependent]"),
            "{error}"
        );
        assert_eq!(e.get_game_time(), 0);
        assert_eq!(e.engine_generation(), generation);
        assert_eq!(e.player_state_for_test("p1"), Some(&player));
        assert_eq!(e.player_bucket_for_test("p1")?, before_bucket);
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 100);
        assert_eq!(e.engine_mod_state("dependent"), Value::Null);
        let decision = e.last_mod_auto_disable().unwrap();
        assert_eq!(decision.consecutive_faults, 2);
        assert_eq!(decision.trigger.tick, 0);
        assert_eq!(decision.trigger.target, source);
        assert_eq!(
            decision
                .trigger
                .error
                .fault
                .as_ref()
                .unwrap()
                .binding
                .as_ref()
                .unwrap()
                .namespace(),
            "bad"
        );
        assert_eq!(
            decision.disabled,
            std::collections::BTreeSet::from(["bad".into(), "dependent".into()])
        );
        assert!(e.needs_mod_recovery());
        e.fail_next_mod_rebuild_for_test()?;
        assert!(e.run_player("p1", "throw Error('不得重跑')").is_err());
        assert_eq!(e.last_mod_auto_disable(), Some(decision.clone()));
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 2);
        let retried = e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(retried.expr["value"], 1);
        assert_eq!(e.get_game_time(), 1);
        assert!(e.mod_fault_streak().is_none());
        assert_eq!(e.get_object(&source).unwrap()["store"]["energy"], 75);
        assert_eq!(e.get_object(&target).unwrap()["store"]["energy"], 24);
        assert_eq!(e.engine_mod_state("healthy"), json!({"energy":75}));
        assert!(
            e.player_bucket_for_test("p1")? < before_bucket,
            "缓存玩家执行费用在成功发布时提交"
        );
        assert_eq!(e.last_mod_auto_disable(), Some(decision.clone()));
        let next = e.run_player("p1", &format!("if(globalThis.n!==1||Memory.n!==1)throw Error('缓存重跑');Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),2)"));
        assert!(next.is_err());
        assert_eq!(e.mod_fault_streak().unwrap().mod_id, "healthy");
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
        assert_eq!(e.last_mod_auto_disable(), Some(decision));
        e.teardown();
        assert!(e.last_mod_auto_disable().is_none());
        assert!(e.mod_fault_streak().is_none());
        assert!(e.disabled_mods().is_empty());
    }
    Ok(())
}

#[test]
fn automatic_disable_drops_faulting_own_actions_and_preserves_schema() -> anyhow::Result<()> {
    for binary in [false, true] {
        let own = decline_package("throw Error('own-fault');");
        let healthy = package(
            "api.handlers.after('builtin:link.tick',{id:'healthy'},ctx=>{ctx.state={ok:true};});",
        );
        let mut e = engine_with_options(
            &[("a", own), ("healthy", healthy)],
            binary,
            &[],
            auto_disable_options(1),
        )?;
        let id = e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":10,"y":12}))?;
        link(&mut e, 20, 0, 0)?;
        let original = e.get_object(&id);
        assert!(e.run_player("p1", &format!("Memory.saved=true;for(const key of ['a:a-accept','a:b-test','a:c-accept'])__kkEmitExtensionIntent(key,'{id}',{{}});'cached'")).is_err());
        assert_eq!(e.get_object(&id), original);
        assert_eq!(e.engine_mod_state("a"), Value::Null);
        assert_eq!(
            e.disabled_mods(),
            std::collections::BTreeSet::from(["a".into()])
        );
        let result = e.run_player("p1", "throw Error('不得重跑')")?;
        assert_eq!(result.expr["value"], "cached");
        assert_eq!(e.get_object(&id), original);
        assert_eq!(e.engine_mod_state("a"), Value::Null);
        assert_eq!(e.engine_mod_state("healthy"), json!({"ok":true}));
        assert_eq!(e.run_player("p1", "Memory.saved")?.expr["value"], true);
        e.place_mod_object("W1N1", "a:counter", &json!({"owner":"p1","x":11,"y":12}))?;
    }
    Ok(())
}

#[test]
fn automatic_disable_handles_control_context_conversion_and_domain_write_faults()
-> anyhow::Result<()> {
    use kk_mod_host::ModFaultKind;
    for (kind, body) in [
        (
            ModFaultKind::Control,
            "api.handlers.wrap('builtin:link.tick',{id:'bad'},(ctx,next)=>{next();next();});",
        ),
        (
            ModFaultKind::Context,
            "api.handlers.wrap('builtin:link.tick',{id:'bad'},(ctx,next)=>{Object.defineProperty(ctx,'state',{value:{},configurable:false});next();});",
        ),
        (
            ModFaultKind::WriteConversion,
            "api.handlers.before('builtin:link.tick',{id:'bad'},ctx=>{ctx.state={bad:Symbol('bad')};});",
        ),
        (
            ModFaultKind::Write,
            "api.handlers.before('builtin:link.tick',{id:'bad'},ctx=>{ctx.target.fields.cooldown=-1;});",
        ),
    ] {
        let mut e = engine_with_options(
            &[("bad", package(body))],
            false,
            &[],
            auto_disable_options(1),
        )?;
        let id = link(&mut e, 20, 0, 10)?;
        assert!(e.tick(1).is_err());
        let decision = e.last_mod_auto_disable().unwrap();
        assert_eq!(decision.trigger.error.fault.unwrap().kind, kind);
        assert_eq!(cooldown(&e, &id), 10);
        assert_eq!(e.get_game_time(), 0);
        e.tick(1)?;
        assert_eq!(cooldown(&e, &id), 9);
    }
    Ok(())
}

#[test]
fn automatic_disable_streak_resets_on_success_or_unattributed_engine_failure() -> anyhow::Result<()>
{
    let bad = package(
        "api.handlers.before('builtin:link.transfer',{id:'bad'},ctx=>{if(ctx.args.amount===1)throw Error('bad')});",
    );
    let mut e = engine_with_options(&[("bad", bad)], false, &[], auto_disable_options(2))?;
    let a = link(&mut e, 20, 100, 0)?;
    let b = link(&mut e, 21, 0, 0)?;
    let code = |amount| {
        format!("Game.getObjectById('{a}').transferEnergy(Game.getObjectById('{b}'),{amount})")
    };
    assert!(e.run_player("p1", &code(1)).is_err());
    assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
    e.discard_pending_round();
    e.run_player("p1", &code(2))?;
    assert!(e.mod_fault_streak().is_none());
    assert!(e.run_player("p1", &code(1)).is_err());
    assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
    e.fail_next_settle_stage_for_test(kk_engine::TickStage::Prepare);
    assert!(e.run_player("p1", "").is_err());
    assert!(e.mod_fault_streak().is_none());
    assert!(e.run_player("p1", "").is_err());
    assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, 1);
    assert!(e.disabled_mods().is_empty());
    assert!(e.run_player("p1", "").is_err());
    assert_eq!(e.last_mod_auto_disable().unwrap().consecutive_faults, 2);
    e.run_player("p1", "")?;
    Ok(())
}

#[test]
fn changing_fault_source_starts_new_streak_instead_of_accumulating_another_mod()
-> anyhow::Result<()> {
    let a = package(
        "api.handlers.before('builtin:link.transfer',{id:'a'},ctx=>{if(ctx.args.amount===1)throw Error('a')});",
    );
    let b = package(
        "api.handlers.before('builtin:link.transfer',{id:'b'},ctx=>{if(ctx.args.amount===2)throw Error('b')});",
    );
    let mut e = engine_with_options(&[("a", a), ("b", b)], false, &[], auto_disable_options(2))?;
    let source = link(&mut e, 20, 100, 0)?;
    let target = link(&mut e, 21, 0, 0)?;
    let code = |amount| {
        format!(
            "Game.getObjectById('{source}').transferEnergy(Game.getObjectById('{target}'),{amount})"
        )
    };
    assert!(e.run_player("p1", &code(1)).is_err());
    assert_eq!(e.mod_fault_streak().unwrap().mod_id, "a");
    e.discard_pending_round();
    assert!(e.run_player("p1", &code(2)).is_err());
    let streak = e.mod_fault_streak().unwrap();
    assert_eq!(streak.mod_id, "b");
    assert_eq!(streak.consecutive_faults, 1);
    assert!(e.run_player("p1", "").is_err());
    assert_eq!(
        e.disabled_mods(),
        std::collections::BTreeSet::from(["b".into()])
    );
    e.run_player("p1", "")?;
    assert_eq!(e.get_game_time(), 1);
    Ok(())
}

#[test]
fn automatic_disable_is_off_by_default_but_counter_is_retained() -> anyhow::Result<()> {
    let mut e = engine(
        &[(
            "bad",
            package(
                "api.handlers.before('builtin:link.tick',{id:'bad'},()=>{throw Error('bad')});",
            ),
        )],
        false,
    )?;
    link(&mut e, 20, 0, 10)?;
    for count in 1..=4 {
        assert!(e.tick(1).is_err());
        assert_eq!(e.mod_fault_streak().unwrap().consecutive_faults, count);
        assert!(e.disabled_mods().is_empty());
        assert!(e.last_mod_auto_disable().is_none());
        assert_eq!(e.get_game_time(), 0);
    }
    Ok(())
}
