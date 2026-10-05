//! Creep 建造族槽（build/repair/dismantle）的链序、权威参数、目标改向/
//! 取消、正常拒绝与故障边界；建造槽为族内深测代表，repair/dismantle 的
//! 世界语义与拒绝恢复见 creep_build_world.rs。
use super::*;
use harness::{events, fighter, observe_pkg, predictable_id};

/// 控制器进度（W1N1 基设对象 id 1）；用作"后续动作继续结算"的观测面。
fn controller_progress(e: &EmbedEngine) -> i64 {
    let controller = e.get_object(&predictable_id(1)).expect("基设控制器");
    assert_eq!(controller["structureType"], "controller", "id 1 应为控制器");
    controller["progress"].as_i64().unwrap_or(0)
}

/// 链序、权威参数与进度落地；native 中心经整槽宿主执行，事件与结算
/// 保持原生意径。wrap 后 args 不被本地篡改影响。
#[test]
fn creep_build_chains_keep_order_and_refresh_authoritative_args() -> anyhow::Result<()> {
    let source = package(
        r#"
api.handlers.before('builtin:creep.build',{id:'pre'},ctx=>{
  if(ctx.target.fields.name!=='b1')throw Error('执行者读面错误');
  if(ctx.args.id===null)throw Error('build 槽缺少目标参数');
  ctx.state={trace:['before']};
});
api.handlers.wrap('builtin:creep.build',{id:'wrap'},(ctx,next)=>{
  const target=ctx.args.id;
  ctx.args.id='000000000000000000000000';
  next();
  if(ctx.args.id!==target)throw Error('权威参数未刷新');
  ctx.state.trace.push('wrap');
});
api.handlers.after('builtin:creep.build',{id:'post'},ctx=>{ctx.state.trace.push('after');});
"#,
    );
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
        )?;
        assert_eq!(
            e.get_object(&site).unwrap()["progress"],
            5,
            "{binary}: 进度按 1×WORK 落地"
        );
        assert_eq!(
            e.get_object(&b1).unwrap()["store"]["energy"],
            25,
            "{binary}"
        );
        assert_eq!(
            e.engine_mod_state("alpha"),
            json!({"trace":["before","wrap","after"]})
        );
        let builds = events(&e, kk_core::constants::EVENT_BUILD);
        assert_eq!(builds.len(), 1, "{binary}");
        assert_eq!(builds[0]["objectId"], json!(b1));
        assert_eq!(builds[0]["data"]["targetId"], json!(site));
        assert_eq!(builds[0]["data"]["amount"], json!(5));
        assert_eq!(builds[0]["data"]["incomplete"], true, "{binary}");
    }
    Ok(())
}

/// replace 中心改向（不经 next、不走 native）与取消；选择性 replace 对
/// 非目标执行者必须自行重发原目标，否则默认中心被静默取消。
#[test]
fn creep_build_replace_redirects_or_cancels_execution() -> anyhow::Result<()> {
    // 改向目标用可预测 ID 嵌入 mod 源码（首个放置对象为 2）。
    let alt_id = predictable_id(2);
    for kind in ["redirect", "cancel"] {
        let body = if kind == "redirect" {
            format!("ctx.build.request('{alt_id}');")
        } else {
            "ctx.build.cancel();".to_string()
        };
        let source = package(&format!(
            r#"
const isTarget=ctx=>ctx.target.fields.name==='b1';
api.handlers.replace('builtin:creep.build',{{id:'{kind}'}},ctx=>{{
  if(isTarget(ctx)){{{body}}}else if(ctx.args.id!==null){{ctx.build.request(ctx.args.id);}}
  ctx.state={{ok:true}};
}});"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", source.clone())], binary)?;
            let alt = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[12,10],"structureType":"road","owner":"p1"}),
            )?;
            assert_eq!(alt, alt_id, "改向目标 ID 预测破裂");
            let b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
            )?;
            let _b2 = fighter(&mut e, "b2", 20, 20, &["work"])?;
            let site3 = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[21,21],"structureType":"road","owner":"p1"}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "const o=Game.getObjectById;[Game.creeps.b1.build(o('{site}')),Game.creeps.b2.build(o('{site3}'))]"
                ),
            )?;
            assert_eq!(
                e.get_object(&site).unwrap()["progress"],
                0,
                "{kind}/{binary}: 原目标"
            );
            assert_eq!(
                e.get_object(&alt).unwrap()["progress"],
                if kind == "redirect" { 5 } else { 0 },
                "{kind}/{binary}: 改向目标"
            );
            assert_eq!(
                e.get_object(&site3).unwrap()["progress"],
                5,
                "{kind}/{binary}: 非目标执行者重发"
            );
            assert_eq!(
                e.get_object(&b1).unwrap()["store"]["energy"],
                if kind == "redirect" { 25 } else { 30 },
                "{kind}: 改向执行扣能，取消不扣"
            );
            assert_eq!(e.engine_mod_state("alpha"), json!({"ok":true}), "{kind}");
        }
    }
    Ok(())
}

