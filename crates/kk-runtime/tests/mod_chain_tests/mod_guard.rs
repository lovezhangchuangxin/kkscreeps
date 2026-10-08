//! G09.c 结构受击槽（builtin:mod.receiveDamage）：托管挂起与排空的
//! 全链 JS 行为——减伤、正常拒绝回退、独立包组合、免疫与击毁。
use super::*;
use harness::{events, fighter};

/// Mod 对象管理快照的耐久读值（信封在 fields 下）。
fn mod_hits(e: &EmbedEngine, id: &str) -> i64 {
    e.get_object(id).unwrap()["fields"]["hits"]
        .as_i64()
        .unwrap()
}

fn ward_define(type_key: &str) -> String {
    format!(
        "api.objects.define('{type_key}', {{
          version:1, playerClass:'GuardCell', positionFields:['x','y'], hitsMax:300,
          fields:{{x:{{type:'u32',min:0,max:49}},y:{{type:'u32',min:0,max:49}}}}
        }});"
    )
}

fn player_surface(type_key: &str) -> String {
    format!(
        "export default function install(api) {{
          class GuardCell extends api.builtins.RoomObject {{
            get hits() {{ return api.view.field(this, 'hits'); }}
            get hitsMax() {{ return api.view.field(this, 'hitsMax'); }}
          }}
          api.objects.bind('{type_key}', GuardCell);
        }}"
    )
}

fn cell(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))
}

/// 减半中心 + 输入形状校验（当次伤害与在场攻击者摘要；目标形状经
/// ctx.target 的耐久信封承载）。
#[test]
fn hosted_structural_incoming_halves_damage_with_bounded_input() -> anyhow::Result<()> {
    let source = package(&format!(
        "{} 
        api.handlers.replace('builtin:mod.receiveDamage',{{id:'armor'}},ctx=>{{
            if(ctx.args.damage!==30)throw Error('当次伤害');
            if(!ctx.args.source||ctx.args.source.sourceId!==ctx.combat.get(ctx.args.source.sourceId).id)throw Error('来源摘要');
            if(ctx.args.source.attackType!==1)throw Error('攻击种类（近战=1）');
            if(ctx.target.fields.hits!==300||ctx.target.fields.hitsMax!==300)throw Error('目标形状经信封');
            ctx.incoming.setDamage(Math.ceil(ctx.args.damage/2));
            ctx.state={{calls:(ctx.state?.calls||0)+1}};
        }});",
        ward_define("guard:cell")
    ));
    for binary in [false, true] {
        let mut e = engine_with_player_files(
            &[("guard", source.clone())],
            &[("guard", &player_surface("guard:cell"))],
            binary,
        )?;
        let a = fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(mod_hits(&e, &b), 285, "槽减半后落地");
        assert_eq!(harness::hits(&e, &a), 100, "无原生近战反击改向");
        assert_eq!(e.engine_mod_state("guard"), json!({"calls":1}));
        let attacks = events(&e, kk_core::constants::EVENT_ATTACK);
        assert_eq!(attacks.len(), 1, "主攻击事件恰一次：{attacks:?}");
        assert_eq!(attacks[0]["data"]["damage"], 30, "事件记原值伤害");
    }
    Ok(())
}

/// 正常拒绝回退默认伤害；免疫（setDamage 0）保持满血。
#[test]
fn structural_decline_falls_back_and_zero_is_immunity() -> anyhow::Result<()> {
    for (handler, expect) in [
        (
            "api.handlers.replace('builtin:mod.receiveDamage',{id:'deny'},ctx=>{ctx.state={dirty:true};return api.decline('peace');});",
            270i64,
        ),
        (
            "api.handlers.replace('builtin:mod.receiveDamage',{id:'immune'},ctx=>ctx.incoming.setDamage(0));",
            300,
        ),
    ] {
        let source = package(&format!("{} {handler}", ward_define("guard:cell")));
        for binary in [false, true] {
            let mut e = engine_with_player_files(
                &[("guard", source.clone())],
                &[("guard", &player_surface("guard:cell"))],
                binary,
            )?;
            fighter(&mut e, "a", 10, 10, &["attack"])?;
            let b = e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))?;
            e.run_player(
                "p1",
                &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
            )?;
            assert_eq!(mod_hits(&e, &b), expect);
        }
    }
    Ok(())
}

