//! 托管受击与攻击拒绝、来源消失和反击状态的组合边界。
use super::*;

/// 正常拒绝丢弃未提交字段；一旦经 next 尝试写入，不能用 catch/decline 掩盖故障。
#[test]
fn incoming_target_fields_are_read_only_even_when_declined() -> anyhow::Result<()> {
    for binary in [false, true] {
        for mode in ["complete", "discard", "flush-then-decline"] {
            let handler = if mode == "flush-then-decline" {
                "api.handlers.wrap('builtin:mod.receiveDamage',{id:'move'},(ctx,next)=>{ctx.target.fields.x=12;try{next()}catch{}return api.decline('hide');});"
            } else if mode == "discard" {
                "api.handlers.replace('builtin:mod.receiveDamage',{id:'move'},ctx=>{ctx.target.fields.x=12;return api.decline('discard');});"
            } else {
                "api.handlers.replace('builtin:mod.receiveDamage',{id:'move'},ctx=>{ctx.target.fields.x=12;});"
            };
            let source = package(&format!("{} {handler}", ward_define("guard:cell")));
            let mut e = engine_with_player_files(
                &[("guard", source)],
                &[("guard", &player_surface("guard:cell"))],
                binary,
            )?;
            fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
            let target = cell(&mut e)?;
            let mut expected = e.get_object(&target).unwrap();
            let result = e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{target}'));"),
            );
            assert_eq!(result.is_err(), mode != "discard", "{mode}/{binary}");
            if mode == "discard" {
                expected["fields"]["hits"] = json!(270); // 正常拒绝回退默认伤害，坐标不变。
            }
            assert_eq!(e.get_object(&target).unwrap(), expected, "{mode}/{binary}");
        }
    }
    Ok(())
}

/// 拒绝只撤销当前攻击；已成功入队的攻击与后续攻击都必须保留。
#[test]
fn attack_decline_restores_pending_damage_for_native_redirect_and_policy_paths()
-> anyhow::Result<()> {
    let calls = [
        ("attack", "ctx.combat.request(ctx.args.id);"),
        ("attack", "ctx.combat.applyDamage(ctx.args.id, 80);"),
        (
            "rangedMassAttack",
            "ctx.combat.applyDamageBatch([{id:ctx.target.fields.sayTarget,amount:80}]);",
        ),
    ];
    // 群体策略的目标由固定放置顺序给出；不使用未声明的 creep 字段。
    let victim = harness::predictable_id(5);
    for binary in [false, true] {
        for (slot, call) in std::iter::once(("attack", "next();")).chain(calls) {
            let call = call.replace("ctx.target.fields.sayTarget", &format!("'{victim}'"));
            let handler = if call == "next();" {
                "api.handlers.wrap('builtin:creep.attack',{id:'deny'},(ctx,next)=>{next();if(ctx.target.fields.name==='a1')return api.decline('cancel');});".to_string()
            } else {
                format!(
                    "api.handlers.replace('builtin:creep.{slot}',{{id:'deny'}},ctx=>{{{call}if(ctx.target.fields.name==='a1')return api.decline('cancel');}});"
                )
            };
            let source = package(&format!(
                "{} {handler}
                api.handlers.before('builtin:mod.receiveDamage',{{id:'observe'}},ctx=>{{ctx.state={{calls:(ctx.state?.calls||0)+1}};}});",
                ward_define("guard:cell")
            ));
            let mut e = engine_with_player_files(
                &[("guard", source)],
                &[("guard", &player_surface("guard:cell"))],
                binary,
            )?;
            for (name, x) in [("a0", 9), ("a1", 10), ("a2", 11)] {
                fighter(&mut e, name, x, 10, &["attack", "ranged_attack", "move"])?;
            }
            let target = cell(&mut e)?;
            assert_eq!(target, victim);
            // 群体 native 不收集 Mod；前后成功动作仍用 attack 走原生路径。
            let middle = if slot == "rangedMassAttack" {
                "rangedMassAttack()".to_string()
            } else {
                "attack(t)".to_string()
            };
            e.run_player("p1", &format!(
                "const t=Game.getObjectById('{target}');Game.creeps.a0.attack(t);Game.creeps.a1.{middle};Game.creeps.a2.attack(t);"
            ))?;
            // 单体策略中心对三个攻击者都施加 80，其他路径施加原生 30。
            let expected = if call.contains("applyDamage(") {
                140
            } else {
                240
            };
            assert_eq!(mod_hits(&e, &target), expected, "{slot}/{call}/{binary}");
            assert_eq!(events(&e, kk_core::constants::EVENT_ATTACK).len(), 2);
            assert_eq!(e.engine_mod_state("guard"), json!({"calls":2}));
        }
    }
    Ok(())
}

