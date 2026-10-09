//! Creep 战斗槽的链序、权威参数、能力改向/取消、正常拒绝与故障边界。
use super::*;
use harness::{enemy, events, fighter, hits, predictable_id};

/// 链序、权威参数与延迟伤害落地；native 中心经整槽宿主执行，事件与
/// 结算保持原生日径。wrap 后 args 不被本地篡改影响。
#[test]
fn creep_combat_chains_keep_order_and_refresh_authoritative_args() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.before('builtin:creep.attack',{id:'pre'},ctx=>{
  if(ctx.target.fields.name!=='a1')throw Error('执行者读面错误');
  if(ctx.args.id===null)throw Error('attack 槽缺少目标参数');
  ctx.state={trace:['before']};
});
api.handlers.wrap('builtin:creep.attack',{id:'wrap'},(ctx,next)=>{
  const target=ctx.args.id;
  ctx.args.id='000000000000000000000000';
  next();
  if(ctx.args.id!==target)throw Error('权威参数未刷新');
  ctx.state.trace.push('wrap');
});
api.handlers.after('builtin:creep.attack',{id:'post'},ctx=>{ctx.state.trace.push('after');});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
        let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
        )?;
        assert_eq!(hits(&e, &e1), 70, "{binary}: 延迟伤害在对象 tick 落地");
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"trace":["before","wrap","after"]})
        );
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 1, "{binary}");
        assert_eq!(attacks[0]["objectId"], json!(a1));
        assert_eq!(attacks[0]["data"]["targetId"], json!(e1));
        assert_eq!(attacks[0]["data"]["damage"], json!(30));
    }
    Ok(())
}