/// 两个独立包组合：armor 拥有类型与替换中心，shield 只 wrap 追加固定
/// 减伤；两包各自的 namespace state 都恰好提交一次。
#[test]
fn separate_packages_compose_replace_center_and_wrap() -> anyhow::Result<()> {
    let armor = package(&format!(
        "{} 
        api.handlers.replace('builtin:mod.receiveDamage',{{id:'armor'}},ctx=>{{
            ctx.incoming.setDamage(Math.ceil(ctx.args.damage/2));ctx.state={{calls:1}};
        }});",
        ward_define("armor:cell")
    ));
    let shield = package(
        "api.handlers.wrap('builtin:mod.receiveDamage',{id:'shield'},(ctx,next)=>{
            next();ctx.incoming.setDamage(Math.max(0,ctx.incoming.getDamage()-4));ctx.state={calls:1};
        });",
    );
    for binary in [false, true] {
        let mut e = engine_with_player_files(
            &[("armor", armor.clone()), ("shield", shield.clone())],
            &[("armor", &player_surface("armor:cell"))],
            binary,
        )?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = e.place_mod_object("W1N1", "armor:cell", &json!({"x":10,"y":11}))?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        // ceil(30/2)=15，wrap 再减 4 → 11。
        assert_eq!(mod_hits(&e, &b), 289);
        assert_eq!(e.engine_mod_state("armor"), json!({"calls":1}));
        assert_eq!(e.engine_mod_state("shield"), json!({"calls":1}));
    }
    Ok(())
}

/// 槽放大伤害到表示域上界击毁目标：对象移除、类型键摧毁事件发出。
#[test]
fn amplified_damage_destroys_target_with_typed_event() -> anyhow::Result<()> {
    let source = package(&format!(
        "{} 
        api.handlers.replace('builtin:mod.receiveDamage',{{id:'doom'}},ctx=>{{
            ctx.incoming.setDamage(9007199254740991);
        }});",
        ward_define("guard:cell")
    ));
    for binary in [false, true] {
        let mut e = engine_with_player_files(
            &[("guard", source.clone())],
            &[("guard", &player_surface("guard:cell"))],
            binary,
        )?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let b = e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))?;
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert!(e.get_object(&b).is_none(), "击毁即移除");
        let destroyed = events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED);
        assert_eq!(destroyed.len(), 1, "恰一条摧毁事件：{destroyed:?}");
        assert_eq!(destroyed[0]["data"]["type"], "guard:cell", "类型键事件");
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK).len(),
            1,
            "主攻击事件仍在意图时刻发出"
        );
    }
    Ok(())
}

/// 集火同 tick 击穿：放大槽在首个挂起项击毁目标后，后续同目标的
/// 挂起项整项丢弃（槽不再跑），tick 正常提交且主攻击事件按攻击者
/// 各自保留。
#[test]
fn focus_fire_drops_later_pending_after_target_death() -> anyhow::Result<()> {
    let source = package(&format!(
        "{} 
        api.handlers.replace('builtin:mod.receiveDamage',{{id:'doom'}},ctx=>{{
            ctx.incoming.setDamage(9007199254740991);
            ctx.state={{calls:(ctx.state?.calls||0)+1}};
        }});",
        ward_define("guard:cell")
    ));
    for binary in [false, true] {
        let mut e = engine_with_player_files(
            &[("guard", source.clone())],
            &[("guard", &player_surface("guard:cell"))],
            binary,
        )?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        fighter(&mut e, "b", 10, 12, &["attack"])?;
        let b_obj = e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))?;
        e.run_player(
            "p1",
            &format!("const t=Game.getObjectById('{b_obj}');Game.creeps.a.attack(t);Game.creeps.b.attack(t);"),
        )?;
        assert!(e.get_object(&b_obj).is_none(), "首个挂起项击毁目标");
        assert_eq!(
            events(&e, kk_core::constants::EVENT_OBJECT_DESTROYED).len(),
            1
        );
        assert_eq!(
            events(&e, kk_core::constants::EVENT_ATTACK).len(),
            2,
            "两名攻击者的主攻击事件都在意图时刻保留"
        );
        assert_eq!(
            e.engine_mod_state("guard"),
            json!({"calls":1}),
            "目标死亡后的挂起项不再跑槽"
        );
    }
    Ok(())
}