/// 五个链位正常拒绝：撤销本动作的进度/能量/事件与 Mod state，保留此前
/// 转账、后续升级与其他执行者的建造；整轮照常发布。工地进度置于 295，
/// 本轮建造即完工——wrap 后/after 的拒绝须连完工变换（工地移除+结构
/// 插入）一起撤销。
#[test]
fn creep_build_decline_at_each_position_restores_only_this_action() -> anyhow::Result<()> {
    for position in ["before", "wrap-before", "replace", "wrap-after", "after"] {
        let alpha = package(
            r#"
const isTarget=ctx=>ctx.target.fields.name==='b1';
api.handlers.before('builtin:creep.build',{id:'pre'},ctx=>{if(!isTarget(ctx))return;ctx.state={dirty:'before'};});
api.handlers.wrap('builtin:creep.build',{id:'outer'},(ctx,next)=>{if(!isTarget(ctx))return next();ctx.state={dirty:'wrap'};try{next()}catch(e){}ctx.state={dirty:'caught'};});
api.handlers.after('builtin:creep.build',{id:'post'},ctx=>{if(isTarget(ctx))throw Error('拒绝后不得运行剩余 after');});
"#,
        );
        let beta = package(&format!(
            r#"
const position='{position}';
const isTarget=ctx=>ctx.target.fields.name==='b1';
api.handlers.before('builtin:creep.build',{{id:'pre'}},ctx=>{{if(!isTarget(ctx))return;ctx.state={{dirty:'before'}};if(position==='before')return api.decline('blocked');}});
api.handlers.wrap('builtin:creep.build',{{id:'inner'}},(ctx,next)=>{{
  if(!isTarget(ctx))return next();
  ctx.state={{dirty:'wrap'}};
  if(position==='wrap-before')return api.decline('blocked');
  next();
  if(position==='wrap-after')return api.decline('blocked');
}});
if(position==='replace')api.handlers.replace('builtin:creep.build',{{id:'center'}},ctx=>{{
  if(isTarget(ctx))return api.decline('blocked');
  if(ctx.args.id!==null)ctx.build.request(ctx.args.id);
}});
api.handlers.after('builtin:creep.build',{{id:'z-write'}},ctx=>{{if(isTarget(ctx))ctx.state={{dirty:'after'}};}});
api.handlers.after('builtin:creep.build',{{id:'post'}},ctx=>{{if(isTarget(ctx))return api.decline('blocked');}});
"#
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let b1 = fighter(&mut e, "b1", 2, 2, &["work", "carry"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[3,3],"structureType":"road","owner":"p1","progress":295}),
            )?;
            let container = e.place_object(
                "W1N1",
                "structure",
                &json!({"pos":[3,2],"structureType":"container","owner":"p1","store":{"energy":20}}),
            )?;
            let _b2 = fighter(&mut e, "b2", 20, 20, &["work"])?;
            let site3 = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[21,21],"structureType":"road","owner":"p1"}),
            )?;
            let progress_before = controller_progress(&e);
            e.run_player(
                "p1",
                &format!(
                    "Memory.committed=true;const o=Game.getObjectById;const b1=Game.creeps.b1;\
                     [b1.transfer(o('{container}'),'energy',5),b1.build(o('{site}')),\
                     b1.upgradeController(Game.rooms.W1N1.controller),Game.creeps.b2.build(o('{site3}'))]"
                ),
            )?;
            assert_eq!(
                e.get_object(&site).unwrap()["progress"],
                295,
                "{position}: 完工变换被撤销，工地复原"
            );
            assert!(
                e.find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
                    .into_iter()
                    .all(|s| s["structureType"] != "road"),
                "{position}: 链内插入的道路被撤销"
            );
            assert_eq!(
                e.get_object(&container).unwrap()["store"]["energy"],
                25,
                "{position}: 此前动作保留"
            );
            assert_eq!(
                controller_progress(&e),
                progress_before + 1,
                "{position}: 后续升级动作保留"
            );
            assert_eq!(
                e.get_object(&site3).unwrap()["progress"],
                5,
                "{position}: 其他执行者建造保留"
            );
            assert_eq!(
                e.get_object(&b1).unwrap()["store"]["energy"],
                24,
                "{position}"
            );
            assert!(
                events(&e, kk_core::constants::EVENT_BUILD)
                    .into_iter()
                    .all(|ev| ev["data"]["targetId"] != json!(site)),
                "{position}: 本动作事件截除"
            );
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
fn creep_build_decline_cannot_hide_write_fault_or_later_exception() -> anyhow::Result<()> {
    for (inner, outer_fault, message, final_progress) in [
        (
            "next();return api.decline('blocked');",
            true,
            "拒绝后异常",
            0,
        ),
        (
            "ctx.target.fields.x=99;try{next()}catch(e){}return api.decline('blocked');",
            false,
            "只读",
            5,
        ),
    ] {
        let alpha = package(&format!(
            r#"
api.handlers.wrap('builtin:creep.build',{{id:'outer'}},(ctx,next)=>{{
  try{{next()}}catch(e){{}}
  if({outer_fault})throw Error('拒绝后异常');
}});"#
        ));
        let beta = package(&format!(
            "api.handlers.wrap('builtin:creep.build',{{id:'inner'}},(ctx,next)=>{{{inner}}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone()), ("beta", beta.clone())], binary)?;
            let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
            )?;
            let before = e.world_snapshot();
            let player = e.player_state("p1").unwrap().clone();
            let error = e
                .run_player(
                    "p1",
                    &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
                )
                .unwrap_err();
            assert!(error.to_string().contains(message), "{error}");
            assert_eq!(e.world_snapshot(), before);
            assert_eq!(e.player_state("p1"), Some(&player));
            assert_eq!(e.engine_mod_state("alpha"), Value::Null);
            assert_eq!(e.engine_mod_state("beta"), Value::Null);
            assert!(e.needs_mod_recovery());
            assert!(e.last_mod_settlement_fault().is_some());
            // 停用来源 Mod 后按剩余链重新结算缓存轮：动作完成。
            e.disable_mod(if outer_fault { "alpha" } else { "beta" })?;
            e.run_player("p1", "throw Error('不得重跑')")?;
            // 拒绝源在 alpha：停用 alpha 后 beta 的拒绝仍在，进度不动；
            // 拒绝源在 beta：停用后 native 直落。两形态都正常发布缓存轮。
            assert_eq!(e.get_object(&site).unwrap()["progress"], final_progress);
        }
    }
    Ok(())
}

