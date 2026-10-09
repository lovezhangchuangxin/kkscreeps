//! power 从玩家施放到 tower 消费、投影和候选恢复的完整链。
use super::*;
use kk_core::constants as c;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":[{"name":"p1","power":100000},"p2"],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}))?;
    Ok(e)
}
fn tower(e: &mut EmbedEngine) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "structure",
        &json!({"structureType":"tower",
        "owner":"p1","pos":[20,20],"store":{"energy":1000}}),
    )
}
fn operator(e: &mut EmbedEngine, name: &str, power: i64, level: i64) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1",
        "pos":[19,20],"powers":{power.to_string():level},"store":{"ops":100}}),
    )
}
fn cast(e: &mut EmbedEngine, name: &str, power: i64, target: &str) -> anyhow::Result<()> {
    let result = e.run_player(
        "p1",
        &format!("Game.powerCreeps.{name}.usePower({power},Game.getObjectById('{target}'))"),
    )?;
    assert_eq!(result.expr["value"], 0);
    Ok(())
}

/// 五等级 × 两效果 × 三动作；预伤害走真实攻击，避免伪造 creep hits。
#[test]
fn tower_powers_consume_ops_and_scale_attack_heal_repair_all_levels() -> anyhow::Result<()> {
    for binary in [false, true] {
        for power in [c::PWR_OPERATE_TOWER, c::PWR_DISRUPT_TOWER] {
            for level in 1..=5 {
                let mut e = fixture(binary)?;
                let t = tower(&mut e)?;
                let pc = operator(&mut e, "operator", power, level)?;
                let enemy = harness::enemy(&mut e, "enemy", 27, 20, &["move"; 30])?;
                let ally = harness::fighter(&mut e, "ally", 28, 20, &["move"; 30])?;
                let road = e.place_object(
                    "W1N1",
                    "structure",
                    &json!({
                    "structureType":"road","pos":[29,20],"hits":100}),
                )?;
                cast(&mut e, "operator", power, &t)?;
                assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 90);
                assert_eq!(
                    e.get_object(&t).unwrap()["effects"][0],
                    json!({
                    "power":power,"effect":power,"level":level,
                    "ticksRemaining":if power==c::PWR_OPERATE_TOWER {99}else{4}})
                );
                assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
                let multiplier = if power == c::PWR_OPERATE_TOWER {
                    1.0 + level as f64 * 0.1
                } else {
                    1.0 - level as f64 * 0.1
                };
                // range 7/8/9：基础量分别 540/340/640，效果后才向下取整。
                e.run_player(
                    "p1",
                    &format!("Game.getObjectById('{t}').attack(Game.getObjectById('{enemy}'))"),
                )?;
                assert_eq!(
                    e.get_object(&enemy).unwrap()["hits"],
                    3000 - (540.0 * multiplier).floor() as i64
                );
                e.run_player(
                    "p1",
                    &format!("Game.getObjectById('{t}').attack(Game.getObjectById('{ally}'))"),
                )?;
                let before = e.get_object(&ally).unwrap()["hits"].as_i64().unwrap();
                e.run_player(
                    "p1",
                    &format!("Game.getObjectById('{t}').heal(Game.getObjectById('{ally}'))"),
                )?;
                assert_eq!(
                    e.get_object(&ally).unwrap()["hits"],
                    before + (340.0 * multiplier).floor() as i64
                );
                e.run_player(
                    "p1",
                    &format!("Game.getObjectById('{t}').repair(Game.getObjectById('{road}'))"),
                )?;
                assert_eq!(
                    e.get_object(&road).unwrap()["hits"],
                    100 + (640.0 * multiplier).floor() as i64
                );
                assert_eq!(e.get_object(&t).unwrap()["store"]["energy"], 960);
                let cooldown = e.run_player(
                    "p1",
                    &format!("Game.powerCreeps.operator.powers[{power}].cooldown"),
                )?;
                assert_eq!(
                    cooldown.expr["value"],
                    if power == c::PWR_OPERATE_TOWER { 5 } else { 0 }
                );
            }
        }
    }
    Ok(())
}
#[test]
fn tower_effects_coexist_refresh_reject_lower_level_and_expire() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let t = tower(&mut e)?;
        operator(&mut e, "high", c::PWR_OPERATE_TOWER, 3)?;
        let low = operator(&mut e, "low", c::PWR_OPERATE_TOWER, 1)?;
        operator(&mut e, "same", c::PWR_OPERATE_TOWER, 3)?;
        operator(&mut e, "disrupt", c::PWR_DISRUPT_TOWER, 2)?;
        let enemy = harness::enemy(&mut e, "enemy", 27, 20, &["move"; 30])?;
        cast(&mut e, "high", c::PWR_OPERATE_TOWER, &t)?;
        let result = e.run_player(
            "p1",
            &format!("Game.powerCreeps.low.usePower(PWR_OPERATE_TOWER,Game.getObjectById('{t}'))"),
        )?;
        assert_eq!(result.expr["value"], c::ERR_FULL);
        assert_eq!(e.get_object(&low).unwrap()["store"]["ops"], 100);
        cast(&mut e, "same", c::PWR_OPERATE_TOWER, &t)?;
        assert_eq!(
            e.get_object(&t).unwrap()["effects"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        cast(&mut e, "disrupt", c::PWR_DISRUPT_TOWER, &t)?;
        let projected = e.get_object(&t).unwrap()["effects"].clone();
        let public = e.run_player("p1", &format!("Game.getObjectById('{t}').effects"))?;
        assert_eq!(public.expr["value"], projected);
        assert_eq!(public.expr["value"].as_array().unwrap().len(), 2);
        e.run_player(
            "p1",
            &format!("Game.getObjectById('{t}').attack(Game.getObjectById('{enemy}'))"),
        )?;
        assert_eq!(
            e.get_object(&enemy).unwrap()["hits"],
            3000 - (540.0_f64 * 1.3 * 0.8).floor() as i64
        );
        e.tick(100)?;
        assert_eq!(e.get_object(&t).unwrap()["effects"], json!([]));
        cast(&mut e, "low", c::PWR_OPERATE_TOWER, &t)?;
        assert_eq!(e.get_object(&t).unwrap()["effects"][0]["level"], 1);
    }
    Ok(())
}
#[test]
fn power_effect_writeback_failure_retries_once_and_cold_checkpoint_keeps_formula()
-> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let t = tower(&mut e)?;
        let pc = operator(&mut e, "operator", c::PWR_OPERATE_TOWER, 5)?;
        let enemy = harness::enemy(&mut e, "enemy", 27, 20, &["move"; 30])?;
        let before = e.world_snapshot();
        let player = e.player_state("p1").unwrap().clone();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = format!(
            "Memory.executions=(Memory.executions||0)+1;Game.powerCreeps.operator.usePower(PWR_OPERATE_TOWER,Game.getObjectById('{t}'))"
        );
        assert!(e.run_player("p1", &code).is_err());
        assert_eq!(e.world_snapshot(), before);
        assert_eq!(e.player_state("p1"), Some(&player));
        assert!(e.run_player("p1", "throw Error('不得重跑玩家')").is_ok());
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 90);
        let saved = e.checkpoint(Value::Null)?;
        assert_eq!(saved["checkpoint_format_version"], 5);
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(saved, None)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        let attack = format!(
            "[Memory.executions,Game.getObjectById('{t}').attack(Game.getObjectById('{enemy}'))]"
        );
        assert_eq!(
            restored.run_player("p1", &attack)?.expr["value"],
            json!([1, 0])
        );
        e.run_player("p1", &attack)?;
        assert_eq!(restored.world_snapshot(), e.world_snapshot());
        assert_eq!(restored.get_object(&enemy).unwrap()["hits"], 2190);
    }
    Ok(())
}
/// 删除条目跟对象一起回滚，拒绝后仍保存同一有效效果。
#[test]
fn power_effect_host_destruction_and_local_decline_restore_the_same_state() -> anyhow::Result<()> {
    for binary in [false, true] {
        for decline in [false, true] {
            let body = if decline {
                "next();return api.decline('peace');"
            } else {
                "next();"
            };
            let mut e = engine_with_shard(
                &[(
                    "alpha",
                    package(&format!(
                        "api.handlers.wrap('builtin:creep.dismantle',{{id:'boundary'}},(ctx,next)=>{{{body}}});"
                    )),
                )],
                binary,
                &[],
                RegisterOptions::default(),
                &json!({"players":["p1"],
                "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true}]}),
            )?;
            let t = e.place_object(
                "W1N1",
                "structure",
                &json!({"structureType":"tower",
                "owner":"p1","pos":[20,20],"hits":40,"store":{"energy":100}}),
            )?;
            operator(&mut e, "operator", c::PWR_OPERATE_TOWER, 1)?;
            harness::fighter(&mut e, "worker", 21, 20, &["work"])?;
            cast(&mut e, "operator", c::PWR_OPERATE_TOWER, &t)?;
            e.run_player(
                "p1",
                &format!("Game.creeps.worker.dismantle(Game.getObjectById('{t}'))"),
            )?;
            let saved = e.checkpoint(Value::Null)?;
            assert_eq!(
                saved["engine"]["world"]["rooms"]["W1N1"]["effects"]
                    .as_object()
                    .unwrap()
                    .len(),
                usize::from(decline)
            );
            if decline {
                assert_eq!(e.get_object(&t).unwrap()["hits"], 40);
                assert_eq!(
                    e.get_object(&t).unwrap()["effects"][0]["ticksRemaining"],
                    98
                );
            } else {
                assert!(e.get_object(&t).is_none());
            }
        }
    }
    Ok(())
}
#[test]
fn malformed_or_missing_effect_checkpoint_does_not_replace_live_world() -> anyhow::Result<()> {
    let mut e = fixture(false)?;
    let t = tower(&mut e)?;
    operator(&mut e, "operator", c::PWR_OPERATE_TOWER, 1)?;
    cast(&mut e, "operator", c::PWR_OPERATE_TOWER, &t)?;
    let saved = e.checkpoint(Value::Null)?;
    let before = e.world_snapshot();
    for kind in [
        "missing_current",
        "missing_previous",
        "orphan",
        "duplicate",
        "level",
        "time",
    ] {
        let mut bad = saved.clone();
        let room = &mut bad["engine"]["world"]["rooms"]["W1N1"];
        let id = room["effects"]
            .as_object()
            .unwrap()
            .keys()
            .next()
            .unwrap()
            .clone();
        match kind {
            "missing_current" | "missing_previous" => {
                room.as_object_mut().unwrap().remove("effects");
            }
            "orphan" => {
                let entry = room["effects"][&id].clone();
                room["effects"]["999999"] = entry;
            }
            "duplicate" => {
                let entry = room["effects"][&id][0].clone();
                room["effects"][&id].as_array_mut().unwrap().push(entry);
            }
            "level" => room["effects"][&id][0]["level"] = json!(6),
            _ => room["effects"][&id][0]["end_time"] = json!(9_007_199_254_740_992u64),
        }
        if kind == "missing_previous" {
            bad["checkpoint_format_version"] = json!(3);
        }
        assert!(e.restore_checkpoint(bad, None).is_err(), "{kind}");
        assert_eq!(e.world_snapshot(), before, "{kind}");
    }
    Ok(())
}
/// 玩家可改公开数组，但不能以脱离副本骗过权威效果覆盖门。
#[test]
fn forged_public_effects_do_not_bypass_native_level_gate() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let t = tower(&mut e)?;
        operator(&mut e, "high", c::PWR_OPERATE_TOWER, 5)?;
        let low = operator(&mut e, "low", c::PWR_OPERATE_TOWER, 1)?;
        cast(&mut e, "high", c::PWR_OPERATE_TOWER, &t)?;
        let result=e.run_player("p1",&format!("const t=Game.getObjectById('{t}');t.effects.length=0;Game.powerCreeps.low.usePower(PWR_OPERATE_TOWER,t)"))?;
        assert_eq!(result.expr["value"], 0);
        assert_eq!(e.get_object(&low).unwrap()["store"]["ops"], 100);
        assert_eq!(e.get_object(&t).unwrap()["effects"][0]["level"], 5);
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());
    }
    Ok(())
}