/// 界值拒绝侧：setDamage 越界（-1）是槽故障，整 tick 候选失败且回滚
/// （耐久不动、state 不提交），与 creep 槽故障纪律一致。
#[test]
fn out_of_range_set_damage_faults_and_rolls_back_tick() -> anyhow::Result<()> {
    for bad in [-1, 9007199254740992i64] {
        let source = package(&format!(
            "{} 
            api.handlers.replace('builtin:mod.receiveDamage',{{id:'bad'}},ctx=>{{
                ctx.incoming.setDamage({bad});ctx.state={{dirty:true}};
            }});",
            ward_define("guard:cell")
        ));
        for binary in [false, true] {
            let mut e = engine_with_player_files(
                &[("guard", source.clone())],
                &[("guard", &player_surface("guard:cell"))],
                binary,
            )?;
            fighter(&mut e, "a", 10, 10, &["attack"])?;
            let b = e.place_mod_object("W1N1", "guard:cell", &json!({"x":10,"y":11}))?;
            assert!(
                e.run_player(
                    "p1",
                    &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
                )
                .is_err(),
                "越界 setDamage 使整 tick 失败"
            );
            assert_eq!(mod_hits(&e, &b), 300, "候选回滚，耐久不动");
            assert_eq!(e.engine_mod_state("guard"), serde_json::Value::Null);
        }
    }
    Ok(())
}

/// creep 与 mod 两个受击槽在同一计划共存：各自目标走各自槽与输入面
///（creep 见 rawDamage 累计形态、mod 见 damage 当次形态），共享的
/// CombatHost 双变体路由不串槽。
#[test]
fn creep_and_mod_incoming_slots_route_independently() -> anyhow::Result<()> {
    let source = package(&format!(
        "{} 
        api.handlers.replace('builtin:creep.receiveDamage',{{id:'creepface'}},ctx=>{{
            if(ctx.args.rawDamage===undefined||ctx.args.damage!==undefined)throw Error('creep 输入形态');
            ctx.incoming.setDamage(1);ctx.state={{...ctx.state,creepCalls:(ctx.state?.creepCalls||0)+1}};
        }});
        api.handlers.replace('builtin:mod.receiveDamage',{{id:'modface'}},ctx=>{{
            if(ctx.args.damage===undefined||ctx.args.rawDamage!==undefined)throw Error('mod 输入形态');
            ctx.incoming.setDamage(2);ctx.state={{...ctx.state,modCalls:(ctx.state?.modCalls||0)+1}};
        }});",
        ward_define("guard:cell")
    ));
    for binary in [false, true] {
        let mut e = engine_with_player_files(
            &[("guard", source.clone())],
            &[("guard", &player_surface("guard:cell"))],
            binary,
        )?;
        fighter(&mut e, "a", 10, 10, &["attack"])?;
        let v = fighter(&mut e, "v", 11, 10, &["tough", "move"])?;
        let b = cell(&mut e)?;
        // 同一 creep 同 tick 只一条 attack 意图：T0 打 creep 目标、T1 打 mod 目标。
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{v}'))"),
        )?;
        // creep 槽把累计伤害改为 1。
        assert_eq!(harness::hits(&e, &v), 199);
        assert_eq!(
            e.engine_mod_state("guard"),
            json!({"creepCalls":1}),
            "mod 槽未随 creep 目标触发"
        );
        e.run_player(
            "p1",
            &format!("Game.creeps.a.attack(Game.getObjectById('{b}'))"),
        )?;
        assert_eq!(mod_hits(&e, &b), 298, "mod 槽固定改 2");
        assert_eq!(
            e.engine_mod_state("guard"),
            json!({"creepCalls":1,"modCalls":1}),
            "两槽独立计数"
        );
    }
    Ok(())
}

mod boundaries;