/// 非法参数、双份消耗、取消后再改向、跨能力面与 native 执行后的能力
/// 调用均为故障；catch + decline 不能掩盖。
#[test]
fn creep_build_capability_faults_cannot_be_masked_by_decline() -> anyhow::Result<()> {
    for call in [
        "ctx.build.request(123)",
        "ctx.build.request(null)",
        "ctx.build.request('zzz')",
        "ctx.build.cancel();ctx.build.request(ctx.args.id)",
        "ctx.build.request(ctx.args.id);ctx.build.request(ctx.args.id)",
        "ctx.resources.read(ctx.target.id,'energy')",
        "ctx.combat.request(ctx.args.id)",
        "ctx.movement.request(1)",
    ] {
        let alpha = package(&format!(
            "api.handlers.replace('builtin:creep.build',{{id:'faulty'}},ctx=>{{try{{{call}}}catch(e){{}}return api.decline('blocked');}});"
        ));
        for binary in [false, true] {
            let mut e = engine(&[("alpha", alpha.clone())], binary)?;
            let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
            let site = e.place_object(
                "W1N1",
                "constructionSite",
                &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
            )?;
            let before = e.world_snapshot();
            assert!(
                e.run_player(
                    "p1",
                    &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))")
                )
                .is_err(),
                "{call}"
            );
            assert_eq!(e.world_snapshot(), before, "{call}");
            assert_eq!(e.get_object(&site).unwrap()["progress"], 0, "{call}");
            assert!(e.needs_mod_recovery(), "{call}");
            assert!(e.last_mod_settlement_fault().is_some(), "{call}");
        }
    }
    // native 中心执行后的 after 处理器再调能力：consumed 单向，故障。
    let alpha = package(
        r#"
api.handlers.after('builtin:creep.build',{id:'late'},ctx=>{
  if(ctx.args.id===null)throw Error('build 缺目标');
  try{ctx.build.request(ctx.args.id)}catch(e){}
});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
    let site = e.place_object(
        "W1N1",
        "constructionSite",
        &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
    )?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    Ok(())
}

