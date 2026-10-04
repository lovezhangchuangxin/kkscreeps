//! Creep 移动槽的链序、权威参数、能力改向/取消、正常拒绝与故障边界。
use super::*;

fn mover(e: &mut EmbedEngine, name: &str, x: u8, y: u8, body: &[&str]) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":name,"owner":"p1","pos":[x,y],"body":body,"store":{"energy":30},"ticksToLive":100}),
    )
}

fn pos(e: &EmbedEngine, id: &str) -> (i64, i64) {
    let o = e.get_object(id).unwrap();
    (
        o["pos"]["x"].as_i64().unwrap(),
        o["pos"]["y"].as_i64().unwrap(),
    )
}

fn events(e: &EmbedEngine, event: i64) -> Vec<Value> {
    e.world_snapshot()["rooms"]["W1N1"]["events"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["event"] == event)
        .cloned()
        .collect()
}

/// 链序、权威参数刷新与 native 登记后的 pending 读面；replace 不经 next
/// 直接改向/取消；wrap 前能力登记会被后续中心覆盖（单请求不变量）。
#[test]
fn creep_move_chains_keep_order_and_refresh_authoritative_args() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.before('builtin:creep.move',{id:'pre'},ctx=>{
  if(ctx.target.fields.name!=='m1'||ctx.args.direction!==3)throw Error('读面或参数错误');
  if(ctx.movement.pending()!==null)throw Error('链首不应有登记');
  ctx.state={trace:['before']};
});
api.handlers.wrap('builtin:creep.move',{id:'wrap'},(ctx,next)=>{
  const dir=ctx.args.direction;
  ctx.args.direction=9;ctx.args.id='000000000000000000000000';
  const first=ctx.movement.request(5);
  if(!first||ctx.movement.pending()!==5)throw Error('能力登记未生效');
  next();
  if(ctx.args.direction!==dir)throw Error('权威参数未刷新');
  if(ctx.movement.pending()!==3)throw Error('native 覆盖能力登记失败');
  ctx.state.trace.push('wrap');
});
api.handlers.after('builtin:creep.move',{id:'post'},ctx=>{ctx.state.trace.push('after');});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
        e.run_player("p1", "Game.creeps.m1.move(RIGHT)")?;
        assert_eq!(pos(&e, &m1), (11, 10));
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"trace":["before","wrap","after"]})
        );
    }
    Ok(())
}

/// replace 中心改向（不经 next、不走 native）与取消登记。
#[test]
fn creep_move_replace_redirects_or_cancels_registration() -> anyhow::Result<()> {
    // redirect：改向 BOTTOM；cancel：先登记再取消，净效果为无请求。
    for kind in ["redirect", "cancel"] {
        let body = if kind == "redirect" {
            "const ok=ctx.movement.request(5);"
        } else {
            "ctx.movement.request(5);ctx.movement.cancel();const ok=true;"
        };
        let source = package(&format!(
            r#"
api.handlers.replace('builtin:creep.move',{{id:'{kind}'}},ctx=>{{
  {body}
  ctx.state={{ok}};
}});"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
            e.run_player("p1", "Game.creeps.m1.move(RIGHT)")?;
            assert_eq!(
                pos(&e, &m1),
                if kind == "redirect" {
                    (10, 11)
                } else {
                    (10, 10)
                },
                "{kind}"
            );
            assert_eq!(e.engine_mod_state("alpha"), json!({"ok":true}), "{kind}");
        }
    }
    // 静态障碍预检与 native 同口径：障碍结构格上 request 返回 false 不登记。
    let source = package(
        "api.handlers.replace('builtin:creep.move',{id:'blocked'},ctx=>{ctx.state={ok:ctx.movement.request(3)}});",
    );
    let mut e = engine(&[("alpha", source)], false)?;
    let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"spawn","owner":"p1"}),
    )?;
    e.run_player("p1", "Game.creeps.m1.move(RIGHT)")?;
    assert_eq!(pos(&e, &m1), (10, 10), "被障碍结构阻挡");
    assert_eq!(e.engine_mod_state("alpha"), json!({"ok":false}));
    Ok(())
}