/// replace 中心改向（不经 next、不走 native）与取消；选择性 replace 对
/// 非目标执行者必须自行重发原目标，否则默认中心被静默取消。
#[test]
fn creep_combat_replace_redirects_or_cancels_execution() -> anyhow::Result<()> {
    // 改向目标用可预测 ID 嵌入 mod 源码（首个放置对象为 2，与资源域
    // 测试同型）；放置后断言命中，假设破裂立即报错而不是静默测错。
    let alt_id = predictable_id(2);
    for kind in ["redirect", "cancel"] {
        let body = if kind == "redirect" {
            format!("ctx.combat.request('{alt_id}');")
        } else {
            "ctx.combat.cancel();".to_string()
        };
        let source = package(&format!(
            r#"
const isTarget=ctx=>ctx.target.fields.name==='a1';
api.handlers.replace('builtin:creep.attack',{{id:'{kind}'}},ctx=>{{
  if(isTarget(ctx)){{{body}}}else if(ctx.args.id!==null){{ctx.combat.request(ctx.args.id);}}
  ctx.state={{ok:true}};
}});"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let e3 = enemy(&mut e, "e3", 9, 10, &["move"])?;
            assert_eq!(e3, alt_id, "改向目标 ID 预测破裂");
            let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
            let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
            let _a2 = fighter(&mut e, "a2", 12, 12, &["attack"])?;
            let e2 = enemy(&mut e, "e2", 12, 13, &["move"])?;
            e.run_player(
                "p1",
                &format!(
                    "const o=Game.getObjectById;[Game.creeps.a1.attack(o('{e1}')),Game.creeps.a2.attack(o('{e2}'))]"
                ),
            )?;
            // redirect：a1 打 e3（改向目标），e1 无伤；cancel：a1 无攻击。
            assert_eq!(hits(&e, &e1), 100, "{kind}/{binary}: 原目标");
            assert_eq!(
                hits(&e, &e3),
                if kind == "redirect" { 70 } else { 100 },
                "{kind}/{binary}: 改向目标"
            );
            // 非目标执行者 a2 重发原目标，默认中心不被静默取消。
            assert_eq!(hits(&e, &e2), 70, "{kind}/{binary}: 非目标执行者重发");
            assert_eq!(e.engine_mod_state("alpha"), json!({"ok":true}), "{kind}");
        }
    }
    Ok(())
}

/// 五个链位正常拒绝：撤销本动作的延迟伤害、事件与状态，保留此前转账、
/// 后续移动与其他执行者的攻击；跨 Mod state 丢弃，整轮照常发布。
#[test]
fn creep_combat_decline_at_each_position_restores_only_this_action() -> anyhow::Result<()> {
    for position in ["before", "wrap-before", "replace", "wrap-after", "after"] {
        let alpha = package(
            r#"
const isTarget=ctx=>ctx.target.fields.name==='a1';
api.handlers.before('builtin:creep.attack',{id:'pre'},ctx=>{if(!isTarget(ctx))return;ctx.state={dirty:'before'};});
api.handlers.wrap('builtin:creep.attack',{id:'outer'},(ctx,next)=>{if(!isTarget(ctx))return next();ctx.state={dirty:'wrap'};try{next()}catch(e){}ctx.state={dirty:'caught'};});
api.handlers.after('builtin:creep.attack',{id:'post'},ctx=>{if(isTarget(ctx))throw Error('拒绝后不得运行剩余 after');});
"#,
        );
        let beta = package(&format!(
            r#"
const position='{position}';
const isTarget=ctx=>ctx.target.fields.name==='a1';
api.handlers.before('builtin:creep.attack',{{id:'pre'}},ctx=>{{if(!isTarget(ctx))return;ctx.state={{dirty:'before'}};if(position==='before')return api.decline('blocked');}});
api.handlers.wrap('builtin:creep.attack',{{id:'inner'}},(ctx,next)=>{{
  if(!isTarget(ctx))return next();
  ctx.state={{dirty:'wrap'}};
  if(position==='wrap-before')return api.decline('blocked');
  next();
  if(position==='wrap-after')return api.decline('blocked');
}});
if(position==='replace')api.handlers.replace('builtin:creep.attack',{{id:'center'}},ctx=>{{
  if(isTarget(ctx))return api.decline('blocked');
  if(ctx.args.id!==null)ctx.combat.request(ctx.args.id);
}});
api.handlers.after('builtin:creep.attack',{{id:'z-write'}},ctx=>{{if(isTarget(ctx))ctx.state={{dirty:'after'}};}});
api.handlers.after('builtin:creep.attack',{{id:'post'}},ctx=>{{if(isTarget(ctx))return api.decline('blocked');}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let a1 = fighter(&mut e, "a1", 10, 10, &["attack", "move", "carry"])?;
            let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
            let _a2 = fighter(&mut e, "a2", 12, 12, &["attack"])?;
            let e2 = enemy(&mut e, "e2", 12, 13, &["move"])?;
            let container = e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[9,10],"structureType":"container","owner":"p1","store":{"energy":20}}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "Memory.committed=true;const o=Game.getObjectById;const a1=Game.creeps.a1;[a1.transfer(o('{container}'),'energy',5),a1.attack(o('{e1}')),a1.move(RIGHT),Game.creeps.a2.attack(o('{e2}'))]"
                ),
            )?;
            assert_eq!(hits(&e, &e1), 100, "{position}: 拒绝后不得有延迟伤害");
            assert_eq!(
                e.get_object(&container).unwrap()["store"]["energy"],
                25,
                "{position}: 此前动作保留"
            );
            let pos = e.get_object(&a1).unwrap()["pos"].clone();
            assert_eq!(pos["x"], 11, "{position}: 后续移动保留");
            assert_eq!(hits(&e, &e2), 70, "{position}: 其他执行者攻击保留");
            let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
            assert_eq!(attacks.len(), 1, "{position}: 仅其他执行者的攻击事件");
            assert_eq!(attacks[0]["data"]["targetId"], json!(e2));
            assert_eq!(e.get_object(&a1).unwrap()["store"]["energy"], 25);
            assert_eq!(e.engine_mod_state("alpha"), Value::Null, "{position}");
            assert_eq!(e.engine_mod_state("beta"), Value::Null, "{position}");
            assert_eq!(
                e.player_state("p1").unwrap().raw_memory,
                "{\"committed\":true}"
            );
            assert_eq!(e.get_game_time(), 2, "{position}: 整轮照常发布");
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
fn creep_combat_decline_cannot_hide_write_fault_or_later_exception() -> anyhow::Result<()> {
    for (inner, outer_fault, message, final_hits) in [
        (
            "next();return api.decline('blocked');",
            true,
            "拒绝后异常",
            100,
        ),
        (
            "ctx.target.fields.x=99;try{next()}catch(e){}return api.decline('blocked');",
            false,
            "只读",
            70,
        ),
    ] {
        let alpha = package(&format!(
            r#"
api.handlers.wrap('builtin:creep.attack',{{id:'outer'}},(ctx,next)=>{{
  try{{next()}}catch(e){{}}
  if({outer_fault})throw Error('拒绝后异常');
}});"#
        ));
        let beta = package(&format!(
            "api.handlers.wrap('builtin:creep.attack',{{id:'inner'}},(ctx,next)=>{{{inner}}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
            let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            let error = e
                .run_player(
                    "p1",
                    &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))"),
                )
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
            assert_eq!(hits(&e, &e1), final_hits);
        }
    }
    Ok(())
}

/// 非法参数、双份消耗、取消后再改向、跨槽能力与 native 执行后的能力
/// 调用均为故障；catch + decline 不能掩盖。
#[test]
fn creep_combat_capability_faults_cannot_be_masked_by_decline() -> anyhow::Result<()> {
    for call in [
        "ctx.combat.request(123)",
        "ctx.combat.request(null)",
        "ctx.combat.request('zzz')",
        "ctx.combat.cancel();ctx.combat.request(ctx.args.id)",
        "ctx.combat.request(ctx.args.id);ctx.combat.request(ctx.args.id)",
        "ctx.resources.read(ctx.target.id,'energy')",
    ] {
        let alpha = package(&format!(
            "api.handlers.replace('builtin:creep.attack',{{id:'faulty'}},ctx=>{{try{{{call}}}catch(e){{}}return api.decline('blocked');}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone())], binary)?;
            let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
            let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
            let before = e.world_snapshot();
            assert!(
                e.run_player(
                    "p1",
                    &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))")
                )
                .is_err(),
                "{call}"
            );
            assert_eq!(e.world_snapshot(), before, "{call}");
            assert_eq!(hits(&e, &e1), 100, "{call}");
            assert!(e.needs_mod_recovery(), "{call}");
            assert!(e.last_mod_settlement_fault().is_some(), "{call}");
        }
    }
    // native 中心执行后的 after 处理器再调能力：consumed 单向，故障。
    let alpha = package(
        r#"
api.handlers.after('builtin:creep.attack',{id:'late'},ctx=>{
  if(ctx.args.id===null)throw Error('attack 缺目标');
  try{ctx.combat.request(ctx.args.id)}catch(e){}
});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
    let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    Ok(())
}

/// 处理器返回后能力过期；跨槽保存的能力失效。两者都是故障。
#[test]
fn creep_combat_capability_expires_after_handler_and_across_slots() -> anyhow::Result<()> {
    let alpha = package(
        r#"
api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{globalThis.saved=ctx.combat;});
api.handlers.after('builtin:creep.attack',{id:'use'},()=>{try{globalThis.saved.cancel()}catch(e){}});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
    let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.a1.attack(Game.getObjectById('{e1}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(hits(&e, &e1), 100);
    assert!(e.needs_mod_recovery());

    let beta = package(
        r#"
api.handlers.before('builtin:creep.attack',{id:'save'},ctx=>{globalThis.saved=ctx.combat;});
api.handlers.before('builtin:creep.transfer',{id:'use'},()=>{globalThis.saved.cancel()});
"#,
    );
    let mut e = engine(&[("beta", beta)], false)?;
    let _a1 = fighter(&mut e, "a1", 10, 10, &["attack", "carry"])?;
    let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
    let container = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"container","owner":"p1","store":{"energy":20}}),
    )?;
    let before = e.world_snapshot();
    // 先触发 attack 槽保存能力，再在 transfer 槽使用：跨槽默认拒绝。
    assert!(
        e.run_player(
            "p1",
            &format!(
                "const o=Game.getObjectById;Game.creeps.a1.attack(o('{e1}'));Game.creeps.a1.transfer(o('{container}'),'energy',5)"
            )
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    Ok(())
}

/// 晚期 RawMemory 写回失败：战斗候选与 Mod state 不发布；重试传入抛错
/// 代码仍使用原玩家缓存，重建 realm 中重新结算并应用改向。
#[test]
fn creep_combat_candidates_survive_writeback_failure_via_cached_retry() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.attack',{{id:'redirect'}},ctx=>{{ctx.combat.request('{alt_id}');ctx.state={{hit:true}};}});"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let e3 = enemy(&mut e, "e3", 9, 10, &["move"])?;
        assert_eq!(e3, alt_id, "改向目标 ID 预测破裂");
        let _a1 = fighter(&mut e, "a1", 10, 10, &["attack"])?;
        let e1 = enemy(&mut e, "e1", 10, 11, &["move"])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.step=1;Game.creeps.a1.attack(Game.getObjectById('{e1}'))")
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert_eq!(e.engine_mod_state("alpha"), Value::Null);
        assert!(e.needs_mod_recovery());
        // 重试使用原缓存玩家代码：改向目标以字面量进入 mod 源码。
        assert_eq!(
            e.run_player("p1", "throw Error('不得重新运行玩家')")?.expr["value"],
            json!(0)
        );
        assert_eq!(hits(&e, &e1), 100, "重试仍应用改向：原目标无伤");
        assert_eq!(hits(&e, &e3), 70, "重试应用 replace 改向");
        assert_eq!(e.engine_mod_state("alpha"), json!({"hit":true}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"step\":1}");
        assert!(!e.needs_mod_recovery());
    }
    Ok(())
}