/// 处理器返回后能力过期；跨槽保存的能力失效。两者都是故障。
#[test]
fn creep_build_capability_expires_after_handler_and_across_slots() -> anyhow::Result<()> {
    let alpha = package(
        r#"
api.handlers.before('builtin:creep.build',{id:'save'},ctx=>{globalThis.saved=ctx.build;});
api.handlers.after('builtin:creep.build',{id:'use'},()=>{try{globalThis.saved.cancel()}catch(e){}});
"#,
    );
    let mut e = engine(&[("alpha", alpha)], false)?;
    let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
    let site = e.place_object(
        "W1N1",
        "constructionSite",
        &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
    )?;
    let before = e.world_snapshot();
    assert!(
        e.run_player(
            "p1",
            &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))")
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert_eq!(e.get_object(&site).unwrap()["progress"], 0);
    assert!(e.needs_mod_recovery());

    let beta = package(
        r#"
api.handlers.before('builtin:creep.build',{id:'save'},ctx=>{globalThis.saved=ctx.build;});
api.handlers.before('builtin:creep.transfer',{id:'use'},()=>{globalThis.saved.cancel()});
"#,
    );
    let mut e = engine(&[("beta", beta)], false)?;
    let _b1 = fighter(&mut e, "b1", 10, 10, &["work", "carry"])?;
    let site = e.place_object(
        "W1N1",
        "constructionSite",
        &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
    )?;
    let container = e.place_object(
        "W1N1",
        "structure",
        &json!({"pos":[11,10],"structureType":"container","owner":"p1","store":{"energy":20}}),
    )?;
    let before = e.world_snapshot();
    // 先触发 build 槽保存能力，再在 transfer 槽使用：跨槽默认拒绝。
    assert!(
        e.run_player(
            "p1",
            &format!(
                "const o=Game.getObjectById;Game.creeps.b1.build(o('{site}'));Game.creeps.b1.transfer(o('{container}'),'energy',5)"
            )
        )
        .is_err()
    );
    assert_eq!(e.world_snapshot(), before);
    assert!(e.needs_mod_recovery());
    Ok(())
}