/// 五个链位正常拒绝：撤销本动作的请求登记与疲劳记录，保留此前 drop、
/// 后续 say 与竞争者移动；跨 Mod state 与事件一并恢复，整轮照常发布。
#[test]
fn creep_move_decline_at_each_position_restores_only_this_action() -> anyhow::Result<()> {
    for position in ["before", "wrap-before", "replace", "wrap-after", "after"] {
        let alpha = package(
            r#"
const isTarget=ctx=>ctx.target.fields.name==='m1';
api.handlers.before('builtin:creep.move',{id:'pre'},ctx=>{if(!isTarget(ctx))return;ctx.state={dirty:'before'};});
api.handlers.wrap('builtin:creep.move',{id:'outer'},(ctx,next)=>{if(!isTarget(ctx))return next();ctx.state={dirty:'wrap'};try{next()}catch(e){}ctx.state={dirty:'caught'};});
api.handlers.after('builtin:creep.move',{id:'post'},ctx=>{if(isTarget(ctx))throw Error('拒绝后不得运行剩余 after');});
"#,
        );
        let beta = package(&format!(
            r#"
const position='{position}';
const isTarget=ctx=>ctx.target.fields.name==='m1';
api.handlers.before('builtin:creep.move',{{id:'pre'}},ctx=>{{if(!isTarget(ctx))return;ctx.state={{dirty:'before'}};if(position==='before')return api.decline('blocked');}});
api.handlers.wrap('builtin:creep.move',{{id:'inner'}},(ctx,next)=>{{
  if(!isTarget(ctx))return next();
  ctx.state={{dirty:'wrap'}};
  if(position==='wrap-before')return api.decline('blocked');
  next();
  if(position==='wrap-after'){{if(ctx.movement.pending()!==3)throw Error('native 未登记');return api.decline('blocked');}}
}});
if(position==='replace')api.handlers.replace('builtin:creep.move',{{id:'center'}},ctx=>{{
  if(isTarget(ctx))return api.decline('blocked');
  // 非目标执行者：replace 须自行恢复默认登记（方向形态等价 native）。
  if(ctx.args.direction!==null)ctx.movement.request(ctx.args.direction);
}});
api.handlers.after('builtin:creep.move',{{id:'z-write'}},ctx=>{{if(isTarget(ctx))ctx.state={{dirty:'after'}};}});
api.handlers.after('builtin:creep.move',{{id:'post'}},ctx=>{{if(isTarget(ctx))return api.decline('blocked');}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let m1 = mover(&mut e, "m1", 10, 10, &["move", "work", "carry"])?;
            let m2 = mover(&mut e, "m2", 12, 10, &["move"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[10,9],"structureType":"road","owner":"p1"}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "Memory.committed=true;const m1=Game.creeps.m1;[m1.drop(RESOURCE_ENERGY,5),m1.move(RIGHT),m1.build(Game.getObjectById('{site}')),Game.creeps.m2.move(LEFT)]"
                ),
            )?;
            assert_eq!(pos(&e, &m1), (10, 10), "{position}: 拒绝后不得位移");
            assert_eq!(pos(&e, &m2), (11, 10), "{position}: 竞争者正常入格");
            assert_eq!(
                e.get_object(&site).unwrap()["progress"],
                5,
                "{position}: 后续动作保留"
            );
            assert_eq!(
                events(&e, kk_core::constants::EVENT_BUILD).len(),
                1,
                "{position}: 后续动作事件"
            );
            let drops = e.find_in_room("W1N1", kk_core::constants::FIND_DROPPED_RESOURCES);
            assert_eq!(drops.len(), 1, "{position}: 此前 drop 保留");
            assert_eq!(drops[0]["amount"], 4, "{position}: 当轮衰减 1");
            assert_eq!(e.get_object(&m1).unwrap()["store"]["energy"], 20);
            assert_eq!(e.engine_mod_state("alpha"), Value::Null, "{position}");
            assert_eq!(e.engine_mod_state("beta"), Value::Null, "{position}");
            assert_eq!(
                e.player_state("p1").unwrap().raw_memory,
                "{\"committed\":true}"
            );
            assert_eq!(e.get_game_time(), 1, "{position}: 整轮照常发布");
            assert!(!e.needs_mod_recovery());
            assert!(e.last_mod_settlement_fault().is_none());
        }
    }
    Ok(())
}

/// wrap 捕获拒绝后再抛异常、或内层先改写只读字段：故障优先，不能用
/// 拒绝掩盖；世界/玩家/Mod 状态全部回滚并进入恢复门，停用后原 native
/// 恢复、缓存玩家代码不重跑。
#[test]
fn creep_move_decline_cannot_hide_write_fault_or_later_exception() -> anyhow::Result<()> {
    // 内层 beta 拒绝（wrap-after 形态），外层 alpha 制造故障：
    // 拒绝后再抛异常 / 内层先改写只读 creep 字段。
    for (inner, outer_fault, message, final_pos) in [
        (
            "next();return api.decline('blocked');",
            true,
            "拒绝后异常",
            (10, 10),
        ),
        (
            "ctx.target.fields.x=99;try{next()}catch(e){}return api.decline('blocked');",
            false,
            "只读",
            (11, 10),
        ),
    ] {
        let alpha = package(&format!(
            r#"
api.handlers.wrap('builtin:creep.move',{{id:'outer'}},(ctx,next)=>{{
  try{{next()}}catch(e){{}}
  if({outer_fault})throw Error('拒绝后异常');
}});"#
        ));
        let beta = package(&format!(
            "api.handlers.wrap('builtin:creep.move',{{id:'inner'}},(ctx,next)=>{{{inner}}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            let error = e
                .run_player("p1", "Game.creeps.m1.move(RIGHT)")
                .unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(e.world_snapshot(), before);
            assert_eq!(e.player_state("p1"), Some(&player));
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            assert_eq!(e.engine_mod_state("beta"), Value::Null);
            assert!(e.needs_mod_recovery());
            assert!(e.last_mod_settlement_fault().is_some());
            // 停用来源 Mod 后按剩余链重新结算缓存轮：动作完成或被正常拒绝。
            e.disable_mod(if outer_fault { "alpha" } else { "beta" })?;
            e.run_player("p1", "throw Error('不得重跑')")?;
            assert_eq!(pos(&e, &m1), final_pos);
        }
    }
    Ok(())
}

/// 非法方向、过期能力、跨槽能力与重复 next 均为故障；catch + decline
/// 不能掩盖。跨槽还包括资源能力出现在移动槽。
#[test]
fn creep_move_capability_faults_cannot_be_masked_by_decline() -> anyhow::Result<()> {
    for call in [
        "ctx.movement.request(0)",
        "ctx.movement.request(9)",
        "ctx.movement.request(1.5)",
        "ctx.movement.request('3')",
        "ctx.resources.read(ctx.target.id,'energy')",
    ] {
        let alpha = package(&format!(
            "api.handlers.replace('builtin:creep.move',{{id:'faulty'}},ctx=>{{try{{{call}}}catch(e){{}}return api.decline('blocked');}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone())], binary)?;
            let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
            let before = e.world_snapshot();
            assert!(
                e.run_player("p1", "Game.creeps.m1.move(RIGHT)").is_err(),
                "{call}"
            );
            assert_eq!(e.world_snapshot(), before, "{call}");
            assert_eq!(pos(&e, &m1), (10, 10), "{call}");
            assert!(e.needs_mod_recovery(), "{call}");
            assert!(e.last_mod_settlement_fault().is_some(), "{call}");
        }
    }
    // 移动能力挂在资源槽：默认宿主拒绝，同样不能被 decline 掩盖。
    let alpha = package(
        "api.handlers.replace('builtin:creep.transfer',{id:'cross'},(ctx)=>{try{ctx.movement.request(3)}catch(e){}return api.decline('blocked');});",
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let m1 = mover(&mut e, "m1", 10, 10, &["move", "carry"])?;
    let container = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"container","owner":"p1","store":{"energy":20}}),
    )?;
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.m1.transfer(Game.getObjectById('{container}'),'energy',5)")
        )
        .is_err()
    );
    assert!(e.needs_mod_recovery());
    assert_eq!(pos(&e, &m1), (10, 10));
    Ok(())
}