#[test]
fn attack_survives_suicide_and_retains_source_summary() -> anyhow::Result<()> {
    for binary in [false, true] {
        for hosted in [false, true] {
            let handler = if hosted {
                "api.handlers.before('builtin:mod.receiveDamage',{id:'observe'},ctx=>{
                    if(ctx.combat.get(ctx.args.source.sourceId)!==null)throw Error('来源应已消失');
                    ctx.state={source:ctx.args.source.sourceId,amount:ctx.args.source.amount};
                });"
            } else {
                ""
            };
            let source = package(&format!(
                "{} {handler}",
                ward_define("guard:cell")
                    .replace("hitsMax:300", "hitsMax:300,retaliationRatio:0.5")
            ));
            let mut e = engine_with_player_files(
                &[("guard", source)],
                &[("guard", &player_surface("guard:cell"))],
                binary,
            )?;
            let attacker = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
            let target = cell(&mut e)?;
            e.run_player(
                "p1",
                &format!(
                    "Game.creeps.a.attack(Game.getObjectById('{target}'));Game.creeps.a.suicide();"
                ),
            )?;
            assert!(e.get_object(&attacker).is_none());
            assert_eq!(mod_hits(&e, &target), 270, "{hosted}/{binary}");
            if hosted {
                assert_eq!(
                    e.engine_mod_state("guard"),
                    json!({"source":attacker,"amount":30})
                );
                assert_eq!(
                    events(&e, kk_core::constants::EVENT_ATTACK).len(),
                    1,
                    "无幽灵反击事件"
                );
            }
        }
    }
    Ok(())
}

/// 后续 unclaim 改变房主不能反向改变攻击时安全模式的反击抑制。
#[test]
fn hosted_retaliation_preserves_attack_time_safe_mode_decision() -> anyhow::Result<()> {
    for binary in [false, true] {
        for hosted in [false, true] {
            let handler = if hosted {
                "api.handlers.before('builtin:mod.receiveDamage',{id:'observe'},ctx=>{});"
            } else {
                ""
            };
            let source = package(&format!(
                "{} {handler}",
                ward_define("guard:cell")
                    .replace("hitsMax:300", "hitsMax:300,retaliationRatio:0.5")
            ));
            let mut e = engine_with_player_files_and_shard(
                &[("guard", source)],
                &[("guard", &player_surface("guard:cell"))],
                binary,
                &json!({"players":["p1"],"rooms":[{"name":"W1N1","controller":false}]}),
            )?;
            // 控制器 ID 大于执行者，确保 unclaim 在攻击之后结算。
            let attacker = fighter(&mut e, "a", 10, 10, &["attack", "move"])?;
            let target = cell(&mut e)?;
            e.place_object("W1N1", "structure", &json!({"structureType":"controller","pos":[25,25],"owner":"p1","level":6,"safeMode":100}))?;
            e.run_player("p1", &format!(
                "Game.creeps.a.attack(Game.getObjectById('{target}'));Game.rooms.W1N1.controller.unclaim();"
            ))?;
            assert_eq!(harness::hits(&e, &attacker), 200, "{hosted}/{binary}");
            assert_eq!(mod_hits(&e, &target), 270);
        }
    }
    Ok(())
}