/// 完工瞬间的拒绝：工地复原、结构与 ID 游标撤销、能量返还；下一 tick
/// 重新建造时结构以与控制组相同的 ID 落成（游标回退后才可能复用）。
#[test]
fn creep_build_completion_decline_rolls_back_site_structure_and_id_cursor() -> anyhow::Result<()> {
    let site_spec = json!({"pos":[11,10],"structureType":"road","owner":"p1","progress":295});
    // 门控用模块内一次性闩（ctx.tick 在首个结算轮为 0，不能作轮次标记；
    // 闩不是权威玩法状态，拒绝不回滚 JS 全局在此无害）。
    let gated = package(
        r#"
api.handlers.wrap('builtin:creep.build',{id:'once'},(ctx,next)=>{
  globalThis.calls=(globalThis.calls||0)+1;
  next();
  if(globalThis.calls===1)return api.decline('rolled');
  ctx.state={done:true};
});
"#,
    );
    // 控制组：单轮直接完工，记录道路 ID。
    let control_road_id = {
        let mut e = engine(&[("alpha", observe_pkg("builtin:creep.build"))], false)?;
        let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
        let site = e.place_object("W1N1", "constructionSite", &site_spec)?;
        e.run_player(
            "p1",
            &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
        )?;
        assert!(e.get_object(&site).is_none(), "控制组：工地完工移除");
        let roads: Vec<_> = e
            .find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
            .into_iter()
            .filter(|s| s["structureType"] == "road")
            .collect();
        assert_eq!(roads.len(), 1);
        assert_eq!(roads[0]["hits"], 5000, "平原道路初始耐久");
        let builds = events(&e, kk_core::constants::EVENT_BUILD);
        assert_eq!(builds.len(), 1);
        assert_eq!(builds[0]["data"]["incomplete"], false, "完工事件标记");
        roads[0]["id"].as_str().unwrap().to_string()
    };
    // 拒绝组：tick 1 完工后被拒，tick 2 重新完工。
    let mut e = engine(&[("alpha", gated)], false)?;
    let b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
    let site = e.place_object("W1N1", "constructionSite", &site_spec)?;
    e.run_player(
        "p1",
        &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
    )?;
    assert_eq!(
        e.get_object(&site).unwrap()["progress"],
        295,
        "完工被拒：工地复原"
    );
    assert!(
        e.find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
            .into_iter()
            .all(|s| s["structureType"] != "road"),
        "完工被拒：结构插入撤销"
    );
    assert_eq!(
        e.get_object(&b1).unwrap()["store"]["energy"],
        30,
        "能量返还"
    );
    assert_eq!(e.get_game_time(), 1);
    assert_eq!(e.engine_mod_state("alpha"), Value::Null);
    e.run_player(
        "p1",
        &format!("Game.creeps.b1.build(Game.getObjectById('{site}'))"),
    )?;
    let roads: Vec<_> = e
        .find_in_room("W1N1", kk_core::constants::FIND_STRUCTURES)
        .into_iter()
        .filter(|s| s["structureType"] == "road")
        .collect();
    assert_eq!(roads.len(), 1, "第二轮完工");
    assert_eq!(
        roads[0]["id"].as_str().unwrap(),
        control_road_id.as_str(),
        "ID 游标回退后重试以同一 ID 落成"
    );
    assert_eq!(e.engine_mod_state("alpha"), json!({"done":true}));
    Ok(())
}

/// 晚期 RawMemory 写回失败：建造候选与 Mod state 不发布；重试传入抛错
/// 代码仍使用原玩家缓存，重建 realm 中重新结算并应用改向。
#[test]
fn creep_build_candidates_survive_writeback_failure_via_cached_retry() -> anyhow::Result<()> {
    let alt_id = predictable_id(2);
    let source = package(&format!(
        "api.handlers.replace('builtin:creep.build',{{id:'redirect'}},ctx=>{{ctx.build.request('{alt_id}');ctx.state={{hit:true}};}});"
    ));
    for binary in [false, true] {
        let mut e = engine(&[("alpha", source.clone())], binary)?;
        let alt = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[12,10],"structureType":"road","owner":"p1"}),
        )?;
        assert_eq!(alt, alt_id, "改向目标 ID 预测破裂");
        let _b1 = fighter(&mut e, "b1", 10, 10, &["work"])?;
        let site = e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[11,10],"structureType":"road","owner":"p1"}),
        )?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!("Memory.step=1;Game.creeps.b1.build(Game.getObjectById('{site}'))")
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
        assert_eq!(
            e.get_object(&site).unwrap()["progress"],
            0,
            "重试仍应用改向：原目标无进度"
        );
        assert_eq!(e.get_object(&alt).unwrap()["progress"], 5, "重试应用改向");
        assert_eq!(e.engine_mod_state("alpha"), json!({"hit":true}));
        assert_eq!(e.player_state("p1").unwrap().raw_memory, "{\"step\":1}");
        assert!(!e.needs_mod_recovery());
    }
    Ok(())
}