/// 处理器返回后能力过期；跨槽保存的 token 失效。两者都是故障。
#[test]
fn creep_move_capability_expires_after_handler_and_across_slots() -> anyhow::Result<()> {
    let alpha = package(
        r#"
api.handlers.before('builtin:creep.move',{id:'save'},ctx=>{globalThis.saved=ctx.movement;});
api.handlers.after('builtin:creep.move',{id:'use'},()=>{try{globalThis.saved.request(3)}catch(e){}});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", "Game.creeps.m1.move(RIGHT)").is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(pos(&e, &m1), (10, 10));
    assert!(e.needs_mod_recovery());

    let beta = package(
        r#"
api.handlers.before('builtin:creep.move',{id:'save'},ctx=>{globalThis.saved=ctx.movement;});
api.handlers.before('builtin:creep.transfer',{id:'use'},()=>{globalThis.saved.request(3)});
"#,
    );
    let mut e = engine(&[("beta", beta)], false)?;
    let _m1 = mover(&mut e, "m1", 10, 10, &["move", "carry"])?;
    let container = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"container","owner":"p1","store":{"energy":20}}),
    )?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.m1.transfer(Game.getObjectById('{container}'),'energy',5)")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    Ok(())
}

/// wrap 重复调用 next 是控制故障；拒绝链不执行 after。
#[test]
fn creep_move_double_next_is_a_control_fault() -> anyhow::Result<()> {
    let alpha = package(
        r#"
api.handlers.wrap('builtin:creep.move',{id:'double'},(ctx,next)=>{next();next();});
api.handlers.after('builtin:creep.move',{id:'post'},()=>{throw Error('不得执行 after');});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
    let before = e.world_snapshot();
    assert!(e.run_player("p1", "Game.creeps.m1.move(RIGHT)").is_err());
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(pos(&e, &m1), (10, 10));
    assert!(e.needs_mod_recovery());
    assert!(e.last_mod_settlement_fault().is_some());
    Ok(())
}

/// 晚期 RawMemory 写回失败：移动候选与 Mod state 不发布；重试传入抛错
/// 代码仍使用原玩家缓存，重建 realm 中重新结算并应用改向。
#[test]
fn creep_move_candidates_survive_writeback_failure_via_cached_retry() -> anyhow::Result<()> {
    let source = package(
        "api.handlers.replace('builtin:creep.move',{id:'redirect'},ctx=>{ctx.movement.request(5);ctx.state={moved:true};});",
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player("p1", "Memory.step=1;Game.creeps.m1.move(RIGHT)")
                .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert!(e.needs_mod_recovery());
        assert_eq!(
            e.run_player("p1", "throw Error('不得重新运行玩家')")?.expr["value"],
            json!(0)
        );
        assert_eq!(pos(&e, &m1), (10, 11), "重试应用 replace 改向");
        assert_eq!(e.engine_mod_state("alpha"), json!({"moved":true}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"step\":1}");
        assert!(!e.needs_mod_recovery());
    }
    Ok(())
}

/// 移动槽不产生事件；拒绝后 EVENT_EXIT 不因本 tick 的登记残留出现。
#[test]
fn creep_move_decline_leaves_no_exit_events() -> anyhow::Result<()> {
    let alpha = package(
        "api.handlers.after('builtin:creep.move',{id:'reject'},()=>api.decline('blocked'));",
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
    e.run_player("p1", "Game.creeps.m1.move(RIGHT)")?;
    assert_eq!(pos(&e, &m1), (10, 10));
    assert!(events(&e, kk_core::constants::EVENT_EXIT).is_empty());
    Ok(())
}

/// 能力登记本身也在拒绝恢复边界内：wrap 前后以 request 改向后拒绝，
/// 登记与疲劳记录一并撤销，竞争者仍可入格。
#[test]
fn creep_move_decline_after_capability_request_restores_registration() -> anyhow::Result<()> {
    for (stage, when) in [("wrap-before", "before"), ("wrap-after", "after")] {
        let alpha = package(&format!(
            r#"
api.handlers.wrap('builtin:creep.move',{{id:'redirect'}},(ctx,next)=>{{
  if(ctx.target.fields.name!=='m1')return next();
  ctx.movement.request(5);
  if('{when}'==='before')return api.decline('blocked');
  next();
  ctx.movement.request(7);
  return api.decline('blocked');
}});"#
        ));
        let _ = stage;
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone())], binary)?;
            let m1 = mover(&mut e, "m1", 10, 10, &["move"])?;
            let m2 = mover(&mut e, "m2", 12, 10, &["move"])?;
            e.run_player("p1", "Game.creeps.m1.move(RIGHT);Game.creeps.m2.move(LEFT)")?;
            assert_eq!(pos(&e, &m1), (10, 10), "{when}: 能力登记被撤销");
            assert_eq!(pos(&e, &m2), (11, 10), "{when}: 竞争者照常入格");
            assert_eq!(e.engine_mod_state("alpha"), Value::Null, "{when}");
            assert!(!e.needs_mod_recovery());
        }
    }
    Ok(())
}
